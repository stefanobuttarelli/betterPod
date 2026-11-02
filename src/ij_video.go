package main

// ij_video.go — Videos (Knulli, with YouTube turned on: Settings → YouTube):
// search YouTube and watch, Recently Watched, and the player's screen. The
// playing itself is in video.go.
//
// The player fills the whole screen, over the header, as the iPod's did:
// the picture fits 640 x 480, in the middle. Over it, for 2.5 s after a key
// (and while it is paused, opening or waiting), the title at the top and
// the time with its bar at the bottom; then nothing covers the picture.
//
//	A             the title and the time, when they're hidden; else play /
//	              pause (again at the end, or after an error)
//	START, X      play / pause
//	LEFT / RIGHT  10 s back / on (held: further)
//	UP / DOWN     volume
//	L / R         the previous / next video of the list it came from
//	Y (hold A)    options: Download Video, Download as One Song (or
//	              Album), Watch from the Start
//	B             back: the video stops; Recently Watched keeps the place
//
// Off on the Miyoo: YouTube is (yt-dlp needs more memory than it has), and
// Onion has no ffmpeg.

import (
	"compress/gzip"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"io"
	"math"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
)

// ---- the main menu's Videos ----

func (ui *IpodUI) videosView() *View {
	v := &View{ID: "videos", Title: "Videos", Kind: KindSplit}
	if done, _, _ := ffmpegChecked(); !done {
		go func() {
			ffmpegReady() // (a moment: it runs ffmpeg once)
			ui.post(func() { ui.refreshView(v) })
		}()
	}
	v.Build = func() []Item {
		items := []Item{{Label: "Search YouTube", Preview: PrevVideos, OnSelect: func() {
			if !ytToolsReady() {
				ui.showView(ui.youtubeSettingsView(), "") // the tools aren't there yet
				return
			}
			ui.showView(ui.youtubeWatchView(), "")
		}}}
		if len(videoHistory()) > 0 {
			items = append(items, Item{Label: "Recently Watched", Preview: PrevVideos,
				OnSelect: func() { ui.showView(ui.recentVideosView(), "") }})
		}
		if len(videosSaved()) > 0 {
			items = append(items, Item{Label: "Downloaded", Preview: PrevVideos,
				OnSelect: func() { ui.showView(ui.savedVideosView(), "") }})
		}
		if it, ok := ui.ffmpegItem(v); ok {
			items = append(items, it)
		}
		if it, ok := ui.denoItem(v); ok {
			items = append(items, it)
		}
		return items
	}
	v.Items = v.Build()
	return v
}

// denoItem: the row that adds Deno to tools that have only QuickJS (got
// before Deno was the first choice): yt-dlp then opens a video in about
// half the time.
func (ui *IpodUI) denoItem(v *View) (Item, bool) {
	if !ytToolsReady() || !strings.HasPrefix(ytRuntime(), "quickjs") {
		return Item{}, false
	}
	busy, status := ytToolsStatus()
	ytTools.mu.Lock()
	failed := ytTools.denoErr
	ytTools.mu.Unlock()
	get := func() { ui.ytInstallTools(false, func() { ui.refreshView(v) }) }
	switch {
	case busy:
		return Item{Label: status, Sublabel: "Deno, from GitHub", Preview: PrevVideos, NoArrow: true}, true
	case failed != "":
		return Item{Label: "Couldn't get Deno", Sublabel: failed + " · press [A] to try again", Preview: PrevVideos, NoArrow: true, OnSelect: get}, true
	}
	return Item{Label: "Open Videos Faster", Sublabel: "Deno for yt-dlp: 40 MB to get, 85 MB on the card", Preview: PrevVideos, OnSelect: get}, true
}

// ffmpegItem: the row that gets ffmpeg, when the system has none that
// runs, or one without what Videos needs (Knulli's has it all: this is
// for another system).
func (ui *IpodUI) ffmpegItem(v *View) (Item, bool) {
	busy, status := ffGet.state()
	done, ok, lacks := ffmpegChecked()
	switch {
	case busy:
		return Item{Label: status, Sublabel: "ffmpeg, a static build from GitHub", Preview: PrevVideos, NoArrow: true}, true
	case !done || ok:
		return Item{}, false
	case status != "":
		l, sub, _ := strings.Cut(status, " · ")
		return Item{Label: l, Sublabel: sub + " · press [A] to try again", Preview: PrevVideos, NoArrow: true,
			OnSelect: func() { ui.getFFmpeg(v) }}, true
	}
	why := "Videos need it, and this system has none"
	if lacks != "" {
		why = "This system's has no " + lacks
	}
	return Item{Label: "Get ffmpeg", Sublabel: why + ": 26 MB to get, 51 MB on the card",
		Preview: PrevVideos, OnSelect: func() { ui.getFFmpeg(v) }}, true
}

// ffGet: the download of ffmpeg (John Van Sickle's static build 7.0.2,
// as packed by the ffmpeg-static project), checked against the SHA-256
// GitHub lists for it, then with a test run.
type ffDownload struct {
	mu     sync.Mutex
	busy   bool
	status string
}

var ffGet ffDownload

const (
	ffURL = "https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-linux-arm64.gz"
	ffSum = "754a678672298bc68156adff58aa7385a592c2b30b1d0ae8750c45c915c4bac0"
)

func (f *ffDownload) state() (bool, string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.busy, f.status
}

// gunzipFile unpacks a .gz file.
func gunzipFile(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	zr, err := gzip.NewReader(in)
	if err != nil {
		return err
	}
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, zr); err != nil {
		out.Close()
		os.Remove(dst)
		return err
	}
	return out.Close()
}

func (ui *IpodUI) getFFmpeg(v *View) {
	ffGet.mu.Lock()
	if ffGet.busy {
		ffGet.mu.Unlock()
		return
	}
	ffGet.busy, ffGet.status = true, "Downloading ffmpeg…"
	ffGet.mu.Unlock()
	set := func(s string) {
		ffGet.mu.Lock()
		ffGet.status = s
		ffGet.mu.Unlock()
		ui.post(func() { ui.refreshView(v) })
	}
	ui.refreshView(v)
	go func() {
		err := func() error {
			dir := ytToolsDir()
			if err := os.MkdirAll(dir, 0755); err != nil {
				return err
			}
			gz := filepath.Join(dir, "ffmpeg.gz")
			defer os.Remove(gz)
			got, err := fetchFile(ffURL, gz, func(d, t int64) {
				if t > 0 {
					set(fmt.Sprintf("Downloading ffmpeg… %d%% of %s", d*100/t, formatBytes(t)))
				}
			})
			if err != nil {
				return err
			}
			if got != ffSum {
				return errors.New("the file doesn't match its published checksum")
			}
			set("Unpacking ffmpeg…")
			tmp := filepath.Join(dir, "ffmpeg.new")
			defer os.Remove(tmp)
			if err := gunzipFile(gz, tmp); err != nil {
				return err
			}
			if err := checkTool(tmp, "-hide_banner", "-version"); err != nil {
				return err
			}
			return os.Rename(tmp, filepath.Join(dir, "ffmpeg"))
		}()
		ffGet.mu.Lock()
		ffGet.busy = false
		ffGet.status = ""
		if err != nil {
			ffGet.status = "Couldn't get ffmpeg · " + shortErr(err)
			logMsg("Videos: ffmpeg: " + err.Error())
		}
		ffGet.mu.Unlock()
		ffmpegRecheck()
		ffmpegReady()
		ui.postSure(func() { ui.refreshView(v) })
	}()
}

