package main

// ij_podcasts.go — Podcasts (main menu, under Music), as on the iPod: find a podcast, see
// its episodes, download them into the library.
//
//	Podcasts                     The Rest Is History
//	  ⌕ Search podcasts             Download the 3 Latest
//	  ▣ The Rest Is History         Episode 512 · Oct 27 · 54 min · 48 MB
//	  ▣ In Our Time                 Episode 511 · Oct 24 · 1 h 2 min · Downloaded
//
// The search is Apple's podcast directory (the iTunes Search API, no key);
// a podcast's episodes come from its RSS feed. An episode is downloaded
// with the Internet Archive screen (progress, time left, Cancel, Try
// Again), continued where it stopped and tried again when the connection
// drops, into Music/Podcasts/<podcast>/ as "<date> <title>.mp3" with its
// tags (tagwrite.go, ij_m4a.go) and the podcast's artwork as cover.jpg, so
// every podcast is an album. A downloaded episode plays at once; Y (or
// holding A) on it deletes it, and on a podcast deletes all its episodes,
// asking first as Browse Files does. The podcasts you download from are
// kept under Your Podcasts (podcasts.json in the data folder), with the
// files each one downloaded: only those count as downloaded and only those
// are deleted (a Music/Podcasts folder of your own is left alone).
//
// A download is checked before it is kept: the file's first bytes decide
// its format (feeds often say the wrong type), and it is tagged while it
// is still name.part, so a finished name is always a whole, tagged file.
// A .part is continued only when the server shows it's the same file
// (If-Range with its ETag or date, the same total size): with ads put in
// at download time, the same link gives different files.
//
// Episodes in a format the console can't play (supportedAudioExt) are
// shown but can't be downloaded. MIYOOPOD_ITUNES_BASE points the search
// elsewhere (tests).

import (
	"bytes"
	"context"
	"encoding/json"
	"encoding/xml"
	"errors"
	"fmt"
	"image"
	"image/jpeg"
	"io"
	"math"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"golang.org/x/text/encoding/htmlindex"
)

// podsInMenu: Podcasts is in the main menu (Settings → Podcasts; saved
// with the prefs).
var podsInMenu = true

type podcast struct {
	Name   string `json:"name"`
	Author string `json:"author"`
	Feed   string `json:"feed"`
	Art    string `json:"art"`    // the artwork, about 100 px (the lists)
	Art600 string `json:"art600"` // the artwork for cover.jpg
	Genre  string `json:"genre,omitempty"`
	Count  int    `json:"-"` // episodes, as the search says

	// what betterPod downloaded into dir(): episode key → file name, and
	// whether cover.jpg is its own. Never changed in place (copied on
	// write), so a copy read on the UI goroutine stays as it was.
	Files map[string]string `json:"files,omitempty"`
	Cover bool              `json:"cover,omitempty"`
}

func (p podcast) dir() string { return filepath.Join(musicRoot(), "Podcasts", safeName(p.Name)) }

type podEpisode struct {
	GUID    string
	Title   string
	Date    time.Time
	Secs    float64
	URL     string
	Size    int64
	Type    string
	Episode int
}

// ext: the file's extension, from its type or its link.
func (e podEpisode) ext() string {
	switch t := strings.ToLower(e.Type); {
	case strings.Contains(t, "mpeg"), strings.Contains(t, "mp3"):
		return ".mp3"
	case strings.Contains(t, "m4a"), strings.Contains(t, "mp4"), strings.Contains(t, "aac"):
		return ".m4a"
	case strings.Contains(t, "ogg"):
		return ".ogg"
	case strings.Contains(t, "flac"):
		return ".flac"
	}
	if u, err := url.Parse(e.URL); err == nil {
		if x := strings.ToLower(filepath.Ext(u.Path)); x != "" {
			return x
		}
	}
	return ".mp3"
}

func (e podEpisode) playable() bool { return supportedAudioExt(e.ext()) }

// key: what tells the episode apart in Files (its guid, else its link).
func (e podEpisode) key() string {
	if e.GUID != "" {
		return e.GUID
	}
	return e.URL
}

// baseName: "<date> <title>", without extension.
func (e podEpisode) baseName() string {
	name := e.Title
	if !e.Date.IsZero() {
		name = e.Date.Format("2006-01-02") + " " + name
	}
	return safeName(name)
}

// freeName: base+ext, or "base (2)ext"… when taken (taken gets lower-case
// names: the SD card doesn't tell case apart).
func freeName(base, ext string, taken func(string) bool) string {
	name := base + ext
	for i := 2; taken(strings.ToLower(name)); i++ {
		name = fmt.Sprintf("%s (%d)%s", base, i, ext)
	}
	return name
}

// ---- Apple's directory ----

func podcastSearch(term string) ([]podcast, error) {
	itunesWait()
	u := fmt.Sprintf("%s/search?media=podcast&entity=podcast&limit=25&term=%s",
		envBase("MIYOOPOD_ITUNES_BASE", "https://itunes.apple.com"), url.QueryEscape(term))
	var out struct {
		Results []struct {
			Name   string `json:"collectionName"`
			Artist string `json:"artistName"`
			Feed   string `json:"feedUrl"`
			Art100 string `json:"artworkUrl100"`
			Art600 string `json:"artworkUrl600"`
			Genre  string `json:"primaryGenreName"`
			Count  int    `json:"trackCount"`
		} `json:"results"`
	}
	if err := coverJSON("iTunes", u, &out); err != nil {
		return nil, err
	}
	var ps []podcast
	for _, r := range out.Results {
		if r.Feed == "" {
			continue // no public feed
		}
		ps = append(ps, podcast{Name: r.Name, Author: r.Artist, Feed: r.Feed, Art: r.Art100, Art600: r.Art600,
			Genre: r.Genre, Count: r.Count})
	}
	return ps, nil
}

