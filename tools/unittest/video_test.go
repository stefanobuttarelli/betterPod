//go:build ignore

// Tests for Videos (video.go, ij_video.go, ij_videodl.go): which of
// YouTube's formats is played, the size and rate of the frames, ffmpeg's
// arguments, the frame queue, the streams' server (ranges in 10 MB pieces,
// a broken connection taken up again, a refusal remembered), Recently
// Watched, the downloaded videos, and the text being typed cut around its
// caret (Album Info). Run with tools/unittest.sh.

package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"
)

func f(id, vc, ac string, w, h, fps, abr float64) ytFormat {
	return ytFormat{ID: id, URL: "https://example.org/" + id, Protocol: "https", VCodec: vc, ACodec: ac, W: w, H: h, FPS: fps, ABR: abr}
}

func TestPickFormats(t *testing.T) {
	ids := func(v, a *ytFormat) string {
		s := "-"
		if v != nil {
			s = v.ID
		}
		if a != nil {
			s += "+" + a.ID
		}
		return s
	}
	hdr := f("399", "avc1.4d401e", "none", 640, 360, 30, 0)
	hdr.Range = "HDR10"
	hls := f("hls", "avc1", "mp4a", 640, 360, 30, 0)
	hls.Protocol = "m3u8_native"
	drc := f("140-drc", "none", "mp4a.40.2", 0, 0, 0, 129)
	drc.LangPref = 10
	orig := f("140", "none", "mp4a.40.2", 0, 0, 0, 129)
	orig.LangPref = 10
	dub := f("140-1", "none", "mp4a.40.2", 0, 0, 0, 129)
	dub.LangPref = -1
	for _, c := range []struct {
		name string
		fs   []ytFormat
		want string
	}{
		{"YouTube's usual list: H.264 360p and AAC, apart", []ytFormat{
			f("sb0", "none", "none", 0, 0, 0, 0), f("160", "avc1.4d400c", "none", 256, 144, 30, 0), f("243", "vp09.00.21.08", "none", 640, 360, 30, 0),
			f("134", "avc1.4d401e", "none", 640, 360, 30, 0), f("136", "avc1.64001f", "none", 1280, 720, 30, 0), f("298", "avc1.4d4020", "none", 1280, 720, 60, 0),
			f("139", "none", "mp4a.40.5", 0, 0, 0, 48), f("140", "none", "mp4a.40.2", 0, 0, 0, 129), f("251", "none", "opus", 0, 0, 0, 135),
			f("18", "avc1.42001E", "mp4a.40.2", 640, 360, 30, 0), hls}, "134+140"},
		{"only one format with both", []ytFormat{f("18", "avc1.42001E", "mp4a.40.2", 640, 360, 30, 0), hls}, "18+18"},
		{"4:3: 480p fills 640x480", []ytFormat{f("134", "avc1", "none", 480, 360, 30, 0), f("135", "avc1", "none", 640, 480, 30, 0),
			f("136", "avc1", "none", 960, 720, 30, 0), f("140", "none", "mp4a.40.2", 0, 0, 0, 129)}, "135+140"},
		{"upright: 360x640 fills 270x480", []ytFormat{f("134", "avc1", "none", 360, 640, 30, 0), f("135", "avc1", "none", 480, 854, 30, 0),
			f("140", "none", "mp4a.40.2", 0, 0, 0, 129)}, "134+140"},
		{"only small: the biggest", []ytFormat{f("160", "avc1", "none", 256, 144, 30, 0), f("133", "avc1", "none", 426, 240, 30, 0),
			f("140", "none", "mp4a.40.2", 0, 0, 0, 129)}, "133+140"},
		{"the same size: fewer frames", []ytFormat{f("a", "avc1", "none", 640, 360, 60, 0), f("b", "avc1", "none", 640, 360, 30, 0),
			f("140", "none", "mp4a.40.2", 0, 0, 0, 129)}, "b+140"},
		{"no H.264: VP9, and Opus", []ytFormat{f("243", "vp09.00.21.08", "none", 640, 360, 30, 0), f("394", "av01.0.00M.08", "none", 640, 360, 30, 0),
			f("251", "none", "opus", 0, 0, 0, 135)}, "243+251"},
		{"H.264 smaller than VP9 is still H.264", []ytFormat{f("133", "avc1", "none", 426, 240, 30, 0), f("244", "vp9", "none", 854, 480, 30, 0),
			f("140", "none", "mp4a.40.2", 0, 0, 0, 129)}, "133+140"},
		{"HDR left out", []ytFormat{hdr, f("134", "avc1.4d401e", "none", 640, 360, 30, 0), f("140", "none", "mp4a.40.2", 0, 0, 0, 129)}, "134+140"},
		{"the original language, without DRC", []ytFormat{f("134", "avc1", "none", 640, 360, 30, 0), dub, drc, orig}, "134+140"},
		{"a picture without sound", []ytFormat{f("134", "avc1", "none", 640, 360, 30, 0)}, "134"},
	} {
		v, a, err := ytPickFormats(c.fs)
		if err != nil {
			t.Errorf("%s: %v", c.name, err)
			continue
		}
		if got := ids(v, a); got != c.want {
			t.Errorf("%s: got %s, want %s", c.name, got, c.want)
		}
	}
	for _, fs := range [][]ytFormat{nil, {f("394", "av01.0.00M.08", "none", 640, 360, 30, 0), f("140", "none", "mp4a.40.2", 0, 0, 0, 129)}, {hls}} {
		if v, a, err := ytPickFormats(fs); err == nil {
			t.Errorf("%v: got %s, want an error", fs, ids(v, a))
		}
	}
}

