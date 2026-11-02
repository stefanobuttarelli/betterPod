package main

// video.go — playing a video (Videos, ij_video.go): ffmpeg decodes it, and
// each frame shows when the sound has reached it.
//
//   - Two ffmpeg processes per stretch of playback (started again at each
//     seek): one makes RGBA frames at the size they are shown, at a steady
//     rate (the fps filter), the other the sound in the mixer's own format,
//     for the music hook (audio.c). Apart, neither waits for the other.
//   - The sound is the clock: frame i shows once the sound heard reaches
//     start + i/fps, so the picture waits when the sound waits (a slow
//     network) and frames that come late are skipped. Without sound (none
//     in the video, or after it ends) the wall clock goes on instead.
//   - YouTube's streams come through a small HTTP server on 127.0.0.1
//     (vProxy) that fetches them in 10 MB ranges, as yt-dlp does, with
//     betterPod's own certificates: ffmpeg needs no TLS, and can seek.
//   - ffmpeg is the system's (Knulli has it, for EmulationStation's videos)
//     or, where there is none, a static build Videos downloads into tools/.
//
// MIYOOPOD_VIDEO_AV_MS adds milliseconds to the sound's latency (positive
// when the picture is ahead of the sound); MIYOOPOD_FFMPEG points at another
// ffmpeg (tests).

/*
void video_audio_spec(int *rate, int *f32, int *channels);
int video_audio_hook(int on);
void video_audio_reset(void);
int video_audio_write(const void *p, int n);
int video_audio_buffered(void);
int video_audio_bytes_per_second(void);
void video_audio_pause(int on);
void video_audio_gain(float g);
double video_audio_pos(double extra);
*/
import "C"

import (
	"crypto/rand"
	hexenc "encoding/hex"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unsafe"
)

// The box a picture is fitted in: the screen under the header.
const (
	videoBoxW = 640
	videoBoxH = 440
	// frames decoded ahead (about a third of a second at 30 fps)
	videoBuffers = 12
)

// ---- ffmpeg ----

// ffmpegPath: the one downloaded into tools/ (only when the system has
// none that works), else the system's; "" if there is none.
func ffmpegPath() string {
	if p := os.Getenv("MIYOOPOD_FFMPEG"); p != "" {
		return p
	}
	if p := filepath.Join(ytToolsDir(), "ffmpeg"); fileExists(p) {
		return p
	}
	if p, err := exec.LookPath("ffmpeg"); err == nil {
		return p
	}
	for _, p := range []string{"/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg"} {
		if fileExists(p) {
			return p
		}
	}
	return ""
}

// ffmpegCheck: whether ffmpeg runs and has what Videos needs, and its
// version line (checked once, and again after a download).
var ffmpegCheck struct {
	mu      sync.Mutex
	done    bool
	ok      bool
	version string
	lacks   string // what this ffmpeg lacks ("": nothing, or there is none)
}

func ffmpegReady() bool {
	ffmpegCheck.mu.Lock()
	defer ffmpegCheck.mu.Unlock()
	if ffmpegCheck.done {
		return ffmpegCheck.ok
	}
	ffmpegCheck.done, ffmpegCheck.ok, ffmpegCheck.lacks = true, false, ""
	p := ffmpegPath()
	if p == "" {
		logMsg("Videos: no ffmpeg")
		return false
	}
	out, err := ffmpegCmd(p, "-hide_banner", "-version").Output()
	if err != nil {
		logMsg("Videos: " + p + " doesn't run: " + err.Error())
		return false
	}
	ffmpegCheck.version, _, _ = strings.Cut(string(out), "\n")
	logMsg("Videos: " + p + ": " + ffmpegCheck.version)
	if l := ffmpegLacks(p); l != "" {
		ffmpegCheck.lacks = l
		logMsg("Videos: " + p + " has no " + l)
		return false
	}
	ffmpegCheck.ok = true
	return true
}

// ffmpegLacks: what of Videos' needs an ffmpeg was built without: the
// http protocol (the streams' server), the H.264 and AAC decoders, the
// filters ("" when it has them all).
func ffmpegLacks(p string) string {
	list := func(arg string, field int) map[string]bool {
		out, _ := ffmpegCmd(p, "-hide_banner", arg).Output()
		m := map[string]bool{}
		for _, l := range strings.Split(string(out), "\n") {
			if f := strings.Fields(l); len(f) > field {
				m[f[field]] = true
			}
		}
		return m
	}
	protocols, decoders, filters := list("-protocols", 0), list("-decoders", 1), list("-filters", 1)
	var lacks []string
	for _, n := range []string{"http"} {
		if !protocols[n] {
			lacks = append(lacks, n)
		}
	}
	for _, n := range []string{"h264", "aac"} {
		if !decoders[n] {
			lacks = append(lacks, n)
		}
	}
	for _, n := range []string{"fps", "scale", "aresample"} {
		if !filters[n] {
			lacks = append(lacks, n)
		}
	}
	return strings.Join(lacks, ", ")
}

