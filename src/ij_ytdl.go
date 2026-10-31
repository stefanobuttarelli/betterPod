package main

// ij_ytdl.go — Music → Search → YouTube: search videos or playlists and
// download them as albums.
//
//   - A video with chapters becomes an album with a track per chapter (one
//     download, then cut: ij_m4a.go); without chapters, one song.
//   - A playlist becomes an album with a track per video.
//
// Tracks are M4A (YouTube's AAC audio, not re-encoded) with their tags and
// a square cover (ij_ytcover.go) inside and as cover.jpg, in
// Music/YouTube/<artist>/<album>. The download uses the same screen as
// Internet Archive (progress, time left, cancel, Try Again).

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"io"
	"math"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"time"
)

type ytThumb struct {
	URL string `json:"url"`
	W   int    `json:"width"`
	H   int    `json:"height"`
}

type ytChapter struct {
	Start float64 `json:"start_time"`
	End   float64 `json:"end_time"`
	Title string  `json:"title"`
}

type ytEntry struct {
	ID         string    `json:"id"`
	Type       string    `json:"_type"`
	IEKey      string    `json:"ie_key"`
	URL        string    `json:"url"`
	Title      string    `json:"title"`
	Channel    string    `json:"channel"`
	Uploader   string    `json:"uploader"`
	Duration   float64   `json:"duration"`
	Count      int       `json:"playlist_count"`
	Thumbnails []ytThumb `json:"thumbnails"`
	Avail      string    `json:"availability"`
}

type ytInfo struct {
	ytEntry
	Artist      string      `json:"artist"`
	Album       string      `json:"album"`
	UploadDate  string      `json:"upload_date"`
	ReleaseYear int         `json:"release_year"`
	Chapters    []ytChapter `json:"chapters"`
	Thumbnail   string      `json:"thumbnail"`
	WebURL      string      `json:"webpage_url"`
	Entries     []ytEntry   `json:"entries"`

	raw []byte // yt-dlp's answer, given back to it for the download
}

func (e ytEntry) isPlaylist() bool {
	if e.Type == "playlist" || e.IEKey == "YoutubeTab" {
		return true
	}
	// a playlist link (not a video played from a playlist or a mix)
	return strings.Contains(e.URL, "list=") && !strings.Contains(e.URL, "v=") && !strings.Contains(e.URL, "youtu.be/")
}

func (e ytEntry) who() string {
	w := e.Channel
	if w == "" {
		w = e.Uploader
	}
	return strings.TrimSuffix(strings.TrimSpace(w), " - Topic")
}

func (e ytEntry) link() string {
	if strings.HasPrefix(e.URL, "http") {
		return e.URL
	}
	if e.isPlaylist() {
		return "https://www.youtube.com/playlist?list=" + e.ID
	}
	return "https://www.youtube.com/watch?v=" + e.ID
}

// ytJSON runs yt-dlp -J and decodes its answer.
func ytJSON(ctx context.Context, args ...string) (*ytInfo, error) {
	return ytJSONSteps(ctx, nil, args...)
}

// ytJSONSteps is ytJSON telling what yt-dlp is doing meanwhile: step gets
// each of its status lines ("[youtube] abc: Downloading webpage"). With
// --no-quiet they come on stdout before the JSON (a line starting "{").
func ytJSONSteps(ctx context.Context, step func(string), args ...string) (*ytInfo, error) {
	a := []string{"-J"}
	if step != nil {
		a = append(a, "--no-quiet")
	}
	cmd := ytCommand(ctx, append(a, args...)...)
	var stderr strings.Builder
	cmd.Stderr = &stderr
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	var out []byte
	r := bufio.NewReaderSize(stdout, 64<<10)
	for {
		line, rerr := r.ReadBytes('\n') // a playlist's JSON can be megabytes on one line
		if t := strings.TrimSpace(string(line)); strings.HasPrefix(t, "{") {
			out = line
		} else if t != "" && step != nil {
			step(t)
		}
		if rerr != nil {
			break
		}
	}
	err = cmd.Wait()
	if err != nil || out == nil {
		if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			return nil, errors.New("YouTube took too long to answer")
		}
		if ctx.Err() != nil {
			return nil, errIACancelled
		}
		if err == nil {
			err = errors.New("no answer from yt-dlp")
		}
		return nil, ytError(stderr.String(), err)
	}
	var info ytInfo
	if err := json.Unmarshal(out, &info); err != nil {
		return nil, fmt.Errorf("unexpected answer from yt-dlp")
	}
	info.raw = out
	return &info, nil
}

// ytStep turns a yt-dlp status line into a step in words and where it is
// in the loading (0..1); ok false for lines that aren't a known step.
func ytStep(line string) (string, float64, bool) {
	l := strings.ToLower(line)
	switch {
	case strings.Contains(l, "extracting url"):
		return "Asking YouTube", 0.12, true
	case strings.Contains(l, "downloading webpage"), strings.Contains(l, "downloading playlist"):
		return "Reading the page", 0.25, true
	case strings.Contains(l, "client config"), strings.Contains(l, "downloading player"), strings.Contains(l, "player api"):
		return "Getting YouTube's player", 0.42, true
	case strings.Contains(l, "jsc"), strings.Contains(l, "challenge"), strings.Contains(l, "signature"), strings.Contains(l, "n function"):
		return "Solving YouTube's challenge (the slow part)", 0.6, true
	case strings.Contains(l, "m3u8"), strings.Contains(l, "formats"), strings.Contains(l, "format(s)"):
		return "Reading the formats", 0.85, true
	case strings.Contains(l, "downloading page"), strings.Contains(l, "api json"):
		return "Reading the list", 0.6, true
	}
	return "", 0, false
}