// ---- Recently Watched ----

// videoSeen: a video watched, and where it was left.
type videoSeen struct {
	ID       string  `json:"id"`
	Title    string  `json:"title"`
	Channel  string  `json:"channel,omitempty"`
	Duration float64 `json:"duration,omitempty"`
	Pos      float64 `json:"position,omitempty"` // 0: from the start (watched to the end, or hardly begun)
	At       int64   `json:"at"`
}

func (s videoSeen) entry() ytEntry {
	return ytEntry{ID: s.ID, Title: s.Title, Channel: s.Channel, Duration: s.Duration}
}

func videoHistoryPath() string { return dataPath("videos.json") }

var videoSeenMu sync.Mutex

func videoHistory() []videoSeen {
	videoSeenMu.Lock()
	defer videoSeenMu.Unlock()
	var h []videoSeen
	if data, err := os.ReadFile(videoHistoryPath()); err == nil {
		json.Unmarshal(data, &h)
	}
	return h
}

// videoRemember puts a video first in Recently Watched (50 at most), with
// the place to go on from.
func videoRemember(e ytEntry, pos, dur float64) {
	if e.ID == "" {
		return
	}
	if dur <= 0 {
		dur = e.Duration
	}
	if pos < 10 || dur > 0 && pos > dur-15 {
		pos = 0
	}
	s := videoSeen{ID: e.ID, Title: e.Title, Channel: e.who(), Duration: dur, Pos: math.Round(pos), At: time.Now().Unix()}
	h := videoHistory()
	out := []videoSeen{s}
	for _, o := range h {
		if o.ID != e.ID && len(out) < 50 {
			out = append(out, o)
		}
	}
	videoSaveHistory(out)
}

func videoForget(id string) {
	var out []videoSeen
	for _, o := range videoHistory() {
		if o.ID != id {
			out = append(out, o)
		}
	}
	videoSaveHistory(out)
}

func videoSaveHistory(h []videoSeen) {
	videoSeenMu.Lock()
	defer videoSeenMu.Unlock()
	if len(h) == 0 {
		os.Remove(videoHistoryPath())
		return
	}
	data, _ := json.Marshal(h)
	_ = writeFileAtomic(videoHistoryPath(), data)
}

func (ui *IpodUI) recentVideosView() *View {
	v := &View{ID: "recentVideos", Title: "Recently Watched", Kind: KindFull, Empty: "No videos"}
	thumbs := map[string]*Album{}
	ctx, cancel := context.WithCancel(context.Background())
	v.onClose = cancel
	v.Build = func() []Item {
		h := videoHistory()
		var items []Item
		var list []ytEntry
		for _, s := range h {
			list = append(list, s.entry())
		}
		for i, s := range h {
			e := list[i]
			th := thumbs[e.ID]
			if th == nil {
				th = &Album{Name: e.Title}
				thumbs[e.ID] = th
			}
			var parts []string
			if s.Channel != "" {
				parts = append(parts, s.Channel)
			}
			switch {
			case s.Pos > 0 && s.Duration > 0:
				parts = append(parts, videoTime(s.Pos, s.Duration)+" of "+videoTime(s.Duration, s.Duration))
			case s.Duration > 0:
				parts = append(parts, formatTrackLength(s.Duration))
			}
			idx, pos := i, s.Pos
			items = append(items, Item{Label: s.Title, Sublabel: strings.Join(parts, " · "), Album: th,
				OnSelect: func() { ui.playVideo(list[idx], th, nil, list, idx, false, pos) },
				LongPress: []SheetOption{
					{Label: "Watch from the Start", OnSelect: func() { ui.playVideo(list[idx], th, nil, list, idx, false, 0) }},
					ui.videoDownloadOption(list[idx], nil),
					{Label: "Remove from Recently Watched", OnSelect: func() {
						videoForget(list[idx].ID)
						ui.refreshView(v)
					}},
				}})
		}
		return items
	}
	v.Items = v.Build()
	var list []ytEntry
	for _, s := range videoHistory() {
		list = append(list, s.entry())
	}
	ytLoadThumbs(ui, v, list, thumbs, ctx, func() bool { return true })
	return v
}

// ---- YouTube's streams ----

// ytFormat: one of a video's formats, as yt-dlp lists them (numbers as
// float64: yt-dlp may write any of them as a float, or null).
type ytFormat struct {
	ID       string            `json:"format_id"`
	URL      string            `json:"url"`
	Protocol string            `json:"protocol"`
	VCodec   string            `json:"vcodec"`
	ACodec   string            `json:"acodec"`
	W        float64           `json:"width"`
	H        float64           `json:"height"`
	FPS      float64           `json:"fps"`
	TBR      float64           `json:"tbr"`
	ABR      float64           `json:"abr"`
	Size     float64           `json:"filesize"`
	Range    string            `json:"dynamic_range"`
	LangPref float64           `json:"language_preference"`
	Headers  map[string]string `json:"http_headers"`
	// when its link starts to work (Unix seconds): a few seconds after
	// yt-dlp gets it, YouTube answers 403 before
	AvailableAt float64 `json:"available_at"`
}

// ytLinksAt: when the links of the formats that may be played work (the
// latest available_at of the candidates; 0: at once).
func ytLinksAt(fs []ytFormat) float64 {
	vids, auds, both := ytCandidates(fs)
	at := 0.0
	for _, l := range [][]*ytFormat{vids, auds, both} {
		for _, f := range l {
			at = math.Max(at, f.AvailableAt)
		}
	}
	return at
}