// ffmpegChecked: whether ffmpegReady has looked already (without looking),
// and what it found lacking in the one there is ("": none, or none lacks).
func ffmpegChecked() (done, ok bool, lacks string) {
	ffmpegCheck.mu.Lock()
	defer ffmpegCheck.mu.Unlock()
	return ffmpegCheck.done, ffmpegCheck.ok, ffmpegCheck.lacks
}

// ffmpegRecheck: the next ffmpegReady looks again (after a download).
func ffmpegRecheck() {
	ffmpegCheck.mu.Lock()
	ffmpegCheck.done = false
	ffmpegCheck.mu.Unlock()
}

// ffmpegCmd runs ffmpeg without the app's library path (betterpod/libs
// holds its own builds of a few libraries; the system's ffmpeg wants the
// system's).
func ffmpegCmd(path string, args ...string) *exec.Cmd {
	cmd := exec.Command(path, args...)
	for _, e := range os.Environ() {
		if !strings.HasPrefix(e, "LD_LIBRARY_PATH=") && !strings.HasPrefix(e, "LD_PRELOAD=") {
			cmd.Env = append(cmd.Env, e)
		}
	}
	return cmd
}

// ffmpegVideoArgs: the picture of src from start (s), as raw RGBA frames
// of w x h at fps, on stdout. The fps filter starting at 0 makes frame i
// the picture at start + i/fps.
func ffmpegVideoArgs(src string, start, fps float64, w, h int) []string {
	a := []string{"-nostdin", "-hide_banner", "-loglevel", "error"}
	if start > 0 {
		a = append(a, "-ss", strconv.FormatFloat(start, 'f', 3, 64))
	}
	vf := fmt.Sprintf("fps=fps=%s:start_time=0,scale=%d:%d:flags=bilinear", strconv.FormatFloat(fps, 'f', 3, 64), w, h)
	return append(a, "-i", src, "-map", "0:v:0", "-an", "-sn", "-dn", "-vf", vf,
		"-pix_fmt", "rgba", "-f", "rawvideo", "pipe:1")
}

// ffmpegAudioArgs: the sound of src from start, as raw samples in the
// mixer's format on stdout, from 0 (silence first if the sound starts
// late).
func ffmpegAudioArgs(src string, start float64, rate int, f32 bool, ch int) []string {
	a := []string{"-nostdin", "-hide_banner", "-loglevel", "error"}
	if start > 0 {
		a = append(a, "-ss", strconv.FormatFloat(start, 'f', 3, 64))
	}
	format := "s16le"
	if f32 {
		format = "f32le"
	}
	return append(a, "-i", src, "-map", "0:a:0", "-vn", "-sn", "-dn",
		"-af", "aresample=async=1:first_pts=0", "-ac", strconv.Itoa(ch), "-ar", strconv.Itoa(rate),
		"-f", format, "pipe:1")
}

// videoFit: the size a w x h picture is shown at in a box (even sizes;
// 16:9 when the size isn't known).
func videoFit(w, h, boxW, boxH int) (int, int) {
	if w <= 0 || h <= 0 {
		w, h = 16, 9
	}
	fw, fh := boxW, boxW*h/w
	if fh > boxH {
		fw, fh = boxH*w/h, boxH
	}
	fw, fh = fw&^1, fh&^1
	return max(fw, 2), max(fh, 2)
}

// videoFPS: the rate frames are made at: the video's own, halved down to
// 30 at most (60 fps would only cost twice for the same screen).
func videoFPS(fps float64) float64 {
	if fps <= 1 {
		return 30
	}
	for fps > 30.5 {
		fps /= 2
	}
	return fps
}

// tail keeps the last bytes written to it (a process's errors).
type tail struct {
	mu  sync.Mutex
	buf []byte
}

func (t *tail) Write(p []byte) (int, error) {
	t.mu.Lock()
	t.buf = append(t.buf, p...)
	if len(t.buf) > 4096 {
		t.buf = append([]byte(nil), t.buf[len(t.buf)-4096:]...)
	}
	t.mu.Unlock()
	return len(p), nil
}