// ytLoading: the loading of a page, shown as a bar that eases towards the
// current step and creeps on while a step lasts (it never sits still).
type ytLoading struct {
	start     time.Time
	stepAt    time.Time
	from, to  float64
	stepLabel string
}

func newYTLoading() *ytLoading {
	now := time.Now()
	return &ytLoading{start: now, stepAt: now, to: 0.05, stepLabel: "Starting yt-dlp"}
}

func (l *ytLoading) set(label string, at float64) {
	if at <= l.to {
		return // steps only go forward
	}
	l.from, l.to = l.value(), at
	l.stepAt, l.stepLabel = time.Now(), label
}

// value: where the bar is now: quickly to the step, then creeping a
// little further (at most halfway to the next likely step).
func (l *ytLoading) value() float64 {
	t := time.Since(l.stepAt).Seconds()
	v := l.from + (l.to-l.from)*math.Min(1, t/0.4)
	creep := (math.Min(0.95, l.to+0.15) - l.to) * 0.5 * (1 - math.Exp(-t/10))
	return math.Min(0.97, v+creep)
}

func (l *ytLoading) item(what string) Item {
	secs := int(time.Since(l.start).Seconds())
	return Item{Label: what, Sublabel: fmt.Sprintf("%s · %d s", l.stepLabel, secs), Progress: math.Max(0.02, l.value()), NoArrow: true}
}

// ytError: yt-dlp's last error line, short.
func ytError(stderr string, err error) error {
	lines := strings.Split(strings.TrimSpace(stderr), "\n")
	for i := len(lines) - 1; i >= 0; i-- {
		l := strings.TrimSpace(lines[i])
		if strings.HasPrefix(l, "ERROR:") {
			l = strings.TrimSpace(strings.TrimPrefix(l, "ERROR:"))
			if i := strings.Index(l, "] "); strings.HasPrefix(l, "[") && i > 0 {
				l = l[i+2:] // "[youtube] abc: …"
			}
			return ytErrText(l)
		}
	}
	return err
}

// ---- search ----

var ytURLRe = regexp.MustCompile(`(?i)^(https?://)?(www\.|m\.|music\.)?(youtube\.com|youtu\.be)/`)

func ytSearch(ctx context.Context, q string, playlists bool) ([]ytEntry, error) {
	target := "ytsearch25:" + q
	if playlists {
		target = "https://www.youtube.com/results?search_query=" + url.QueryEscape(q) + "&sp=EgIQAw%3D%3D"
	}
	info, err := ytJSON(ctx, "--flat-playlist", "--playlist-end", "25", target)
	if err != nil {
		return nil, err
	}
	return info.Entries, nil
}

// youtubeView: the search row (the keyboard opens at once), Videos /
// Playlists, and the results. A YouTube link opens that video or playlist.
func (ui *IpodUI) youtubeView() *View {
	st := &struct {
		term, ran, status string
		busy, playlists   bool
		res               []ytEntry
		gen               int
		cancel            context.CancelFunc
		thumbs            map[string]*Album // video thumbnails, by result
	}{thumbs: map[string]*Album{}}
	v := &View{ID: "youtube", Title: "YouTube", Kind: KindFull}
	v.onClose = func() {
		if st.cancel != nil {
			st.cancel() // a search still running stops with the view
		}
	}
	var run func()
	v.Build = func() []Item {
		note := ""
		if st.ran != "" && !st.busy && !ui.keyboardEditing(&st.term) {
			note = fmt.Sprintf("%d %s", len(st.res), pluralize(len(st.res), "result", "results"))
		}
		edit := func() {
			ui.showKeyboard(&st.term, func() { ui.refreshView(v) }, run)
			ui.refreshView(v)
		}
		bar := ui.searchField(&st.term, "Search YouTube or paste a link", note, edit, nil)
		// what it searches for: by the search bar's gear (as Weather's)
		bar.Gear = func() {
			ui.showView(ui.ytSearchSettingsView(&st.playlists, func() {
				if st.ran != "" {
					run()
				} else {
					ui.refreshView(v)
				}
			}), "")
		}
		items := []Item{bar}
		kind := "Videos"
		if st.playlists {
			kind = "Playlists & Albums"
		}
		items = append(items, Item{Header: true, Label: "Showing: " + kind})
		switch {
		case st.busy:
			items = append(items, Item{Label: "Searching…", NoArrow: true})
		case st.status != "":
			l, sb, _ := strings.Cut(st.status, " · ")
			items = append(items, Item{Label: l, Sublabel: sb, NoArrow: true, OnSelect: func() {
				if strings.HasSuffix(st.status, "press [A] to retry") {
					run()
				}
			}})
		}
		if st.ran == "" && !st.busy {
			items = append(items, ui.recentItems("youtube", func(term string) {
				st.term = term
				run()
			}, func() { ui.refreshView(v) })...)
		}
		for _, e := range st.res {
			e := e
			var parts []string
			if w := e.who(); w != "" {
				parts = append(parts, w)
			}
			if e.isPlaylist() {
				if e.Count > 0 {
					parts = append(parts, fmt.Sprintf("%d videos", e.Count))
				} else {
					parts = append(parts, "Playlist")
				}
			} else if e.Duration > 0 {
				parts = append(parts, formatTrackLength(e.Duration))
			}
			th := st.thumbs[e.ID+e.URL]
			if th == nil {
				th = &Album{Name: e.Title} // the default art until the thumbnail comes
				st.thumbs[e.ID+e.URL] = th
			}
			items = append(items, Item{Label: e.Title, Sublabel: strings.Join(parts, " · "), Album: th,
				OnSelect: func() { ui.showView(ui.ytItemView(e, th), "") }})
		}
		return items
	}
	run = func() {
		q := strings.TrimSpace(st.term)
		if q == "" {
			return
		}
		if ytURLRe.MatchString(q) {
			// a link (typed without https:// too): open it
			if !strings.HasPrefix(strings.ToLower(q), "http") {
				q = "https://" + q
			}
			ui.showView(ui.ytItemView(ytEntry{URL: q, Title: q}, nil), "")
			return
		}
		if st.cancel != nil {
			st.cancel()
		}
		addRecentSearch("youtube", q)
		ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
		st.cancel = cancel
		st.gen++
		gen := st.gen
		st.busy, st.status, st.ran, st.res = true, "", q, nil
		v.Index = 0
		ui.refreshView(v)
		playlists := st.playlists
		// the thumbnails load after the results, until the view is left or
		// searches again
		ctx2, cancel2 := context.WithCancel(context.Background())
		st.cancel = func() { cancel(); cancel2() }
		go func() {
			res, err := ytSearch(ctx, q, playlists)
			ui.postSure(func() {
				if gen != st.gen {
					return
				}
				st.busy = false
				switch {
				case err != nil:
					st.status = "Couldn't search · " + iaErrText(err) + " · press [A] to retry"
				case len(res) == 0:
					st.status = "No results"
				}
				st.res = res
				ui.refreshView(v)
				if len(res) > 0 {
					v.Index = 2
					ytLoadThumbs(ui, v, res, st.thumbs, ctx2, func() bool { return gen == st.gen })
				}
			})
		}()
	}
	v.Items = v.Build()
	if len(recentSearches("youtube")) == 0 {
		ui.showKeyboard(&st.term, func() { ui.refreshView(v) }, run)
	} else {
		v.Index = 2 // the recent searches (the field above opens the keyboard)
	}
	return v
}

