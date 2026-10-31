package main

// ij_listen.go — Listen, on an Internet Archive item's page and a YouTube
// video's or playlist's: a song plays right there, with play and pause on
// its row, before (and without) a download. No Now Playing: the page stays,
// with Download under it.
//
// The songs are fetched into the database folder's tmp/listen (not the
// music folder: they don't join the library): the one asked for and the
// one after it, no further. Each plays once it is whole (YouTube's audio,
// fragmented MP4, is made a plain M4A first, as for a download), then the
// next one, to the last. The user's own music is set aside meanwhile (paused, its
// queue and place kept) and comes back as it was when the preview stops:
// leaving the page, Download, or the last song's end. The playback poller
// leaves the mixer alone while a preview plays (previewActive).

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

// listenSong: one song to listen to: its title, and how to get its file.
type listenSong struct {
	Title    string
	Duration float64
	Ext      string                                                                       // ".mp3", ".m4a"…
	fetch    func(ctx context.Context, dst string, progress func(got, total int64)) error // into dst
}

type preview struct {
	key    string // the page's item
	songs  []listenSong
	cancel context.CancelFunc
	kick   chan struct{} // another song asked for: the fetcher looks again

	mu      sync.Mutex
	files   []string        // where each song goes
	ready   []chan struct{} // closed when its file is whole (or failed)
	failed  []string        // why a song couldn't come ("" when it came)
	got     []int64         // bytes so far, by song
	total   []int64
	cur     int  // the song asked for
	loaded  bool // it is in the mixer
	paused  bool
	waiting bool // for its file
	err     string

	// the user's own music, set aside
	userWasPlaying bool
}

var (
	previewMu  sync.Mutex
	previewCur *preview
)

func listenDir() string { return filepath.Join(dataDir(), "tmp", "listen") }

// previewActive: a preview has the mixer (the poller leaves it alone).
func previewActive() bool {
	previewMu.Lock()
	defer previewMu.Unlock()
	return previewCur != nil
}

// previewRow: what a song's row of key says: playing or paused, and where
// (ok false: not this one).
type previewInfo struct {
	ok, loading, paused bool
	cur                 int
	pos, dur, progress  float64
	err                 string
}