// lastLine: the last line written, short (without the proxy's address
// ffmpeg puts before its errors).
func (t *tail) lastLine() string {
	t.mu.Lock()
	defer t.mu.Unlock()
	lines := strings.Split(strings.TrimSpace(string(t.buf)), "\n")
	l := strings.TrimSpace(lines[len(lines)-1])
	if strings.HasPrefix(l, "http://127.0.0.1:") {
		if _, rest, ok := strings.Cut(l, ": "); ok {
			l = rest
		}
	}
	if r := []rune(l); len(r) > 90 {
		l = string(r[:90]) + "…"
	}
	return l
}

// ---- frames ----

type vFrame struct {
	pix []byte
	t   float64
}

// vQueue: the frames decoded ahead, in order, and the free buffers.
type vQueue struct {
	mu     sync.Mutex
	cond   *sync.Cond
	free   [][]byte
	ready  []vFrame
	closed bool
}

func newVQueue(bufs [][]byte) *vQueue {
	q := &vQueue{free: bufs}
	q.cond = sync.NewCond(&q.mu)
	return q
}

// get: a buffer for the next frame (it waits for one); nil once closed.
func (q *vQueue) get() []byte {
	q.mu.Lock()
	defer q.mu.Unlock()
	for len(q.free) == 0 && !q.closed {
		q.cond.Wait()
	}
	if q.closed {
		return nil
	}
	b := q.free[len(q.free)-1]
	q.free = q.free[:len(q.free)-1]
	return b
}

func (q *vQueue) put(b []byte, t float64) {
	q.mu.Lock()
	if q.closed {
		q.free = append(q.free, b)
	} else {
		q.ready = append(q.ready, vFrame{b, t})
	}
	q.mu.Unlock()
}

func (q *vQueue) giveBack(b []byte) {
	q.mu.Lock()
	q.free = append(q.free, b)
	q.cond.Signal()
	q.mu.Unlock()
}

// take: the newest frame due at t, and its time (those before it are
// skipped: their buffers go back, and skipped counts them); nil when none
// is due.
func (q *vQueue) take(t float64) (due []byte, at float64, skipped int) {
	q.mu.Lock()
	defer q.mu.Unlock()
	n := 0
	for n < len(q.ready) && q.ready[n].t <= t+0.002 {
		if due != nil {
			q.free = append(q.free, due)
			skipped++
		}
		due, at = q.ready[n].pix, q.ready[n].t
		n++
	}
	if n > 0 {
		q.ready = append(q.ready[:0], q.ready[n:]...)
		q.cond.Broadcast()
	}
	return due, at, skipped
}

// next: the time of the next frame, if one is decoded.
func (q *vQueue) next() (float64, bool) {
	q.mu.Lock()
	defer q.mu.Unlock()
	if len(q.ready) == 0 {
		return 0, false
	}
	return q.ready[0].t, true
}

func (q *vQueue) waiting() int {
	q.mu.Lock()
	defer q.mu.Unlock()
	return len(q.ready)
}

// close wakes the reader and gives back every buffer it holds.
func (q *vQueue) close() [][]byte {
	q.mu.Lock()
	defer q.mu.Unlock()
	q.closed = true
	q.cond.Broadcast()
	out := q.free
	for _, f := range q.ready {
		out = append(out, f.pix)
	}
	q.free, q.ready = nil, nil
	return out
}

// ---- the player ----

// vSource: what a player plays.
type vSource struct {
	Video, Audio string  // what each ffmpeg opens: a URL (the proxy's) or a file; the same one for both when it has both
	W, H         int     // the picture's size (0: unknown)
	FPS          float64 // its frame rate (0: unknown)
	Duration     float64 // (0: unknown)
}

type vState int

const (
	vLoading   vState = iota // starting, or after a seek
	vPlaying                 // (or paused: videoPlayer.paused)
	vBuffering               // the sound ran out: waiting for more
	vEnded
	vFailed
)

// vSession: one stretch of playback, from a start to the next seek.
type vSession struct {
	start      float64
	q          *vQueue
	cmds       []*exec.Cmd
	stop       chan struct{}
	wg         sync.WaitGroup
	videoDone  atomic.Bool
	audioDone  atomic.Bool
	frames     atomic.Int64
	audioBytes atomic.Int64
	videoErr   atomic.Value // string: why the picture failed, before any frame
}

