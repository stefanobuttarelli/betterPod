package main

// ij_suno.go — Music → Search → Suno: the public songs of a Suno profile,
// or a song (or playlist) by its code, to listen to and download into the
// music folder.
//
// Suno has no public API. Its web player's own endpoints answer without an
// account for public songs (the same ones the open-source downloaders use):
//
//	profile   /api/profiles/<handle>?page=N&clips_sort_by=created_at
//	song      /api/clip/<id>
//	playlist  /api/playlist/<id>?page=N
//
// and each song's audio_url is a plain MP3. They may change without notice:
// every request says what went wrong on its row and can be tried again.
// A song code is what a share link ends with (suno.com/s/<code>, which
// redirects to suno.com/song/<id>), or the song's id itself; a console
// can't paste, so it is typed.
//
// Each song is a single: <music>/Suno/<artist>/<title>/<title>.mp3 with its
// own cover.jpg, tagged (title, artist, the title as album, the first style
// as genre, the year). Downloads are noted in suno.json in the database
// folder (song id → file), which is how a song shows as downloaded.

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// sunoAPI: Suno's web API (MIYOOPOD_SUNO_BASE points elsewhere, for tests);
// sunoSite: suno.com, for share links (MIYOOPOD_SUNO_SITE).
func sunoAPI() string  { return envBase("MIYOOPOD_SUNO_BASE", "https://studio-api.prod.suno.com") }
func sunoSite() string { return envBase("MIYOOPOD_SUNO_SITE", "https://suno.com") }

// sunoClip: one song.
type sunoClip struct {
	ID, Title, Artist, Handle string
	Audio, Image, ImageLarge  string
	Style                     string // its styles, as Suno shows them ("dreamy synthwave, female vocals")
	Duration                  float64
	Year                      int
}

// sunoClipJSON: what Suno says of a song (only what betterPod reads).
type sunoClipJSON struct {
	ID          string `json:"id"`
	Title       string `json:"title"`
	AudioURL    string `json:"audio_url"`
	ImageURL    string `json:"image_url"`
	ImageLarge  string `json:"image_large_url"`
	DisplayName string `json:"display_name"`
	Handle      string `json:"handle"`
	Status      string `json:"status"`
	CreatedAt   string `json:"created_at"`
	DisplayTags string `json:"display_tags"`
	Metadata    struct {
		Tags     string  `json:"tags"`
		Duration float64 `json:"duration"`
	} `json:"metadata"`
}

func (j sunoClipJSON) clip() (sunoClip, bool) {
	if j.ID == "" || (j.Status != "" && j.Status != "complete" && j.Status != "streaming") {
		return sunoClip{}, false // still being made, or failed
	}
	c := sunoClip{ID: j.ID, Title: strings.TrimSpace(j.Title), Artist: strings.TrimSpace(j.DisplayName), Handle: j.Handle,
		Audio: j.AudioURL, Image: j.ImageURL, ImageLarge: j.ImageLarge, Duration: j.Metadata.Duration}
	// a song still streaming has an m3u8 address: its MP3 is at the CDN
	if c.Audio == "" || !strings.Contains(strings.ToLower(c.Audio), ".mp3") {
		c.Audio = "https://cdn1.suno.ai/" + j.ID + ".mp3"
	}
	if c.ImageLarge == "" {
		c.ImageLarge = c.Image
	}
	if c.Title == "" {
		c.Title = "Untitled"
	}
	if c.Artist == "" {
		c.Artist = c.Handle
	}
	if c.Artist == "" {
		c.Artist = "Unknown Artist"
	}
	c.Style = strings.TrimSpace(j.DisplayTags)
	if c.Style == "" {
		c.Style = strings.TrimSpace(j.Metadata.Tags)
	}
	if len(j.CreatedAt) >= 4 {
		c.Year, _ = strconv.Atoi(j.CreatedAt[:4])
	}
	return c, true
}

// genre: the first of a song's styles, capitalized ("Dreamy synthwave").
func (c sunoClip) genre() string {
	g, _, _ := strings.Cut(c.Style, ",")
	g = strings.TrimSpace(g)
	if r := []rune(g); len(r) > 0 {
		if len(r) > 40 {
			r = r[:40]
		}
		g = strings.ToUpper(string(r[0])) + string(r[1:])
	}
	return g
}

