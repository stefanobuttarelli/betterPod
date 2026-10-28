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
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
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
}

func (e ytEntry) isPlaylist() bool {
	return e.Type == "playlist" || e.IEKey == "YoutubeTab" || strings.Contains(e.URL, "list=")
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
	cmd := ytCommand(ctx, append([]string{"-J"}, args...)...)
	var stderr strings.Builder
	cmd.Stderr = &stderr
	out, err := cmd.Output()
	if err != nil {
		if ctx.Err() != nil {
			return nil, errIACancelled
		}
		return nil, ytError(stderr.String(), err)
	}
	var info ytInfo
	if err := json.Unmarshal(out, &info); err != nil {
		return nil, fmt.Errorf("unexpected answer from yt-dlp")
	}
	return &info, nil
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
			return errors.New(l)
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
	}{}
	v := &View{ID: "youtube", Title: "YouTube", Kind: KindFull}
	var run func()
	v.Build = func() []Item {
		editing := ui.keyboardEditing(&st.term)
		label, sub := "Search YouTube", "Enter text or a link, then ✓"
		switch {
		case editing:
			sub = ui.withCaret(&st.term)
			if st.term == "" {
				sub = "Type, then ✓ to search"
			}
		case st.ran != "" && !st.busy:
			label = "Edit search"
			sub = fmt.Sprintf("%s · %d %s", st.ran, len(st.res), pluralize(len(st.res), "result", "results"))
		}
		edit := func() {
			ui.showKeyboard(&st.term, func() { ui.refreshView(v) }, run)
			ui.refreshView(v)
		}
		items := []Item{{Label: label, Sublabel: sub, Icon: "search_icon", OnSelect: edit}}
		kind, other := "Videos", "Playlists & Albums"
		if st.playlists {
			kind, other = "Playlists & Albums", "Videos"
		}
		items = append(items, Item{Label: "Showing: " + kind, Sublabel: "Press for " + other, NoArrow: true, OnSelect: func() {
			st.playlists = !st.playlists
			if st.ran != "" {
				run()
			} else {
				ui.refreshView(v)
			}
		}})
		switch {
		case st.busy:
			items = append(items, Item{Label: "Searching…", NoArrow: true})
		case st.status != "":
			l, sb, _ := strings.Cut(st.status, " · ")
			items = append(items, Item{Label: l, Sublabel: sb, NoArrow: true, OnSelect: func() {
				if strings.HasSuffix(st.status, "press to retry") {
					run()
				}
			}})
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
			items = append(items, Item{Label: e.Title, Sublabel: strings.Join(parts, " · "),
				OnSelect: func() { ui.showView(ui.ytItemView(e), "") }})
		}
		return items
	}
	run = func() {
		q := strings.TrimSpace(st.term)
		if q == "" {
			return
		}
		if ytURLRe.MatchString(q) {
			// a link: open it
			e := ytEntry{URL: q, Title: q}
			if strings.Contains(q, "list=") && !strings.Contains(q, "watch") {
				e.Type = "playlist"
			}
			ui.showView(ui.ytItemView(e), "")
			return
		}
		if st.cancel != nil {
			st.cancel()
		}
		ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
		st.cancel = cancel
		st.gen++
		gen := st.gen
		st.busy, st.status, st.ran, st.res = true, "", q, nil
		v.Index = 0
		ui.refreshView(v)
		playlists := st.playlists
		go func() {
			res, err := ytSearch(ctx, q, playlists)
			cancel()
			ui.postSure(func() {
				if gen != st.gen {
					return
				}
				st.busy = false
				switch {
				case err != nil:
					st.status = "Couldn't search · " + iaErrText(err) + " · press to retry"
				case len(res) == 0:
					st.status = "No results"
				}
				st.res = res
				ui.refreshView(v)
				if len(res) > 0 {
					v.Index = 2
				}
			})
		}()
	}
	v.Items = v.Build()
	ui.showKeyboard(&st.term, func() { ui.refreshView(v) }, run)
	return v
}

// ---- a video or a playlist ----