func TestVideoFit(t *testing.T) {
	for _, c := range []struct{ w, h, fw, fh int }{
		{1920, 1080, 640, 360}, {640, 360, 640, 360}, {480, 360, 640, 480}, {1080, 1920, 270, 480},
		{0, 0, 640, 360}, {1280, 544, 640, 272}, {640, 480, 640, 480},
	} {
		if fw, fh := videoFit(c.w, c.h, videoBoxW, videoBoxH); fw != c.fw || fh != c.fh {
			t.Errorf("videoFit(%d, %d) = %dx%d, want %dx%d", c.w, c.h, fw, fh, c.fw, c.fh)
		}
	}
	for _, c := range []struct{ in, out float64 }{{60, 30}, {59.94, 29.97}, {50, 25}, {30, 30}, {25, 25}, {24, 24}, {0, 30}, {120, 30}} {
		if got := videoFPS(c.in); got != c.out {
			t.Errorf("videoFPS(%v) = %v, want %v", c.in, got, c.out)
		}
	}
	for _, c := range []struct {
		s, d float64
		want string
	}{{0, 40, "00:00"}, {75.9, 600, "01:15"}, {3723, 4000, "1:02:03"}, {62, 4000, "0:01:02"}, {-3, 40, "00:00"}} {
		if got := videoTime(c.s, c.d); got != c.want {
			t.Errorf("videoTime(%v, %v) = %s, want %s", c.s, c.d, got, c.want)
		}
	}
}

func TestFFmpegArgs(t *testing.T) {
	v := strings.Join(ffmpegVideoArgs("http://x/v", 12.5, 29.97, 640, 360), " ")
	for _, want := range []string{"-ss 12.500 -i http://x/v", "-map 0:v:0", "fps=fps=29.970:start_time=0,scale=640:360", "-pix_fmt rgba -f rawvideo pipe:1"} {
		if !strings.Contains(v, want) {
			t.Errorf("video args %q lack %q", v, want)
		}
	}
	if strings.Contains(strings.Join(ffmpegVideoArgs("f.mp4", 0, 30, 2, 2), " "), "-ss") {
		t.Error("-ss at 0")
	}
	a := strings.Join(ffmpegAudioArgs("http://x/a", 3, 48000, true, 2), " ")
	for _, want := range []string{"-ss 3.000 -i http://x/a", "-map 0:a:0", "aresample=async=1:first_pts=0", "-ac 2 -ar 48000 -f f32le pipe:1"} {
		if !strings.Contains(a, want) {
			t.Errorf("audio args %q lack %q", a, want)
		}
	}
	if !strings.Contains(strings.Join(ffmpegAudioArgs("x", 0, 44100, false, 2), " "), "-f s16le") {
		t.Error("16-bit sound not asked as s16le")
	}
}