// sunoPage: one page of a list of songs.
type sunoPage struct {
	Title string // the profile's or playlist's name ("" for one song)
	Clips []sunoClip
	Total int  // songs in all (0: unknown)
	More  bool // there may be another page
}

// sunoWait keeps Suno's API to a request every 1.5 s (it answers 429 to
// clients in a hurry).
var (
	sunoWaitMu sync.Mutex
	sunoLast   time.Time
)

func sunoWait() {
	sunoWaitMu.Lock()
	defer sunoWaitMu.Unlock()
	if d := 1500*time.Millisecond - time.Since(sunoLast); d > 0 {
		time.Sleep(d)
	}
	sunoLast = time.Now()
}

var errSunoNotFound = errors.New("not found")

// sunoGet: a JSON answer from Suno's API; a 429 is waited out (10, 15, 20 s).
func sunoGet(path string, v any) error {
	for try := 0; ; try++ {
		sunoWait()
		req, _ := http.NewRequest("GET", sunoAPI()+path, nil)
		req.Header.Set("User-Agent", "betterPod/"+BETTERPOD_VERSION)
		req.Header.Set("Accept", "application/json")
		resp, err := updateClient(20 * time.Second).Do(req)
		if err != nil {
			return err
		}
		switch {
		case resp.StatusCode == 429 && try < 3:
			resp.Body.Close()
			time.Sleep(time.Duration(10+5*try) * time.Second)
			continue
		case resp.StatusCode == 404 || resp.StatusCode == 422:
			resp.Body.Close()
			return errSunoNotFound
		case resp.StatusCode != 200:
			resp.Body.Close()
			return fmt.Errorf("Suno answered %d", resp.StatusCode)
		}
		err = json.NewDecoder(io.LimitReader(resp.Body, 8<<20)).Decode(v)
		resp.Body.Close()
		return err
	}
}

// sunoProfile: page (from 1) of a profile's public songs, newest first.
func sunoProfile(handle string, page int) (sunoPage, error) {
	var out struct {
		DisplayName string         `json:"display_name"`
		Handle      string         `json:"handle"`
		Clips       []sunoClipJSON `json:"clips"`
		Total       int            `json:"num_total_clips"`
	}
	err := sunoGet(fmt.Sprintf("/api/profiles/%s?page=%d&playlists_sort_by=created_at&clips_sort_by=created_at",
		url.PathEscape(handle), page), &out)
	if err != nil {
		return sunoPage{}, err
	}
	p := sunoPage{Title: out.DisplayName, Total: out.Total, More: len(out.Clips) > 0}
	if p.Title == "" {
		p.Title = "@" + handle
	}
	for _, j := range out.Clips {
		if c, ok := j.clip(); ok {
			p.Clips = append(p.Clips, c)
		}
	}
	return p, nil
}

// sunoSong: one song by its id.
func sunoSong(id string) (sunoPage, error) {
	var j sunoClipJSON
	if err := sunoGet("/api/clip/"+url.PathEscape(id), &j); err != nil {
		return sunoPage{}, err
	}
	c, ok := j.clip()
	if !ok {
		return sunoPage{}, errors.New("This song isn't finished")
	}
	return sunoPage{Title: c.Title, Clips: []sunoClip{c}, Total: 1}, nil
}

// sunoPlaylist: page (from 1) of a playlist.
func sunoPlaylist(id string, page int) (sunoPage, error) {
	var out struct {
		Name  string `json:"name"`
		Clips []struct {
			Clip sunoClipJSON `json:"clip"`
		} `json:"playlist_clips"`
		Total int `json:"num_total_results"`
	}
	if err := sunoGet(fmt.Sprintf("/api/playlist/%s?page=%d", url.PathEscape(id), page), &out); err != nil {
		return sunoPage{}, err
	}
	p := sunoPage{Title: out.Name, Total: out.Total, More: len(out.Clips) > 0}
	for _, c := range out.Clips {
		if cl, ok := c.Clip.clip(); ok {
			p.Clips = append(p.Clips, cl)
		}
	}
	if p.Total > 0 && page*len(out.Clips) >= p.Total {
		p.More = false
	}
	return p, nil
}