// ytWaitLinks waits until YouTube's links work (at, Unix seconds; 0 or
// past: no wait), 20 s at most (a clock set wrong), or until ctx ends.
func ytWaitLinks(ctx context.Context, at float64) error {
	d := time.Until(time.Unix(0, int64(at*1e9)))
	if at <= 0 || d <= 0 {
		return nil
	}
	select {
	case <-time.After(min(d, 20*time.Second)):
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// vcodecRank: 0 H.264, 1 VP9 (ffmpeg decodes both in software, H.264 for
// less), -1 a codec not to count on (AV1 needs a decoder ffmpeg may lack).
func vcodecRank(c string) int {
	switch {
	case strings.HasPrefix(c, "avc1"), strings.HasPrefix(c, "h264"):
		return 0
	case strings.HasPrefix(c, "vp09"), strings.HasPrefix(c, "vp9"), strings.HasPrefix(c, "vp8"):
		return 1
	}
	return -1
}

func acodecRank(c string) int {
	switch {
	case strings.HasPrefix(c, "mp4a"):
		return 0
	case strings.HasPrefix(c, "opus"):
		return 1
	case strings.HasPrefix(c, "vorbis"), strings.HasPrefix(c, "mp3"):
		return 2
	}
	return -1
}

// ytCandidates: a video's formats betterPod can play, each kind best
// first: the pictures alone (H.264 before VP9; within a codec, the
// smallest that fills the screen, then bigger ones, then those under it,
// biggest first), the sounds alone (the original language, without DRC,
// AAC, about 128 kbit/s), and those with both.
func ytCandidates(fs []ytFormat) (vids, auds, both []*ytFormat) {
	for i := range fs {
		f := &fs[i]
		if f.URL == "" || f.Protocol != "https" && f.Protocol != "http" || strings.HasPrefix(f.Range, "HDR") {
			continue
		}
		hasV := f.VCodec != "" && f.VCodec != "none"
		hasA := f.ACodec != "" && f.ACodec != "none"
		switch {
		case hasV && hasA:
			if vcodecRank(f.VCodec) >= 0 && acodecRank(f.ACodec) >= 0 {
				both = append(both, f)
			}
		case hasV:
			if vcodecRank(f.VCodec) >= 0 {
				vids = append(vids, f)
			}
		case hasA:
			if acodecRank(f.ACodec) >= 0 {
				auds = append(auds, f)
			}
		}
	}
	return rankPictures(vids), rankSounds(auds), rankPictures(both)
}

// ytPickFormats: ytCandidates' first choice: the best picture with the
// best sound, else the best format with both; audio is nil for a picture
// without sound.
func ytPickFormats(fs []ytFormat) (video, audio *ytFormat, err error) {
	vids, auds, both := ytCandidates(fs)
	switch {
	case len(vids) > 0 && len(auds) > 0:
		return vids[0], auds[0], nil
	case len(both) > 0:
		return both[0], both[0], nil
	case len(vids) > 0:
		return vids[0], nil, nil
	}
	return nil, nil, errors.New("no format this console can play")
}

func rankPictures(fs []*ytFormat) []*ytFormat {
	out := append([]*ytFormat(nil), fs...)
	enough := func(f *ytFormat) bool {
		tw, th := videoFit(int(f.W), int(f.H), videoBoxW, videoBoxH)
		return f.W >= float64(tw)*0.9 && f.H >= float64(th)*0.9
	}
	sort.SliceStable(out, func(i, j int) bool {
		a, b := out[i], out[j]
		if ra, rb := vcodecRank(a.VCodec), vcodecRank(b.VCodec); ra != rb {
			return ra < rb
		}
		if ea, eb := enough(a), enough(b); ea != eb {
			return ea
		}
		if pa, pb := a.W*a.H, b.W*b.H; pa != pb {
			if enough(a) {
				return pa < pb // the smallest that fills the screen
			}
			return pa > pb // else the biggest
		}
		if a.FPS != b.FPS {
			return a.FPS < b.FPS // the same size: fewer frames, fewer bits
		}
		return a.TBR < b.TBR
	})
	return out
}

func rankSounds(fs []*ytFormat) []*ytFormat {
	out := append([]*ytFormat(nil), fs...)
	drc := func(f *ytFormat) bool { return strings.Contains(f.ID, "drc") }
	sort.SliceStable(out, func(i, j int) bool {
		a, b := out[i], out[j]
		if a.LangPref != b.LangPref {
			return a.LangPref > b.LangPref // the original language first
		}
		if drc(a) != drc(b) {
			return !drc(a)
		}
		if ra, rb := acodecRank(a.ACodec), acodecRank(b.ACodec); ra != rb {
			return ra < rb
		}
		return math.Abs(a.ABR-128) < math.Abs(b.ABR-128)
	})
	return out
}

// ytChooseStreams: the candidates in their order, each link tried first
// with a range of 2 bytes, since YouTube refuses some of a video's links
// now and then (a 403 for one format, its bytes for the next): the first
// picture and sound that answer, else the first format with both that
// does.
func ytChooseStreams(ctx context.Context, info *ytInfo) (video, audio *ytFormat, err error) {
	vids, auds, both := ytCandidates(info.Formats)
	if len(vids)+len(both) == 0 {
		return nil, nil, errors.New("no format this console can play")
	}
	refused := 0
	first := func(fs []*ytFormat, most int) *ytFormat {
		for i, f := range fs {
			if i >= most || ctx.Err() != nil {
				break
			}
			code := ytAnswers(ctx, info, f)
			if code == 206 || code == 200 {
				return f
			}
			if code != 0 {
				refused = code
			}
			logMsg(fmt.Sprintf("Videos: %s: format %s answered %d, trying the next", info.ID, f.ID, code))
		}
		return nil
	}
	if len(auds) > 0 {
		if v := first(vids, 6); v != nil {
			if a := first(auds, 4); a != nil {
				return v, a, nil
			}
		}
	}
	if b := first(both, 3); b != nil {
		return b, b, nil
	}
	if len(auds) == 0 {
		if v := first(vids, 6); v != nil {
			return v, nil, nil // (a video without sound)
		}
	}
	if refused != 0 {
		return nil, nil, vRefused(refused)
	}
	return nil, nil, errors.New("No connection to YouTube")
}

// videoErrText: why a video couldn't be played, in words.
func videoErrText(err error) string {
	var r vRefused
	if errors.As(err, &r) {
		return fmt.Sprintf("YouTube refused the stream (error %d)", int(r))
	}
	return err.Error()
}

// ytAnswers: the status a format's link answers with (0: no answer).
func ytAnswers(ctx context.Context, info *ytInfo, f *ytFormat) int {
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, "GET", f.URL, nil)
	if err != nil {
		return 0
	}
	for k, v := range ytHeaders(info, f) {
		req.Header.Set(k, v)
	}
	req.Header.Set("Range", "bytes=0-1")
	resp, err := vHTTP().Do(req)
	if err != nil {
		return 0
	}
	io.Copy(io.Discard, io.LimitReader(resp.Body, 64))
	resp.Body.Close()
	return resp.StatusCode
}

// ytHeaders: the headers a format's URL wants (the video's, then its own).
func ytHeaders(info *ytInfo, f *ytFormat) map[string]string {
	h := map[string]string{}
	for k, v := range info.HTTPHeaders {
		h[k] = v
	}
	for k, v := range f.Headers {
		h[k] = v
	}
	return h
}

// ---- the player's screen ----

// VideoView: a YouTube video playing (or opening) in the player's screen.
type VideoView struct {
	e        ytEntry
	thumb    *Album    // its little picture, from the list
	list     []ytEntry // the list it was chosen from (L / R)
	idx      int
	autoNext bool    // a playlist: the next one follows at the end
	at       float64 // where to start

	v       *View
	gen     int     // each video opened in this screen
	info    *ytInfo // once yt-dlp has opened it
	preInfo *ytInfo // given by the video's page (no wait)
	load    *ytLoading
	cancel  context.CancelFunc
	err     string // "Couldn't play it · why"
	pl      *videoPlayer
	urls    []string    // the proxy's, forgotten at the end
	back    *image.RGBA // the video's picture while it opens
	ended   bool
	retried bool      // the links were refused once and asked for again
	openAt  time.Time // when this video began to open (for the log)
	linksAt time.Time // YouTube's links work from then (the spinner says so)
	timed   bool      // its first frame's time is logged

	ctl       *Anim // the title and the controls over the picture (1 shown)
	ctlUntil  time.Time
	flash     *Anim   // the play mark after a pause, fading (1 shown)
	seekTo    float64 // the place chosen with LEFT / RIGHT
	seeking   bool
	seekAt    time.Time
	skipAt    time.Time
	spinDrawn int // the spinner's step last drawn
}

// playVideo opens the player on e. info: its formats when known already
// (the video's page): it plays at once. list and idx: the list it is from
// (L / R); autoNext: a playlist (each video follows the last); at: where
// to start (s).
func (ui *IpodUI) playVideo(e ytEntry, th *Album, info *ytInfo, list []ytEntry, idx int, autoNext bool, at float64) {
	if th == nil {
		th = &Album{Name: e.Title}
	}
	vv := &VideoView{e: e, thumb: th, list: list, idx: idx, autoNext: autoNext, at: at, preInfo: info, ctl: newAnim(1), flash: newAnim(0)}
	v := &View{ID: "video", Title: e.Title, Kind: KindFull, Custom: vv}
	vv.v = v
	v.onClose = func() { vv.stop(ui) }
	ui.showView(v, "")
	vv.open(ui)
}

// open: yt-dlp finds the video's streams (unless its page did), then the
// player starts; meanwhile the video's picture shows, dimmed.
func (vv *VideoView) open(ui *IpodUI) {
	vv.gen++
	gen := vv.gen
	vv.err, vv.info, vv.ended, vv.load = "", nil, false, nil
	vv.openAt, vv.timed = time.Now(), false
	vv.showControls()
	if vv.back == nil {
		go vv.fetchBack(ui, gen, vv.e.ID)
	}
	if s, ok := videoSavedInfo(vv.e.ID); ok {
		vv.preInfo = nil
		vv.playSaved(ui, s)
		return
	}
	if info := vv.preInfo; info != nil {
		vv.preInfo = nil
		vv.start(ui, info)
		return
	}
	vv.load = newYTLoading()
	ctx, cancel := context.WithTimeout(context.Background(), 120*time.Second)
	vv.cancel = cancel
	go func() {
		step := func(line string) {
			if label, at, ok := ytStep(line); ok {
				ui.post(func() {
					if gen == vv.gen && vv.load != nil {
						vv.load.set(label, at)
					}
				})
			}
		}
		// (no HLS: the formats played are the others, and its list is one
		// more request)
		info, err := ytJSONSteps(ctx, step, "--no-playlist", "--extractor-args", "youtube:skip=hls", vv.e.link())
		cancel()
		if err == nil {
			logMsg(fmt.Sprintf("Videos: %s: yt-dlp opened it in %.1f s (%s)", info.ID, time.Since(vv.openAt).Seconds(), ytRuntimeName()))
		}
		ui.postSure(func() {
			if gen != vv.gen {
				return
			}
			vv.load = nil
			if err != nil {
				vv.err = "Couldn't open it · " + iaErrText(err)
				ui.dirty = true
				return
			}
			vv.start(ui, info)
		})
	}()
}

// fetchBack: the video's picture (16:9, as YouTube makes them), for the
// screen while the video opens.
func (vv *VideoView) fetchBack(ui *IpodUI, gen int, id string) {
	if id == "" {
		return
	}
	ctx, done := context.WithTimeout(context.Background(), 20*time.Second)
	defer done()
	saved := videoSavedImage(id) // (a downloaded video's, offline)
	for _, u := range []string{"", "https://i.ytimg.com/vi/" + id + "/mqdefault.jpg", "https://i.ytimg.com/vi/" + id + "/hqdefault.jpg"} {
		img := saved
		if u != "" {
			img = ytFetchImage(ctx, u)
		}
		if img == nil {
			continue
		}
		w, h := videoFit(img.Bounds().Dx(), img.Bounds().Dy(), videoBoxW, 360)
		sc := scaleImage(img, w, h)
		ui.postSure(func() {
			if gen == vv.gen {
				vv.back = sc
				ui.dirty = true
			}
		})
		return
	}
}

// start: the video's info is there: its streams are chosen, then played.
func (vv *VideoView) start(ui *IpodUI, info *ytInfo) {
	vv.info = info
	if info.Title != "" && info.Title != vv.e.Title {
		vv.v.Title = info.Title
		if ui.topScreen() == vv.v {
			ui.headerTitle = info.Title
		}
	}
	if vv.back == nil && info.ID != "" && vv.e.ID == "" {
		go vv.fetchBack(ui, vv.gen, info.ID) // (opened from a link)
	}
	if info.IsLive {
		vv.err = "Couldn't play it · live videos can't be played yet"
		ui.dirty = true
		return
	}
	// the links tried first (the spinner turns meanwhile)
	gen, retried := vv.gen, vv.retried
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	vv.cancel = cancel
	if at := ytLinksAt(info.Formats); at > 0 {
		vv.linksAt = time.Unix(0, int64(at*1e9))
		if d := time.Until(vv.linksAt); d > 0 {
			logMsg(fmt.Sprintf("Videos: %s: YouTube's links work in %.1f s: waiting", info.ID, d.Seconds()))
		}
	}
	go func() {
		ytWaitLinks(ctx, ytLinksAt(info.Formats))
		vf, af, err := ytChooseStreams(ctx, info)
		cancel()
		if isRefusal(err) && !retried {
			// every link refused: YouTube does that for a moment now and
			// then; new ones after a pause, once
			logMsg("Videos: " + info.ID + ": every link refused: asking for new ones")
			time.Sleep(2 * time.Second)
			ui.postSure(func() {
				if gen == vv.gen {
					vv.retried = true
					vv.retry(ui)
				}
			})
			return
		}
		ui.postSure(func() {
			if gen != vv.gen {
				return
			}
			if err != nil {
				vv.err = "Couldn't play it · " + videoErrText(err)
				ui.dirty = true
				return
			}
			vv.play(ui, info, vf, af)
		})
	}()
}

// play: the streams through the proxy, the music paused, the player
// started.
func (vv *VideoView) play(ui *IpodUI, info *ytInfo, vf, af *ytFormat) {
	logMsg(fmt.Sprintf("Videos: %s: picture %s (%s %.0fx%.0f %.3g fps), links checked at %.1f s", info.ID, vf.ID, vf.VCodec, vf.W, vf.H, vf.FPS, time.Since(vv.openAt).Seconds()))
	vurl, err := videoProxy.add(vf.URL, ytHeaders(info, vf), int64(vf.Size))
	if err != nil {
		vv.err = "Couldn't play it · " + err.Error()
		ui.dirty = true
		return
	}
	aurl := vurl
	vv.urls = []string{vurl}
	if af != nil && af != vf {
		logMsg(fmt.Sprintf("Videos: %s: sound %s (%s %.0f kbit/s)", info.ID, af.ID, af.ACodec, af.ABR))
		if aurl, err = videoProxy.add(af.URL, ytHeaders(info, af), int64(af.Size)); err != nil {
			vv.err = "Couldn't play it · " + err.Error()
			ui.dirty = true
			return
		}
		vv.urls = append(vv.urls, aurl)
	}
	src := vSource{Video: vurl, Audio: aurl, W: int(vf.W), H: int(vf.H), FPS: vf.FPS, Duration: info.Duration}
	if af == nil {
		src.Audio = ""
	}
	vv.playSource(ui, info, src)
}

// playSaved: a downloaded video, from its file.
func (vv *VideoView) playSaved(ui *IpodUI, s videoSaved) {
	logMsg(fmt.Sprintf("Videos: %s: from its file (%dx%d)", s.ID, s.W, s.H))
	info := &ytInfo{ytEntry: s.entry()}
	vv.info = info
	if s.Title != "" && s.Title != vv.v.Title {
		vv.v.Title = s.Title
	}
	f := videoSavedPath(s.ID, ".mp4")
	vv.playSource(ui, info, vSource{Video: f, Audio: f, W: s.W, H: s.H, FPS: s.FPS, Duration: s.Duration})
}

// playSource: the music paused, the player started on src.
func (vv *VideoView) playSource(ui *IpodUI, info *ytInfo, src vSource) {
	ui.stopListenFor()         // a Listen stops (its page is under the player)
	ui.pauseMusicFor("Videos") // the music pauses for the video, as on the iPod
	p, err := newVideoPlayer(src, vv.at)
	if err != nil {
		vv.err = "Couldn't play it · " + err.Error()
		if !ffmpegReady() {
			vv.err = "Couldn't play it · this system has no ffmpeg (Videos → Get ffmpeg)"
		}
		ui.dirty = true
		return
	}
	ui.videoGain()
	vv.pl = p
	e := vv.e
	if e.Title == "" || strings.HasPrefix(e.Title, "http") {
		e.Title = info.Title
	}
	e.ID = info.ID
	if e.Channel == "" {
		e.Channel = info.who()
	}
	vv.e = e
	videoRemember(e, vv.at, info.Duration)
	ui.dirty = true
}

// stop: the player closed and the place remembered (the screen leaves, or
// another video comes in it).
func (vv *VideoView) stop(ui *IpodUI) {
	vv.gen++
	if vv.cancel != nil {
		vv.cancel()
		vv.cancel = nil
	}
	if p := vv.pl; p != nil {
		pos := p.pos
		if vv.ended {
			pos = 0
		}
		dur := 0.0
		if vv.info != nil {
			dur = vv.info.Duration
		}
		videoRemember(vv.e, pos, dur)
		p.close()
		vv.pl = nil
		ui.refreshVideosMenu()
	}
	videoProxy.forget(vv.urls...)
	vv.urls = nil
	vv.seeking = false
}

// skip: the previous (-1) or next (+1) video of the list, in this screen.
func (vv *VideoView) skip(ui *IpodUI, d int) bool {
	i := vv.idx + d
	if i < 0 || i >= len(vv.list) {
		return false
	}
	vv.stop(ui)
	vv.idx, vv.e, vv.at, vv.retried = i, vv.list[i], 0, false
	vv.thumb = &Album{Name: vv.e.Title}
	vv.back = nil
	vv.v.Title = vv.e.Title
	if ui.topScreen() == vv.v {
		ui.headerTitle = vv.e.Title
	}
	vv.open(ui)
	return true
}

// showControls: the title and the controls, for 2.5 s from now (longer
// while nothing plays: still).
func (vv *VideoView) showControls() {
	vv.ctlUntil = animNow().Add(2500 * time.Millisecond)
	if vv.ctl.Target() != 1 {
		vv.ctl.To(1, 150*time.Millisecond, easeCSS, 0)
	}
}

// still: nothing moves on its own (the controls stay up).
func (vv *VideoView) still() bool {
	p := vv.pl
	return p == nil || p.paused || p.state != vPlaying || vv.seeking || vv.err != ""
}

func (vv *VideoView) togglePause(ui *IpodUI) {
	vv.showControls()
	switch p := vv.pl; {
	case vv.err != "":
		vv.retried = false
		vv.retry(ui)
	case p == nil:
	case p.state == vEnded:
		vv.ended = false
		p.paused = false
		p.seek(0)
	default:
		if p.paused && p.state != vFailed {
			// playing again: the play mark, a moment at the centre
			vv.flash.Set(1)
			vv.flash.To(0, 450*time.Millisecond, easeCSS, 350*time.Millisecond)
		}
		p.setPaused(!p.paused)
	}
	ui.dirty = true
}

// retry: after an error, again from where it was.
func (vv *VideoView) retry(ui *IpodUI) {
	if p := vv.pl; p != nil {
		vv.at = p.pos
	}
	vv.stop(ui)
	vv.open(ui)
}

func (vv *VideoView) pause(ui *IpodUI) {
	if p := vv.pl; p != nil && !p.paused && p.state != vEnded {
		p.setPaused(true)
		vv.showControls()
		ui.dirty = true
	}
}

func (vv *VideoView) seekBy(ui *IpodUI, d float64) {
	p := vv.pl
	if p == nil || p.state == vFailed {
		return
	}
	now := animNow()
	if !vv.seeking {
		vv.seekTo, vv.seeking = p.pos, true
	} else if now.Sub(vv.seekAt) < 250*time.Millisecond {
		d /= 2 // held: 5 s a step
	}
	vv.seekTo = math.Max(0, vv.seekTo+d)
	if dur := p.src.Duration; dur > 0 {
		vv.seekTo = math.Min(vv.seekTo, math.Max(0, dur-1))
	}
	vv.seekAt = now
	vv.showControls()
	ui.dirty = true
}

func (vv *VideoView) OnKey(ui *IpodUI, k Key) bool {
	switch k {
	case START, X:
		vv.togglePause(ui)
	case LEFT, RIGHT:
		d := 10.0
		if k == LEFT {
			d = -d
		}
		vv.seekBy(ui, d)
	case UP:
		vv.OnScroll(ui, 1, 0)
	case DOWN:
		vv.OnScroll(ui, -1, 0)
	case L, R:
		// held, it would go through the list: one step a press
		if now := animNow(); now.Sub(vv.skipAt) > 600*time.Millisecond {
			vv.skipAt = now
			d := 1
			if k == L {
				d = -1
			}
			if !vv.skip(ui, d) {
				ui.showToast("No other video in the list")
			}
		}
	default:
		if k != A && k != B {
			vv.showControls() // any other key brings them back (A: OnCenter)
		}
		return false
	}
	return true
}

// OnCenter: A shows the title and the controls when they're hidden, as a
// tap on a phone's video does; with them shown it plays or pauses.
func (vv *VideoView) OnCenter(ui *IpodUI) {
	if p := vv.pl; p != nil && p.state == vPlaying && !p.paused && vv.err == "" && vv.ctl.Target() == 0 {
		vv.showControls()
		ui.dirty = true
		return
	}
	vv.togglePause(ui)
}

// OnScroll: the wheel (and UP / DOWN) is the volume, as on Now Playing.
func (vv *VideoView) OnScroll(ui *IpodUI, dir int, velocity float64) {
	if dir > 0 {
		ui.volume = math.Min(ui.volume+0.04, 1)
	} else {
		ui.volume = math.Max(ui.volume-0.04, 0.01)
	}
	ui.applyVolume()
	ui.videoGain()
	ui.savePrefs()
	ui.showVolumeOSD()
	vv.showControls()
}

func (vv *VideoView) OnLongPress(ui *IpodUI) {
	var opts []SheetOption
	if p := vv.pl; p != nil && p.state != vFailed {
		opts = append(opts, SheetOption{Label: "Watch from the Start", OnSelect: func() {
			vv.ended, p.paused = false, false
			p.seek(0)
		}})
	}
	if vv.e.ID != "" {
		// (offered: the video streams either way)
		opts = append(opts, ui.videoDownloadOption(vv.e, vv.info))
	}
	if info := vv.info; info != nil {
		if len(info.Chapters) >= 2 {
			a := albumFromVideo(info, true)
			opts = append(opts, SheetOption{Label: fmt.Sprintf("Download as Album (%d tracks)", len(a.Tracks)),
				OnSelect: func() { ui.startYTDownload(info, a) }})
		}
		one := albumFromVideo(info, false)
		opts = append(opts, SheetOption{Label: "Download as One Song", OnSelect: func() { ui.startYTDownload(info, one) }})
	}
	if len(opts) > 0 {
		ui.showSheetTitled(vv.v.Title, opts)
	}
}

// Animating: the controls fading; while it opens or waits, a frame per
// step of the spinner (12 a turn, 14 a second). (The video's frames are asked
// for by videoTick.)
func (vv *VideoView) Animating(now time.Time) bool {
	if vv.ctl.Running() || vv.flash.Running() {
		return true
	}
	return vv.waiting() && spinnerStep(now) != vv.spinDrawn
}

// waiting: opening, loading or buffering (the spinner shows).
func (vv *VideoView) waiting() bool {
	p := vv.pl
	return vv.err == "" && (vv.load != nil || p == nil || p.state == vLoading || p.state == vBuffering)
}

// layout: the picture's size and place in the w x h screen (in the
// middle).
func (vv *VideoView) layout(w, h int) (fw, fh, ox, oy int) {
	switch {
	case vv.pl != nil:
		fw, fh = vv.pl.fw, vv.pl.fh
	case vv.back != nil:
		fw, fh = vv.back.Bounds().Dx(), vv.back.Bounds().Dy()
	default:
		fw, fh = videoFit(16, 9, w, h)
	}
	return fw, fh, (w - fw) / 2, (h - fh) / 2
}

// Render: the view's buffer, under the header (only while it slides in or
// out: in place, renderScreen draws it over the whole screen). The picture
// is where it will be on the whole screen.
func (vv *VideoView) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	vv.draw(ui, dst, -px(20), now)
}