// ytLoadThumbs fetches the results' thumbnails (three at a time), crops
// each to a square from its middle and gives it to the row's album.
func ytLoadThumbs(ui *IpodUI, v *View, res []ytEntry, thumbs map[string]*Album, ctx context.Context, current func() bool) {
	type job struct {
		th   *Album
		urls []string
	}
	var jobs []job
	for _, e := range res {
		th := thumbs[e.ID+e.URL]
		if th == nil || th.ArtImg != nil {
			continue
		}
		id := e.ID
		if e.isPlaylist() {
			id = "" // a playlist's id isn't a video's
		}
		// small ones first (a 48 px row needs little)
		var urls []string
		if id != "" {
			urls = append(urls, "https://i.ytimg.com/vi/"+id+"/mqdefault.jpg")
		}
		ts := ytThumbs(e.Thumbnails, "")
		for i := len(ts) - 1; i >= 0; i-- {
			urls = append(urls, ts[i]) // smallest first
		}
		if len(urls) > 0 {
			jobs = append(jobs, job{th, urls})
		}
	}
	work := make(chan job)
	for w := 0; w < 3; w++ {
		go func() {
			for j := range work {
				img := ytThumbImage(ctx, j.urls)
				if img == nil {
					continue
				}
				th := j.th
				ui.postSure(func() {
					if !current() {
						return
					}
					th.ArtImg = img
					ui.artc.dropAlbum(th)
					v.dirty = true
					ui.dirty = true
				})
			}
		}()
	}
	go func() {
		defer close(work)
		for _, j := range jobs {
			select {
			case work <- j:
			case <-ctx.Done():
				return
			}
		}
	}()
}

// ytThumbImage: the first thumbnail that loads, cut to a square from its
// middle (small: it is for a list row).
func ytThumbImage(ctx context.Context, urls []string) image.Image {
	for _, u := range urls {
		req, err := http.NewRequestWithContext(ctx, "GET", u, nil)
		if err != nil {
			return nil
		}
		resp, err := updateClient(20 * time.Second).Do(req)
		if err != nil {
			if ctx.Err() != nil {
				return nil
			}
			continue
		}
		if resp.StatusCode != 200 {
			resp.Body.Close()
			continue
		}
		img, _, err := image.Decode(io.LimitReader(resp.Body, 4<<20))
		resp.Body.Close()
		if err != nil {
			continue
		}
		b := img.Bounds()
		s := min(b.Dx(), b.Dy())
		sq := image.Rect(b.Min.X+(b.Dx()-s)/2, b.Min.Y+(b.Dy()-s)/2, b.Min.X+(b.Dx()-s)/2+s, b.Min.Y+(b.Dy()-s)/2+s)
		return scaleImage(cropTo(img, sq), 96, 96)
	}
	return nil
}

// ---- a video or a playlist ----

// ytAlbum: how a download will be named and cut.
type ytAlbum struct {
	Artist, Album, Year string
	Tracks              []ytTrack
	CoverURLs           []string // best first
	// the picture and the square chosen on the video's page (Cover row,
	// ij_ytcoverpick.go); nil: the automatic cover from CoverURLs
	CoverSrc    image.Image
	CoverChoice ytCoverChoice
}

type ytTrack struct {
	Title      string
	VideoID    string  // playlists: each track is a video
	Start, End float64 // a video's chapters
	Duration   float64
}

var (
	ytJunkRe   = regexp.MustCompile(`(?i)\s*[\(\[][^\)\]]*\b(full album|official|audio|video|lyrics?|hd|hq|4k|visuali[sz]er|playlist)\b[^\)\]]*[\)\]]`)
	ytFullRe   = regexp.MustCompile(`(?i)\s*[-–|:]?\s*\bfull album\b\s*`)
	ytNumberRe = regexp.MustCompile(`^\s*(\d{1,2}:\d{2}(:\d{2})?\s*[-–:.]?\s*|\d{1,3}\s*[.)\-–:]\s+|\d{1,3}\s+-\s+)`)
)