// ---- the feed ----

// podMaxEpisodes: the newest episodes kept from a feed (some list
// thousands); podFeedItems: the most read (the feed's order isn't always
// newest first, so they are sorted).
const (
	podMaxEpisodes = 200
	podFeedItems   = 5000
)

var podDateLayouts = []string{time.RFC1123Z, time.RFC1123, "Mon, 2 Jan 2006 15:04:05 -0700", "Mon, 2 Jan 2006 15:04:05 MST",
	"2 Jan 2006 15:04:05 -0700", "Mon, 2 Jan 2006 15:04 -0700", "Mon, 02 Jan 2006 15:04:05 Z", "2006-01-02T15:04:05Z07:00", "Mon, 2 Jan 2006"}

func podDate(s string) time.Time {
	s = strings.TrimSpace(s)
	for _, l := range podDateLayouts {
		if t, err := time.Parse(l, s); err == nil {
			return t
		}
	}
	return time.Time{}
}

// podDuration: "1:02:03", "62:03" or seconds.
func podDuration(s string) float64 {
	s = strings.TrimSpace(s)
	if s == "" {
		return 0
	}
	t := 0.0
	for _, p := range strings.Split(s, ":") {
		v, err := strconv.ParseFloat(strings.TrimSpace(p), 64)
		if err != nil {
			return 0
		}
		t = t*60 + v
	}
	if math.IsNaN(t) || math.IsInf(t, 0) || t < 0 || t > 100*3600 {
		return 0
	}
	return t
}

type xmlText struct {
	XMLName xml.Name
	Text    string `xml:",chardata"`
}

type rssItem struct {
	Titles    []xmlText `xml:"title"` // <title> and <itunes:title>: the plain one wins
	GUID      string    `xml:"guid"`
	PubDate   string    `xml:"pubDate"`
	Duration  string    `xml:"duration"`
	Episode   string    `xml:"episode"`
	Enclosure struct {
		URL    string `xml:"url,attr"`
		Length string `xml:"length,attr"`
		Type   string `xml:"type,attr"`
	} `xml:"enclosure"`
}

// parseFeed reads a podcast's RSS: its name and author, and its newest
// episodes with audio.
func parseFeed(r io.Reader) (podcast, []podEpisode, error) {
	var p podcast
	var eps []podEpisode
	d := xml.NewDecoder(r)
	d.Strict = false
	d.Entity = xml.HTMLEntity // &rsquo; and the like, common in feeds
	d.CharsetReader = func(cs string, in io.Reader) (io.Reader, error) {
		if enc, err := htmlindex.Get(cs); err == nil && enc != nil {
			return enc.NewDecoder().Reader(in), nil
		}
		return in, nil // unknown: read as UTF-8
	}
	depth, inChannel := 0, false
	for len(eps) < podFeedItems {
		tok, err := d.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			if len(eps) > 0 {
				break // a damaged end: what was read is kept
			}
			return p, nil, fmt.Errorf("not a podcast feed (%v)", err)
		}
		switch t := tok.(type) {
		case xml.StartElement:
			depth++
			name := t.Name.Local
			switch {
			case name == "channel":
				inChannel = true
			case name == "item" && inChannel:
				var it rssItem
				err := d.DecodeElement(&it, &t)
				depth--
				if err != nil || it.Enclosure.URL == "" {
					continue
				}
				title := ""
				for _, x := range it.Titles {
					if x.XMLName.Space == "" || title == "" {
						title = x.Text
					}
					if x.XMLName.Space == "" {
						break
					}
				}
				e := podEpisode{GUID: strings.TrimSpace(it.GUID), Title: strings.TrimSpace(title), Date: podDate(it.PubDate),
					Secs: podDuration(it.Duration), URL: strings.TrimSpace(it.Enclosure.URL), Type: it.Enclosure.Type}
				e.Size, _ = strconv.ParseInt(strings.TrimSpace(it.Enclosure.Length), 10, 64)
				if e.Size < 0 || e.Size > 4<<30 {
					e.Size = 0 // made up
				}
				e.Episode, _ = strconv.Atoi(strings.TrimSpace(it.Episode))
				if e.Title == "" {
					e.Title = "Episode"
				}
				if strings.HasPrefix(strings.ToLower(e.Type), "video/") {
					continue
				}
				eps = append(eps, e)
			case inChannel && depth == 3 && name == "title" && t.Name.Space == "" && p.Name == "":
				var s string
				if d.DecodeElement(&s, &t) == nil {
					depth--
					p.Name = strings.TrimSpace(s)
				}
			case inChannel && depth == 3 && name == "author" && p.Author == "":
				var s string
				if d.DecodeElement(&s, &t) == nil {
					depth--
					p.Author = strings.TrimSpace(s)
				}
			case inChannel && depth == 3 && name == "image" && t.Name.Space != "" && p.Art600 == "":
				for _, a := range t.Attr {
					if a.Name.Local == "href" {
						p.Art600 = a.Value
					}
				}
			}
		case xml.EndElement:
			depth--
		}
	}
	if p.Name == "" && len(eps) == 0 {
		return p, nil, errors.New("not a podcast feed")
	}
	// newest first (undated ones after, in the feed's order)
	sort.SliceStable(eps, func(i, j int) bool {
		a, b := eps[i].Date, eps[j].Date
		if a.IsZero() || b.IsZero() {
			return !a.IsZero() && b.IsZero()
		}
		return a.After(b)
	})
	if len(eps) > podMaxEpisodes {
		eps = eps[:podMaxEpisodes]
	}
	return p, eps, nil
}

