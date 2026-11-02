package main

// ij_videodl.go — Videos: a video downloaded to watch offline (Y →
// Download → As Video, on the player, a result, Recently Watched or Watch
// Video on the video's page; As Audio Only is the songs' download). Videos are
// streamed: the download is only offered. yt-dlp gets the picture and the
// sound the player would stream (H.264 at the size that fills the screen,
// AAC) and joins them into one MP4 with ffmpeg. It lands in the data
// folder's videos/, not the music folder (the library would take an MP4 for
// a song), with a small JSON beside it (title, channel, the picture's size)
// and the video's picture. Videos → Downloaded lists them, and a video
// downloaded plays from its file wherever it is chosen: no network, no
// wait.

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
)

// videoSaved: a downloaded video's record (videos/<id>.json).
type videoSaved struct {
	ID       string  `json:"id"`
	Title    string  `json:"title"`
	Channel  string  `json:"channel,omitempty"`
	Duration float64 `json:"duration,omitempty"`
	W        int     `json:"width,omitempty"`
	H        int     `json:"height,omitempty"`
	FPS      float64 `json:"fps,omitempty"`
	Size     int64   `json:"size,omitempty"`
	At       int64   `json:"at"`
}

func (s videoSaved) entry() ytEntry {
	return ytEntry{ID: s.ID, Title: s.Title, Channel: s.Channel, Duration: s.Duration}
}

func videosDir() string { return dataPath("videos/") }

// videoSavedPath: a downloaded video's file (ext ".mp4"), record (".json")
// or picture (".jpg").
func videoSavedPath(id, ext string) string { return filepath.Join(videosDir(), safeName(id)+ext) }

// videoSavedInfo: the record of a downloaded video (ok false when it isn't
// downloaded).
func videoSavedInfo(id string) (videoSaved, bool) {
	if id == "" || !fileExists(videoSavedPath(id, ".mp4")) {
		return videoSaved{}, false
	}
	data, err := os.ReadFile(videoSavedPath(id, ".json"))
	if err != nil {
		return videoSaved{}, false
	}
	var s videoSaved
	if json.Unmarshal(data, &s) != nil || s.ID == "" {
		return videoSaved{}, false
	}
	return s, true
}

// videosSaved: the downloaded videos, the newest first.
func videosSaved() []videoSaved {
	ents, _ := os.ReadDir(videosDir())
	var out []videoSaved
	for _, e := range ents {
		if id, ok := strings.CutSuffix(e.Name(), ".json"); ok {
			if s, ok := videoSavedInfo(id); ok {
				out = append(out, s)
			}
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].At > out[j].At })
	return out
}

func videoDeleteSaved(id string) {
	for _, ext := range []string{".mp4", ".json", ".jpg"} {
		os.Remove(videoSavedPath(id, ext))
	}
}

// videoSavedImage: a downloaded video's picture, as it was saved (nil if
// none).
func videoSavedImage(id string) image.Image {
	f, err := os.Open(videoSavedPath(id, ".jpg"))
	if err != nil {
		return nil
	}
	defer f.Close()
	img, _, err := image.Decode(io.LimitReader(f, 4<<20))
	if err != nil {
		return nil
	}
	return img
}

// videoDownloadFormats: what yt-dlp is asked for (as its -f): the picture
// and the sound the player would choose first, else the format with both
// (also the fallback when the pair can't be had).
func videoDownloadFormats(info *ytInfo) (sel string, video, audio *ytFormat) {
	vids, auds, both := ytCandidates(info.Formats)
	switch {
	case len(vids) > 0 && len(auds) > 0:
		sel = vids[0].ID + "+" + auds[0].ID
		if len(both) > 0 {
			sel += "/" + both[0].ID
		}
		return sel, vids[0], auds[0]
	case len(both) > 0:
		return both[0].ID, both[0], nil
	}
	return "", nil, nil
}

// startVideoDownload: the download screen, and yt-dlp getting the video
// (info: its formats when known, else they are asked for first).
func (ui *IpodUI) startVideoDownload(e ytEntry, info *ytInfo) {
	if iaCur != nil && !iaCur.finished {
		ui.showView(ui.iaDownloadView(), "")
		return
	}
	title, who := e.Title, e.who()
	if info != nil {
		if info.Title != "" {
			title = info.Title
		}
		if w := info.who(); w != "" {
			who = w
		}
	}
	d := &iaDownload{item: &iaItem{Title: title, Creator: who}, files: []iaFile{{Title: title}}, format: "MP4",
		dir: videosDir(), cancel: make(chan struct{}), state: "Starting…", source: "YouTube", video: true}
	// (Try Again asks for the links again: the ones it had may have expired)
	ui.startDownloadJob(d, func() error { return videoDownloadRun(d, e, info) }, func() { ui.startVideoDownload(e, nil) })
}