func ytClean(s string) string {
	s = ytJunkRe.ReplaceAllString(s, "")
	s = ytFullRe.ReplaceAllString(s, " ")
	return strings.Trim(strings.Join(strings.Fields(s), " "), " -–|:")
}

// splitArtist: "Artist - Title" → (Artist, Title).
func splitArtist(s string) (string, string, bool) {
	for _, sep := range []string{" - ", " – ", " — "} {
		if a, b, ok := strings.Cut(s, sep); ok && strings.TrimSpace(a) != "" && strings.TrimSpace(b) != "" {
			return strings.TrimSpace(a), strings.TrimSpace(b), true
		}
	}
	return "", s, false
}

func ytYear(i *ytInfo) string {
	if i.ReleaseYear > 0 {
		return strconv.Itoa(i.ReleaseYear)
	}
	if len(i.UploadDate) >= 4 {
		return i.UploadDate[:4]
	}
	return ""
}

// ytThumbs: thumbnail URLs, a square one first, then the biggest.
func ytThumbs(ts []ytThumb, id string) []string {
	var sq, big []ytThumb
	for _, t := range ts {
		if t.W > 0 && t.W == t.H && t.W >= 300 {
			sq = append(sq, t)
		} else {
			big = append(big, t)
		}
	}
	best := func(l []ytThumb) []string {
		var out []string
		for len(l) > 0 {
			bi := 0
			for i, t := range l {
				if t.W*t.H > l[bi].W*l[bi].H {
					bi = i
				}
			}
			out = append(out, l[bi].URL)
			l = append(l[:bi:bi], l[bi+1:]...)
			if len(out) == 3 {
				break
			}
		}
		return out
	}
	out := append(best(sq), best(big)...)
	if id != "" {
		out = append(out, "https://i.ytimg.com/vi/"+id+"/maxresdefault.jpg", "https://i.ytimg.com/vi/"+id+"/hqdefault.jpg")
	}
	return out
}

// albumFromVideo: the chapters as tracks (or the video as one song).
func albumFromVideo(i *ytInfo, asAlbum bool) ytAlbum {
	a := ytAlbum{Year: ytYear(i), CoverURLs: ytThumbs(i.Thumbnails, i.ID)}
	title := ytClean(i.Title)
	switch {
	case i.Album != "" && i.Artist != "":
		a.Artist, a.Album = strings.TrimSpace(strings.Split(i.Artist, ",")[0]), i.Album
	default:
		if ar, al, ok := splitArtist(title); ok {
			a.Artist, a.Album = ar, al
		} else {
			a.Artist, a.Album = i.who(), title
		}
	}
	if a.Artist == "" {
		a.Artist = "Unknown Artist"
	}
	if asAlbum && len(i.Chapters) >= 2 {
		chs := append([]ytChapter(nil), i.Chapters...)
		sort.SliceStable(chs, func(x, y int) bool { return chs[x].Start < chs[y].Start })
		for k := range chs {
			// a missing end: the next chapter's start (or the end of the video)
			if chs[k].End <= chs[k].Start {
				chs[k].End = 0
				if k+1 < len(chs) {
					chs[k].End = chs[k+1].Start
				}
			}
		}
		for _, c := range chs {
			t := strings.TrimSpace(ytNumberRe.ReplaceAllString(c.Title, ""))
			if ar, ti, ok := splitArtist(t); ok && strings.EqualFold(ar, a.Artist) {
				t = ti
			}
			if t == "" {
				t = c.Title
			}
			dur := c.End - c.Start
			if c.End <= 0 {
				dur = i.Duration - c.Start
			}
			a.Tracks = append(a.Tracks, ytTrack{Title: t, Start: c.Start, End: c.End, Duration: dur})
		}
		return a
	}
	// one song: the video's title (without the artist) on an album of its own
	song := title
	if i.Artist != "" && i.Album != "" {
		if t := strings.TrimSpace(i.Title); t != "" {
			song = t
		}
	} else if _, t, ok := splitArtist(title); ok {
		song = t
	}
	a.Tracks = []ytTrack{{Title: song, Duration: i.Duration}}
	if !asAlbum && i.Album == "" {
		a.Album = song
	}
	return a
}

// albumFromPlaylist: a track per video.
func albumFromPlaylist(i *ytInfo) ytAlbum {
	a := ytAlbum{Year: ytYear(i)}
	a.Album = ytClean(strings.TrimPrefix(i.Title, "Album - "))
	a.Artist = i.who()
	if ar, al, ok := splitArtist(a.Album); ok {
		a.Artist, a.Album = ar, al
	}
	if a.Artist == "" {
		a.Artist = "Various Artists"
	}
	for _, e := range i.Entries {
		if e.ID == "" || e.Avail == "private" || e.Avail == "needs_auth" ||
			e.Title == "[Deleted video]" || e.Title == "[Private video]" {
			continue // gone from YouTube
		}
		t := ytClean(e.Title)
		if ar, ti, ok := splitArtist(t); ok && strings.EqualFold(ar, a.Artist) {
			t = ti
		}
		a.Tracks = append(a.Tracks, ytTrack{Title: t, VideoID: e.ID, Duration: e.Duration})
		if len(a.CoverURLs) == 0 {
			a.CoverURLs = ytThumbs(e.Thumbnails, e.ID)
		}
	}
	return a
}