// renderScreen: the player over the whole screen.
func (vv *VideoView) renderScreen(ui *IpodUI, dst *image.RGBA, now time.Time) {
	vv.draw(ui, dst, 0, now)
}

// draw: the player as on the whole 640 x 480 screen, into dst whose row 0
// is the screen's row -top.
func (vv *VideoView) draw(ui *IpodUI, dst *image.RGBA, top int, now time.Time) {
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	fillRect(dst, 0, 0, W, H, colBlack)
	fw, fh, ox, oy := vv.layout(W, 480)
	oy += top
	p := vv.pl
	switch {
	case p != nil && p.shown != nil:
		// the frame, row by row (it is already the right size)
		for y := 0; y < fh && oy+y < H; y++ {
			if oy+y < 0 {
				continue
			}
			d := dst.PixOffset(ox, oy+y)
			copy(dst.Pix[d:d+fw*4], p.shown[y*fw*4:(y+1)*fw*4])
		}
	case vv.back != nil:
		bw, bh := vv.back.Bounds().Dx(), vv.back.Bounds().Dy()
		bx, by := (W-bw)/2, oy+(fh-bh)/2
		blitOpaque(dst, vv.back, bx, by)
		fillRect(dst, bx, by, bw, bh, Col{0, 0, 0, 0.55})
	}
	cx, cy := float64(W)/2/DPR, float64(240+top)/DPR // the screen's centre, CSS px
	vv.spinDrawn = spinnerStep(now)
	switch {
	case vv.err != "":
		l, sub, _ := strings.Cut(vv.err, " · ")
		vv.drawCentered(dst, cx, cy-14, []string{l, sub, "Press [A] to try again"})
	case vv.load != nil:
		vv.drawLoading(dst, cx, cy, now)
	case p == nil || p.state == vLoading || p.state == vBuffering:
		label := "Loading…"
		if p != nil && p.state == vBuffering {
			label = "Buffering…"
		} else if p == nil && now.Before(vv.linksAt) {
			label = fmt.Sprintf("Waiting for YouTube · %d s", int(math.Ceil(vv.linksAt.Sub(now).Seconds())))
		}
		drawSpinnerIn(dst, cx*DPR, (cy-8)*DPR, 24, now, colWhite)
		vv.drawCentered(dst, cx, cy+16, []string{label})
	case p.state == vEnded:
		vv.drawCentered(dst, cx, cy-8, []string{"Press [A] to watch again"})
	case p.paused && !vv.seeking:
		drawPauseGlyph(dst, cx, cy, 1)
	default:
		if a := vv.flash.Get(now); a > 0.01 {
			drawPlayGlyph(dst, cx, cy, a)
		}
	}
	// the title and the controls: for a moment after a key, and while
	// nothing plays
	if !vv.still() && now.After(vv.ctlUntil) && vv.ctl.Target() != 0 {
		vv.ctl.To(0, 300*time.Millisecond, easeCSS, 0)
	}
	if a := vv.ctl.Get(now); a > 0.01 {
		vv.drawTitle(dst, top, a)
		vv.drawControls(ui, dst, 480-px(40)+top, a, now)
	}
}