var (
	sunoUUID      = regexp.MustCompile(`(?i)\b([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\b`)
	sunoHex32     = regexp.MustCompile(`(?i)^[0-9a-f]{32}$`)
	sunoShortCode = regexp.MustCompile(`^[A-Za-z0-9]{4,40}$`)
	sunoKindID    = regexp.MustCompile(`(?i)/(song|playlist)/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})`)
)

// sunoTarget: what a typed code points to.
type sunoTarget struct {
	Playlist bool
	ID       string
}

// sunoParseCode reads what was typed: a link (suno.com/song/<id>,
// /playlist/<id>, /s/<code>), a song's id (with or without its dashes), or
// a share code. short is the share code to look up ("" when the id is
// there already).
func sunoParseCode(s string) (t sunoTarget, short string, ok bool) {
	s = strings.TrimSpace(s)
	if m := sunoKindID.FindStringSubmatch(s); m != nil {
		return sunoTarget{Playlist: strings.EqualFold(m[1], "playlist"), ID: strings.ToLower(m[2])}, "", true
	}
	if m := sunoUUID.FindStringSubmatch(s); m != nil {
		return sunoTarget{ID: strings.ToLower(m[1])}, "", true
	}
	if sunoHex32.MatchString(s) {
		h := strings.ToLower(s)
		return sunoTarget{ID: h[:8] + "-" + h[8:12] + "-" + h[12:16] + "-" + h[16:20] + "-" + h[20:]}, "", true
	}
	if i := strings.LastIndex(s, "/s/"); i >= 0 {
		s = s[i+3:]
	}
	s = strings.TrimRight(strings.SplitN(s, "?", 2)[0], "/")
	if sunoShortCode.MatchString(s) {
		return sunoTarget{}, s, true
	}
	return sunoTarget{}, "", false
}

// sunoResolve: where a share code leads (suno.com/s/<code> redirects to the
// song's or playlist's page; its address, or failing that the page, says
// the id).
func sunoResolve(code string) (sunoTarget, error) {
	c := *updateClient(20 * time.Second)
	c.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	u := sunoSite() + "/s/" + url.PathEscape(code)
	for hop := 0; hop < 5; hop++ {
		req, _ := http.NewRequest("GET", u, nil)
		req.Header.Set("User-Agent", "Mozilla/5.0 (betterPod/"+BETTERPOD_VERSION+")")
		resp, err := c.Do(req)
		if err != nil {
			return sunoTarget{}, err
		}
		loc := resp.Header.Get("Location")
		if resp.StatusCode >= 300 && resp.StatusCode < 400 && loc != "" {
			resp.Body.Close()
			if m := sunoKindID.FindStringSubmatch(loc); m != nil {
				return sunoTarget{Playlist: strings.EqualFold(m[1], "playlist"), ID: strings.ToLower(m[2])}, nil
			}
			next, err := url.Parse(loc)
			if err != nil {
				break
			}
			u = req.URL.ResolveReference(next).String()
			continue
		}
		body, _ := io.ReadAll(io.LimitReader(resp.Body, 2<<20))
		resp.Body.Close()
		if resp.StatusCode == 404 {
			return sunoTarget{}, errSunoNotFound
		}
		if resp.StatusCode != 200 {
			return sunoTarget{}, fmt.Errorf("suno.com answered %d", resp.StatusCode)
		}
		if m := sunoKindID.FindStringSubmatch(string(body)); m != nil {
			return sunoTarget{Playlist: strings.EqualFold(m[1], "playlist"), ID: strings.ToLower(m[2])}, nil
		}
		break
	}
	return sunoTarget{}, errSunoNotFound
}

// sunoHandle: a typed profile name, without "@" or the link around it.
func sunoHandle(s string) string {
	s = strings.TrimSpace(s)
	if i := strings.LastIndex(s, "@"); i >= 0 {
		s = s[i+1:]
	}
	s = strings.SplitN(strings.SplitN(s, "/", 2)[0], "?", 2)[0]
	return strings.TrimSpace(s)
}

func sunoErrText(err error) string {
	if errors.Is(err, errSunoNotFound) {
		return "Not found on Suno"
	}
	return iaErrText(err)
}