// fetchFeed reads a feed; an http:// one through https:// first (many
// directory entries are old links of servers that have TLS now).
func fetchFeed(feed string) (podcast, []podEpisode, error) {
	get := func(u string) (podcast, []podEpisode, error) {
		resp, err := coverGetAccept(u, "application/rss+xml, application/xml, text/xml, */*", 30*time.Second)
		if err != nil {
			return podcast{}, nil, err
		}
		defer resp.Body.Close()
		if resp.StatusCode != 200 {
			return podcast{}, nil, fmt.Errorf("the podcast's server answered %d", resp.StatusCode)
		}
		return parseFeed(io.LimitReader(resp.Body, 32<<20))
	}
	if rest, ok := strings.CutPrefix(feed, "http://"); ok {
		if p, eps, err := get("https://" + rest); err == nil {
			return p, eps, nil
		}
	}
	return get(feed)
}

// ---- Your Podcasts ----

var podSaved struct {
	mu   sync.Mutex
	list []podcast
	read bool
}

func podSavedPath() string { return dataPath("podcasts.json") }

func savedPodcasts() []podcast {
	podSaved.mu.Lock()
	defer podSaved.mu.Unlock()
	if !podSaved.read {
		podSaved.read = true
		if b, err := os.ReadFile(podSavedPath()); err == nil {
			json.Unmarshal(b, &podSaved.list)
		}
	}
	return append([]podcast(nil), podSaved.list...)
}

func setSavedPodcasts(f func([]podcast) []podcast) {
	savedPodcasts()
	podSaved.mu.Lock()
	podSaved.list = f(podSaved.list)
	b, _ := json.MarshalIndent(podSaved.list, "", "  ")
	podSaved.mu.Unlock()
	if err := writeFileAtomic(podSavedPath(), b); err != nil {
		logMsg("Podcasts: saving the list: " + err.Error())
	}
}

// podUpdate changes p's entry with f (Files already copied, so f can
// change it); add: put p in the list if it isn't.
func podUpdate(p podcast, add bool, f func(r *podcast)) {
	setSavedPodcasts(func(l []podcast) []podcast {
		i := -1
		for j, x := range l {
			if x.Feed == p.Feed {
				i = j
				break
			}
		}
		if i < 0 {
			if !add {
				return l
			}
			r := p
			r.Files, r.Cover = nil, false
			l, i = append([]podcast{r}, l...), 0
		}
		r := l[i]
		files := make(map[string]string, len(r.Files)+1)
		for k, n := range r.Files {
			files[k] = n
		}
		r.Files = files
		f(&r)
		l[i] = r
		return l
	})
}

// rememberPodcast puts p in Your Podcasts (or brings its name, artwork…
// up to date), keeping what it downloaded.
func rememberPodcast(p podcast) {
	podUpdate(p, true, func(r *podcast) {
		files, cover := r.Files, r.Cover
		*r = p
		r.Files, r.Cover = files, cover
	})
}

// podRecord: p's entry in Your Podcasts (what it downloaded).
func podRecord(feed string) podcast {
	for _, x := range savedPodcasts() {
		if x.Feed == feed {
			return x
		}
	}
	return podcast{}
}

// podForgetMissing drops from p's entry the files that are no longer
// there (after deleting).
func podForgetMissing(p podcast) {
	podUpdate(p, false, func(r *podcast) {
		for k, n := range r.Files {
			if !fileExists(filepath.Join(p.dir(), n)) {
				delete(r.Files, k)
			}
		}
		if r.Cover && !fileExists(filepath.Join(p.dir(), "cover.jpg")) {
			r.Cover = false
		}
	})
}

// dirSet: the names in dir.
func dirSet(dir string) map[string]bool {
	es, _ := os.ReadDir(dir)
	set := make(map[string]bool, len(es))
	for _, e := range es {
		set[e.Name()] = true
	}
	return set
}

func forgetPodcast(feed string) {
	setSavedPodcasts(func(l []podcast) []podcast {
		out := l[:0]
		for _, x := range l {
			if x.Feed != feed {
				out = append(out, x)
			}
		}
		return out
	})
}

// ---- thumbnails ----

var podThumbCache sync.Map // artwork URL → image.Image

// podThumbs loads the artwork of the rows' stand-in albums, one by one.
func (ui *IpodUI) podThumbs(v *View, albums []*Album, urls []string, gen *atomic.Int32) {
	g := gen.Load()
	go func() {
		for i, a := range albums {
			if gen.Load() != g || !ui.app.Running {
				return
			}
			u := urls[i]
			if u == "" {
				continue
			}
			img, ok := podThumbCache.Load(u)
			if !ok {
				data, err := coverImage("iTunes", u)
				if err != nil || data == nil {
					continue
				}
				m, _, err := image.Decode(bytes.NewReader(data))
				if err != nil {
					continue
				}
				podThumbCache.Store(u, m)
				img = m
			}
			a := a
			ui.postSure(func() {
				a.ArtImg = img.(image.Image)
				ui.artc.dropAlbum(a)
				ui.refreshView(v)
			})
		}
	}()
}

// ---- Podcasts ----

