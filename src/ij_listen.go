package main

// ij_listen.go — Listen, on an Internet Archive item's page and a YouTube
// video's or playlist's: its songs play before (and without) a download.
//
// The songs are fetched one after the other into the database folder's
// tmp/listen (not the music folder: they don't join the library), and the
// first one plays as soon as it is whole; the next ones keep coming while
// it plays. Skipping to one that hasn't come yet waits for it
// (listenWait, from playCurrentQueueTrack). A new Listen empties the
// folder first, so it never holds more than one item's songs. Now Playing
// shows the item's picture (listenAlbumFor, from trackAlbum).

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

// listenSong: one song to listen to: its tags, and how to get its file.
type listenSong struct {
	Title    string
	Duration float64
	Ext      string                                                                       // ".mp3", ".m4a"…
	fetch    func(ctx context.Context, dst string, progress func(got, total int64)) error // into dst
}

type listenSession struct {
	key    string // what is listened to (the item, the video)
	cancel context.CancelFunc
	album  *Album // the item's name, artist and picture (Now Playing)
	tracks []*Track

	mu      sync.Mutex
	ready   map[string]chan struct{} // closed when the song's file is whole
	got     int64                    // the first song: bytes so far
	total   int64
	playing bool   // the first song started
	err     string // the first song couldn't be fetched
}

var (
	listenMu  sync.Mutex
	listenCur *listenSession
)

func listenDir() string { return filepath.Join(dataDir(), "tmp", "listen") }

// listenWait: a song of the Listen session that hasn't come yet is waited
// for (at most 3 minutes, or until another Listen starts). Player goroutine.
func listenWait(path string) {
	listenMu.Lock()
	s := listenCur
	listenMu.Unlock()
	if s == nil {
		return
	}
	s.mu.Lock()
	ch := s.ready[path]
	s.mu.Unlock()
	if ch == nil {
		return
	}
	select {
	case <-ch:
	case <-time.After(3 * time.Minute):
	}
}

// isListenPath: a song of a Listen (not counted in the statistics, not
// kept as the queue to come back to).
func isListenPath(p string) bool {
	return strings.HasPrefix(p, listenDir()+string(filepath.Separator))
}

// listenPlayingNow: a Listen song is playing (or paused) now.
func listenPlayingNow() bool {
	ui := ipodUI
	if ui == nil || ui.app.Playing == nil {
		return false
	}
	p := ui.app.Playing
	return p.Track != nil && p.State != StateStopped && isListenPath(p.Track.Path)
}

// listenAlbumFor: the album of a Listen song (its item's picture).
func listenAlbumFor(t *Track) *Album {
	if t == nil || !isListenPath(t.Path) {
		return nil
	}
	listenMu.Lock()
	defer listenMu.Unlock()
	if listenCur != nil {
		return listenCur.album
	}
	return nil
}

// listenState: what the Listen row says for key ("" when not this one).
func listenState(key string) (label, sub string, progress float64) {
	listenMu.Lock()
	s := listenCur
	listenMu.Unlock()
	if s == nil || s.key != key {
		return "", "", 0
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	switch {
	case s.err != "":
		return "Couldn't play it", s.err + " · press [A] to try again", 0
	case s.playing && listenPlayingNow():
		return "Now Playing", "Press [A] to see it", 0
	case s.playing:
		return "", "", 0 // (heard, and something else plays now: Listen again)
	case s.total > 0:
		p := float64(s.got) / float64(s.total)
		return "Loading…", fmt.Sprintf("%d%% of the first song", int(p*100)), max(p, 0.02)
	}
	return "Loading…", "Getting the first song", 0.02
}

// startListen plays songs (key: the item's own, for its row) with the
// item's name, artist and picture; refresh redraws its page.
func (ui *IpodUI) startListen(key string, album *Album, songs []listenSong, refresh func()) {
	if len(songs) == 0 {
		return
	}
	listenMu.Lock()
	old := listenCur
	listenMu.Unlock()
	if old != nil && old.key == key && old.err == "" && (!old.playing || listenPlayingNow()) {
		if old.playing {
			ui.showNowPlaying() // already this one
		}
		return
	}
	if old != nil {
		old.cancel()
	}
	// what was being listened to before goes (unless it is playing now)
	if p := ui.app.Playing; p != nil && p.Track != nil && isListenPath(p.Track.Path) {
		ui.clearNowPlaying()
	}
	os.RemoveAll(listenDir())
	os.MkdirAll(listenDir(), 0o755)
	ctx, cancel := context.WithCancel(context.Background())
	s := &listenSession{key: key, cancel: cancel, album: album, ready: map[string]chan struct{}{}}
	for i, sg := range songs {
		t := &Track{Path: filepath.Join(listenDir(), fmt.Sprintf("%02d%s", i+1, sg.Ext)), Title: sg.Title,
			Artist: album.Artist, AlbumArtist: album.Artist, Album: album.Name, TrackNum: i + 1, TrackTotal: len(songs),
			Duration: sg.Duration}
		s.tracks = append(s.tracks, t)
		s.ready[t.Path] = make(chan struct{})
	}
	listenMu.Lock()
	listenCur = s
	listenMu.Unlock()
	logMsg(fmt.Sprintf("Listen: %q, %d songs", album.Name, len(songs)))
	tick := time.NewTicker(200 * time.Millisecond)
	go func() {
		defer tick.Stop()
		for {
			select {
			case <-tick.C:
				ui.post(refresh)
			case <-ctx.Done():
				return
			}
		}
	}()
	go func() {
		for i, sg := range songs {
			t := s.tracks[i]
			progress := func(got, total int64) {
				if i == 0 {
					s.mu.Lock()
					s.got, s.total = got, total
					s.mu.Unlock()
				}
			}
			var err error
			for try := 0; try < 3; try++ {
				if err = sg.fetch(ctx, t.Path+".part", progress); err == nil || ctx.Err() != nil {
					break
				}
				time.Sleep(2 * time.Second)
			}
			if ctx.Err() != nil {
				return
			}
			if err == nil {
				err = os.Rename(t.Path+".part", t.Path)
			}
			if err != nil {
				logMsg(fmt.Sprintf("Listen: %q: %v", sg.Title, err))
				if i == 0 {
					s.mu.Lock()
					s.err = iaErrText(err)
					s.mu.Unlock()
					cancel()
					ui.post(refresh)
					return
				}
				// a song that won't come is skipped: its wait ends, it fails
			}
			s.mu.Lock()
			close(s.ready[t.Path])
			s.mu.Unlock()
			if i == 0 && err == nil {
				s.mu.Lock()
				s.playing = true
				s.mu.Unlock()
				app := ui.app
				go func() {
					app.Queue.Shuffle = false // (in its own order)
					app.playTrackFromList(s.tracks, 0)
					ui.post(func() {
						ui.onTrackChanged()
						ui.showNowPlaying()
						refresh()
					})
				}()
			}
		}
		tick.Stop()
		ui.post(refresh)
	}()
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
		return fmt.Errorf("archive.org answered %d", resp.StatusCode)
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
				return os.Rename(got, dst)
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