// drawTitle: the video's title at the top, on a dark gradient, faded by a.
func (vv *VideoView) drawTitle(dst *image.RGBA, top int, a float64) {
	W := dst.Bounds().Dx()
	l := newCanvasCached(&videoTitleLayer, W, px(40))
	clearRGBA(l)
	fillVGradient(l, 0, 0, W, px(40), []GradStop{{0, Col{0, 0, 0, 0.8}}, {0.6, Col{0, 0, 0, 0.55}}, {1, Col{0, 0, 0, 0}}})
	f := face(W700, 13)
	_, asc := lineBox(13)
	t := ellipsize(f, vv.v.Title, float64(W)-24*DPR)
	drawTextShadow(l, f, t, 12*DPR, (9+asc)*DPR, colWhite, colBlack)
	blit(dst, l, 0, top, a)
}

var videoTitleLayer *image.RGBA

// drawControls: the time gone, the bar and the time left at the bottom
// (from row y0 of dst), on a dark gradient, faded by a. While the volume
// changes, the volume instead (as on the iPod: the header that shows it
// elsewhere is hidden here).
func (vv *VideoView) drawControls(ui *IpodUI, dst *image.RGBA, y0 int, a float64, now time.Time) {
	W := dst.Bounds().Dx()
	l := newCanvasCached(&videoCtlLayer, W, px(40))
	clearRGBA(l)
	fillVGradient(l, 0, 0, W, px(40), []GradStop{{0, Col{0, 0, 0, 0}}, {0.35, Col{0, 0, 0, 0.6}}, {1, Col{0, 0, 0, 0.8}}})
	y := 12.0 // (16 high, its reflection under it)
	if now.Before(volOSD.until) {
		blit(l, volIcon(headphonesOut(), colWhite), px(12), px(y+1), 1)
		bx := 12.0 + 14 + 8
		ui.drawProgressBar(l, bx, y, float64(W)/DPR-bx-12, ui.volume*100, false)
		blit(dst, l, 0, y0, a)
		return
	}
	pos, dur := vv.at, vv.e.Duration
	if vv.info != nil && vv.info.Duration > 0 {
		dur = vv.info.Duration
	}
	if p := vv.pl; p != nil {
		pos = p.pos
		if p.state == vEnded {
			pos = dur
		}
	}
	if vv.seeking {
		pos = vv.seekTo
	}
	cur, rem := videoTime(pos, dur), "--:--"
	if dur > 0 {
		rem = "-" + videoTime(math.Max(0, dur-pos), dur)
	}
	f := face(W700, 12)
	_, asc := lineBox(12)
	lw := math.Max(textWidth(f, cur), textWidth(f, rem))/DPR + 2
	white := Col{255, 255, 255, 1}
	drawText(l, f, cur, 12*DPR, (y+0.5+asc)*DPR, white)
	drawText(l, f, rem, (float64(W)/DPR-12-textWidth(f, rem)/DPR)*DPR, (y+0.5+asc)*DPR, white)
	bx := 12 + lw + 8
	bw := float64(W)/DPR - 2*bx
	pct := 0.0
	if dur > 0 {
		pct = pos / dur * 100
	}
	ui.drawProgressBar(l, bx, y, bw, pct, vv.seeking)
	blit(dst, l, 0, y0, a)
}