// videoPlayer: one video, from open to close. Its methods run on the UI
// goroutine; the readers of each session run apart.
type videoPlayer struct {
	src     vSource
	fw, fh  int
	fps     float64
	rate    int
	ch      int
	f32     bool
	noSound bool    // the mixer couldn't take it: the wall clock instead
	extra   float64 // seconds added to the sound's latency

	sess   *vSession
	pool   [][]byte
	state  vState
	paused bool
	err    string

	pos       float64   // where playback is (s)
	moved     time.Time // when the sound's clock last moved
	soundOver bool      // the sound has all been heard: the wall clock goes on
	wallAt    time.Time
	wallPos   float64

	shown    []byte // the frame on screen (nil: none yet)
	shownSeq int    // changes with it
	shownT   float64

	// for MIYOOPOD_PERF's line each second
	nShown, nSkipped int
	perfAt           time.Time
	specAt           time.Time // when the mixer's format was last looked at
}

// newVideoPlayer starts playing src at the position at (s).
func newVideoPlayer(src vSource, at float64) (*videoPlayer, error) {
	if !ffmpegReady() {
		return nil, errors.New("ffmpeg doesn't run")
	}
	p := &videoPlayer{src: src, fps: videoFPS(src.FPS)}
	p.fw, p.fh = videoFit(src.W, src.H, videoBoxW, videoBoxH)
	p.extra = float64(envInt("MIYOOPOD_VIDEO_AV_MS", 0, -500, 1000)) / 1000
	if C.video_audio_hook(1) == 0 {
		var rate, f32, ch C.int
		C.video_audio_spec(&rate, &f32, &ch)
		p.rate, p.f32, p.ch = int(rate), f32 != 0, int(ch)
	} else {
		p.noSound = true
		logMsg("Videos: the mixer isn't open: no sound, the picture follows the clock")
	}
	logMsg(fmt.Sprintf("Videos: %dx%d at %.3g fps, shown %dx%d; sound %d Hz, %d ch, f32 %v", src.W, src.H, src.FPS, p.fw, p.fh, p.rate, p.ch, p.f32))
	p.startSession(at)
	return p, nil
}

func (p *videoPlayer) startSession(at float64) {
	s := &vSession{start: at, stop: make(chan struct{})}
	bufs := p.pool
	p.pool = nil
	for len(bufs) < videoBuffers {
		bufs = append(bufs, make([]byte, p.fw*p.fh*4))
	}
	s.q = newVQueue(bufs)
	p.sess, p.state, p.pos, p.soundOver, p.err = s, vLoading, at, false, ""
	ff := ffmpegPath()

	vc := ffmpegCmd(ff, ffmpegVideoArgs(p.src.Video, at, p.fps, p.fw, p.fh)...)
	verr := &tail{}
	vc.Stderr = verr
	vout, err := vc.StdoutPipe()
	if err == nil {
		err = vc.Start()
	}
	if err != nil {
		s.videoErr.Store("ffmpeg: " + err.Error())
		s.videoDone.Store(true)
	} else {
		s.cmds = append(s.cmds, vc)
		s.wg.Add(1)
		go func() {
			defer s.wg.Done()
			s.readVideo(vout, p.fps)
			werr := vc.Wait()
			if werr != nil && s.frames.Load() == 0 && !s.stopped() {
				why := verr.lastLine()
				if why == "" {
					why = werr.Error()
				}
				s.videoErr.Store(why)
				logMsg("Videos: picture: " + why)
			}
		}()
	}

	if p.noSound {
		s.audioDone.Store(true)
		return
	}
	ac := ffmpegCmd(ff, ffmpegAudioArgs(p.src.Audio, at, p.rate, p.f32, p.ch)...)
	aerr := &tail{}
	ac.Stderr = aerr
	aout, err := ac.StdoutPipe()
	if err == nil {
		err = ac.Start()
	}
	if err != nil {
		logMsg("Videos: sound: " + err.Error())
		s.audioDone.Store(true)
		return
	}
	s.cmds = append(s.cmds, ac)
	s.wg.Add(1)
	go func() {
		defer s.wg.Done()
		s.readAudio(aout)
		if werr := ac.Wait(); werr != nil && !s.stopped() {
			logMsg("Videos: sound: " + aerr.lastLine() + " (" + werr.Error() + ")")
		}
	}()
}

func (s *vSession) stopped() bool {
	select {
	case <-s.stop:
		return true
	default:
		return false
	}
}

// readVideo: frames into the queue until the end (or the session stops).
func (s *vSession) readVideo(r io.Reader, fps float64) {
	defer s.videoDone.Store(true)
	for i := 0; ; i++ {
		b := s.q.get()
		if b == nil {
			return
		}
		if _, err := io.ReadFull(r, b); err != nil {
			s.q.giveBack(b)
			return
		}
		s.frames.Add(1)
		s.q.put(b, s.start+float64(i)/fps)
	}
}

