package main

// ij_podcasts.go — Podcasts (main menu, under Music), as on the iPod: find a podcast, see
// its episodes, download them into the library.
//
//	Podcasts                     The Rest Is History
//	  ⌕ Search podcasts             Download the 3 Latest
//	  Your Podcasts                 Episode 512 · Oct 27 · 54 min · 48 MB
//	  ▣ The Rest Is History         Episode 511 · Oct 24 · 1 h 2 min · Downloaded
//	  ▣ In Our Time
//
// The search is Apple's podcast directory (the iTunes Search API, no key);
// a podcast's episodes come from its RSS feed. An episode is downloaded
// with the Internet Archive screen (progress, time left, Cancel, Try
// Again), continued where it stopped and tried again when the connection
// drops, into Music/Podcasts/<podcast>/ as "<date> <title>.mp3" with its
// tags (tagwrite.go) and the podcast's artwork as cover.jpg, so every
// podcast is an album. A downloaded episode plays at once; Y (or holding
// A) on it deletes it, and on a podcast deletes all its episodes, asking
// first as Browse Files does. The podcasts you download from are kept
// under Your Podcasts (podcasts.json in the data folder). Episodes in a
// format the console can't play (supportedAudioExt) are shown but can't be
// downloaded. MIYOOPOD_ITUNES_BASE points the search elsewhere (tests).

import (
	"bytes"
	"context"
	"encoding/json"
	"encoding/xml"
	"errors"
	"fmt"
	"image"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"golang.org/x/text/encoding/charmap"
)

type podcast struct {
	Name   string `json:"name"`
	Author string `json:"author"`
	Feed   string `json:"feed"`
	Art    string `json:"art"`    // the artwork, about 100 px (the lists)
	Art600 string `json:"art600"` // the artwork for cover.jpg
	Genre  string `json:"genre,omitempty"`
	Count  int    `json:"-"` // episodes, as the search says
}

func (p podcast) dir() string { return filepath.Join(musicRoot(), "Podcasts", safeName(p.Name)) }