func TestVideoQueue(t *testing.T) {
	bufs := [][]byte{make([]byte, 4), make([]byte, 4), make([]byte, 4)}
	q := newVQueue(bufs)
	for i := 0; i < 3; i++ {
		b := q.get()
		b[0] = byte(i)
		q.put(b, float64(i)/30)
	}
	if _, _, n := q.take(-0.1); n != 0 || q.waiting() != 3 {
		t.Fatal("a frame taken before its time")
	}
	b, at, skipped := q.take(0.07) // frames 0, 1 and 2 (0.067) are due: 2 shows, 0 and 1 are skipped
	if b == nil || b[0] != 2 || skipped != 2 || at != 2.0/30 || q.waiting() != 0 {
		t.Fatalf("take: %v %v %d", b, at, skipped)
	}
	// two buffers came back: two more frames can be read, not a third
	got := make(chan []byte)
	go func() {
		q.get()
		q.get()
		got <- q.get() // waits: the third is on screen
	}()
	q.giveBack(b)
	if g := <-got; g == nil {
		t.Fatal("no buffer after the one on screen came back")
	}
	if left := q.close(); len(left) != 0 {
		t.Fatalf("close gave back %d buffers that were out", len(left))
	}
	if q.get() != nil {
		t.Fatal("get after close")
	}
}

// The streams' server: ffmpeg's ranges are fetched in pieces of 10 MB at
// most, with the stream's headers; a connection that breaks halfway is
// taken up again from where it broke; a refusal is remembered.
func TestVideoProxy(t *testing.T) {
	const size = 23<<20 + 12345
	data := make([]byte, size)
	for i := range data {
		data[i] = byte(i*7 + i>>11)
	}
	var mu sync.Mutex
	var ranges []string
	broke := false
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("X-Test") != "yes" {
			w.WriteHeader(403)
			return
		}
		rg := r.Header.Get("Range")
		mu.Lock()
		ranges = append(ranges, rg)
		mu.Unlock()
		a, b, ok := parseRange(rg)
		if !ok {
			t.Errorf("a request without a range: %q", rg)
			w.WriteHeader(400)
			return
		}
		if b < 0 || b > size-1 {
			b = size - 1
		}
		if b-a+1 > 10<<20 {
			t.Errorf("a piece of %d bytes", b-a+1)
		}
		w.Header().Set("Content-Range", fmt.Sprintf("bytes %d-%d/%d", a, b, size))
		w.Header().Set("Content-Length", strconv.FormatInt(b-a+1, 10))
		w.WriteHeader(206)
		mu.Lock()
		breakNow := !broke && a >= 10<<20
		broke = broke || breakNow
		mu.Unlock()
		if breakNow {
			// the second piece: 1 MB, then the connection drops
			w.Write(data[a : a+1<<20])
			if hj, ok := w.(http.Hijacker); ok {
				c, _, _ := hj.Hijack()
				c.Close()
			}
			return
		}
		w.Write(data[a : b+1])
	}))
	defer srv.Close()

	local, err := videoProxy.add(srv.URL+"/v", map[string]string{"X-Test": "yes"}, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer videoProxy.forget(local)
	get := func(rg string) (*http.Response, []byte) {
		req, _ := http.NewRequest("GET", local, nil)
		if rg != "" {
			req.Header.Set("Range", rg)
		}
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatal(err)
		}
		defer resp.Body.Close()
		b, _ := io.ReadAll(resp.Body)
		return resp, b
	}
	resp, b := get("bytes=0-")
	if resp.StatusCode != 206 || resp.Header.Get("Content-Range") != fmt.Sprintf("bytes 0-%d/%d", size-1, size) {
		t.Fatalf("whole: %d %q", resp.StatusCode, resp.Header.Get("Content-Range"))
	}
	if !bytes.Equal(b, data) {
		t.Fatalf("whole: %d bytes, not the stream's", len(b))
	}
	if !broke {
		t.Fatal("the connection never broke")
	}
	resp, b = get("bytes=21000000-21000099")
	if resp.StatusCode != 206 || !bytes.Equal(b, data[21000000:21000100]) {
		t.Fatalf("part: %d, %d bytes", resp.StatusCode, len(b))
	}
	if resp, _ := get(fmt.Sprintf("bytes=%d-", size)); resp.StatusCode != 416 {
		t.Fatalf("past the end: %d", resp.StatusCode)
	}
	if videoProxy.refusal(local) != 0 {
		t.Fatal("a refusal without one")
	}

	// a refusal (the links expired): 502 for ffmpeg, 403 remembered
	bad, _ := videoProxy.add(srv.URL+"/v", nil, 0)
	defer videoProxy.forget(bad)
	req, _ := http.NewRequest("GET", bad, nil)
	r2, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	r2.Body.Close()
	if r2.StatusCode != 502 || videoProxy.refusal(bad) != 403 || videoProxy.refusal(local) != 0 {
		t.Fatalf("refused: %d, refusal %d", r2.StatusCode, videoProxy.refusal(bad))
	}
	for _, r := range []struct {
		h        string
		from, to int64
		ok       bool
	}{{"bytes=0-", 0, -1, true}, {"bytes=5-9", 5, 9, true}, {"bytes=9-5", 0, 0, false}, {"bytes=-5", 0, 0, false},
		{"bytes=0-1,5-6", 0, 0, false}, {"items=0-1", 0, 0, false}} {
		from, to, ok := parseRange(r.h)
		if ok != r.ok || ok && (from != r.from || to != r.to) {
			t.Errorf("parseRange(%q) = %d, %d, %v", r.h, from, to, ok)
		}
	}
	if contentTotal("bytes 0-99/12345") != 12345 || contentTotal("bytes 0-99/*") != 0 {
		t.Error("contentTotal")
	}
}