var videoCtlLayer *image.RGBA

// drawLoading: the spinner while yt-dlp opens the video, with its step and
// the seconds so far (no bar: nothing is downloaded yet, only asked for).
func (vv *VideoView) drawLoading(dst *image.RGBA, cx, cy float64, now time.Time) {
	l := vv.load
	secs := int(time.Since(l.start).Seconds())
	drawSpinnerIn(dst, cx*DPR, (cy-14)*DPR, 24, now, colWhite)
	vv.drawCentered(dst, cx, cy+10, []string{"Opening the video", fmt.Sprintf("%s · %d s", l.stepLabel, secs)})
}

// drawCentered: lines of white text centred at cx from cy (CSS px), the
// first bold, with a shadow (they may be over a picture).
func (vv *VideoView) drawCentered(dst *image.RGBA, cx, cy float64, lines []string) {
	y := cy
	for i, s := range lines {
		if s == "" {
			continue
		}
		size, w := 12.0, W500
		if i == 0 {
			size, w = 14, W700
		}
		f := face(w, size)
		lh, asc := lineBox(size)
		s = ellipsize(f, s, 300*DPR)
		drawTextShadow(dst, f, s, cx*DPR-textWidth(f, s)/2, (y+asc)*DPR, colWhite, colBlack)
		y += lh + 2
	}
}