// videoDownloadRun: the download itself (on its own goroutine). What came
// stays in the work folder, and Try Again goes on from there.
func videoDownloadRun(d *iaDownload, e ytEntry, info *ytInfo) error {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go func() {
		select {
		case <-d.cancel:
			cancel()
		case <-ctx.Done():
		}
	}()
	setState := func(s string, whole bool) {
		d.mu.Lock()
		d.state, d.whole = s, whole
		d.mu.Unlock()
	}
	if !ffmpegReady() {
		return errors.New("This system has no ffmpeg (Videos → Get ffmpeg)")
	}
	if info == nil || len(info.Formats) == 0 || len(info.raw) == 0 {
		setState("Opening the video…", true)
		i, err := ytJSON(ctx, "--no-playlist", "--extractor-args", "youtube:skip=hls", e.link())
		if err != nil {
			if ctx.Err() != nil {
				return errIACancelled
			}
			return err
		}
		info = i
	}
	if info.IsLive {
		return errors.New("Live videos can't be downloaded")
	}
	sel, vf, af := videoDownloadFormats(info)
	if sel == "" {
		return errors.New("No format this console can play")
	}
	sizes := map[string]float64{vf.ID: vf.Size}
	if af != nil {
		sizes[af.ID] = af.Size
	}
	var total float64
	for _, n := range sizes {
		total += n
	}
	d.mu.Lock()
	d.total = int64(total)
	d.mu.Unlock()
	dir := videosDir()
	if err := os.MkdirAll(dir, 0755); err != nil {
		return err
	}
	// room for the two parts and the MP4 they become
	if fb := freeBytes(dir); fb >= 0 && fb < int64(total)*2+50<<20 {
		return fmt.Errorf("Not enough space (%s free)", formatBytes(fb))
	}
	work := filepath.Join(ytToolsDir(), "work", "video-"+safeName(info.ID))
	if err := os.MkdirAll(work, 0755); err != nil {
		return err
	}
	infoJSON := filepath.Join(work, "info.json")
	if err := os.WriteFile(infoJSON, info.raw, 0644); err != nil {
		return err
	}
	setState("Downloading the video", false)
	args := []string{"-f", sel, "--merge-output-format", "mp4", "--ffmpeg-location", ffmpegPath(), "--no-playlist",
		"--progress", "--newline",
		"--progress-template", "download:KP %(info.format_id)s %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s",
		"-o", filepath.Join(work, "%(id)s.%(ext)s"), "--print", "after_move:KPDONE %(filepath)s",
		"--load-info-json", infoJSON}
	cmd := ytCommand(ctx, args...)
	// (ffmpeg, which yt-dlp runs to join them, wants the system's libraries)
	var env []string
	for _, kv := range cmd.Env {
		if !strings.HasPrefix(kv, "LD_LIBRARY_PATH=") && !strings.HasPrefix(kv, "LD_PRELOAD=") {
			env = append(env, kv)
		}
	}
	cmd.Env = env
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return err
	}
	if err := cmd.Start(); err != nil {
		return err
	}
	var lastErr string
	errDone := make(chan struct{})
	go func() {
		defer close(errDone)
		sc := bufio.NewScanner(stderr)
		sc.Buffer(make([]byte, 0, 64<<10), 1<<20)
		for sc.Scan() {
			if l := strings.TrimSpace(sc.Text()); strings.HasPrefix(l, "ERROR:") {
				lastErr = l
			}
		}
	}()
	got := map[string]int64{} // each part's bytes so far
	final := ""
	sc := bufio.NewScanner(stdout)
	sc.Buffer(make([]byte, 0, 64<<10), 1<<20)
	for sc.Scan() {
		line := sc.Text()
		f := strings.Fields(line)
		switch {
		case len(f) >= 5 && f[0] == "KP":
			n, _ := strconv.ParseFloat(f[2], 64)
			t, _ := strconv.ParseFloat(f[3], 64)
			if t <= 0 {
				t, _ = strconv.ParseFloat(f[4], 64)
			}
			got[f[1]] = int64(n)
			var done int64
			for _, b := range got {
				done += b
			}
			d.mu.Lock()
			d.done = done
			if t > 0 && sizes[f[1]] == 0 {
				// a size yt-dlp didn't know before: now it does
				sizes[f[1]] = t
				d.total = 0
				for _, s := range sizes {
					d.total += int64(s)
				}
			}
			d.mu.Unlock()
		case strings.HasPrefix(line, "[Merger]"):
			setState("Joining the picture and the sound…", true)
		case len(f) >= 2 && f[0] == "KPDONE":
			final = strings.TrimSpace(strings.TrimPrefix(line, "KPDONE"))
		}
	}
	io.Copy(io.Discard, stdout)
	werr := cmd.Wait()
	<-errDone
	if ctx.Err() != nil {
		return errIACancelled
	}
	if werr != nil || final == "" {
		if lastErr != "" {
			return ytError(lastErr, werr)
		}
		if werr == nil {
			werr = errors.New("yt-dlp gave no file")
		}
		return werr
	}
	if err := os.Rename(final, videoSavedPath(info.ID, ".mp4")); err != nil {
		return err
	}
	rec := videoSaved{ID: info.ID, Title: info.Title, Channel: info.who(), Duration: info.Duration,
		W: int(vf.W), H: int(vf.H), FPS: vf.FPS, At: time.Now().Unix()}
	if st, err := os.Stat(videoSavedPath(info.ID, ".mp4")); err == nil {
		rec.Size = st.Size()
	}
	data, _ := json.Marshal(rec)
	if err := writeFileAtomic(videoSavedPath(info.ID, ".json"), data); err != nil {
		os.Remove(videoSavedPath(info.ID, ".mp4"))
		return err
	}
	// its picture, for the list and while it opens (not needed to play)
	setState("Saving its picture…", true)
	if b := videoFetchBytes(ctx, "https://i.ytimg.com/vi/"+info.ID+"/mqdefault.jpg"); b != nil {
		_ = writeFileAtomic(videoSavedPath(info.ID, ".jpg"), b)
	}
	os.RemoveAll(work)
	d.mu.Lock()
	d.done = d.total
	d.mu.Unlock()
	return nil
}