// ---- what was downloaded ----

var sunoRecMu sync.Mutex

func sunoRecPath() string { return filepath.Join(dataDir(), "suno.json") }

// sunoRecord: song id → its file, relative to the music folder.
func sunoRecord() map[string]string {
	sunoRecMu.Lock()
	defer sunoRecMu.Unlock()
	m := map[string]string{}
	if b, err := os.ReadFile(sunoRecPath()); err == nil {
		_ = json.Unmarshal(b, &m)
	}
	return m
}

func sunoNote(id, path string) {
	sunoRecMu.Lock()
	defer sunoRecMu.Unlock()
	m := map[string]string{}
	if b, err := os.ReadFile(sunoRecPath()); err == nil {
		_ = json.Unmarshal(b, &m)
	}
	if rel, err := filepath.Rel(musicRoot(), path); err == nil {
		path = rel
	}
	m[id] = path
	if b, err := json.MarshalIndent(m, "", " "); err == nil {
		_ = writeFileAtomic(sunoRecPath(), b)
	}
}

// sunoHave: the song is downloaded (its file is still there).
func sunoHave(rec map[string]string, id string) bool {
	p := rec[id]
	if p == "" {
		return false
	}
	if !filepath.IsAbs(p) {
		p = filepath.Join(musicRoot(), p)
	}
	return fileExists(p)
}

// ---- views ----

// sunoView: Profile and Song Code (each typed on the keyboard), and the
// recent ones.
func (ui *IpodUI) sunoView() *View {
	st := &struct{ profile, code, err string }{}
	v := &View{ID: "suno", Title: "Suno", Kind: KindFull}
	var openProfile, openCode func()
	open := func(term string) {
		if h, ok := strings.CutPrefix(term, "@"); ok {
			st.profile = h
			openProfile()
		} else {
			st.code = term
			openCode()
		}
	}
	v.Build = func() []Item {
		items := []Item{
			ui.searchField(&st.profile, "Profile (the name after @)", "", func() {
				ui.showKeyboard(&st.profile, func() { ui.refreshView(v) }, openProfile)
				ui.refreshView(v)
			}, nil),
			ui.searchField(&st.code, "Song code (suno.com/s/…)", "", func() {
				ui.showKeyboard(&st.code, func() { ui.refreshView(v) }, openCode)
				ui.refreshView(v)
			}, nil),
		}
		if st.err != "" {
			items = append(items, Item{Label: st.err, NoArrow: true})
		}
		if rs := ui.recentItems("suno", open, func() { ui.refreshView(v) }); len(rs) > 0 {
			items = append(items, Item{Label: "Recent", Header: true})
			for i := range rs {
				if h, ok := strings.CutPrefix(rs[i].Label, "@"); ok {
					rs[i].Sublabel = strings.Replace(rs[i].Sublabel, "Recent search", "Profile", 1)
					rs[i].Label = "@" + h
				} else {
					rs[i].Sublabel = strings.Replace(rs[i].Sublabel, "Recent search", "Song code", 1)
				}
			}
			items = append(items, rs...)
		}
		return items
	}
	openProfile = func() {
		h := sunoHandle(st.profile)
		if h == "" {
			return
		}
		st.profile, st.err = h, ""
		addRecentSearch("suno", "@"+h)
		ui.refreshView(v)
		ui.showView(ui.sunoListView("@"+h, "suno:@"+strings.ToLower(h), func(page int) (sunoPage, error) {
			return sunoProfile(h, page)
		}), "")
	}
	openCode = func() {
		t, short, ok := sunoParseCode(st.code)
		if !ok {
			if strings.TrimSpace(st.code) != "" {
				st.err = "That isn't a song code: letters and digits, as after suno.com/s/"
				ui.refreshView(v)
			}
			return
		}
		st.code, st.err = strings.TrimSpace(st.code), ""
		addRecentSearch("suno", st.code)
		ui.refreshView(v)
		target := func() (sunoTarget, error) {
			if short == "" {
				return t, nil
			}
			return sunoResolve(short)
		}
		var once sync.Once
		var tg sunoTarget
		var terr error
		ui.showView(ui.sunoListView(st.code, "suno:"+st.code, func(page int) (sunoPage, error) {
			once.Do(func() { tg, terr = target() })
			if terr != nil {
				return sunoPage{}, terr
			}
			if tg.Playlist {
				return sunoPlaylist(tg.ID, page)
			}
			if page > 1 {
				return sunoPage{}, nil
			}
			return sunoSong(tg.ID)
		}), "")
	}
	v.Items = v.Build()
	if len(recentSearches("suno")) == 0 {
		ui.showKeyboard(&st.profile, func() { ui.refreshView(v) }, openProfile)
	}
	return v
}