// ytItemView: a video (its chapters) or a playlist (its videos), and the
// download buttons. The header says YouTube; the first row is the video
// itself, its thumbnail (th: the search row's, or loaded here) and title.
func (ui *IpodUI) ytItemView(e ytEntry, th *Album) *View {
	st := &ytItemState{}
	if th == nil {
		th = &Album{Name: e.Title}
	}
	st.thumb = th
	v := &View{ID: "ytItem", Title: "YouTube", Kind: KindFull, Index: 1}
	var load func()
	var cancel context.CancelFunc
	v.onClose = func() {
		if cancel != nil {
			cancel()
		}
	}
	head := func() Item {
		title, who, dur, count := e.Title, e.who(), e.Duration, e.Count
		if i := st.info; i != nil {
			if i.Title != "" {
				title = i.Title
			}
			if w := i.who(); w != "" {
				who = w
			}
			if i.Duration > 0 {
				dur = i.Duration
			}
			if n := len(i.Entries); n > 0 {
				count = n
			}
		}
		var parts []string
		if who != "" {
			parts = append(parts, who)
		}
		if e.isPlaylist() {
			if count > 0 {
				parts = append(parts, fmt.Sprintf("%d videos", count))
			} else {
				parts = append(parts, "Playlist")
			}
		} else if dur > 0 {
			parts = append(parts, formatTrackLength(dur))
		}
		return Item{Label: title, Sublabel: strings.Join(parts, " · "), Album: th, NoArrow: true}
	}
	v.Build = func() []Item {
		items := []Item{head()}
		if st.info == nil {
			if st.err != "" {
				l, sub, _ := strings.Cut(st.err, " · ")
				return append(items, Item{Label: l, Sublabel: sub + " · press [A] to try again", NoArrow: true, OnSelect: func() {
					st.err = ""
					ui.refreshView(v)
					load()
				}})
			}
			if st.load != nil {
				return append(items, st.load.item("Opening the video"))
			}
			return append(items, Item{Label: "Loading…", NoArrow: true})
		}
		i := st.info
		// Listen first: it plays, nothing is kept (ij_listen.go)
		if songs := ytListenSongs(i); len(songs) > 0 {
			key := "yt:" + i.ID + e.ID
			sub := "Before you download"
			if len(songs) > 1 {
				sub = fmt.Sprintf("%d songs, before you download", len(songs))
			}
			listen := Item{Label: "Listen", Sublabel: sub, OnSelect: func() {
				al := &Album{Name: ytClean(i.Title), Artist: i.who()}
				if st.thumb != nil {
					al.ArtImg = st.thumb.ArtImg
				}
				ui.startListen(key, al, songs, func() { ui.refreshView(v) })
				ui.refreshView(v)
			}}
			if l, sb, p := listenState(key); l != "" {
				listen.Label, listen.Sublabel, listen.Progress = l, sb, p
			}
			items = append(items, listen)
		}
		// the cover it will get (choosing it opens the square)
		switch {
		case st.coverSrc != nil:
			items = append(items, Item{Label: "Cover", Sublabel: ytCoverText(st.coverSrc, st.coverChoice), Album: st.coverPrev,
				OnSelect: func() {
					ui.showView(ui.ytCoverEditorView(st.coverSrc, st.coverChoice, func(c ytCoverChoice) {
						st.coverChoice = c
						ui.refreshView(v)
						// its little picture made away from the screen (a blurred whole
						// picture takes a moment)
						src, al := st.coverSrc, st.coverPrev
						go func() {
							img := ytCoverPreview(src, c, px(48))
							ui.postSure(func() {
								if st.coverChoice == c {
									al.ArtImg = img
									ui.artc.dropAlbum(al)
									ui.refreshView(v)
								}
							})
						}()
					}), "")
				}})
		case !st.coverTried:
			items = append(items, Item{Label: "Cover", Sublabel: "Loading the picture…", NoArrow: true})
		}
		withCover := func(a ytAlbum) ytAlbum {
			a.CoverSrc, a.CoverChoice = st.coverSrc, st.coverChoice
			return a
		}
		if e.isPlaylist() || len(i.Entries) > 0 {
			a := albumFromPlaylist(i)
			n := len(a.Tracks)
			items = append(items, Item{Label: fmt.Sprintf("Download as Album · %d %s", n, pluralize(n, "song", "songs")),
				Sublabel: a.Artist + " — " + a.Album, OnSelect: func() { ui.startYTDownload(i, withCover(a)) }})
			for _, t := range a.Tracks {
				items = append(items, Item{Label: t.Title, Value: lengthOrEmpty(t.Duration), NoArrow: true})
			}
			return items
		}
		if len(i.Chapters) >= 2 {
			a := albumFromVideo(i, true)
			items = append(items, Item{Label: fmt.Sprintf("Download as Album · %d tracks", len(a.Tracks)),
				Sublabel: "A track per chapter · " + a.Artist + " — " + a.Album, OnSelect: func() { ui.startYTDownload(i, withCover(a)) }})
		}
		one := albumFromVideo(i, false)
		items = append(items, Item{Label: "Download as One Song", Sublabel: one.Artist + " — " + one.Tracks[0].Title + " · " + lengthOrEmpty(i.Duration),
			OnSelect: func() { ui.startYTDownload(i, withCover(one)) }})
		if len(i.Chapters) >= 2 {
			for _, t := range albumFromVideo(i, true).Tracks {
				items = append(items, Item{Label: t.Title, Value: lengthOrEmpty(t.Duration), NoArrow: true})
			}
		}
		return items
	}
	v.Items = v.Build()
	load = func() {
		var ctx context.Context
		ctx, cancel = context.WithTimeout(context.Background(), 120*time.Second)
		go ytLoadItem(ui, v, st, e, ctx, cancel)
	}
	load()
	return v
}

type ytItemState struct {
	thumb *Album // the first row's picture
	// the cover: the picture it's made from (loaded after the info), the
	// choice, and its preview for the Cover row
	coverSrc    image.Image
	coverChoice ytCoverChoice
	coverPrev   *Album
	coverTried  bool
	info        *ytInfo
	err         string
	load        *ytLoading // while loading
}