// readAudio: the sound into the mixer's ring (it waits while the ring is
// full).
func (s *vSession) readAudio(r io.Reader) {
	defer s.audioDone.Store(true)
	buf := make([]byte, 32<<10)
	for {
		n, err := r.Read(buf)
		b := buf[:n]
		for len(b) > 0 {
			if s.stopped() {
				return
			}
			w := int(C.video_audio_write(unsafe.Pointer(&b[0]), C.int(len(b))))
			s.audioBytes.Add(int64(w))
			b = b[w:]
			if len(b) > 0 {
				time.Sleep(10 * time.Millisecond)
			}
		}
		if err != nil {
			return
		}
	}
}

// stopSession ends the processes and the readers; the frames' buffers
// go back to the pool (the one on screen stays).
func (p *videoPlayer) stopSession() {
	s := p.sess
	if s == nil {
		return
	}
	p.sess = nil
	close(s.stop)
	bufs := s.q.close()
	for _, c := range s.cmds {
		if c.Process != nil {
			c.Process.Kill()
		}
	}
	s.wg.Wait()
	p.pool = append(p.pool, bufs...)
	p.pool = append(p.pool, s.q.close()...) // (one the reader gave back while stopping)
}

// soundSeconds: the sound waiting in the ring.
func (p *videoPlayer) soundSeconds() float64 {
	if p.noSound {
		return 0
	}
	bps := float64(C.video_audio_bytes_per_second())
	if bps <= 0 {
		return 0
	}
	return float64(C.video_audio_buffered()) / bps
}

// tick (every loop, on screen or not): where playback is now, and the
// frame for it. It returns true when the screen must be drawn again.
func (p *videoPlayer) tick(now time.Time) bool {
	s := p.sess
	if s == nil {
		return false
	}
	if !p.noSound && now.Sub(p.specAt) > 500*time.Millisecond {
		p.specAt = now
		if p.mixerChanged() {
			return true
		}
	}
	changed := false
	buffered := p.soundSeconds()
	soundDone := s.audioDone.Load()
	noSound := p.noSound || soundDone && s.audioBytes.Load() == 0
	switch p.state {
	case vLoading:
		if why, _ := s.videoErr.Load().(string); why != "" {
			p.fail(why)
			return true
		}
		// the first frame shows at once (while the sound comes)
		if f, t, _ := s.q.take(s.start); f != nil {
			p.show(f, t)
			changed = true
		}
		haveFrame := p.shown != nil || s.videoDone.Load()
		if haveFrame && (soundDone || buffered >= 0.3) {
			p.state = vPlaying
			p.moved, p.wallAt, p.wallPos = now, now, p.pos
			if !p.paused {
				C.video_audio_pause(0)
			}
			changed = true
		}
	case vPlaying:
		if !p.paused && !soundDone && buffered < 0.02 {
			p.state = vBuffering // the network is slower than the video
			C.video_audio_pause(1)
			changed = true
		}
	case vBuffering:
		if soundDone || buffered >= 0.6 {
			p.state = vPlaying
			p.moved = now
			if !p.paused {
				C.video_audio_pause(0)
			}
			changed = true
		}
	}
	if p.state != vPlaying {
		return changed
	}
	switch {
	case p.paused:
		p.wallAt, p.wallPos = now, p.pos
	case !noSound && !p.soundOver:
		a := s.start + float64(C.video_audio_pos(C.double(p.extra)))
		if a > p.pos+0.0005 {
			p.pos, p.moved = a, now
		} else if soundDone && buffered < 0.01 && now.Sub(p.moved) > 200*time.Millisecond {
			p.soundOver = true // all heard: a longer picture goes on by the clock
		}
		p.wallAt, p.wallPos = now, p.pos
	default:
		p.pos = p.wallPos + now.Sub(p.wallAt).Seconds()
	}
	if f, t, skipped := s.q.take(p.pos); f != nil {
		p.show(f, t)
		p.nSkipped += skipped
		changed = true
	}
	if s.videoDone.Load() && s.q.waiting() == 0 && (noSound || p.soundOver) {
		p.state = vEnded
		C.video_audio_pause(1)
		changed = true
	}
	return changed
}