func TestVideoHistory(t *testing.T) {
	old := dataDirOverride
	dataDirOverride = t.TempDir()
	defer func() { dataDirOverride = old }()
	e := ytEntry{ID: "a", Title: "A", Channel: "Ch", Duration: 600}
	videoRemember(e, 125.4, 600)
	videoRemember(ytEntry{ID: "b", Title: "B"}, 5, 300) // hardly begun: from the start
	h := videoHistory()
	if len(h) != 2 || h[0].ID != "b" || h[0].Pos != 0 || h[1].ID != "a" || h[1].Pos != 125 || h[1].Channel != "Ch" {
		t.Fatalf("history: %+v", h)
	}
	videoRemember(e, 590, 600) // nearly the end: from the start next time, and first again
	if h = videoHistory(); h[0].ID != "a" || h[0].Pos != 0 || len(h) != 2 {
		t.Fatalf("again: %+v", h)
	}
	for i := 0; i < 60; i++ {
		videoRemember(ytEntry{ID: fmt.Sprint("v", i), Title: "V"}, 30, 600)
	}
	if h = videoHistory(); len(h) != 50 || h[0].ID != "v59" {
		t.Fatalf("kept %d, first %s", len(h), h[0].ID)
	}
	videoForget("v59")
	if h = videoHistory(); h[0].ID != "v58" || len(h) != 49 {
		t.Fatalf("after forget: %d, first %s", len(h), h[0].ID)
	}
	videoRemember(ytEntry{Title: "no id"}, 30, 60) // (a link that never opened)
	if len(videoHistory()) != 49 {
		t.Fatal("a video without an id kept")
	}
}