func previewState(key string) previewInfo {
	previewMu.Lock()
	p := previewCur
	previewMu.Unlock()
	if p == nil || p.key != key {
		return previewInfo{}
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	in := previewInfo{ok: true, cur: p.cur, paused: p.paused, err: p.err}
	if !p.loaded {
		in.loading = true
		if t := p.total[p.cur]; t > 0 {
			in.progress = float64(p.got[p.cur]) / float64(t)
		}
		return in
	}
	st := audioGetState()
	in.pos, in.dur = st.Position, st.Duration
	if in.dur <= 0 {
		in.dur = p.songs[p.cur].Duration
	}
	if in.dur > 0 {
		in.progress = in.pos / in.dur
	}
	return in
}

// previewRow: the row of song i of a page's preview (the Listen row is
// song 0 with label Listen): its label, sublabel and bar.
func (ui *IpodUI) previewRow(key string, songs []listenSong, i int, label, sub string, refresh func()) Item {
	it := Item{Label: label, Sublabel: sub, NoArrow: true,
		OnSelect: func() { ui.previewToggle(key, songs, i, refresh) }}
	in := previewState(key)
	if !in.ok || in.cur != i {
		return it
	}
	switch {
	case in.err != "":
		it.Sublabel = in.err + " · press [A] to try again"
	case in.loading:
		it.Label, it.Sublabel = "Loading…", fmt.Sprintf("%d%% · %s", int(in.progress*100), songs[i].Title)
		it.Progress = max(in.progress, 0.02)
	default:
		state := "Playing"
		if in.paused {
			state = "Paused · press [A] to play"
		}
		it.Label = songs[i].Title
		it.Sublabel = fmt.Sprintf("%s · %s of %s", state, formatTrackLength(in.pos), formatTrackLength(in.dur))
		it.Progress = max(in.progress, 0.01)
	}
	return it
}

// previewToggle: A on a song's row: it plays (fetched first), or pauses,
// or plays on.
func (ui *IpodUI) previewToggle(key string, songs []listenSong, i int, refresh func()) {
	previewMu.Lock()
	p := previewCur
	previewMu.Unlock()
	if p != nil && p.key == key && p.cur == i && p.err == "" {
		p.mu.Lock()
		if p.loaded {
			p.paused = !p.paused
			if p.paused {
				audioPause()
			} else {
				audioResume()
			}
		}
		p.mu.Unlock()
		refresh()
		return
	}
	if p != nil && p.key == key && p.err == "" {
		ui.previewPlay(p, i, refresh) // another song of the same page
		return
	}
	ui.previewStop()
	ui.previewStart(key, songs, i, refresh)
}

func (ui *IpodUI) previewStart(key string, songs []listenSong, first int, refresh func()) {
	if len(songs) == 0 {
		return
	}
	os.RemoveAll(listenDir())
	os.MkdirAll(listenDir(), 0o755)
	ctx, cancel := context.WithCancel(context.Background())
	p := &preview{key: key, songs: songs, cancel: cancel, cur: first, kick: make(chan struct{}, 1),
		got: make([]int64, len(songs)), total: make([]int64, len(songs)), failed: make([]string, len(songs))}
	for i, s := range songs {
		p.files = append(p.files, filepath.Join(listenDir(), fmt.Sprintf("%02d%s", i+1, s.Ext)))
		p.ready = append(p.ready, make(chan struct{}))
	}
	// the user's music is set aside: paused, its queue and place kept
	app := ui.app
	if pl := app.Playing; pl != nil && pl.Track != nil && pl.State == StatePlaying {
		p.userWasPlaying = true
	}
	if pl := app.Playing; pl != nil && pl.Track != nil && pl.State != StateStopped {
		pl.State = StatePaused
		audioPause()
	}
	previewMu.Lock()
	previewCur = p
	previewMu.Unlock()
	logMsg(fmt.Sprintf("Listen: %s, %d songs, from %d", key, len(songs), first+1))
	// the songs come one at a time: the one asked for, then the one after
	// it (no further: a profile's or a playlist's dozens of songs aren't
	// fetched ahead); another song asked for comes next
	go func() {
		fetched := make([]bool, len(songs))
		for {
			p.mu.Lock()
			cur := p.cur
			p.mu.Unlock()
			i := -1
			for _, j := range []int{cur, cur + 1} {
				if j < len(songs) && !fetched[j] {
					i = j
					break
				}
			}
			if i < 0 {
				select {
				case <-ctx.Done():
					return
				case <-p.kick:
				}
				continue
			}
			fetched[i] = true
			dst := p.files[i]
			prog := func(got, total int64) {
				p.mu.Lock()
				p.got[i], p.total[i] = got, total
				p.mu.Unlock()
			}
			var err error
			for try := 0; try < 3; try++ {
				if err = songs[i].fetch(ctx, dst+".part", prog); err == nil || ctx.Err() != nil {
					break
				}
				time.Sleep(2 * time.Second)
			}
			if ctx.Err() != nil {
				return
			}
			if err == nil {
				err = os.Rename(dst+".part", dst)
			}
			p.mu.Lock()
			if err != nil {
				logMsg(fmt.Sprintf("Listen: %q: %v", songs[i].Title, err))
				p.failed[i] = iaErrText(err)
			}
			close(p.ready[i])
			p.mu.Unlock()
		}
	}()
	// the page is redrawn while it plays (the time, the bar), and the next
	// song follows the end of one
	go func() {
		tick := time.NewTicker(250 * time.Millisecond)
		defer tick.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-tick.C:
			}
			p.mu.Lock()
			loaded, paused, cur := p.loaded, p.paused, p.cur
			p.mu.Unlock()
			if loaded && !paused && audioCheckFinished() {
				if cur+1 < len(songs) {
					ui.postSure(func() { ui.previewPlay(p, cur+1, refresh) })
				} else {
					ui.postSure(func() { ui.previewStop(); refresh() })
					return
				}
			}
			ui.post(refresh)
		}
	}()
	ui.previewPlay(p, first, refresh)
}

// previewPlay: song i of the preview p plays as soon as its file is whole.
func (ui *IpodUI) previewPlay(p *preview, i int, refresh func()) {
	p.mu.Lock()
	p.cur, p.loaded, p.paused, p.err = i, false, false, ""
	ready, file := p.ready[i], p.files[i]
	p.mu.Unlock()
	select {
	case p.kick <- struct{}{}:
	default:
	}
	audioStop()
	refresh()
	go func() {
		<-ready
		previewMu.Lock()
		still := previewCur == p
		previewMu.Unlock()
		p.mu.Lock()
		if !still || p.cur != i {
			p.mu.Unlock()
			return // another song (or none) was asked for meanwhile
		}
		why := p.failed[i]
		p.mu.Unlock()
		err := error(nil)
		if why == "" {
			err = ui.app.mpvLoadFile(file)
		}
		p.mu.Lock()
		switch {
		case why != "":
			p.err = why
		case err != nil:
			p.err = "Can't play it"
			logMsg("Listen: " + err.Error())
		default:
			p.loaded = true
		}
		p.mu.Unlock()
		ui.post(refresh)
	}()
}

