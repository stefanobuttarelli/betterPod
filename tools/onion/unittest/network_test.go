//go:build ignore

// Tests for Software Update's install, Internet Archive's file names and
// Share's received names: run with tools/onion/unittest.sh.

package main

import (
	"archive/zip"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func writeFiles(t *testing.T, root string, files map[string]string) {
	t.Helper()
	for name, body := range files {
		p := filepath.Join(root, name)
		if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte(body), 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

func readFile(t *testing.T, p string) string {
	t.Helper()
	b, err := os.ReadFile(p)
	if err != nil {
		t.Fatalf("%s: %v", p, err)
	}
	return string(b)
}

// A file that can't be staged leaves the installation as it was; when all
// are staged they all take their place, and the old binary is kept.
func TestInstallFilesStaged(t *testing.T) {
	ports := t.TempDir()
	port := filepath.Join(ports, appDir)
	newDir := t.TempDir()
	writeFiles(t, ports, map[string]string{
		"betterpod/a.txt":     "old a",
		"betterpod/betterpod": "old bin",
		"betterPod.sh":        "old script",
	})
	writeFiles(t, newDir, map[string]string{
		"betterpod/a.txt":       "new a",
		"betterpod/libs/b.so":   "new b",
		"betterpod/betterpod":   "new bin",
		"betterPod.sh":          "new script",
		"betterpod/zz/last.txt": "new last",
	})
	files := []string{"betterpod/betterpod", "betterpod/a.txt", "betterPod.sh", "betterpod/libs/b.so", "betterpod/zz/last.txt"}

	// the last file can't be staged: a folder is in the way
	block := filepath.Join(ports, "betterpod/zz/last.txt.kpnew")
	writeFiles(t, block, map[string]string{"x": "x"})
	if err := installFiles(newDir, ports, port, files); err == nil {
		t.Fatal("staging into a folder succeeded")
	}
	for p, want := range map[string]string{"betterpod/a.txt": "old a", "betterpod/betterpod": "old bin", "betterPod.sh": "old script"} {
		if got := readFile(t, filepath.Join(ports, p)); got != want {
			t.Errorf("%s = %q after a failed update, want %q", p, got, want)
		}
	}
	for _, p := range []string{"betterpod/a.txt.kpnew", "betterpod/betterpod.kpnew", "betterPod.sh.kpnew", "betterpod/libs/b.so.kpnew", "betterpod/libs/b.so", "betterpod/betterpod.prev"} {
		if fileExists(filepath.Join(ports, p)) {
			t.Errorf("%s left by a failed update", p)
		}
	}

	os.RemoveAll(block)
	if err := installFiles(newDir, ports, port, files); err != nil {
		t.Fatal(err)
	}
	for p, want := range map[string]string{"betterpod/a.txt": "new a", "betterpod/betterpod": "new bin", "betterPod.sh": "new script",
		"betterpod/libs/b.so": "new b", "betterpod/zz/last.txt": "new last", "betterpod/betterpod.prev": "old bin"} {
		if got := readFile(t, filepath.Join(ports, p)); got != want {
			t.Errorf("%s = %q, want %q", p, got, want)
		}
	}
	if fileExists(filepath.Join(ports, "betterpod/a.txt.kpnew")) {
		t.Error("a staged file was left")
	}
}

// Internet Archive: a long name keeps its extension, the same name in two
// folders of an item gets two files, a file saved under its old name
// still counts.
func TestIANames(t *testing.T) {
	long := strings.Repeat("A very long track title ", 6) + ".flac"
	if n := safeFileName(long); !strings.HasSuffix(n, ".flac") || len([]rune(n)) > 80+len(".flac") {
		t.Errorf("safeFileName(long) = %q", n)
	}
	if n := safeFileName(`What? Now: "Live".mp3`); n != "What_ Now_ _Live_.mp3" {
		t.Errorf("safeFileName = %q", n)
	}

	it := &iaItem{ID: "x", Title: "Album", Creator: "Artist", Groups: map[string][]iaFile{
		"FLAC": {{Name: "CD1/01 Intro.flac", Size: 3}, {Name: "CD2/01 Intro.flac", Size: 4}, {Name: "Bonus.flac", Size: 5}},
		"MP3":  {{Name: "CD1/01 Intro.mp3", Size: 6}, {Name: "CD2/01 intro.mp3", Size: 7}},
	}}
	names := iaNames(it)
	want := map[string]string{"CD1/01 Intro.flac": "CD1 - 01 Intro.flac", "CD2/01 Intro.flac": "CD2 - 01 Intro.flac",
		"Bonus.flac": "Bonus.flac", "CD1/01 Intro.mp3": "CD1 - 01 Intro.mp3", "CD2/01 intro.mp3": "CD2 - 01 intro.mp3"}
	for k, v := range want {
		if names[k] != v {
			t.Errorf("%s → %q, want %q", k, names[k], v)
		}
	}
	// in one folder: the bare names, upper/lower case told apart for FAT
	one := &iaItem{Groups: map[string][]iaFile{"MP3": {{Name: "a.mp3"}, {Name: "A.mp3"}, {Name: long}}}}
	n1 := iaNames(one)
	if n1["a.mp3"] != "a.mp3" || n1["A.mp3"] != "A (2).mp3" || !strings.HasSuffix(n1[long], ".flac") {
		t.Errorf("names in one folder: %v", n1)
	}

	// downloaded before 1.0 under its cut name: still there
	root := t.TempDir()
	t.Setenv("MIYOOPOD_MUSIC_DIR", root)
	old := MUSIC_ROOT
	MUSIC_ROOT = musicRoot()
	defer func() { MUSIC_ROOT = old }()
	li := &iaItem{ID: "y", Title: "T", Creator: "C", Groups: map[string][]iaFile{"FLAC": {{Name: long, Size: 2}, {Name: "b.flac", Size: 3}}}}
	dir := iaDir(li)
	writeFiles(t, dir, map[string]string{safeName(long): "ok"})
	if have, left := iaHave(li, li.Groups["FLAC"]); have != 1 || left != 3 {
		t.Errorf("iaHave = %d, %d; want 1, 3", have, left)
	}
}

// Share: names the card can't hold are cleaned, a failed unpack leaves
// nothing in Shared.
func TestShareUnpackNames(t *testing.T) {
	if p := shareCleanPath(filepath.Join(`Who? Live`, `Track: 1 "Intro".flac`)); p != filepath.Join("Who_ Live", "Track_ 1 _Intro_.flac") {
		t.Errorf("shareCleanPath = %q", p)
	}
	root := t.TempDir()
	t.Setenv("MIYOOPOD_MUSIC_DIR", root)
	old := MUSIC_ROOT
	MUSIC_ROOT = musicRoot()
	defer func() { MUSIC_ROOT = old }()
	zp := filepath.Join(t.TempDir(), "r.zip")
	f, _ := os.Create(zp)
	zw := zip.NewWriter(f)
	for _, n := range []string{"Al:bum/01 a?.mp3", "Al:bum/cover.jpg", "Al:bum/notes.exe"} {
		w, _ := zw.Create(n)
		w.Write([]byte(n))
	}
	zw.Close()
	f.Close()
	dest, err := shareUnpack(zp, "Al:bum")
	if err != nil {
		t.Fatal(err)
	}
	if filepath.Base(dest) != "Al_bum" || !fileExists(filepath.Join(dest, "Al_bum", "01 a_.mp3")) || fileExists(filepath.Join(dest, "Al_bum", "notes.exe")) {
		t.Errorf("unpacked into %s", dest)
	}
	if fileExists(filepath.Join(root, "Shared", ".receiving")) {
		t.Error("the work folder was left")
	}
	// a broken zip: no new folder in Shared
	os.WriteFile(zp, []byte("PK\x03\x04 not a zip"), 0o644)
	if _, err := shareUnpack(zp, "Other"); err == nil {
		t.Error("a broken zip was unpacked")
	}
	if fileExists(filepath.Join(root, "Shared", "Other")) {
		t.Error("a failed share left a folder")
	}
}

// Internet Archive: a .part is continued only when the server shows it's
// the same file (If-Range); a changed file starts over.
func TestIAResume(t *testing.T) {
	body := "0123456789abcdefghij"
	etag := `"v1"`
	var ranges []string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		ranges = append(ranges, r.Header.Get("Range")+"|"+r.Header.Get("If-Range"))
		w.Header().Set("ETag", etag)
		http.ServeContent(w, r, "f.flac", time.Time{}, strings.NewReader(body))
	}))
	defer srv.Close()
	t.Setenv("MIYOOPOD_IA_BASE", srv.URL)
	root := t.TempDir()
	t.Setenv("MIYOOPOD_MUSIC_DIR", root)
	old := MUSIC_ROOT
	MUSIC_ROOT = musicRoot()
	defer func() { MUSIC_ROOT = old }()
	files := []iaFile{{Name: "f.flac", Size: int64(len(body))}}
	it := &iaItem{ID: "x", Title: "T", Creator: "C", Groups: map[string][]iaFile{"FLAC": files}}
	dst := filepath.Join(iaDir(it), "f.flac")
	run := func(part, info string) {
		os.Remove(dst)
		writeFiles(t, iaDir(it), map[string]string{"f.flac.part": part})
		if info != "" {
			writeFiles(t, iaDir(it), map[string]string{"f.flac.part.info": info})
		}
		d := &iaDownload{item: it, files: files, dir: iaDir(it), cancel: make(chan struct{})}
		if err := d.run(nil); err != nil {
			t.Fatal(err)
		}
		if got := readFile(t, dst); got != body {
			t.Errorf("got %q", got)
		}
		if fileExists(dst+".part") || fileExists(dst+".part.info") {
			t.Error("the .part was left")
		}
	}
	u := srv.URL + "/download/x/f.flac"
	// the same file: continued from byte 10
	ranges = nil
	run("0123456789", `{"url":"`+u+`","etag":"\"v1\"","total":20}`)
	if len(ranges) != 1 || ranges[0] != `bytes=10-|"v1"` {
		t.Errorf("requests %v", ranges)
	}
	// the file changed: a stale .part is replaced, not continued
	ranges = nil
	run("XXXXXXXXXX", `{"url":"`+u+`","etag":"\"v0\"","total":20}`)
	if len(ranges) != 1 || ranges[0] != `bytes=10-|"v0"` {
		t.Errorf("requests %v", ranges)
	}
	// a .part from before 1.0 (no info): continued when the total matches
	ranges = nil
	run("0123456789", "")
	if len(ranges) != 1 || ranges[0] != `bytes=10-|` {
		t.Errorf("requests %v", ranges)
	}
}