func (ui *IpodUI) podcastsView() *View {
	st := &struct {
		term, status string
		busy         bool
		res          []podcast
		searched     bool
		gen          atomic.Int32 // the search
		thumbGen     atomic.Int32 // the thumbnails being loaded
		recs         map[string]podcast
	}{}
	v := &View{ID: "podcasts", Title: "Podcasts", Kind: KindFull}
	var run func()
	thumbs := map[string]*Album{}
	stand := func(p podcast) *Album {
		a := thumbs[p.Feed]
		if a == nil {
			a = &Album{Name: p.Name, Artist: p.Author}
			thumbs[p.Feed] = a
		}
		return a
	}
	row := func(p podcast) Item {
		var parts []string
		if p.Author != "" {
			parts = append(parts, p.Author)
		}
		if p.Genre != "" {
			parts = append(parts, p.Genre)
		}
		if p.Count > 0 {
			parts = append(parts, fmt.Sprintf("%d %s", p.Count, pluralize(p.Count, "episode", "episodes")))
		}
		it := Item{Label: p.Name, Sublabel: strings.Join(parts, " · "), Album: stand(p),
			OnSelect: func() { ui.showView(ui.podcastView(p), "") }}
		if len(st.recs[p.Feed].Files) > 0 {
			it.LongPress = []SheetOption{{Label: "Delete Downloaded Episodes", Danger: true, OnSelect: func() {
				ui.deletePodcastEpisodes(p, func() { ui.refreshView(v) })
			}}}
		}
		return it
	}
	loadThumbs := func(ps []podcast) {
		var as []*Album
		var us []string
		for _, p := range ps {
			if a := stand(p); a.ArtImg == nil {
				as, us = append(as, a), append(us, p.Art)
			}
		}
		st.thumbGen.Add(1) // the ones of the other list stop
		ui.podThumbs(v, as, us, &st.thumbGen)
	}
	v.onClose = func() { st.gen.Add(1); st.thumbGen.Add(1) }
	v.Build = func() []Item {
		saved := savedPodcasts()
		st.recs = make(map[string]podcast, len(saved))
		for _, p := range saved {
			st.recs[p.Feed] = p
		}
		note := ""
		if !st.searched && len(saved) > 0 && !ui.keyboardEditing(&st.term) {
			note = "Your Podcasts" // the list below
		}
		if st.searched && !st.busy && !ui.keyboardEditing(&st.term) {
			note = fmt.Sprintf("%d %s", len(st.res), pluralize(len(st.res), "result", "results"))
		}
		edit := func() {
			ui.showKeyboard(&st.term, func() { ui.refreshView(v) }, run)
			ui.refreshView(v)
		}
		items := []Item{ui.searchField(&st.term, "Search podcasts", note, edit, []SheetOption{
			{Label: "Clear search", OnSelect: func() { st.term = ""; edit() }},
		})}
		switch {
		case st.busy:
			items = append(items, Item{Label: "Searching…", NoArrow: true})
		case st.status != "":
			l, s, _ := strings.Cut(st.status, " · ")
			items = append(items, Item{Label: l, Sublabel: s, NoArrow: true, OnSelect: func() { run() }})
		}
		if st.searched {
			for _, p := range st.res {
				items = append(items, row(p))
			}
			return items
		}
		// the last few searches, then Your Podcasts
		rec := ui.recentItems("podcasts", func(term string) {
			st.term = term
			run()
		}, func() { ui.refreshView(v) })
		if len(rec) > 3 {
			rec = rec[:3]
		}
		items = append(items, rec...)
		if len(saved) == 0 {
			if len(rec) > 0 {
				return items
			}
			return append(items, Item{Label: "Find a podcast", Sublabel: "Search above; the ones you download from are kept here", NoArrow: true})
		}
		for _, p := range saved {
			it := row(p)
			it.LongPress = append(it.LongPress, SheetOption{Label: "Remove from Your Podcasts", Danger: true, OnSelect: func() {
				forgetPodcast(p.Feed)
				if len(p.Files) > 0 {
					ui.showToast("Its episodes stay in Music › Podcasts")
				}
				ui.refreshView(v)
			}})
			items = append(items, it)
		}
		return items
	}
	run = func() {
		term := strings.TrimSpace(st.term)
		if term == "" {
			st.gen.Add(1)
			st.searched, st.res, st.status, st.busy = false, nil, "", false
			ui.refreshView(v)
			loadThumbs(savedPodcasts())
			return
		}
		g := st.gen.Add(1)
		addRecentSearch("podcasts", term)
		st.busy, st.status, st.searched, st.res = true, "", true, nil
		v.Index = 0
		ui.refreshView(v)
		go func() {
			ps, err := podcastSearch(term)
			ui.postSure(func() {
				if st.gen.Load() != g {
					return
				}
				st.busy = false
				switch {
				case err != nil:
					st.status = "No answer · " + iaErrText(err) + " · press A to retry"
				case len(ps) == 0:
					st.status = "No podcasts found · try other words"
				}
				st.res = ps
				ui.refreshView(v)
				if len(ps) > 0 {
					v.Index = 1
				}
				loadThumbs(ps)
			})
		}()
	}
	v.Items = v.Build()
	loadThumbs(savedPodcasts())
	return v
}