// sunoListView: songs (a profile's, a playlist's, or one), loaded a page
// at a time: Listen (A plays a song on its row), Download All, and each
// song (Y: download just that one).
func (ui *IpodUI) sunoListView(title, key string, load func(page int) (sunoPage, error)) *View {
	st := &struct {
		clips  []sunoClip
		total  int
		page   int
		more   bool
		busy   bool
		err    string
		thumbs map[string]*Album
		live   atomic.Bool
		rec    map[string]string
	}{thumbs: map[string]*Album{}, more: true, rec: sunoRecord()}
	st.live.Store(true)
	v := &View{ID: "sunoList", Title: title, Kind: KindFull}
	v.onClose = func() { st.live.Store(false); ui.previewStop() }
	var next func()
	refresh := func() { ui.refreshView(v) }
	v.Build = func() []Item {
		var items []Item
		n := len(st.clips)
		songs := make([]listenSong, n)
		for i, c := range st.clips {
			songs[i] = sunoListenSong(c)
		}
		pkey := fmt.Sprintf("%s:%d", key, n) // (more songs: another preview)
		if n > 1 {
			sub := fmt.Sprintf("%d %s, before you download", n, pluralize(n, "song", "songs"))
			items = append(items, ui.previewRow(pkey, songs, 0, "Listen", sub, refresh))
		}
		if n > 0 {
			var left []sunoClip
			for _, c := range st.clips {
				if !sunoHave(st.rec, c.ID) {
					left = append(left, c)
				}
			}
			label, sub := "Download All", fmt.Sprintf("%d %s · MP3", len(left), pluralize(len(left), "song", "songs"))
			if n == 1 {
				label, sub = "Download", "MP3 · "+formatTrackLength(st.clips[0].Duration)
			}
			if st.more && n > 1 {
				sub += " (the ones shown)"
			}
			row := Item{Label: label, Sublabel: sub, OnSelect: func() {
				ui.previewStop()
				ui.startSunoDownload(left, v.Title, func() { st.rec = sunoRecord() })
			}}
			if len(left) == 0 {
				row = Item{Label: "Downloaded", Sublabel: "In your library", NoArrow: true}
				if n > 1 {
					row.Label = "All Downloaded"
				}
			}
			items = append(items, row)
		}
		for i, c := range st.clips {
			c := c
			th := st.thumbs[c.ID]
			if th == nil {
				th = &Album{Name: c.Title, Artist: c.Artist}
				st.thumbs[c.ID] = th
			}
			sub := c.Style
			if strings.HasPrefix(key, "suno:@") || sub == "" {
				// on a profile, the artist is known: the styles; elsewhere who made it
				sub = strings.Trim(c.Artist+" · "+c.Style, " ·")
				if strings.HasPrefix(key, "suno:@") {
					sub = c.Style
				}
			}
			if sunoHave(st.rec, c.ID) {
				sub = strings.Trim("Downloaded · "+sub, " ·")
			}
			row := ui.previewRow(pkey, songs, i, c.Title, sub, refresh)
			row.Album = th
			if p := previewState(pkey); !p.ok || p.cur != i {
				row.Value = formatTrackLength(c.Duration)
			}
			row.LongPress = []SheetOption{{Label: "Download This Song", OnSelect: func() {
				ui.previewStop()
				ui.startSunoDownload([]sunoClip{c}, c.Title, func() { st.rec = sunoRecord() })
			}}}
			items = append(items, row)
		}
		switch {
		case st.busy:
			items = append(items, Item{Label: "Loading…", NoArrow: true})
		case st.err != "":
			items = append(items, Item{Label: st.err, Sublabel: "Press [A] to try again", OnSelect: next})
		case n == 0:
			items = append(items, Item{Label: "No public songs", NoArrow: true})
		case st.more && n > 1:
			sub := fmt.Sprintf("%d shown", n)
			if st.total > n {
				sub = fmt.Sprintf("%d of %d shown", n, st.total)
			}
			items = append(items, Item{Label: "More Songs…", Sublabel: sub, OnSelect: next})
		}
		return items
	}
	next = func() {
		if st.busy || !st.more {
			return
		}
		st.busy, st.err = true, ""
		page := st.page + 1
		ui.refreshView(v)
		go func() {
			p, err := load(page)
			ui.postSure(func() {
				st.busy = false
				if err != nil {
					st.err = sunoErrText(err)
					ui.refreshView(v)
					return
				}
				first := len(st.clips) == 0
				seen := map[string]bool{}
				for _, c := range st.clips {
					seen[c.ID] = true
				}
				var added []sunoClip
				for _, c := range p.Clips {
					if !seen[c.ID] {
						seen[c.ID] = true
						added = append(added, c)
					}
				}
				st.clips = append(st.clips, added...)
				st.page, st.total = page, p.Total
				st.more = p.More && len(added) > 0 && (p.Total == 0 || len(st.clips) < p.Total)
				if p.Title != "" && page == 1 {
					v.Title = p.Title
					if ui.topScreen() == v {
						ui.headerTitle = p.Title
					}
				}
				var jobs []thumbJob
				for _, c := range added {
					th := st.thumbs[c.ID]
					if th == nil {
						th = &Album{Name: c.Title, Artist: c.Artist}
						st.thumbs[c.ID] = th
					}
					if c.Image != "" {
						jobs = append(jobs, thumbJob{th, c.Image})
					}
				}
				ui.loadThumbs(v, "Suno", jobs, st.live.Load)
				idx := v.Index
				ui.refreshView(v)
				if first && len(v.Items) > 0 {
					v.Index = 0
				} else {
					v.Index = idx
				}
			})
		}()
	}
	v.Items = v.Build()
	next()
	return v
}

