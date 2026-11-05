//go:build ignore

// Tests for Listen's streamed YouTube audio (ij_ytstream.go): a DASH-like
// fragmented M4A (ftyp, moov, sidx, moof + mdat, as YouTube's format 140)
// made by ffmpeg, served with ranges; the parts are planned from its sidx
// and each one is written as a playable M4A of its length. Skipped without
// ffmpeg. Run with tools/unittest.sh.

package main

import (
	"context"
	"math"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestYTStream(t *testing.T) {
	if _, err := exec.LookPath("ffmpeg"); err != nil {
		t.Skip("no ffmpeg")
	}
	dir := t.TempDir()
	src := filepath.Join(dir, "dash.m4a")
	out, err := exec.Command("ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=f=330:d=500",
		"-c:a", "aac", "-b:a", "128k", "-movflags", "+dash+global_sidx", "-frag_duration", "10000000", src).CombinedOutput()
	if err != nil {
		t.Fatalf("ffmpeg: %v %s", err, out)
	}
	ranges := 0
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Range") != "" {
			ranges++
		}
		f, _ := os.Open(src)
		defer f.Close()
		http.ServeContent(w, r, "a.m4a", fileTime(src), f)
	}))
	defer srv.Close()

	s, err := ytStreamFrom(context.Background(), srv.URL, map[string]string{"User-Agent": "test"})
	if err != nil {
		t.Fatal(err)
	}
	if math.Abs(s.dur-500) > 1 {
		t.Errorf("duration %.1f, want 500", s.dur)
	}
	// a short first part, then parts of about 3 minutes, all of it
	if len(s.parts) < 3 || s.parts[0].dur > ytStreamFirst+10 || s.parts[1].dur < ytStreamNext-10 {
		t.Fatalf("parts: %+v", s.parts)
	}
	var sum float64
	for i, p := range s.parts {
		if i > 0 && math.Abs(p.start-sum) > 0.01 {
			t.Errorf("part %d starts at %.2f, want %.2f", i, p.start, sum)
		}
		sum += p.dur
	}
	for _, k := range []int{0, len(s.parts) - 1} {
		dst := filepath.Join(dir, "part.m4a")
		if err := s.fetchPart(context.Background(), k, dst, "Test", nil); err != nil {
			t.Fatalf("part %d: %v", k, err)
		}
		m, err := openM4A(dst)
		if err != nil {
			t.Fatalf("part %d unreadable: %v", k, err)
		}
		if d := m.Duration(); math.Abs(d-s.parts[k].dur) > 0.5 {
			t.Errorf("part %d lasts %.2f s, planned %.2f", k, d, s.parts[k].dur)
		}
		m.Close()
		if _, err := os.Stat(dst + ".frag"); err == nil {
			t.Errorf("part %d: its piece was left", k)
		}
	}
	if ranges < 3 {
		t.Errorf("only %d range requests", ranges)
	}

	// not a fragmented file: refused (Listen fetches it whole)
	plain := filepath.Join(dir, "plain.m4a")
	exec.Command("ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=d=5", "-c:a", "aac", plain).Run()
	srv2 := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		f, _ := os.Open(plain)
		defer f.Close()
		http.ServeContent(w, r, "p.m4a", fileTime(plain), f)
	}))
	defer srv2.Close()
	if _, err := ytStreamFrom(context.Background(), srv2.URL, nil); err == nil {
		t.Error("a plain M4A was taken for a streamable one")
	}
}

func fileTime(p string) (t time.Time) {
	if st, err := os.Stat(p); err == nil {
		t = st.ModTime()
	}
	return t
}