// mixerChanged: the mixer was opened again at another format (a song that
// was loading when the video started reopens it at its own rate): the
// sound is hooked again and asked for in the new format, from where it
// was. It returns true when it was.
func (p *videoPlayer) mixerChanged() bool {
	var rate, f32, ch C.int
	C.video_audio_spec(&rate, &f32, &ch)
	if int(rate) == p.rate && (f32 != 0) == p.f32 && int(ch) == p.ch {
		return false
	}
	logMsg(fmt.Sprintf("Videos: the mixer is now %d Hz, %d ch, f32 %v: the sound again from %.1f s", int(rate), int(ch), f32 != 0, p.pos))
	if rate <= 0 || C.video_audio_hook(1) != 0 {
		p.fail("the sound's device closed")
		return true
	}
	p.rate, p.f32, p.ch = int(rate), f32 != 0, int(ch)
	p.seek(p.pos)
	return true
}

// frameWait: how long until the next frame is due (ok false when nothing
// is playing).
func (p *videoPlayer) frameWait() (time.Duration, bool) {
	s := p.sess
	if s == nil || p.paused || p.state != vPlaying {
		return 0, false
	}
	t, ok := s.q.next()
	if !ok {
		return 10 * time.Millisecond, true // waiting for the decoder
	}
	d := time.Duration((t - p.pos) * float64(time.Second))
	return max(d, 0), true
}

func (p *videoPlayer) show(f []byte, t float64) {
	if p.shown != nil {
		if p.sess != nil {
			p.sess.q.giveBack(p.shown)
		} else {
			p.pool = append(p.pool, p.shown)
		}
	}
	p.shown, p.shownT = f, t
	p.shownSeq++
	p.nShown++
}

// perfLine: MIYOOPOD_PERF's line about the video, once a second (where it
// is, the frames shown and skipped, the sound waiting).
func (p *videoPlayer) perfLine(now time.Time) {
	if !perfLog || now.Sub(p.perfAt) < time.Second {
		return
	}
	p.perfAt = now
	waiting, made := 0, int64(0)
	if s := p.sess; s != nil {
		waiting, made = s.q.waiting(), s.frames.Load()
	}
	logMsg(fmt.Sprintf("VIDEO: state %d paused %v, at %.2f s, frame %.2f s (%d shown, %d skipped, %d ready, %d decoded), sound %.2f s waiting",
		p.state, p.paused, p.pos, p.shownT, p.nShown, p.nSkipped, waiting, made, p.soundSeconds()))
	p.nShown, p.nSkipped = 0, 0
}

func (p *videoPlayer) fail(why string) {
	p.stopSession()
	p.state, p.err = vFailed, why
	C.video_audio_pause(1)
}

func (p *videoPlayer) setPaused(on bool) {
	p.paused = on
	if p.state == vPlaying || p.state == vLoading {
		C.video_audio_pause(boolInt(on))
	}
	p.wallAt, p.wallPos = animNow(), p.pos
}

// seek: playback again from t (s): new processes, the ring emptied.
func (p *videoPlayer) seek(t float64) {
	if d := p.src.Duration; d > 0 && t > d-1 {
		t = d - 1
	}
	t = max(t, 0)
	p.stopSession()
	C.video_audio_reset()
	p.startSession(t)
}

// close stops everything; the music player is back (its song paused).
func (p *videoPlayer) close() {
	p.stopSession()
	C.video_audio_hook(0)
	p.pool, p.shown = nil, nil
}

// setGain: the app's own volume (MIYOOPOD_SOFT_VOLUME) for the sound, as
// the mixer gives it to the music (0..100 of 128); 1 with the console's.
func videoSetGain(v float64) { C.video_audio_gain(C.float(v)) }

func boolInt(b bool) C.int {
	if b {
		return 1
	}
	return 0
}

// ---- the streams' server ----

// vProxy serves remote streams to ffmpeg from 127.0.0.1: each request's
// range is fetched in pieces of vChunk (YouTube slows down bigger ones)
// with the stream's headers, read ahead into memory, and taken up again
// where it broke when the network fails.
type vProxy struct {
	once    sync.Once
	base    string
	err     error
	mu      sync.Mutex
	streams map[string]*vStream
}

type vStream struct {
	url     string
	header  http.Header
	size    atomic.Int64 // total bytes (0: not known yet)
	refused atomic.Int64 // the HTTP status the server last refused it with
}

// vRefused: a server's refusal (403: YouTube's links refused or expired).
type vRefused int

func (e vRefused) Error() string { return fmt.Sprintf("the server answered %d", int(e)) }

func isRefusal(err error) bool {
	var r vRefused
	return errors.As(err, &r) && r != 416
}

const vChunk = 10 << 20

var videoProxy vProxy

func (vp *vProxy) start() error {
	vp.once.Do(func() {
		ln, err := net.Listen("tcp", "127.0.0.1:0")
		if err != nil {
			vp.err = err
			return
		}
		vp.base = "http://" + ln.Addr().String() + "/v/"
		vp.streams = map[string]*vStream{}
		go http.Serve(ln, http.HandlerFunc(vp.serve))
	})
	return vp.err
}