// previewStop: the preview ends (the page left, Download, its last song's
// end), and the user's music comes back as it was.
func (ui *IpodUI) previewStop() {
	previewMu.Lock()
	p := previewCur
	previewCur = nil
	previewMu.Unlock()
	if p == nil {
		return
	}
	p.cancel()
	audioStop()
	app := ui.app
	if pl := app.Playing; pl != nil && pl.Track != nil {
		pl.State = StatePaused
		if p.userWasPlaying {
			// (play reloads the track into the mixer at its place)
			go app.mpvTogglePause()
		}
	}
	app.NPCacheDirty = true
	ui.dirty = true
}

// httpFetch: url into dst, with the progress (a Listen song from
// archive.org).
func httpFetch(ctx context.Context, u, dst string, progress func(got, total int64)) error {
	req, err := http.NewRequestWithContext(ctx, "GET", u, nil)
	if err != nil {
		return err
	}
	req.Header.Set("User-Agent", "betterPod/"+BETTERPOD_VERSION)
	resp, err := updateClient(0).Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return fmt.Errorf("the server answered %d", resp.StatusCode)
	}
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer out.Close()
	buf := make([]byte, 64<<10)
	var got int64
	for {
		n, rerr := resp.Body.Read(buf)
		if n > 0 {
			if _, err := out.Write(buf[:n]); err != nil {
				return err
			}
			got += int64(n)
			progress(got, resp.ContentLength)
		}
		if rerr == io.EOF {
			return nil
		}
		if rerr != nil {
			return rerr
		}
	}
}

// iaListenSongs: an Internet Archive item's songs for Listen: its MP3s
// when it has them (the smallest), else its first format betterPod plays.
func iaListenSongs(it *iaItem) []listenSong {
	g := ""
	for _, o := range []string{"MP3", "FLAC", "WAV", "AIFF", "ALAC"} {
		if len(it.Groups[o]) > 0 {
			if _, no := onionUnsupported(".m4a"); no && o == "ALAC" {
				continue // (not on the Miyoo)
			}
			g = o
			break
		}
	}
	var out []listenSong
	for _, f := range it.Groups[g] {
		u := iaBase() + "/download/" + url.PathEscape(it.ID) + "/" + escapePath(f.Name)
		out = append(out, listenSong{Title: f.Title, Duration: f.Length, Ext: strings.ToLower(filepath.Ext(f.Name)),
			fetch: func(ctx context.Context, dst string, progress func(int64, int64)) error {
				return httpFetch(ctx, u, dst, progress)
			}})
	}
	return out
}

// ytListenSongs: a YouTube video (one song, all of it) or playlist (its
// videos) for Listen; yt-dlp gets each one's audio (m4a, as a download).
func ytListenSongs(i *ytInfo) []listenSong {
	one := func(link, title string, dur float64, infoJSON []byte) listenSong {
		return listenSong{Title: title, Duration: dur, Ext: ".m4a",
			fetch: func(ctx context.Context, dst string, progress func(int64, int64)) error {
				work := filepath.Join(listenDir(), "work")
				os.MkdirAll(work, 0o755)
				infoPath := ""
				if infoJSON != nil {
					infoPath = filepath.Join(work, "info.json")
					_ = os.WriteFile(infoPath, infoJSON, 0o644)
				}
				var got string
				errs, err := ytFetch(ctx, []string{link}, infoPath, work,
					func(_ string, g, t int64) { progress(g, t) },
					func(_, path string) { got = path })
				if err == nil {
					for _, e := range errs {
						err = e
					}
				}
				if got == "" {
					if err == nil {
						err = fmt.Errorf("no audio")
					}
					return err
				}
				// YouTube's audio is fragmented MP4: made a plain M4A, as a
				// download's tracks are (the mixer plays only that)
				m, err := openM4A(got)
				if err != nil {
					return err
				}
				err = m.WriteRange(dst, 0, m.Duration(), m4aTags{Title: title})
				m.Close()
				os.Remove(got)
				return err
			}}
	}
	if len(i.Entries) > 0 {
		var out []listenSong
		for _, e := range i.Entries {
			if e.ID == "" || e.Avail == "private" || e.Avail == "needs_auth" ||
				e.Title == "[Deleted video]" || e.Title == "[Private video]" {
				continue
			}
			out = append(out, one("https://www.youtube.com/watch?v="+e.ID, ytClean(e.Title), e.Duration, nil))
		}
		return out
	}
	return []listenSong{one(i.link(), ytClean(i.Title), i.Duration, i.raw)}
}