// ytAlbum: how a download will be named and cut.
type ytAlbum struct {
	Artist, Album, Year string
	Tracks              []ytTrack
	CoverURLs           []string // best first
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
		for _, c := range i.Chapters {
			t := strings.TrimSpace(ytNumberRe.ReplaceAllString(c.Title, ""))
			if ar, ti, ok := splitArtist(t); ok && strings.EqualFold(ar, a.Artist) {
				t = ti
			}
			if t == "" {
				t = c.Title
			}
			a.Tracks = append(a.Tracks, ytTrack{Title: t, Start: c.Start, End: c.End, Duration: c.End - c.Start})
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
		if e.ID == "" {
			continue
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
// download buttons.
func (ui *IpodUI) ytItemView(e ytEntry) *View {
	st := &struct {
		info *ytInfo
		err  string
	}{}
	v := &View{ID: "ytItem", Title: e.Title, Kind: KindFull}
	v.Build = func() []Item {
		if st.info == nil {
			if st.err != "" {
				l, sub, _ := strings.Cut(st.err, " · ")
				return []Item{{Label: l, Sublabel: sub, NoArrow: true}}
			}
			return []Item{{Label: "Loading…", NoArrow: true}}
		}
		i := st.info
		var items []Item
		if e.isPlaylist() || len(i.Entries) > 0 {
			a := albumFromPlaylist(i)
			n := len(a.Tracks)
			items = append(items, Item{Label: fmt.Sprintf("Download as Album · %d %s", n, pluralize(n, "song", "songs")),
				Sublabel: a.Artist + " — " + a.Album, OnSelect: func() { ui.startYTDownload(i, a) }})
			for _, t := range a.Tracks {
				items = append(items, Item{Label: t.Title, Value: lengthOrEmpty(t.Duration), NoArrow: true})
			}
			return items
		}
		if len(i.Chapters) >= 2 {
			a := albumFromVideo(i, true)
			items = append(items, Item{Label: fmt.Sprintf("Download as Album · %d tracks", len(a.Tracks)),
				Sublabel: "A track per chapter · " + a.Artist + " — " + a.Album, OnSelect: func() { ui.startYTDownload(i, a) }})
		}
		one := albumFromVideo(i, false)
		items = append(items, Item{Label: "Download as One Song", Sublabel: one.Artist + " — " + one.Tracks[0].Title + " · " + lengthOrEmpty(i.Duration),
			OnSelect: func() { ui.startYTDownload(i, one) }})
		if len(i.Chapters) >= 2 {
			for _, t := range albumFromVideo(i, true).Tracks {
				items = append(items, Item{Label: t.Title, Value: lengthOrEmpty(t.Duration), NoArrow: true})
			}
		}
		return items
	}
	v.Items = v.Build()
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
		defer cancel()
		var info *ytInfo
		var err error
		if e.isPlaylist() {
			info, err = ytJSON(ctx, "--flat-playlist", e.link())
		} else {
			info, err = ytJSON(ctx, "--no-playlist", e.link())
		}
		ui.postSure(func() {
			st.info = info
			if err != nil {
				st.err = "Couldn't open it · " + iaErrText(err)
			} else if info.Title != "" {
				v.Title = info.Title
				if ui.topScreen() == v {
					ui.headerTitle = info.Title
				}
			}
			ui.refreshView(v)
		})
	}()
	return v
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

// ytFetchAudio downloads a video's AAC audio into dir, following yt-dlp's
// progress (done(bytes, total)); it returns the file.
func ytFetchAudio(ctx context.Context, link, dir string, done func(int64, int64)) (string, error) {
	cmd := ytCommand(ctx, "-f", "140/bestaudio[ext=m4a]", "--no-playlist", "--fixup", "never",
		"--newline", "--progress-template", "download:KP %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s",
		"-o", filepath.Join(dir, "%(id)s.%(ext)s"), "--print", "after_move:filepath", link)
	var stderr strings.Builder
	cmd.Stderr = &stderr
	out, err := cmd.StdoutPipe()
	if err != nil {
		return "", err
	}
	if err := cmd.Start(); err != nil {
		return "", err
	}
	path := ""
	sc := bufio.NewScanner(out)
	for sc.Scan() {
		l := strings.TrimSpace(sc.Text())
		if strings.HasPrefix(l, "KP ") {
			f := strings.Fields(l)
			if len(f) >= 4 {
				got, _ := strconv.ParseFloat(f[1], 64)
				total, _ := strconv.ParseFloat(f[2], 64)
				if total <= 0 {
					total, _ = strconv.ParseFloat(f[3], 64)
				}
				done(int64(got), int64(total))
			}
		} else if l != "" && (filepath.IsAbs(l) || strings.HasPrefix(l, dir)) {
			path = l
		}
	}
	err = cmd.Wait()
	if ctx.Err() != nil {
		return "", errIACancelled
	}
	if err != nil {
		return "", ytError(stderr.String(), err)
	}
	if path == "" {
		return "", errors.New("yt-dlp didn't say where the file is")
	}
	return path, nil
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

// ytRun: the download (in the background).
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
	tmp := filepath.Join(ytToolsDir(), "work")
	os.RemoveAll(tmp)
	if err := os.MkdirAll(tmp, 0755); err != nil {
		return err
	}
	defer os.RemoveAll(tmp)
	setState := func(s string) {
		d.mu.Lock()
		d.state = s
		d.mu.Unlock()
	}
	// the cover first, so the album shows with it
	setState("Making the cover…")
	cover := ytCover(ctx, a.CoverURLs)
	if cover != nil {
		_ = writeFileAtomic(filepath.Join(d.dir, "cover.jpg"), cover)
	}
	n := len(a.Tracks)
	tags := func(i int) m4aTags {
		return m4aTags{Title: a.Tracks[i].Title, Artist: a.Artist, AlbumArtist: a.Artist, Album: a.Album,
			Year: a.Year, Track: i + 1, Tracks: n, Cover: cover}
	}
	trackPath := func(i int) string {
		return filepath.Join(d.dir, fmt.Sprintf("%02d %s.m4a", i+1, safeName(a.Tracks[i].Title)))
	}
	progress := func(base int64) func(int64, int64) {
		return func(got, total int64) {
			d.mu.Lock()
			if a.Tracks[0].VideoID == "" && total > 0 {
				d.total = total // one download (a video): its real size
			}
			d.done = base + got
			d.mu.Unlock()
		}
	}

	if a.Tracks[0].VideoID == "" {
		// one video: download once, then cut at the chapters
		setState("Getting the audio")
		src, err := ytFetchAudio(ctx, info.link(), tmp, progress(0))
		if err != nil {
			return err
		}
		m, err := openM4A(src)
		if err != nil {
			return err
		}
		defer m.Close()
		for i, t := range a.Tracks {
			d.mu.Lock()
			d.index, d.state = i, "Making the tracks"
			d.mu.Unlock()
			end := t.End
			if i == n-1 {
				end = 0 // to the very end
			}
			if err := m.WriteRange(trackPath(i), t.Start, end, tags(i)); err != nil {
				if isDiskErr(err) {
					return err
				}
				d.mu.Lock()
				d.failed = append(d.failed, t.Title)
				d.lastErr = err.Error()
				d.mu.Unlock()
			}
		}
		d.mu.Lock()
		d.index, d.done = n, d.total
		d.mu.Unlock()
		return nil
	}

	// a playlist: a download per video
	netFails := 0
	for i, t := range a.Tracks {
		d.mu.Lock()
		d.index, d.state = i, t.Title
		base := d.done
		d.mu.Unlock()
		if fileExists(trackPath(i)) {
			continue // already there (Try Again)
		}
		src, err := ytFetchAudio(ctx, "https://www.youtube.com/watch?v="+t.VideoID, tmp, progress(base))
		if err == nil {
			var m *m4aSource
			if m, err = openM4A(src); err == nil {
				err = m.WriteRange(trackPath(i), 0, 0, tags(i))
				m.Close()
			}
			os.Remove(src)
		}
		switch {
		case err == nil:
			netFails = 0
		case errors.Is(err, errIACancelled), isDiskErr(err):
			return err
		default:
			logMsg("YouTube: " + t.Title + ": " + err.Error())
			d.mu.Lock()
			d.failed = append(d.failed, t.Title)
			d.lastErr = err.Error()
			d.mu.Unlock()
			if netFails++; netFails >= 3 {
				return fmt.Errorf("%s (stopped: three videos in a row failed)", iaErrText(err))
			}
		}
	}
	d.mu.Lock()
	d.index = n
	d.mu.Unlock()
	return nil
}

// ---- Music → Search ----

// searchHubView: the library, Internet Archive and (when turned on)
// YouTube.
func (ui *IpodUI) searchHubView() *View {
	v := &View{ID: "searchHub", Title: "Search", Kind: KindSplit}
	v.Build = func() []Item {
		items := []Item{
			{Label: "Library", Preview: PrevMusic, OnSelect: func() { ui.showView(ui.searchView(), "") }},
			{Label: "Internet Archive", Preview: PrevMusic, OnSelect: func() { ui.showView(ui.iaSearchView(), "") }},
		}
		if ytEnabled {
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