// add: a local URL for a remote stream (size: its length, if known).
func (vp *vProxy) add(url string, header map[string]string, size int64) (string, error) {
	if err := vp.start(); err != nil {
		return "", err
	}
	b := make([]byte, 12)
	rand.Read(b)
	token := hexenc.EncodeToString(b)
	st := &vStream{url: url, header: http.Header{}}
	for k, v := range header {
		st.header.Set(k, v)
	}
	if st.header.Get("User-Agent") == "" {
		st.header.Set("User-Agent", "Mozilla/5.0")
	}
	st.size.Store(size)
	vp.mu.Lock()
	vp.streams[token] = st
	vp.mu.Unlock()
	return vp.base + token, nil
}

// refusal: the HTTP status a server refused one of these local URLs'
// streams with (0: none).
func (vp *vProxy) refusal(urls ...string) int {
	vp.mu.Lock()
	defer vp.mu.Unlock()
	for _, u := range urls {
		if st := vp.streams[strings.TrimPrefix(u, vp.base)]; st != nil {
			if c := st.refused.Load(); c != 0 {
				return int(c)
			}
		}
	}
	return 0
}

// forget drops the streams of the local URLs given.
func (vp *vProxy) forget(urls ...string) {
	vp.mu.Lock()
	for _, u := range urls {
		delete(vp.streams, strings.TrimPrefix(u, vp.base))
	}
	vp.mu.Unlock()
}

// parseRange: "bytes=a-b" (b -1: to the end); ok false for anything else.
func parseRange(h string) (int64, int64, bool) {
	s, ok := strings.CutPrefix(strings.TrimSpace(h), "bytes=")
	if !ok || strings.Contains(s, ",") {
		return 0, 0, false
	}
	a, b, ok := strings.Cut(s, "-")
	if !ok || a == "" {
		return 0, 0, false
	}
	from, err := strconv.ParseInt(a, 10, 64)
	if err != nil || from < 0 {
		return 0, 0, false
	}
	to := int64(-1)
	if b != "" {
		if to, err = strconv.ParseInt(b, 10, 64); err != nil || to < from {
			return 0, 0, false
		}
	}
	return from, to, true
}

// contentTotal: the total of a "bytes a-b/total" header (0 if not given).
func contentTotal(cr string) int64 {
	_, t, ok := strings.Cut(cr, "/")
	if !ok {
		return 0
	}
	n, _ := strconv.ParseInt(strings.TrimSpace(t), 10, 64)
	return n
}

var vClient = struct {
	once sync.Once
	c    *http.Client
}{}

// vHTTP: the client for the streams: betterPod's certificates, no time
// limit for a whole answer (a paused video holds its connection).
func vHTTP() *http.Client {
	vClient.once.Do(func() {
		vClient.c = &http.Client{Transport: updateClient(0).Transport}
	})
	return vClient.c
}

// open: the stream's bytes from..to (to included), with its total length.
// A server that ignores the range answers 200: what comes before from is
// skipped.
func (st *vStream) open(r *http.Request, from, to int64) (io.ReadCloser, int64, error) {
	req, err := http.NewRequestWithContext(r.Context(), "GET", st.url, nil)
	if err != nil {
		return nil, 0, err
	}
	req.Header = st.header.Clone()
	req.Header.Set("Range", fmt.Sprintf("bytes=%d-%d", from, to))
	resp, err := vHTTP().Do(req)
	if err != nil {
		return nil, 0, err
	}
	switch resp.StatusCode {
	case 206:
		return resp.Body, contentTotal(resp.Header.Get("Content-Range")), nil
	case 200:
		if from > 0 {
			if _, err := io.CopyN(io.Discard, resp.Body, from); err != nil {
				resp.Body.Close()
				return nil, 0, err
			}
		}
		total := resp.ContentLength
		if total < 0 {
			total = 0
		}
		return resp.Body, total, nil
	}
	resp.Body.Close()
	return nil, 0, vRefused(resp.StatusCode)
}

// stallReader closes its body when no bytes came for d (a stalled
// connection, or one left idle while ffmpeg waits), so the read breaks
// off and the piece is opened again from there.
type stallReader struct {
	rc io.ReadCloser
	t  *time.Timer
	d  time.Duration
}

func newStallReader(rc io.ReadCloser, d time.Duration) *stallReader {
	return &stallReader{rc: rc, d: d, t: time.AfterFunc(d, func() { rc.Close() })}
}