// podcastView: a podcast's episodes, newest first.
func (ui *IpodUI) podcastView(p podcast) *View {
	st := &struct {
		eps    []podEpisode
		status string
		busy   bool
	}{busy: true}
	v := &View{ID: "podcast", Title: p.Name, Kind: KindFull}
	download := func(eps []podEpisode) { ui.startPodDownload(p, eps) }
	v.Build = func() []Item {
		// what is downloaded: in the record and still there (one look at
		// the folder per rebuild)
		rec := podRecord(p.Feed)
		var onDisk map[string]bool
		if len(rec.Files) > 0 {
			onDisk = dirSet(p.dir())
		}
		have := func(e podEpisode) string {
			if n := rec.Files[e.key()]; n != "" && onDisk[n] {
				return filepath.Join(p.dir(), n)
			}
			return ""
		}
		if st.busy {
			return []Item{{Label: "Loading episodes…", NoArrow: true}}
		}
		if st.status != "" {
			l, s, _ := strings.Cut(st.status, " · ")
			return []Item{{Label: l, Sublabel: s, NoArrow: true}}
		}
		var items []Item
		var latest []podEpisode
		for _, e := range st.eps {
			if len(latest) < 3 && e.playable() && have(e) == "" {
				latest = append(latest, e)
			}
		}
		if len(latest) > 1 {
			items = append(items, Item{Label: fmt.Sprintf("Download the %d Latest", len(latest)),
				Sublabel: "Episodes not downloaded yet", OnSelect: func() { download(latest) }})
		}
		for _, e := range st.eps {
			var parts []string
			if !e.Date.IsZero() {
				f := "Jan 2"
				if e.Date.Year() != time.Now().Year() {
					f = "Jan 2, 2006"
				}
				parts = append(parts, e.Date.Format(f))
			}
			if e.Secs > 0 {
				parts = append(parts, podLength(e.Secs))
			}
			path := have(e)
			it := Item{Label: e.Title}
			switch {
			case path != "":
				parts = append(parts, "Downloaded")
				it.OnSelect = func() { ui.playPodEpisode(path) }
				it.LongPress = []SheetOption{{Label: "Delete Download", Danger: true, OnSelect: func() {
					ui.deletePodFiles(p, []string{path}, "“"+e.Title+"”", func() { ui.refreshView(v) })
				}}}
			case !e.playable():
				parts = append(parts, strings.ToUpper(strings.TrimPrefix(e.ext(), "."))+" can't be played here")
				it.NoArrow = true
			default:
				if e.Size > 0 {
					parts = append(parts, formatBytes(e.Size))
				}
				it.OnSelect = func() { download([]podEpisode{e}) }
			}
			it.Sublabel = strings.Join(parts, " · ")
			items = append(items, it)
		}
		if len(items) == 0 {
			items = append(items, Item{Label: "No episodes with audio", NoArrow: true})
		}
		return items
	}
	go func() {
		fp, eps, err := fetchFeed(p.Feed)
		ui.postSure(func() {
			st.busy = false
			if err != nil {
				st.status = "Episodes not loaded · " + iaErrText(err)
			} else {
				st.eps = eps
				if fp.Art600 != "" && p.Art600 == "" {
					p.Art600 = fp.Art600
				}
				if p.Author == "" {
					p.Author = fp.Author
				}
			}
			ui.refreshView(v)
		})
	}()
	v.Items = v.Build()
	return v
}

func podLength(s float64) string {
	m := int(s/60 + 0.5)
	if m < 60 {
		return fmt.Sprintf("%d min", max(m, 1))
	}
	return fmt.Sprintf("%d h %d min", m/60, m%60)
}

// playPodEpisode plays a downloaded episode (once the library has it).
func (ui *IpodUI) playPodEpisode(path string) {
	if lib := ui.app.Library; lib != nil && lib.TracksByPath != nil {
		if t := lib.TracksByPath[path]; t != nil {
			ui.playSong([]*Track{t}, 0, true)
			return
		}
	}
	ui.showToast("Still being added to the library")
}

// ---- deleting ----

// deletePodcastEpisodes deletes what betterPod downloaded from p (its
// episodes, the unfinished ones and its cover.jpg; nothing else in the
// folder), after asking; the folder goes too if that leaves it empty.
func (ui *IpodUI) deletePodcastEpisodes(p podcast, after func()) {
	if d := iaCur; d != nil && !d.finished && d.dir == p.dir() {
		ui.showToast("Wait for the download to end")
		return
	}
	rec := podRecord(p.Feed)
	dir := p.dir()
	var paths []string
	n, size := 0, int64(0)
	onDisk := dirSet(dir)
	for _, name := range rec.Files {
		if onDisk[name] {
			paths = append(paths, filepath.Join(dir, name))
			n++
			if fi, err := os.Stat(filepath.Join(dir, name)); err == nil {
				size += fi.Size()
			}
		}
	}
	for name := range onDisk {
		if strings.HasSuffix(name, ".part") || strings.HasSuffix(name, ".part.info") {
			paths = append(paths, filepath.Join(dir, name))
		}
	}
	if rec.Cover && onDisk["cover.jpg"] {
		paths = append(paths, filepath.Join(dir, "cover.jpg"))
	}
	if len(paths) == 0 {
		ui.showToast("Nothing downloaded")
		return
	}
	what := fmt.Sprintf("the %d %s of “%s” (%s)", n, pluralize(n, "episode", "episodes"), p.Name, formatBytes(size))
	if n == 0 {
		what = "what is left of “" + p.Name + "”"
	}
	ui.deletePodFiles(p, paths, what, func() {
		os.Remove(dir) // only if empty
		if after != nil {
			after()
		}
	})
}

// deletePodFiles asks, then deletes paths (as Browse Files does: only
// inside the music folder), forgets them in p's entry, takes them out of
// the queue and scans the library again.
func (ui *IpodUI) deletePodFiles(p podcast, paths []string, what string, after func()) {
	ui.showSheetTitled("Delete "+what+"? This can't be undone.", []SheetOption{{Label: "Delete", Danger: true, OnSelect: func() {
		go func() {
			failed := removeMusicPaths(paths)
			podForgetMissing(p)
			ui.postSure(func() {
				ui.dropDeletedFromQueue(paths)
				if failed != nil {
					ui.showToast("Couldn't delete: " + shortErr(failed))
				} else {
					ui.showToast("Deleted")
				}
				logMsg(fmt.Sprintf("Podcasts: deleted %d file(s) in %s", len(paths), p.dir()))
				if after != nil {
					after()
				}
				ui.rescanLibrary()
			})
		}()
	}}})
}