// videoFetchBytes: a small file from the web (nil when it can't be had).
func videoFetchBytes(ctx context.Context, url string) []byte {
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
	b, err := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return nil
	}
	return b
}

// savedVideosView: Videos → Downloaded.
func (ui *IpodUI) savedVideosView() *View {
	v := &View{ID: "savedVideos", Title: "Downloaded", Kind: KindFull, Empty: "No videos downloaded"}
	thumbs := map[string]*Album{}
	v.Build = func() []Item {
		saved := videosSaved()
		var list []ytEntry
		for _, s := range saved {
			list = append(list, s.entry())
		}
		var items []Item
		for i, s := range saved {
			th := thumbs[s.ID]
			if th == nil {
				th = &Album{Name: s.Title}
				if img := videoSavedImage(s.ID); img != nil {
					b := img.Bounds()
					n := min(b.Dx(), b.Dy())
					sq := image.Rect(b.Min.X+(b.Dx()-n)/2, b.Min.Y+(b.Dy()-n)/2, b.Min.X+(b.Dx()-n)/2+n, b.Min.Y+(b.Dy()-n)/2+n)
					th.ArtImg = scaleImage(cropTo(img, sq), 96, 96)
				}
				thumbs[s.ID] = th
			}
			var parts []string
			if s.Channel != "" {
				parts = append(parts, s.Channel)
			}
			if s.Duration > 0 {
				parts = append(parts, formatTrackLength(s.Duration))
			}
			if s.Size > 0 {
				parts = append(parts, formatBytes(s.Size))
			}
			idx, id := i, s.ID
			items = append(items, Item{Label: s.Title, Sublabel: strings.Join(parts, " · "), Album: th,
				OnSelect: func() { ui.playVideo(list[idx], th, nil, list, idx, false, videoPlace(id)) },
				LongPress: []SheetOption{
					{Label: "Watch from the Start", OnSelect: func() { ui.playVideo(list[idx], th, nil, list, idx, false, 0) }},
					{Label: "Delete Download", Danger: true, OnSelect: func() {
						videoDeleteSaved(id)
						ui.refreshVideosMenu()
					}},
				}})
		}
		return items
	}
	v.Items = v.Build()
	return v
}

// videoDownloadOptions: the download rows of a video's options (Y):
// Download, which opens a second sheet (As Video; As Audio Only, the songs'
// download of the video's page: one song, or an album of its chapters), and
// Delete Download once the video is downloaded (Download then offers the
// audio only). info: the video's, when known (else the audio's album is
// made from the list's entry, and yt-dlp reads the video when it starts).
func (ui *IpodUI) videoDownloadOptions(e ytEntry, info *ytInfo) []SheetOption {
	_, saved := videoSavedInfo(e.ID)
	audio := info
	if audio == nil {
		audio = &ytInfo{ytEntry: e}
	}
	open := func() {
		var opts []SheetOption
		if !saved {
			opts = append(opts, SheetOption{Label: "As Video", OnSelect: func() { ui.startVideoDownload(e, info) }})
		}
		one := albumFromVideo(audio, false)
		opts = append(opts, SheetOption{Label: "As Audio Only", OnSelect: func() { ui.startYTDownload(audio, one) }})
		if len(audio.Chapters) >= 2 {
			a := albumFromVideo(audio, true)
			opts = append(opts, SheetOption{Label: fmt.Sprintf("As an Album (%d tracks)", len(a.Tracks)),
				OnSelect: func() { ui.startYTDownload(audio, a) }})
		}
		ui.showSheetTitled("Download", opts)
	}
	opts := []SheetOption{{Label: "Download", OnSelect: open}}
	if saved {
		opts = append(opts, SheetOption{Label: "Delete Download", Danger: true, OnSelect: func() {
			videoDeleteSaved(e.ID)
			ui.refreshVideosMenu()
		}})
	}
	return opts
}

// videoPlace: where Recently Watched left a video (0: from the start).
func videoPlace(id string) float64 {
	for _, s := range videoHistory() {
		if s.ID == id {
			return s.Pos
		}
	}
	return 0
}