// sunoListenSong: a song for Listen (its MP3, fetched whole first).
func sunoListenSong(c sunoClip) listenSong {
	return listenSong{Title: c.Title, Duration: c.Duration, Ext: ".mp3",
		fetch: func(ctx context.Context, dst string, progress func(int64, int64)) error {
			return httpFetch(ctx, c.Audio, dst, progress)
		}}
}

// ---- download ----

// sunoDir: the folder of a song (its own: a single, with its cover).
func sunoDir(c sunoClip) string {
	return filepath.Join(musicRoot(), "Suno", safeName(c.Artist), safeName(c.Title))
}

// sunoEstimate: a song's size before the server says (Suno's MP3s are
// about 192 kbit/s).
func sunoEstimate(c sunoClip) int64 {
	if c.Duration > 0 {
		return int64(c.Duration * 24000)
	}
	return 4 << 20
}

func (ui *IpodUI) startSunoDownload(clips []sunoClip, title string, done func()) {
	if iaCur != nil && !iaCur.finished {
		ui.showView(ui.iaDownloadView(), "")
		return
	}
	if len(clips) == 0 {
		ui.showToast("Already downloaded")
		return
	}
	var files []iaFile
	var total int64
	artist := clips[0].Artist
	for _, c := range clips {
		files = append(files, iaFile{Name: c.Audio, Title: c.Title, Size: sunoEstimate(c), Length: c.Duration})
		total += sunoEstimate(c)
		if c.Artist != artist {
			artist = "Various Artists"
		}
	}
	if title == artist || title == "" {
		title = fmt.Sprintf("%d %s", len(clips), pluralize(len(clips), "song", "songs")) // (a profile: its name is the artist)
	}
	d := &iaDownload{item: &iaItem{Title: title, Creator: artist}, files: files, format: "MP3",
		dir: filepath.Join(musicRoot(), "Suno"), cancel: make(chan struct{}), state: "Starting…", source: "Suno", total: total}
	ui.startDownloadJob(d, func() error {
		err := sunoRun(d, clips)
		ui.post(done)
		return err
	}, func() { ui.startSunoDownload(clips, title, done) })
}