// ---- downloading ----

// podJob: an episode to download and the file name it gets.
type podJob struct {
	ep   podEpisode
	name string
}

// startPodDownload downloads the episodes of eps that aren't downloaded
// yet (Try Again calls it again: the finished ones are skipped). On the UI
// goroutine.
func (ui *IpodUI) startPodDownload(p podcast, eps []podEpisode) {
	if iaCur != nil && !iaCur.finished {
		ui.showView(ui.iaDownloadView(), "")
		return
	}
	rec := podRecord(p.Feed)
	dir := p.dir()
	onDisk := dirSet(dir)
	taken := map[string]bool{}
	for n := range onDisk {
		taken[strings.ToLower(n)] = true
	}
	for _, n := range rec.Files {
		taken[strings.ToLower(n)] = true
	}
	var jobs []podJob
	var files []iaFile
	var total int64
	kinds := map[string]bool{}
	var kind []string
	for _, e := range eps {
		if !e.playable() {
			continue
		}
		if n := rec.Files[e.key()]; n != "" && onDisk[n] {
			continue // downloaded
		}
		name := freeName(e.baseName(), e.ext(), func(n string) bool { return taken[n] })
		taken[strings.ToLower(name)] = true
		jobs = append(jobs, podJob{e, name})
		files = append(files, iaFile{Name: e.URL, Title: e.Title, Size: e.Size, Length: e.Secs})
		total += e.Size
		if k := strings.ToUpper(strings.TrimPrefix(e.ext(), ".")); !kinds[k] {
			kinds[k] = true
			kind = append(kind, k)
		}
	}
	if len(jobs) == 0 {
		ui.showToast("Already downloaded")
		return
	}
	rememberPodcast(p)
	d := &iaDownload{item: &iaItem{Title: p.Name, Creator: p.Author}, files: files, format: strings.Join(kind, ", "), dir: dir,
		cancel: make(chan struct{}), state: "Starting…", source: "Podcasts", total: total}
	ui.startDownloadJob(d, func() error { return podRun(d, p, jobs) }, func() { ui.startPodDownload(p, eps) })
}

// podAttempts: how many times an episode is tried before it is skipped.
const podAttempts = 5

var errPodRetry = errors.New("retry")

// podRun downloads the episodes into the podcast's folder, checks and tags
// them, and saves the artwork as cover.jpg.
func podRun(d *iaDownload, p podcast, jobs []podJob) error {
	if err := os.MkdirAll(d.dir, 0755); err != nil {
		return err
	}
	// what a crash in the middle of tagging left
	for name := range dirSet(d.dir) {
		if strings.HasSuffix(name, ".tagtmp") || strings.HasSuffix(name, ".m4atmp") {
			os.Remove(filepath.Join(d.dir, name))
		}
	}
	var need int64
	for _, j := range jobs {
		need += j.ep.Size
	}
	if fb := freeBytes(d.dir); fb >= 0 && fb < need+50<<20 {
		return fmt.Errorf("Not enough space (%s free)", formatBytes(fb))
	}
	cover := filepath.Join(d.dir, "cover.jpg")
	if !fileExists(cover) && p.Art600 != "" {
		if err := podSaveCover(p.Art600, cover); err == nil {
			podUpdate(p, true, func(r *podcast) { r.Cover = true })
		} else {
			logMsg("Podcasts: the artwork: " + err.Error())
		}
	}
	netFails, failMsg := 0, ""
	for i, j := range jobs {
		e := j.ep
		d.mu.Lock()
		d.index, d.state = i, e.Title
		base := d.done
		d.mu.Unlock()
		part := filepath.Join(d.dir, j.name) + ".part"
		size := e.Size // the feed's, until the server says
		var err error
		for try := 1; ; try++ {
			err = podGet(d, e.URL, part, base, &size)
			if !errors.Is(err, errPodRetry) || try == podAttempts {
				break
			}
			wait := time.Duration(3<<(try-1)) * time.Second // 3, 6, 12, 24 s
			for end := time.Now().Add(wait); time.Now().Before(end); {
				d.mu.Lock()
				d.retry = fmt.Sprintf("%s · retrying in %d s (%d/%d)", d.lastErr, int(time.Until(end).Seconds()+0.99), try+1, podAttempts)
				d.mu.Unlock()
				select {
				case <-d.cancel:
					return errIACancelled
				case <-time.After(250 * time.Millisecond):
				}
			}
		}
		d.mu.Lock()
		d.retry = ""
		d.mu.Unlock()
		if err == nil {
			err = podFinish(p, e, part, j.name, cover)
		}
		switch {
		case err == nil:
			netFails = 0
			if failMsg != "" {
				d.mu.Lock()
				d.lastErr = failMsg // why the failed one failed (not a retry's error)
				d.mu.Unlock()
			}
		case errors.Is(err, errIACancelled), isDiskErr(err):
			return err
		default:
			logMsg("Podcasts: " + e.URL + ": " + err.Error())
			d.mu.Lock()
			d.failed = append(d.failed, e.Title)
			failMsg = strings.TrimPrefix(err.Error(), errPodRetry.Error()+": ")
			d.lastErr = failMsg
			d.done = base + size
			d.mu.Unlock()
			if netFails++; netFails >= 2 {
				return errors.New("Stopped: two episodes in a row failed")
			}
		}
	}
	d.mu.Lock()
	d.index = len(d.files)
	d.mu.Unlock()
	return nil
}