func ytLoadItem(ui *IpodUI, v *View, st *ytItemState, e ytEntry, ctx context.Context, cancel context.CancelFunc) {
	// the loading bar: steps from yt-dlp, and a redraw a few times a second
	load := newYTLoading()
	ui.postSure(func() { st.load = load; ui.refreshView(v) })
	tick := time.NewTicker(150 * time.Millisecond)
	defer tick.Stop()
	go func() {
		for {
			select {
			case <-tick.C:
				ui.post(func() {
					if st.load == load && st.info == nil && st.err == "" && ui.topScreen() == v {
						ui.refreshView(v)
					}
				})
			case <-ctx.Done():
				return
			}
		}
	}()
	step := func(line string) {
		if label, at, ok := ytStep(line); ok {
			ui.post(func() { load.set(label, at) })
		}
	}
	func() {
		defer cancel()
		var info *ytInfo
		var err error
		if e.isPlaylist() {
			info, err = ytJSONSteps(ctx, step, "--flat-playlist", e.link())
		} else {
			info, err = ytJSONSteps(ctx, step, "--no-playlist", e.link())
		}
		ui.postSure(func() {
			st.info = info
			if err != nil {
				st.err = "Couldn't open it · " + iaErrText(err)
			} else if st.thumb.ArtImg == nil {
				// opened from a link: its thumbnail now
				var urls []string
				if info.ID != "" && !e.isPlaylist() {
					urls = append(urls, "https://i.ytimg.com/vi/"+info.ID+"/mqdefault.jpg")
				}
				if info.Thumbnail != "" {
					urls = append(urls, info.Thumbnail)
				}
				urls = append(urls, ytThumbs(info.Thumbnails, "")...)
				th := st.thumb
				go func() {
					tctx, done := context.WithTimeout(context.Background(), 20*time.Second)
					defer done()
					if img := ytThumbImage(tctx, urls); img != nil {
						ui.postSure(func() {
							th.ArtImg = img
							ui.artc.dropAlbum(th)
							ui.refreshView(v)
						})
					}
				}()
			}
			v.Index = 1 // the first row is the video itself
			if err == nil {
				// the picture the cover is made from, for the Cover row
				urls := ytThumbs(info.Thumbnails, info.ID)
				if e.isPlaylist() || len(info.Entries) > 0 {
					urls = ytThumbs(info.Thumbnails, "")
					if len(info.Entries) > 0 {
						urls = append(urls, ytThumbs(info.Entries[0].Thumbnails, info.Entries[0].ID)...)
					}
				}
				go func() {
					cctx, done := context.WithTimeout(context.Background(), 40*time.Second)
					defer done()
					img := ytCoverSource(cctx, urls)
					var prev *image.RGBA
					if img != nil {
						prev = ytCoverPreview(img, ytCoverChoice{}, px(48))
					}
					ui.postSure(func() {
						st.coverTried = true
						if img != nil {
							st.coverSrc = img
							st.coverPrev = &Album{Name: "Cover", ArtImg: prev}
						}
						ui.refreshView(v)
					})
				}()
			}
			ui.refreshView(v)
		})
	}()
}

func lengthOrEmpty(sec float64) string {
	if sec <= 0 {
		return ""
	}
	return formatTrackLength(sec)
}

// ---- the download ----

func (ui *IpodUI) startYTDownload(info *ytInfo, a ytAlbum) {
	if iaCur != nil && !iaCur.finished {
		ui.showView(ui.iaDownloadView(), "")
		return
	}
	if len(a.Tracks) == 0 {
		return
	}
	dir := filepath.Join(musicRoot(), "YouTube", safeName(a.Artist), safeName(a.Album))
	var files []iaFile
	var secs float64
	for _, t := range a.Tracks {
		files = append(files, iaFile{Title: t.Title})
		secs += t.Duration
	}
	if secs <= 0 {
		secs = info.Duration
	}
	d := &iaDownload{item: &iaItem{Title: a.Album, Creator: a.Artist}, files: files, format: "M4A", dir: dir,
		cancel: make(chan struct{}), state: "Starting…", source: "YouTube"}
	d.total = int64(secs * 16000) // ~128 kbit/s AAC, until yt-dlp says
	ui.startDownloadJob(d, func() error { return ytRun(d, info, a) }, func() { ui.startYTDownload(info, a) })
}