// YouTube refuses some links now and then: the next candidate is tried,
// then a format with both, then the error says why.
func TestChooseStreams(t *testing.T) {
	refuse := map[string]bool{}
	var mu sync.Mutex
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		no := refuse[strings.TrimPrefix(r.URL.Path, "/")]
		mu.Unlock()
		if no || r.Header.Get("Range") != "bytes=0-1" || r.Header.Get("X-Test") != "yes" {
			w.WriteHeader(403)
			return
		}
		w.Header().Set("Content-Range", "bytes 0-1/100")
		w.WriteHeader(206)
		w.Write([]byte("ok"))
	}))
	defer srv.Close()
	mk := func(id, vc, ac string, w, h, abr float64) ytFormat {
		return ytFormat{ID: id, URL: srv.URL + "/" + id, Protocol: "https", VCodec: vc, ACodec: ac, W: w, H: h, FPS: 30, ABR: abr,
			Headers: map[string]string{"X-Test": "yes"}}
	}
	info := &ytInfo{ytEntry: ytEntry{ID: "v"}, Formats: []ytFormat{
		mk("134", "avc1.4d401e", "none", 640, 360, 0), mk("135", "avc1.4d401e", "none", 854, 480, 0),
		mk("140", "none", "mp4a.40.2", 0, 0, 129), mk("139", "none", "mp4a.40.5", 0, 0, 48),
		mk("18", "avc1.42001E", "mp4a.40.2", 640, 360, 0)}}
	pick := func(no ...string) (string, error) {
		mu.Lock()
		refuse = map[string]bool{}
		for _, n := range no {
			refuse[n] = true
		}
		mu.Unlock()
		v, a, err := ytChooseStreams(context.Background(), info)
		if err != nil {
			return "", err
		}
		return v.ID + "+" + a.ID, nil
	}
	for _, c := range []struct {
		no   []string
		want string
	}{{nil, "134+140"}, {[]string{"140"}, "134+139"}, {[]string{"134"}, "135+140"}, {[]string{"140", "139"}, "18+18"}} {
		if got, err := pick(c.no...); err != nil || got != c.want {
			t.Errorf("refused %v: got %s (%v), want %s", c.no, got, err, c.want)
		}
	}
	if _, err := pick("134", "135", "18"); err == nil || !strings.Contains(err.Error(), "403") {
		t.Errorf("all refused: %v", err)
	}
	srv.Close()
	if _, err := pick(); err == nil || !strings.Contains(err.Error(), "No connection") {
		t.Errorf("no server: %v", err)
	}
}

// Download Video asks yt-dlp for what the player would stream first, with
// the format that has both as the fallback.
func TestVideoDownloadFormats(t *testing.T) {
	usual := &ytInfo{Formats: []ytFormat{
		f("160", "avc1.4d400c", "none", 256, 144, 30, 0), f("134", "avc1.4d401e", "none", 640, 360, 30, 0),
		f("136", "avc1.64001f", "none", 1280, 720, 30, 0), f("140", "none", "mp4a.40.2", 0, 0, 0, 129),
		f("251", "none", "opus", 0, 0, 0, 135), f("18", "avc1.42001E", "mp4a.40.2", 640, 360, 30, 0)}}
	if sel, v, a := videoDownloadFormats(usual); sel != "134+140/18" || v.ID != "134" || a.ID != "140" {
		t.Errorf("usual: %q", sel)
	}
	if sel, v, a := videoDownloadFormats(&ytInfo{Formats: []ytFormat{f("18", "avc1.42001E", "mp4a.40.2", 640, 360, 30, 0)}}); sel != "18" || v.ID != "18" || a != nil {
		t.Errorf("only both: %q", sel)
	}
	if sel, _, _ := videoDownloadFormats(&ytInfo{Formats: []ytFormat{f("394", "av01.0.00M.08", "none", 640, 360, 30, 0)}}); sel != "" {
		t.Errorf("nothing playable: %q", sel)
	}
}