// drawPauseGlyph: two bars on a dark disc at the screen's centre while it
// is paused (a: its opacity).
func drawPauseGlyph(dst *image.RGBA, cx, cy, a float64) {
	fillCircle(dst, cx, cy, 24, Col{0, 0, 0, 0.45 * a})
	for _, x := range []float64{cx - 8, cx + 2} {
		fillRoundedGradient(dst, x*DPR, (cy-10)*DPR, (x+6)*DPR, (cy+10)*DPR, 2*DPR,
			[]GradStop{{0, Col{255, 255, 255, 0.95}}, {1, Col{225, 225, 225, 0.95}}}, a)
	}
}

// drawPlayGlyph: the play mark on a dark disc, a moment after a pause.
func drawPlayGlyph(dst *image.RGBA, cx, cy, a float64) {
	fillCircle(dst, cx, cy, 24, Col{0, 0, 0, 0.45 * a})
	fillTriangle(dst, (cx-6)*DPR, (cy-11)*DPR, (cx-6)*DPR, (cy+11)*DPR, (cx+12)*DPR, cy*DPR, Col{255, 255, 255, 0.95 * a})
}

// videoTime: m:ss (h:mm:ss for an hour or more), as long as dur's.
func videoTime(sec, dur float64) string {
	s := int(math.Max(0, sec))
	if dur >= 3600 || s >= 3600 {
		return fmt.Sprintf("%d:%02d:%02d", s/3600, s/60%60, s%60)
	}
	return fmt.Sprintf("%02d:%02d", s/60, s%60)
}

// ---- the main loop's part ----

// videoTick (every loop): each player follows its sound and gets its
// frame; a screen covered by another one pauses; a playlist goes on.
func (ui *IpodUI) videoTick(now time.Time) {
	for _, v := range ui.full {
		vv, ok := v.Custom.(*VideoView)
		if !ok || vv.pl == nil {
			continue
		}
		p := vv.pl
		top := ui.topScreen() == v && !v.exiting
		if !top && !p.paused {
			vv.pause(ui) // another screen over it (a download's)
		}
		if vv.seeking && now.Sub(vv.seekAt) >= 450*time.Millisecond {
			vv.seeking, vv.ended = false, false
			p.seek(vv.seekTo)
			ui.dirty = true
		}
		if p.tick(now) && top {
			ui.dirty = true
		}
		p.perfLine(now)
		switch p.state {
		case vEnded:
			if !vv.ended && vv.refused(ui) {
				continue // (not the end: YouTube stopped giving it)
			}
			if !vv.ended {
				vv.ended = true
				vv.showControls()
				ui.dirty = true
				if vv.autoNext && top {
					vv.skip(ui, 1)
				}
			}
		case vFailed:
			if vv.err == "" && !vv.refused(ui) {
				vv.err = "Couldn't play it · " + p.err
				ui.dirty = true
			}
		case vPlaying:
			if !p.paused {
				ui.app.LastActivityTime = now // no screensaver while it plays
			}
			if !vv.timed && p.shown != nil {
				vv.timed = true
				vv.showControls() // the title and the time, a moment as it starts
				logMsg(fmt.Sprintf("Videos: %s: playing %.1f s after it was chosen", vv.e.ID, time.Since(vv.openAt).Seconds()))
			}
		}
	}
}