// ytFetch downloads the AAC audio of the targets (video links, or none
// with infoJSON: a video's info from ytItemView, which saves yt-dlp a new
// extraction) into dir with one yt-dlp: progress(id, bytes, total) while
// it goes, done(id, path) as each file is complete. A video that fails
// doesn't stop the others; their errors come back by id. Partial files
// stay in dir, and a later call continues them.
func ytFetch(ctx context.Context, targets []string, infoJSON, dir string,
	progress func(id string, got, total int64), done func(id, path string)) (map[string]error, error) {
	args := []string{"-f", "140/bestaudio[ext=m4a]", "--no-playlist", "--fixup", "never", "--ignore-errors",
		"--progress", "--newline",
		"--progress-template", "download:KP %(info.id)s %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s",
		"-o", filepath.Join(dir, "%(id)s.%(ext)s"), "--print", "after_move:KPDONE %(id)s %(filepath)s"}
	if infoJSON != "" {
		args = append(args, "--load-info-json", infoJSON)
	} else {
		args = append(args, targets...)
	}
	cmd := ytCommand(ctx, args...)
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, err
	}
	// errors by video ("ERROR: [youtube] ID: why"), read beside stdout so
	// neither pipe fills up
	errs := map[string]error{}
	var lastErr string
	errDone := make(chan struct{})
	go func() {
		defer close(errDone)
		sc := bufio.NewScanner(stderr)
		sc.Buffer(make([]byte, 0, 64<<10), 1<<20)
		for sc.Scan() {
			l := strings.TrimSpace(sc.Text())
			if !strings.HasPrefix(l, "ERROR:") {
				continue
			}
			lastErr = l
			rest := strings.TrimSpace(strings.TrimPrefix(l, "ERROR:"))
			if strings.HasPrefix(rest, "[") {
				if i := strings.Index(rest, "] "); i > 0 {
					rest = rest[i+2:]
				}
			}
			if id, why, ok := strings.Cut(rest, ": "); ok && !strings.ContainsAny(id, " /") {
				errs[id] = ytErrText(why)
			}
		}
	}()
	sc := bufio.NewScanner(stdout)
	sc.Buffer(make([]byte, 0, 64<<10), 1<<20)
	for sc.Scan() {
		f := strings.Fields(sc.Text())
		switch {
		case len(f) >= 5 && f[0] == "KP":
			got, _ := strconv.ParseFloat(f[2], 64)
			total, _ := strconv.ParseFloat(f[3], 64)
			if total <= 0 {
				total, _ = strconv.ParseFloat(f[4], 64)
			}
			progress(f[1], int64(got), int64(total))
		case len(f) >= 3 && f[0] == "KPDONE":
			done(f[1], strings.TrimSpace(strings.SplitN(sc.Text(), " ", 3)[2]))
		}
	}
	io.Copy(io.Discard, stdout)
	werr := cmd.Wait()
	<-errDone
	if ctx.Err() != nil {
		return errs, errIACancelled
	}
	if werr != nil && len(errs) == 0 && lastErr != "" {
		return errs, ytError(lastErr, werr)
	}
	return errs, nil
}

// ytCover: the first thumbnail that loads, made square (JPEG).
func ytCover(ctx context.Context, urls []string) []byte {
	for _, u := range urls {
		req, _ := http.NewRequestWithContext(ctx, "GET", u, nil)
		resp, err := updateClient(30 * time.Second).Do(req)
		if err != nil {
			continue
		}
		if resp.StatusCode != 200 {
			resp.Body.Close()
			continue
		}
		img, _, err := image.Decode(io.LimitReader(resp.Body, 8<<20))
		resp.Body.Close()
		if err != nil || img.Bounds().Dx() < 120 {
			continue
		}
		if b, err := squareCover(img); err == nil {
			return b
		}
	}
	return nil
}

// ytRun: the download (in the background). The work folder (per video or
// playlist) keeps partial files, so Try Again continues; it is removed
// once the album is complete. Tracks already made are kept.
func ytRun(d *iaDownload, info *ytInfo, a ytAlbum) error {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go func() {
		select {
		case <-d.cancel:
			cancel()
		case <-ctx.Done():
		}
	}()
	if err := os.MkdirAll(d.dir, 0755); err != nil {
		return err
	}
	key := info.ID
	if key == "" {
		key = safeName(a.Album)
	}
	work := filepath.Join(ytToolsDir(), "work", safeName(key))
	if err := os.MkdirAll(work, 0755); err != nil {
		return err
	}
	n := len(a.Tracks)
	trackPath := func(i int) string {
		return filepath.Join(d.dir, fmt.Sprintf("%02d %s.m4a", i+1, safeName(a.Tracks[i].Title)))
	}
	missing := 0
	for i := range a.Tracks {
		if !fileExists(trackPath(i)) {
			missing++
		}
	}
	if missing == 0 {
		os.RemoveAll(work)
		return nil // all there already
	}
	// room for the audio (twice over for a video cut into chapters)
	d.mu.Lock()
	need := d.total
	d.mu.Unlock()
	if a.Tracks[0].VideoID == "" {
		need *= 2
	}
	if fb := freeBytes(d.dir); fb >= 0 && fb < need+50<<20 {
		return fmt.Errorf("Not enough space (%s free)", formatBytes(fb))
	}
	setState := func(s string) {
		d.mu.Lock()
		d.state, d.whole = s, true
		d.mu.Unlock()
	}
	// the cover first, so the album shows with it
	setState("Making the cover…")
	coverPath := filepath.Join(d.dir, "cover.jpg")
	cover, _ := os.ReadFile(coverPath)
	if a.CoverSrc != nil && (a.CoverChoice.mode != "" || cover == nil) {
		// the one chosen on the video's page (it replaces an earlier one);
		// the automatic one only when there's none yet (a download going on)
		if b, err := ytCoverMake(a.CoverSrc, a.CoverChoice); err == nil {
			cover = b
			_ = writeFileAtomic(coverPath, cover)
		}
	}
	if cover == nil {
		if cover = ytCover(ctx, a.CoverURLs); cover != nil {
			_ = writeFileAtomic(coverPath, cover)
		}
	}
	tags := func(i int) m4aTags {
		return m4aTags{Title: a.Tracks[i].Title, Artist: a.Artist, AlbumArtist: a.Artist, Album: a.Album,
			Year: a.Year, Track: i + 1, Tracks: n, Cover: cover}
	}
	fail := func(i int, err error) {
		d.mu.Lock()
		d.failed = append(d.failed, a.Tracks[i].Title)
		d.lastErr = err.Error()
		d.mu.Unlock()
	}

	if a.Tracks[0].VideoID == "" {
		// one video: download once, then cut at the chapters
		setState("Getting the audio")
		var src string
		var info2 string
		if len(info.raw) > 0 {
			info2 = filepath.Join(work, "info.json")
			_ = os.WriteFile(info2, info.raw, 0644)
		}
		fetch := func(infoJSON string) (map[string]error, error) {
			return ytFetch(ctx, []string{info.link()}, infoJSON, work,
				func(_ string, got, total int64) {
					d.mu.Lock()
					if total > 0 {
						d.total = total
					}
					d.done = got
					d.mu.Unlock()
				},
				func(_, path string) { src = path })
		}
		errs, err := fetch(info2)
		if err == nil && src == "" && info2 != "" && !errors.Is(err, errIACancelled) {
			// the saved info may have expired (its links last some hours)
			errs, err = fetch("")
		}
		if err != nil {
			return err
		}
		if src == "" {
			for _, e := range errs {
				return e
			}
			return errors.New("yt-dlp didn't download the audio")
		}
		m, err := openM4A(src)
		if err != nil {
			return err
		}
		defer m.Close()
		for i, t := range a.Tracks {
			if ctx.Err() != nil {
				return errIACancelled
			}
			d.mu.Lock()
			d.index, d.state, d.whole = i, "Making the tracks", false
			d.mu.Unlock()
			if fileExists(trackPath(i)) {
				continue
			}
			end := t.End
			if i == n-1 {
				end = 0 // to the very end
			}
			if err := m.WriteRange(trackPath(i), t.Start, end, tags(i)); err != nil {
				if isDiskErr(err) {
					return err
				}
				fail(i, err)
			}
		}
		m.Close()
		d.mu.Lock()
		d.index, d.done = n, d.total
		d.mu.Unlock()
		os.RemoveAll(work)
		return nil
	}

	// a playlist: one yt-dlp for all its videos; each is tagged as it lands
	byID := map[string]int{}
	var targets []string
	for i, t := range a.Tracks {
		if !fileExists(trackPath(i)) {
			byID[t.VideoID] = i
			targets = append(targets, "https://www.youtube.com/watch?v="+t.VideoID)
		}
	}
	var finished int64 // bytes of the videos done
	made := 0
	errs, err := ytFetch(ctx, targets, "", work,
		func(id string, got, _ int64) {
			d.mu.Lock()
			if i, ok := byID[id]; ok {
				d.index, d.state, d.whole = i, a.Tracks[i].Title, false
			}
			d.done = finished + got
			d.mu.Unlock()
		},
		func(id, path string) {
			i, ok := byID[id]
			if !ok {
				return
			}
			if st, err := os.Stat(path); err == nil {
				finished += st.Size()
			}
			m, err := openM4A(path)
			if err == nil {
				err = m.WriteRange(trackPath(i), 0, 0, tags(i))
				m.Close()
			}
			os.Remove(path)
			if err != nil {
				fail(i, err)
				return
			}
			made++
			delete(byID, id)
		})
	if errors.Is(err, errIACancelled) {
		return err
	}
	// the videos that didn't come
	for id, i := range byID {
		e := errs[id]
		if e == nil {
			e = errors.New("not downloaded")
		}
		fail(i, e)
	}
	if made == 0 && len(targets) > 0 {
		if err != nil {
			return err
		}
		for _, e := range errs {
			return e
		}
		return errors.New("no video could be downloaded")
	}
	d.mu.Lock()
	d.index = n
	d.mu.Unlock()
	if len(byID) == 0 {
		os.RemoveAll(work)
	}
	return nil
}