func (s *stallReader) Read(p []byte) (int, error) {
	n, err := s.rc.Read(p)
	if n > 0 {
		s.t.Reset(s.d)
	}
	return n, err
}

func (s *stallReader) Close() error {
	s.t.Stop()
	return s.rc.Close()
}

// serve: one request of ffmpeg's (a range of a stream).
func (vp *vProxy) serve(w http.ResponseWriter, r *http.Request) {
	vp.mu.Lock()
	st := vp.streams[strings.TrimPrefix(r.URL.Path, "/v/")]
	vp.mu.Unlock()
	if st == nil {
		http.NotFound(w, r)
		return
	}
	from, to, ranged := parseRange(r.Header.Get("Range"))
	if !ranged {
		from, to = 0, -1
	}
	if size := st.size.Load(); size > 0 && from >= size {
		w.Header().Set("Content-Range", fmt.Sprintf("bytes */%d", size))
		w.WriteHeader(http.StatusRequestedRangeNotSatisfiable)
		return
	}
	end := func(off int64) int64 { // the end of the piece from off
		e := off + vChunk - 1
		if to >= 0 && e > to {
			e = to
		}
		if size := st.size.Load(); size > 0 && e > size-1 {
			e = size - 1
		}
		return e
	}
	body, total, err := st.open(r, from, end(from))
	for try := 1; err != nil && try <= 2 && isRefusal(err) && r.Context().Err() == nil; try++ {
		// YouTube refuses for a moment now and then (too many requests
		// close together): again after a pause
		select {
		case <-time.After(time.Duration(try) * 1500 * time.Millisecond):
		case <-r.Context().Done():
		}
		body, total, err = st.open(r, from, end(from))
	}
	if err != nil {
		logMsg("Videos: stream: " + err.Error())
		var refused vRefused
		if errors.As(err, &refused) {
			st.refused.Store(int64(refused))
		}
		http.Error(w, err.Error(), http.StatusBadGateway)
		return
	}
	if total > 0 {
		st.size.Store(total)
	}
	size := st.size.Load()
	last := to
	if size > 0 && (last < 0 || last > size-1) {
		last = size - 1
	}
	h := w.Header()
	h.Set("Accept-Ranges", "bytes")
	h.Set("Content-Type", "application/octet-stream")
	if last >= 0 {
		h.Set("Content-Length", strconv.FormatInt(last-from+1, 10))
	}
	if ranged && size > 0 {
		h.Set("Content-Range", fmt.Sprintf("bytes %d-%d/%d", from, last, size))
		w.WriteHeader(http.StatusPartialContent)
	} else {
		w.WriteHeader(http.StatusOK)
	}

	// read ahead (up to 4 MB) while ffmpeg takes what it needs
	blocks := make(chan []byte, 64)
	go func() {
		defer close(blocks)
		const stall = 15 * time.Second
		off, pieceEnd := from, end(from)
		var cur io.ReadCloser = newStallReader(body, stall)
		tries := 0
		for last < 0 || off <= last {
			if cur == nil {
				if tries > 0 {
					select {
					case <-time.After(time.Duration(tries) * time.Second):
					case <-r.Context().Done():
						return
					}
				}
				pieceEnd = end(off)
				b, _, err := st.open(r, off, pieceEnd)
				if err != nil {
					var refused vRefused
					if errors.As(err, &refused) && (refused == 416 || tries >= 3) {
						// past the end, or still refused after three pauses
						// (the links expired): the screen asks for new ones
						st.refused.Store(int64(refused))
						logMsg("Videos: stream refused: " + err.Error())
						return
					}
					if tries++; tries > 5 || r.Context().Err() != nil {
						logMsg("Videos: stream lost: " + err.Error())
						return
					}
					continue
				}
				cur = newStallReader(b, stall)
			}
			b := make([]byte, 64<<10)
			n, err := cur.Read(b) // (what has come: ffmpeg starts sooner)
			if n > 0 {
				tries = 0
				off += int64(n)
				select {
				case blocks <- b[:n]:
				case <-r.Context().Done():
					cur.Close()
					return
				}
			}
			if err == nil && off <= pieceEnd {
				continue
			}
			cur.Close()
			cur = nil
			switch {
			case off > pieceEnd:
				// the piece is done: the next one
			case r.Context().Err() != nil:
				return
			case last < 0 && (err == io.EOF || err == io.ErrUnexpectedEOF):
				return // the end of a stream of unknown length
			default:
				tries++ // broken off (or stalled): again from off
			}
		}
	}()
	for b := range blocks {
		if _, err := w.Write(b); err != nil {
			return
		}
	}
}