// sunoRun downloads the songs, each into its folder with its cover, tagged.
func sunoRun(d *iaDownload, clips []sunoClip) error {
	root := filepath.Join(musicRoot(), "Suno")
	if err := os.MkdirAll(root, 0755); err != nil {
		return err
	}
	var need int64
	for _, c := range clips {
		need += sunoEstimate(c)
	}
	if fb := freeBytes(root); fb >= 0 && fb < need+50<<20 {
		return fmt.Errorf("Not enough space (%s free)", formatBytes(fb))
	}
	netFails, failMsg := 0, ""
	for i, c := range clips {
		d.mu.Lock()
		d.index, d.state = i, c.Title
		base := d.done
		d.mu.Unlock()
		size := sunoEstimate(c)
		err := sunoOne(d, c, base, &size)
		switch {
		case err == nil:
			netFails = 0
			if failMsg != "" {
				d.mu.Lock()
				d.lastErr = failMsg
				d.mu.Unlock()
			}
		case errors.Is(err, errIACancelled), isDiskErr(err):
			return err
		default:
			logMsg("Suno: " + c.ID + ": " + err.Error())
			d.mu.Lock()
			d.failed = append(d.failed, c.Title)
			failMsg = strings.TrimPrefix(err.Error(), errPodRetry.Error()+": ")
			d.lastErr = failMsg
			d.done = base + size
			d.mu.Unlock()
			if netFails++; netFails >= 2 {
				return errors.New("Stopped: two songs in a row failed")
			}
		}
	}
	d.mu.Lock()
	d.index = len(d.files)
	d.mu.Unlock()
	return nil
}

// sunoOne: one song into its folder (another song of the same name and
// artist gets "Title (2)"), its cover first, then the MP3, tagged.
func sunoOne(d *iaDownload, c sunoClip, base int64, size *int64) error {
	rec := sunoRecord()
	if sunoHave(rec, c.ID) {
		d.mu.Lock()
		d.done = base + *size
		d.mu.Unlock()
		return nil
	}
	// the folder: this song's (a .part of it, from an earlier try), or a
	// free one
	mine := map[string]bool{}
	for id, p := range rec {
		if id != c.ID {
			if !filepath.IsAbs(p) {
				p = filepath.Join(musicRoot(), p)
			}
			mine[strings.ToLower(filepath.Dir(p))] = true
		}
	}
	dir := sunoDir(c)
	for n := 2; ; n++ {
		taken := mine[strings.ToLower(dir)]
		if !taken {
			if names := dirSet(dir); len(names) > 0 {
				// on disk but not betterPod's: someone else's folder, unless
				// it holds this song's .part
				taken = !names[safeName(c.Title)+".mp3.part"]
				for name := range names {
					if name == safeName(c.Title)+".mp3" {
						taken = true
					}
				}
			}
		}
		if !taken {
			break
		}
		dir = sunoDir(c) + fmt.Sprintf(" (%d)", n)
	}
	if err := os.MkdirAll(dir, 0755); err != nil {
		return err
	}
	cover := filepath.Join(dir, "cover.jpg")
	if !fileExists(cover) && c.ImageLarge != "" {
		if err := podSaveCover(c.ImageLarge, cover); err != nil {
			logMsg("Suno: the cover: " + err.Error())
		}
	}
	name := safeName(c.Title) + ".mp3"
	part := filepath.Join(dir, name) + ".part"
	var err error
	for try := 1; ; try++ {
		err = podGet(d, c.Audio, part, base, size)
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
	if err != nil {
		return err
	}
	if ext, ok := sniffAudio(part); ext != ".mp3" || !ok {
		os.Remove(part)
		return errors.New("Suno didn't send an MP3")
	}
	t := songTags{Title: c.Title, Artist: c.Artist, Album: c.Title, AlbumArtist: c.Artist, Genre: c.genre(),
		Track: 1, TrackTotal: 1, Year: c.Year}
	if err := tagPart(part, ".mp3", t, cover); err != nil {
		logMsg("Suno: tagging " + name + ": " + err.Error()) // kept untagged
	}
	dst := filepath.Join(dir, name)
	if err := os.Rename(part, dst); err != nil {
		return err
	}
	sunoNote(c.ID, dst)
	return nil
}