// ytErrText: yt-dlp's reasons in a few words (the network ones as "No
// connection to YouTube").
func ytErrText(why string) error {
	low := strings.ToLower(why)
	for _, k := range []string{"name resolution", "network is unreachable", "timed out", "connection refused", "unable to download webpage", "connection reset", "no route to host"} {
		if strings.Contains(low, k) {
			return errors.New("No connection to YouTube")
		}
	}
	if strings.Contains(low, "sign in to confirm") {
		return errors.New("YouTube asks to confirm you're not a bot (try later)")
	}
	if r := []rune(why); len(r) > 90 {
		why = string(r[:90]) + "…"
	}
	return errors.New(why)
}

// ---- Music → Search ----

// searchHubView: the library, Internet Archive and (when turned on)
// YouTube.
// ytSearchSettingsView: YouTube's search options (the search bar's gear):
// what it looks for. changed runs the search again.
func (ui *IpodUI) ytSearchSettingsView(playlists *bool, changed func()) *View {
	v := &View{ID: "yt-settings", Title: "Search Settings", Kind: KindFull}
	v.Build = func() []Item {
		pick := func(label, sub string, on bool) Item {
			return Item{Label: label, Sublabel: sub, Checked: *playlists == on, NoArrow: true, OnSelect: func() {
				if *playlists != on {
					*playlists = on
					changed()
				}
				ui.refreshView(v)
			}}
		}
		return []Item{
			pick("Videos", "Songs, live sets, albums in one video", false),
			pick("Playlists & Albums", "Each video a song of the album", true),
		}
	}
	v.Items = v.Build()
	if *playlists {
		v.Index = 1
	}
	return v
}

func (ui *IpodUI) searchHubView() *View {
	v := &View{ID: "searchHub", Title: "Search", Kind: KindSplit}
	v.Build = func() []Item {
		items := []Item{
			{Label: "Library", Preview: PrevMusic, OnSelect: func() { ui.showView(ui.searchView(), "") }},
			{Label: "Internet Archive", Preview: PrevMusic, OnSelect: func() { ui.showView(ui.iaSearchView(), "") }},
		}
		if ytEnabled && !isOnion { // (no YouTube on the Miyoo, see onion.go)
			items = append(items, Item{Label: "YouTube", Preview: PrevMusic, OnSelect: func() {
				if !ytToolsReady() {
					ui.showView(ui.youtubeSettingsView(), "") // the tools aren't there yet
					return
				}
				ui.showView(ui.youtubeView(), "")
			}})
		}
		return items
	}
	v.Items = v.Build()
	return v
}