// The downloaded videos: listed newest first, a record without its file
// left out, deleted with their picture.
func TestVideosSaved(t *testing.T) {
	old := dataDirOverride
	dataDirOverride = t.TempDir()
	defer func() { dataDirOverride = old }()
	if len(videosSaved()) != 0 {
		t.Fatal("videos before any")
	}
	os.MkdirAll(videosDir(), 0755)
	save := func(id string, at int64, file bool) {
		data, _ := json.Marshal(videoSaved{ID: id, Title: "T " + id, W: 640, H: 360, FPS: 30, Duration: 40, At: at})
		os.WriteFile(videoSavedPath(id, ".json"), data, 0644)
		if file {
			os.WriteFile(videoSavedPath(id, ".mp4"), []byte("mp4"), 0644)
			os.WriteFile(videoSavedPath(id, ".jpg"), []byte("jpg"), 0644)
		}
	}
	save("old", 100, true)
	save("new", 200, true)
	save("half", 300, false) // (a download that never finished)
	got := videosSaved()
	if len(got) != 2 || got[0].ID != "new" || got[1].ID != "old" || got[0].W != 640 {
		t.Fatalf("saved: %+v", got)
	}
	if _, ok := videoSavedInfo("half"); ok {
		t.Error("a record without its file")
	}
	if s, ok := videoSavedInfo("old"); !ok || s.Title != "T old" {
		t.Errorf("old: %+v %v", s, ok)
	}
	videoDeleteSaved("new")
	if _, err := os.Stat(videoSavedPath("new", ".jpg")); err == nil || len(videosSaved()) != 1 {
		t.Error("not deleted")
	}
}

// Text being typed (Album Info) is cut so its caret stays in sight.
func TestCaretFit(t *testing.T) {
	oldFonts := fontDir
	fontDir = "../port/betterpod/assets/fonts/"
	defer func() { fontDir = oldFonts }()
	fc := face(W400, 12)
	if got := caretFit(fc, "Short|", 400); got != "Short|" {
		t.Errorf("short: %q", got)
	}
	long := strings.Repeat("Album Artist ", 12)
	w := textWidth(fc, "Album Artist Album Artist")
	end := caretFit(fc, long+"|", w)
	if !strings.HasPrefix(end, "…") || !strings.HasSuffix(end, "|") || textWidth(fc, end) > w {
		t.Errorf("caret at the end: %q", end)
	}
	start := caretFit(fc, "Al|"+long, w)
	if !strings.HasPrefix(start, "Al|") || !strings.HasSuffix(start, "…") || textWidth(fc, start) > w {
		t.Errorf("caret near the start: %q", start)
	}
	mid := caretFit(fc, long+"|"+long, w)
	if !strings.Contains(mid, "|") || textWidth(fc, mid) > w {
		t.Errorf("caret in the middle: %q", mid)
	}
}

// YouTube's links answer 403 until their available_at: the player and
// Listen wait for the latest of the formats that may be played.
func TestWaitLinks(t *testing.T) {
	now := float64(time.Now().UnixNano()) / 1e9
	v, a, hls := f("134", "avc1", "none", 640, 360, 30, 0), f("140", "none", "mp4a.40.2", 0, 0, 0, 129), f("hls", "avc1", "mp4a", 640, 360, 30, 0)
	v.AvailableAt, a.AvailableAt = now+3, now+4
	hls.Protocol, hls.AvailableAt = "m3u8_native", now+60 // (never played: not waited for)
	if got := ytLinksAt([]ytFormat{v, a, hls}); got != now+4 {
		t.Errorf("ytLinksAt: %.1f s from now, want 4", got-now)
	}
	if ytLinksAt([]ytFormat{f("18", "avc1", "mp4a.40.2", 640, 360, 30, 0)}) != 0 {
		t.Error("no available_at: no wait")
	}
	for _, at := range []float64{0, now - 5} {
		t0 := time.Now()
		if err := ytWaitLinks(context.Background(), at); err != nil || time.Since(t0) > 50*time.Millisecond {
			t.Errorf("at %v: waited %v (%v)", at, time.Since(t0), err)
		}
	}
	t0 := time.Now()
	ytWaitLinks(context.Background(), now+0.3)
	if d := time.Since(t0); d < 250*time.Millisecond || d > 600*time.Millisecond {
		t.Errorf("waited %v for 0.3 s", d)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()
	t0 = time.Now()
	if err := ytWaitLinks(ctx, now+10); err == nil || time.Since(t0) > 400*time.Millisecond {
		t.Errorf("cancelled: %v after %v", err, time.Since(t0))
	}
}