// podSaveCover saves the podcast's artwork as cover.jpg (checked to be an
// image; a PNG or WebP one is made JPEG, at most 1000 px).
func podSaveCover(u, path string) error {
	data, err := coverImage("The podcast's server", u)
	if err != nil {
		return err
	}
	if data == nil {
		return errors.New("no artwork")
	}
	if sniffImageExt(data) != "jpg" || len(data) > 2<<20 {
		img, _, err := image.Decode(bytes.NewReader(data))
		if err != nil {
			return err
		}
		if b := img.Bounds(); b.Dx() > 1000 || b.Dy() > 1000 {
			img = scaleImage(img, 1000, 1000*b.Dy()/max(b.Dx(), 1))
		}
		var buf bytes.Buffer
		if err := jpeg.Encode(&buf, img, &jpeg.Options{Quality: 90}); err != nil {
			return err
		}
		data = buf.Bytes()
	}
	return writeFileAtomic(path, data)
}

// podFinish makes a downloaded .part an episode: its format from its first
// bytes (the name's extension changes if the feed was wrong), its tags,
// then its name, and it's noted in p's entry.
func podFinish(p podcast, e podEpisode, part, name, cover string) error {
	ext, ok := sniffAudio(part)
	if !ok {
		os.Remove(part)
		if ext == "" {
			return errors.New("the server didn't send an audio file")
		}
		return fmt.Errorf("the episode is %s, which can't be played here", strings.ToUpper(strings.TrimPrefix(ext, ".")))
	}
	dir := filepath.Dir(part)
	if old := filepath.Ext(name); !strings.EqualFold(old, ext) {
		rec := podRecord(p.Feed)
		used := map[string]bool{}
		for _, n := range rec.Files {
			used[strings.ToLower(n)] = true
		}
		name = freeName(strings.TrimSuffix(name, old), ext, func(n string) bool {
			return used[n] || fileExists(filepath.Join(dir, n)) || fileExists(filepath.Join(dir, n+".part"))
		})
	}
	t := songTags{Title: e.Title, Artist: p.Author, Album: p.Name, AlbumArtist: p.Name, Genre: "Podcast", Track: e.Episode}
	if t.Artist == "" {
		t.Artist = p.Name
	}
	if !e.Date.IsZero() {
		t.Year = e.Date.Year()
	}
	if err := tagPart(part, ext, t, cover); err != nil {
		logMsg("Podcasts: tagging " + name + ": " + err.Error()) // kept untagged
	}
	dst := filepath.Join(dir, name)
	if err := os.Rename(part, dst); err != nil {
		return err
	}
	podUpdate(p, true, func(r *podcast) { r.Files[e.key()] = name })
	return nil
}

// tagPart writes the tags into a downloaded file still named .part: in
// place or through a new file for MP3, FLAC and Ogg (tagwrite.go), through
// a new file for M4A (ij_m4a.go, with the cover inside).
func tagPart(part, ext string, t songTags, cover string) error {
	if ext != ".m4a" {
		if !canWriteTags("x" + ext) {
			return nil
		}
		return writeTagsAs(part, ext, t)
	}
	src, err := openM4A(part)
	if err != nil {
		return err
	}
	var art []byte
	if fi, err := os.Stat(cover); err == nil && fi.Size() < 4<<20 {
		art, _ = os.ReadFile(cover)
	}
	tmp := part + ".m4atmp"
	err = src.WriteRange(tmp, 0, 0, m4aTags{Title: t.Title, Artist: t.Artist, AlbumArtist: t.AlbumArtist, Album: t.Album,
		Year: itoaPos(t.Year), Track: t.Track, Cover: art})
	src.Close()
	if err != nil {
		os.Remove(tmp)
		return err
	}
	return os.Rename(tmp, part)
}

// sniffAudio: a file's format from its first bytes, as an extension, and
// whether it plays here ("" when it isn't audio at all: an error page).
func sniffAudio(path string) (string, bool) {
	f, err := os.Open(path)
	if err != nil {
		return "", false
	}
	defer f.Close()
	b := make([]byte, 64)
	n, _ := f.ReadAt(b, 0)
	b = b[:n]
	tagged := false
	if n >= 10 && string(b[:3]) == "ID3" {
		// what follows the ID3 tag
		off := int64(10 + syncsafe(b[6:10]))
		if b[5]&0x10 != 0 {
			off += 10
		}
		b = b[:cap(b)]
		n, _ = f.ReadAt(b, off)
		b, tagged = b[:n], true
	}
	switch {
	case len(b) >= 8 && string(b[4:8]) == "ftyp":
		return ".m4a", true
	case len(b) >= 4 && string(b[:4]) == "fLaC":
		return ".flac", true
	case len(b) >= 4 && string(b[:4]) == "OggS":
		if bytes.Contains(b, []byte("OpusHead")) {
			return ".opus", false
		}
		return ".ogg", true
	case len(b) >= 12 && string(b[:4]) == "RIFF" && string(b[8:12]) == "WAVE":
		return ".wav", true
	case len(b) >= 2 && b[0] == 0xFF && b[1]&0xE0 == 0xE0:
		if (b[1]>>1)&3 == 0 {
			return ".aac", false // ADTS (raw AAC)
		}
		return ".mp3", true
	case tagged:
		return ".mp3", true // an ID3 tag, then padding or junk before the frames
	}
	return "", false
}