type podEpisode struct {
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

func (e podEpisode) path(dir string) string {
	name := e.Title
	if !e.Date.IsZero() {
		name = e.Date.Format("2006-01-02") + " " + name
	}
	return filepath.Join(dir, safeName(name)+e.ext())
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

// podMaxEpisodes: the newest episodes read from a feed (some list
// thousands).
const podMaxEpisodes = 200

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
	return t
}

type rssItem struct {
	Title     string `xml:"title"`
	PubDate   string `xml:"pubDate"`
	Duration  string `xml:"duration"`
	Episode   string `xml:"episode"`
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
	d.CharsetReader = func(cs string, in io.Reader) (io.Reader, error) {
		switch strings.ToLower(cs) {
		case "iso-8859-1", "latin1", "latin-1":
			return charmap.ISO8859_1.NewDecoder().Reader(in), nil
		case "windows-1252", "cp1252":
			return charmap.Windows1252.NewDecoder().Reader(in), nil
		}
		return in, nil // UTF-8 or something close enough
	}
	depth, inChannel := 0, false
	for len(eps) < podMaxEpisodes {
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
				if err := d.DecodeElement(&it, &t); err != nil {
					continue
				}
				depth--
				if it.Enclosure.URL == "" {
					continue
				}
				e := podEpisode{Title: strings.TrimSpace(it.Title), Date: podDate(it.PubDate), Secs: podDuration(it.Duration),
					URL: strings.TrimSpace(it.Enclosure.URL), Type: it.Enclosure.Type}
				e.Size, _ = strconv.ParseInt(strings.TrimSpace(it.Enclosure.Length), 10, 64)
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
	return p, eps, nil
}

func fetchFeed(feed string) (podcast, []podEpisode, error) {
	resp, err := coverGetAccept(feed, "application/rss+xml, application/xml, text/xml, */*", 30*time.Second)
	if err != nil {
		return podcast{}, nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return podcast{}, nil, fmt.Errorf("the podcast's server answered %d", resp.StatusCode)
	}
	return parseFeed(io.LimitReader(resp.Body, 32<<20))
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

func rememberPodcast(p podcast) {
	setSavedPodcasts(func(l []podcast) []podcast {
		for i, x := range l {
			if x.Feed == p.Feed {
				l[i] = p
				return l
			}
		}
		return append([]podcast{p}, l...)
	})
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
		gen          atomic.Int32
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
		if fileExists(p.dir()) {
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
		ui.podThumbs(v, as, us, &st.gen)
	}
	v.Build = func() []Item {
		note := ""
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
		saved := savedPodcasts()
		if len(saved) == 0 {
			return append(items, Item{Label: "Find a podcast", Sublabel: "Search above; the ones you download from are kept here", NoArrow: true})
		}
		items = append(items, Item{Label: "Your Podcasts", NoArrow: true})
		for _, p := range saved {
			it := row(p)
			it.LongPress = append(it.LongPress, SheetOption{Label: "Remove from Your Podcasts", Danger: true, OnSelect: func() {
				forgetPodcast(p.Feed)
				ui.refreshView(v)
			}})
			items = append(items, it)
		}
		return items
	}
	run = func() {
		term := strings.TrimSpace(st.term)
		if term == "" {
			st.searched, st.res, st.status = false, nil, ""
			ui.refreshView(v)
			return
		}
		g := st.gen.Add(1)
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
					st.status = "No answer · " + iaErrText(err) + " · press to retry"
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
	download := func(eps []podEpisode) {
		var todo []podEpisode
		for _, e := range eps {
			if e.playable() && !fileExists(e.path(p.dir())) {
				todo = append(todo, e)
			}
		}
		if len(todo) > 0 {
			rememberPodcast(p)
			ui.startPodDownload(p, todo)
		}
	}
	v.Build = func() []Item {
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
			if len(latest) < 3 && e.playable() && !fileExists(e.path(p.dir())) {
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
			path := e.path(p.dir())
			it := Item{Label: e.Title}
			switch {
			case fileExists(path):
				parts = append(parts, "Downloaded")
				it.OnSelect = func() { ui.playPodEpisode(path) }
				it.LongPress = []SheetOption{{Label: "Delete Download", Danger: true, OnSelect: func() {
					ui.deletePodFiles([]string{path}, "“"+e.Title+"”", func() { ui.refreshView(v) })
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

// podDownloads: the episodes downloaded from p, and their size.
func podDownloads(p podcast) (n int, size int64) {
	es, _ := os.ReadDir(p.dir())
	for _, e := range es {
		switch strings.ToLower(filepath.Ext(e.Name())) {
		case ".mp3", ".m4a", ".ogg", ".oga", ".flac", ".opus":
			if !e.IsDir() {
				n++
				if fi, err := e.Info(); err == nil {
					size += fi.Size()
				}
			}
		}
	}
	return n, size
}

// deletePodcastEpisodes deletes the podcast's folder (its episodes, the
// unfinished ones and the artwork), after asking.
func (ui *IpodUI) deletePodcastEpisodes(p podcast, after func()) {
	if d := iaCur; d != nil && !d.finished && d.dir == p.dir() {
		ui.showToast("Wait for the download to end")
		return
	}
	n, size := podDownloads(p)
	what := fmt.Sprintf("the %d %s of “%s” (%s)", n, pluralize(n, "episode", "episodes"), p.Name, formatBytes(size))
	if n == 0 {
		what = "what is left of “" + p.Name + "”"
	}
	ui.deletePodFiles([]string{p.dir()}, what, after)
}

// deletePodFiles asks, then deletes paths (only inside the music folder,
// as Browse Files), takes them out of the queue and scans the library
// again.
func (ui *IpodUI) deletePodFiles(paths []string, what string, after func()) {
	ui.showSheetTitled("Delete "+what+"? This can't be undone.", []SheetOption{{Label: "Delete", Danger: true, OnSelect: func() {
		go func() {
			var failed error
			for _, p := range paths {
				if !insideMusic(p) {
					failed = errors.New("outside the music folder")
					continue
				}
				if err := os.RemoveAll(p); err != nil && failed == nil {
					failed = err
				}
				forgetDirInfo(p)
			}
			ui.postSure(func() {
				// the one playing goes on: the player has it open
				ui.app.remapQueueWith(func(t *Track) *Track {
					for _, p := range paths {
						if t.Path == p || strings.HasPrefix(t.Path, p+string(filepath.Separator)) {
							return nil
						}
					}
					return t
				})
				if failed != nil {
					ui.showToast("Couldn't delete: " + shortErr(failed))
				} else {
					ui.showToast("Deleted")
				}
				logMsg(fmt.Sprintf("Podcasts: deleted %s", strings.Join(paths, ", ")))
				ui.rescanLibrary()
				if after != nil {
					after()
				}
			})
		}()
	}}})
}

// ---- downloading ----

func (ui *IpodUI) startPodDownload(p podcast, eps []podEpisode) {
	if iaCur != nil && !iaCur.finished {
		ui.showView(ui.iaDownloadView(), "")
		return
	}
	var files []iaFile
	var total int64
	for _, e := range eps {
		files = append(files, iaFile{Name: e.URL, Title: e.Title, Size: e.Size, Length: e.Secs})
		total += e.Size
	}
	d := &iaDownload{item: &iaItem{Title: p.Name, Creator: p.Author}, files: files, format: "MP3", dir: p.dir(),
		cancel: make(chan struct{}), state: "Starting…", source: "Podcasts", total: total}
	ui.startDownloadJob(d, func() error { return podRun(d, p, eps) }, func() { ui.startPodDownload(p, eps) })
}

// podAttempts: how many times an episode is tried before it is skipped.
const podAttempts = 5

var errPodRetry = errors.New("retry")

// podRun downloads the episodes into the podcast's folder, tags them and
// saves the artwork as cover.jpg.
func podRun(d *iaDownload, p podcast, eps []podEpisode) error {
	if err := os.MkdirAll(d.dir, 0755); err != nil {
		return err
	}
	var need int64
	for _, e := range eps {
		need += e.Size
	}
	if fb := freeBytes(d.dir); fb >= 0 && need > 0 && fb < need+50<<20 {
		return fmt.Errorf("Not enough space (%s free)", formatBytes(fb))
	}
	if c := filepath.Join(d.dir, "cover.jpg"); !fileExists(c) && p.Art600 != "" {
		var none int64
		_ = podGet(d, p.Art600, c, false, 0, &none) // best effort
	}
	netFails := 0
	for i, e := range eps {
		d.mu.Lock()
		d.index, d.state = i, e.Title
		base := d.done
		d.mu.Unlock()
		dst := e.path(d.dir)
		size := e.Size // the feed's, until the server says
		var err error
		for try := 1; try <= podAttempts; try++ {
			err = podGet(d, e.URL, dst, true, base, &size)
			if !errors.Is(err, errPodRetry) {
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
		switch {
		case err == nil:
			netFails = 0
			t := songTags{Title: e.Title, Artist: p.Author, Album: p.Name, AlbumArtist: p.Name, Genre: "Podcast", Track: e.Episode}
			if t.Artist == "" {
				t.Artist = p.Name
			}
			if !e.Date.IsZero() {
				t.Year = e.Date.Year()
			}
			if canWriteTags(dst) {
				if err := writeTags(dst, t); err != nil {
					logMsg("Podcasts: tagging " + dst + ": " + err.Error())
				}
			}
		case errors.Is(err, errIACancelled), isDiskErr(err):
			return err
		default:
			logMsg("Podcasts: " + e.URL + ": " + err.Error())
			d.mu.Lock()
			d.failed = append(d.failed, e.Title)
			d.lastErr = strings.TrimPrefix(err.Error(), errPodRetry.Error()+": ")
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

// podGet downloads u into dst through dst.part, continuing a .part left
// by an earlier try; a busy server or a dropped connection is errPodRetry.
// *size is the file's size as known (the feed's is often wrong): the
// server's replaces it, in the total too.
func podGet(d *iaDownload, u, dst string, count bool, base int64, size *int64) error {
	part := dst + ".part"
	var have int64
	if st, err := os.Stat(part); err == nil {
		have = st.Size()
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
	req, _ := http.NewRequestWithContext(ctx, "GET", u, nil)
	req.Header.Set("User-Agent", "KnulliPod/"+KNULLIPOD_VERSION)
	if have > 0 {
		req.Header.Set("Range", fmt.Sprintf("bytes=%d-", have))
	}
	resp, err := updateClient(0).Do(req)
	if err != nil {
		select {
		case <-d.cancel:
			return errIACancelled
		default:
		}
		return retry(iaErrText(err))
	}
	defer resp.Body.Close()
	flags := os.O_WRONLY | os.O_CREATE
	switch {
	case resp.StatusCode == 206 && have > 0 && strings.HasPrefix(resp.Header.Get("Content-Range"), fmt.Sprintf("bytes %d-", have)):
		flags |= os.O_APPEND
	case resp.StatusCode == 200:
		flags |= os.O_TRUNC
		have = 0
	case resp.StatusCode == 416 && have > 0:
		return os.Rename(part, dst) // nothing after the .part's end
	case resp.StatusCode == 429 || resp.StatusCode >= 500:
		return retry(fmt.Sprintf("the server answered %d", resp.StatusCode))
	default:
		return fmt.Errorf("the server answered %d (%s)", resp.StatusCode, http.StatusText(resp.StatusCode))
	}
	if count && resp.ContentLength > 0 {
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
			if count {
				d.mu.Lock()
				d.done = base + got
				d.mu.Unlock()
			}
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
	return os.Rename(part, dst)
}