// refused: YouTube refused the video's links (403: refused now and then,
// or expired after some hours): asked for again once, from where it was;
// after that the error shows. It returns false when nothing was refused.
func (vv *VideoView) refused(ui *IpodUI) bool {
	code := videoProxy.refusal(vv.urls...)
	if code == 0 {
		return false
	}
	if !vv.retried && (code == 403 || code == 410) {
		logMsg(fmt.Sprintf("Videos: the links were refused (%d): asking for them again", code))
		vv.retried = true
		vv.retry(ui)
		return true
	}
	vv.err = fmt.Sprintf("Couldn't play it · YouTube refused the stream (error %d)", code)
	ui.dirty = true
	return true
}

// videoFrameWait: how long the loop may sleep before the next frame of
// the video on screen.
func (ui *IpodUI) videoFrameWait() (time.Duration, bool) {
	if vv := ui.videoOnTop(); vv != nil && vv.pl != nil {
		return vv.pl.frameWait()
	}
	return 0, false
}

// videoOnTop: the player when it is the screen on top.
func (ui *IpodUI) videoOnTop() *VideoView {
	if len(ui.full) == 0 {
		return nil
	}
	if v := ui.topScreen(); !v.exiting {
		if vv, ok := v.Custom.(*VideoView); ok {
			return vv
		}
	}
	return nil
}

// refreshVideosMenu: Videos' lists built again, under the screens over
// them (a row comes or goes: Recently Watched, Downloaded; split menus
// aren't built again when they come back on top).
func (ui *IpodUI) refreshVideosMenu() {
	for _, st := range [][]*View{ui.split, ui.full} {
		for _, v := range st {
			if (v.ID == "videos" || v.ID == "recentVideos" || v.ID == "savedVideos") && v.Build != nil {
				idx := v.Index
				v.Items = v.Build()
				v.Index = max(0, min(idx, len(v.Items)-1))
				v.dirty = true
			}
		}
	}
	ui.dirty = true
}

// videoInPlace: the player on top and still (not sliding in or out): it is
// drawn over the whole screen, header included.
func (ui *IpodUI) videoInPlace(now time.Time) *VideoView {
	vv := ui.videoOnTop()
	if vv == nil || ui.fullX.Get(now) != 0 {
		return nil
	}
	if v := vv.v; v.enter.Get(now) != 0 || v.hide.Get(now) != 0 {
		return nil
	}
	return vv
}

// videoPlaying: a player that is playing (to pause it with the music).
func (ui *IpodUI) videoPlaying() *VideoView {
	for _, v := range ui.full {
		if vv, ok := v.Custom.(*VideoView); ok && vv.pl != nil && !vv.pl.paused && vv.pl.state != vEnded {
			return vv
		}
	}
	return nil
}

// videoMediaKey: headphone and Bluetooth buttons go to the video on screen.
func (ui *IpodUI) videoMediaKey(k Key) bool {
	vv := ui.videoOnTop()
	if vv == nil {
		return false
	}
	p := vv.pl
	playing := p != nil && !p.paused && p.state != vEnded
	switch k {
	case MEDIA_PLAYPAUSE, MEDIA_HOOK:
		vv.togglePause(ui)
	case MEDIA_PLAY:
		if !playing {
			vv.togglePause(ui)
		}
	case MEDIA_PAUSE, MEDIA_STOP:
		vv.pause(ui)
	case MEDIA_NEXT:
		vv.skip(ui, 1)
	case MEDIA_PREV:
		vv.skip(ui, -1)
	default:
		return false // (the volume, unplugging: as usual)
	}
	ui.dirty = true
	return true
}

// videoHeaderIcon: the header's play / pause for the video on screen ("":
// none yet).
func (vv *VideoView) headerIcon() string {
	p := vv.pl
	switch {
	case p == nil || p.state == vFailed:
		return ""
	case p.paused || p.state == vEnded:
		return "pause"
	}
	return "play"
}

// stopListenFor: a Listen playing (ij_listen.go) stops for a video, and
// the music it set aside stays paused: previewStop would play it again,
// reloading it into the mixer under the video.
func (ui *IpodUI) stopListenFor() {
	previewMu.Lock()
	p := previewCur
	if p != nil {
		p.userWasPlaying = false
	}
	previewMu.Unlock()
	if p != nil {
		ui.previewStop()
		logMsg("Videos: the Listen stopped")
	}
}

// pauseMusicFor pauses the music (and its Now Playing) for a video. A
// song still loading (a skip) pauses once it plays (togglePlayPause).
func (ui *IpodUI) pauseMusicFor(why string) {
	if p := ui.app.Playing; p != nil && p.Track != nil && p.State == StatePlaying {
		ui.app.togglePlayPause()
		logMsg(why + ": the music paused")
	}
}

// videoGain: the video's sound at the app's own volume when the console's
// isn't used (MIYOOPOD_SOFT_VOLUME), as the mixer plays the music.
func (ui *IpodUI) videoGain() {
	g := 1.0
	if !isOnion && !sysVol.ok {
		g = float64(int(ui.volume*100+0.5)) / 128
	}
	videoSetGain(g)
}

// ytFetchImage: a picture from the web (nil if it doesn't load).
func ytFetchImage(ctx context.Context, url string) image.Image {
	req, err := http.NewRequestWithContext(ctx, "GET", url, nil)
	if err != nil {
		return nil
	}
	resp, err := updateClient(20 * time.Second).Do(req)
	if err != nil {
		return nil
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return nil
	}
	img, _, err := image.Decode(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return nil
	}
	return img
}

// ---- the preview ----

var videosIconImg *image.RGBA

// videosIcon: the preview's picture: a screen with a play mark, in the
// glossy style of the other apps' icons.
func videosIcon() *image.RGBA {
	if videosIconImg != nil {
		return videosIconImg
	}
	const S = 192.0
	img := newCanvas(int(S), int(S))
	fillRoundedGradient(img, 12, 12, S-12, S-12, 36, []GradStop{{0, Col{86, 98, 118, 1}}, {1, Col{28, 33, 44, 1}}}, 1)
	// the screen, with a film's edges
	fillRoundedGradient(img, 30, 48, S-30, S-48, 10, []GradStop{{0, Col{40, 120, 220, 1}}, {1, Col{18, 52, 120, 1}}}, 1)
	for i := 0; i < 6; i++ {
		x := 34 + float64(i)*22
		fillRoundedGradient(img, x, 30, x+12, 40, 3, []GradStop{{0, Col{255, 255, 255, 0.75}}, {1, Col{255, 255, 255, 0.55}}}, 1)
		fillRoundedGradient(img, x, S-40, x+12, S-30, 3, []GradStop{{0, Col{255, 255, 255, 0.75}}, {1, Col{255, 255, 255, 0.55}}}, 1)
	}
	// the play mark
	cx, cy := S/2+4, S/2
	for row := -28; row <= 28; row++ {
		w := (28 - math.Abs(float64(row))) * 1.7
		fillRect(img, int(cx-24), int(cy)+row, int(w), 1, Col{255, 255, 255, 0.95})
	}
	// the gloss
	fillRoundedGradient(img, 15, 15, S-15, S/2, 32, []GradStop{{0, Col{255, 255, 255, 0.4}}, {1, Col{255, 255, 255, 0.04}}}, 1)
	videosIconImg = img
	return img
}