// podPartInfo: what tells whether a .part can be continued (name.part.info).
type podPartInfo struct {
	URL   string `json:"url"`
	ETag  string `json:"etag,omitempty"`
	Mod   string `json:"modified,omitempty"`
	Total int64  `json:"total,omitempty"`
}

// validator: the If-Range value (a weak ETag can't be one).
func (m podPartInfo) validator() string {
	if m.ETag != "" && !strings.HasPrefix(m.ETag, "W/") {
		return m.ETag
	}
	return m.Mod
}

// contentRange reads "bytes 100-999/1000" ("/*": total -1).
func contentRange(s string) (from, total int64, ok bool) {
	rest, found := strings.CutPrefix(strings.TrimSpace(s), "bytes ")
	if !found {
		return 0, 0, false
	}
	rng, tot, found := strings.Cut(rest, "/")
	a, _, found2 := strings.Cut(rng, "-")
	if !found || !found2 {
		return 0, 0, false
	}
	from, err := strconv.ParseInt(a, 10, 64)
	if err != nil {
		return 0, 0, false
	}
	total = -1
	if tot != "*" {
		if total, err = strconv.ParseInt(tot, 10, 64); err != nil {
			return 0, 0, false
		}
	}
	return from, total, true
}

// podGet downloads u into part, continuing it when the server shows it's
// the same file (else it starts again); a busy server or a dropped
// connection is errPodRetry. *size is the file's size as known (the feed's
// is often wrong): the server's replaces it, in the total too.
func podGet(d *iaDownload, u, part string, base int64, size *int64) error {
	infoPath := part + ".info"
	var have int64
	var info podPartInfo
	if st, err := os.Stat(part); err == nil {
		have = st.Size()
	}
	if have > 0 {
		b, err := os.ReadFile(infoPath)
		if err != nil || json.Unmarshal(b, &info) != nil || info.URL != u || info.validator() == "" {
			have = 0 // nothing tells it's the same file: from the start
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	var stalled atomic.Bool
	watchdog := time.AfterFunc(iaStall, func() { stalled.Store(true); cancel() })
	defer watchdog.Stop()
	go func() {
		select {
		case <-d.cancel:
			cancel()
		case <-ctx.Done():
		}
	}()
	retry := func(msg string) error {
		d.mu.Lock()
		d.lastErr = msg
		d.mu.Unlock()
		return fmt.Errorf("%w: %s", errPodRetry, msg)
	}
	restart := func(msg string) error {
		os.Remove(part)
		os.Remove(infoPath)
		return retry(msg)
	}
	req, _ := http.NewRequestWithContext(ctx, "GET", u, nil)
	req.Header.Set("User-Agent", "betterPod/"+BETTERPOD_VERSION)
	if have > 0 {
		req.Header.Set("Range", fmt.Sprintf("bytes=%d-", have))
		req.Header.Set("If-Range", info.validator())
	}
	resp, err := updateClient(0).Do(req)
	if err != nil {
		select {
		case <-d.cancel:
			return errIACancelled
		default:
		}
		if stalled.Load() {
			return retry("Connection stalled")
		}
		return retry(iaErrText(err))
	}
	defer resp.Body.Close()
	flags := os.O_WRONLY | os.O_CREATE
	switch {
	case resp.StatusCode == 206 && have > 0:
		from, total, ok := contentRange(resp.Header.Get("Content-Range"))
		if !ok || from != have || (info.Total > 0 && total > 0 && total != info.Total) {
			return restart("The episode changed on the server: starting again")
		}
		flags |= os.O_APPEND
	case resp.StatusCode == 200:
		flags |= os.O_TRUNC
		have = 0
		info = podPartInfo{URL: u, ETag: resp.Header.Get("ETag"), Mod: resp.Header.Get("Last-Modified"), Total: resp.ContentLength}
		if b, err := json.Marshal(info); err == nil {
			_ = os.WriteFile(infoPath, b, 0644)
		}
	case resp.StatusCode == 416 && have > 0:
		if info.Total > 0 && have == info.Total {
			os.Remove(infoPath)
			return nil // it was all there
		}
		return restart("The episode changed on the server: starting again")
	case resp.StatusCode == 429 || resp.StatusCode >= 500:
		return retry(fmt.Sprintf("the server answered %d", resp.StatusCode))
	default:
		return fmt.Errorf("the server answered %d (%s)", resp.StatusCode, http.StatusText(resp.StatusCode))
	}
	if resp.ContentLength > 0 {
		// the feed's size is often wrong: the server's counts
		if real := have + resp.ContentLength; real != *size {
			d.mu.Lock()
			d.total += real - *size
			d.mu.Unlock()
			*size = real
		}
	}
	out, err := os.OpenFile(part, flags, 0644)
	if err != nil {
		return err
	}
	buf := make([]byte, 64<<10)
	got := have
	for {
		n, rerr := resp.Body.Read(buf)
		if n > 0 {
			watchdog.Reset(iaStall)
			if _, err := out.Write(buf[:n]); err != nil {
				out.Close()
				return err
			}
			got += int64(n)
			d.mu.Lock()
			d.done = base + got
			d.mu.Unlock()
		}
		if rerr == io.EOF {
			break
		}
		if rerr != nil {
			out.Close() // the .part stays: the next try continues it
			select {
			case <-d.cancel:
				return errIACancelled
			default:
			}
			if stalled.Load() {
				return retry("Connection stalled")
			}
			return retry(iaErrText(rerr))
		}
	}
	if err := out.Close(); err != nil {
		return err
	}
	os.Remove(infoPath)
	return nil
}
