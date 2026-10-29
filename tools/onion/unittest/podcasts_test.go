//go:build ignore

// Tests for Podcasts (ij_podcasts.go, KnulliPod's since 0.41): the feed,
// the search, a download that is cut off halfway and continued, what
// counts as downloaded, and no M4A on the Miyoo. Run with
// tools/onion/unittest.sh.

package main

import (
	"bytes"
	"image"
	"image/jpeg"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/dhowden/tag"
)

// a Latin-1 feed with the iTunes namespace, as many hosts write them
const testFeed = "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?>\n" +
	`<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel>
<title>Caf` + "\xe9" + ` Stories</title>
<itunes:author>Radio Uno</itunes:author>
<itunes:image href="https://example.org/art.jpg"/>
<item><title>Episode 2</title><pubDate>Tue, 27 Oct 2026 06:00:00 +0000</pubDate><itunes:duration>1:02:03</itunes:duration>
<itunes:episode>2</itunes:episode><enclosure url="https://example.org/e2.mp3" length="1000" type="audio/mpeg"/></item>
<item><title>Episode 1 (AAC)</title><pubDate>Mon, 5 Oct 2026 06:00:00 GMT</pubDate><itunes:duration>3725</itunes:duration>
<enclosure url="https://example.org/e1.m4a?x=1" length="2000" type="audio/x-m4a"/></item>
<item><title>Trailer video</title><enclosure url="https://example.org/t.mp4" type="video/mp4"/></item>
<item><title>No audio</title></item>
</channel></rss>`

func TestParseFeed(t *testing.T) {
	p, eps, err := parseFeed(strings.NewReader(testFeed))
	if err != nil {
		t.Fatal(err)
	}
	if p.Name != "Café Stories" || p.Author != "Radio Uno" || p.Art600 != "https://example.org/art.jpg" {
		t.Errorf("podcast: %+v", p)
	}
	if len(eps) != 2 {
		t.Fatalf("episodes: %+v", eps)
	}
	e2, e1 := eps[0], eps[1]
	if e2.Title != "Episode 2" || e2.Secs != 3723 || e2.Episode != 2 || e2.Size != 1000 || e2.Date.Day() != 27 || e2.ext() != ".mp3" {
		t.Errorf("episode 2: %+v", e2)
	}
	if e1.Secs != 3725 || e1.Date.IsZero() || e1.ext() != ".m4a" {
		t.Errorf("episode 1: %+v", e1)
	}
	if got := e2.baseName() + e2.ext(); got != "2026-10-27 Episode 2.mp3" {
		t.Errorf("name: %s", got)
	}
	isOnion = true
	if !e2.playable() || e1.playable() {
		t.Error("on the Miyoo only MP3 plays")
	}
	isOnion = false
}

func TestPodcastSearch(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Query().Get("media") != "podcast" {
			http.Error(w, "bad", 400)
			return
		}
		w.Write([]byte(`{"results":[{"collectionName":"Café Stories","artistName":"Radio Uno","feedUrl":"https://example.org/feed",
			"artworkUrl100":"a100","artworkUrl600":"a600","primaryGenreName":"History","trackCount":42},
			{"collectionName":"No feed","artistName":"x"}]}`))
	}))
	defer srv.Close()
	t.Setenv("MIYOOPOD_ITUNES_BASE", srv.URL)
	itunesLast = time.Time{}
	ps, err := podcastSearch("cafe")
	if err != nil || len(ps) != 1 || ps[0].Feed != "https://example.org/feed" || ps[0].Count != 42 || ps[0].Art600 != "a600" {
		t.Errorf("search: %+v %v", ps, err)
	}
}

// usePodData points podcasts.json at a folder of the test's own.
func usePodData(t *testing.T) {
	old := dataDirOverride
	dataDirOverride = t.TempDir()
	podSaved.list, podSaved.read = nil, false
	t.Cleanup(func() {
		dataDirOverride = old
		podSaved.list, podSaved.read = nil, false
	})
}

// An episode cut off halfway is continued (Range with If-Range) on the
// next try, then tagged and noted in the podcast's entry; the artwork
// becomes cover.jpg.
func TestPodcastDownload(t *testing.T) {
	if _, err := exec.LookPath("lame"); err != nil {
		t.Skip("lame not installed")
	}
	usePodData(t)
	dir := t.TempDir()
	wav, mp3 := filepath.Join(dir, "a.wav"), filepath.Join(dir, "a.mp3")
	sineWAV(t, wav)
	runTool(t, "lame", "--quiet", wav, mp3)
	audio, _ := os.ReadFile(mp3)
	var calls, ranged atomic.Int32
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/art.jpg":
			w.Write(jpegBytes())
		case "/e.mp3":
			w.Header().Set("ETag", `"e1"`)
			if calls.Add(1) == 1 {
				// the first try: half of it, then the connection drops
				w.Header().Set("Content-Length", itoaPos(len(audio)))
				w.WriteHeader(200)
				w.Write(audio[:len(audio)/2])
				if hj, ok := w.(http.Hijacker); ok {
					c, _, _ := hj.Hijack()
					c.Close()
				}
				return
			}
			if r.Header.Get("Range") != "" && r.Header.Get("If-Range") == `"e1"` {
				ranged.Add(1)
			}
			http.ServeContent(w, r, "e.mp3", time.Time{}, bytes.NewReader(audio))
		}
	}))
	defer srv.Close()
	out := filepath.Join(dir, "Podcasts", "Café Stories")
	p := podcast{Name: "Café Stories", Author: "Radio Uno", Feed: srv.URL + "/feed", Art600: srv.URL + "/art.jpg"}
	e := podEpisode{GUID: "ep-2", Title: "Episode 2", Date: time.Date(2026, 10, 27, 6, 0, 0, 0, time.UTC), URL: srv.URL + "/e.mp3",
		Type: "audio/mpeg", Size: 1000, Episode: 2}
	name := e.baseName() + e.ext()
	d := &iaDownload{item: &iaItem{Title: p.Name}, files: []iaFile{{Name: e.URL, Title: e.Title, Size: e.Size}},
		dir: out, cancel: make(chan struct{}), total: e.Size}
	rememberPodcast(p)
	if err := podRun(d, p, []podJob{{e, name}}); err != nil {
		t.Fatal(err)
	}
	if len(d.failed) > 0 {
		t.Fatalf("failed: %v (%s)", d.failed, d.lastErr)
	}
	if ranged.Load() == 0 {
		t.Error("the second try started over instead of continuing")
	}
	path := filepath.Join(out, name)
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if id := id3Size(got); !bytes.Equal(got[id:], audio) {
		t.Error("the audio differs from the server's")
	}
	if d.total != int64(len(audio)) || d.done != d.total {
		t.Errorf("progress %d of %d, the file is %d", d.done, d.total, len(audio))
	}
	f, _ := os.Open(path)
	m, err := tag.ReadFrom(f)
	f.Close()
	if err != nil || m.Title() != "Episode 2" || m.Album() != "Café Stories" || m.Artist() != "Radio Uno" || m.Genre() != "Podcast" || m.Year() != 2026 {
		t.Errorf("tags: %v %+v", err, m)
	}
	if !fileExists(filepath.Join(out, "cover.jpg")) {
		t.Error("no cover.jpg")
	}
	if fileExists(path+".part") || fileExists(path+".part.info") {
		t.Error("the .part was left")
	}
	if r := podRecord(p.Feed); r.Files[e.key()] != name || !r.Cover {
		t.Errorf("the podcast's entry: %+v", r)
	}
}

func jpegBytes() []byte {
	var b bytes.Buffer
	jpeg.Encode(&b, image.NewRGBA(image.Rect(0, 0, 8, 8)), nil)
	return b.Bytes()
}

// Only the files the podcast downloaded are its own: a file that is gone
// leaves its entry, and only what is inside the music folder can go.
func TestPodcastFilesAndGuard(t *testing.T) {
	usePodData(t)
	root := t.TempDir()
	t.Setenv("MIYOOPOD_MUSIC_DIR", root)
	old := MUSIC_ROOT
	MUSIC_ROOT = musicRoot()
	defer func() { MUSIC_ROOT = old }()
	p := podcast{Name: "Café Stories", Feed: "https://example.org/feed"}
	os.MkdirAll(p.dir(), 0755)
	for _, name := range []string{"2026-10-27 Episode 2.mp3", "2026-10-05 Episode 1.mp3", "mine.mp3"} {
		os.WriteFile(filepath.Join(p.dir(), name), make([]byte, 100), 0644)
	}
	rememberPodcast(p)
	podUpdate(p, true, func(r *podcast) {
		r.Files["ep-2"] = "2026-10-27 Episode 2.mp3"
		r.Files["ep-1"] = "2026-10-05 Episode 1.mp3"
	})
	os.Remove(filepath.Join(p.dir(), "2026-10-05 Episode 1.mp3"))
	podForgetMissing(p)
	if r := podRecord(p.Feed); len(r.Files) != 1 || r.Files["ep-2"] == "" {
		t.Errorf("the podcast's files: %+v", r.Files)
	}
	if !insideMusic(p.dir()) || !insideMusic(filepath.Join(p.dir(), "2026-10-27 Episode 2.mp3")) {
		t.Error("the podcast's folder is inside the music folder")
	}
	if insideMusic(root) || insideMusic(t.TempDir()) || insideMusic(filepath.Join(root, "..", "x")) {
		t.Error("the music folder itself and what is outside it can't be deleted")
	}
}

// On the Miyoo an episode that turns out to be M4A (whatever the feed
// said) is reported and not kept: its SDL_mixer has no AAC.
func TestPodcastM4AOnion(t *testing.T) {
	usePodData(t)
	dir := t.TempDir()
	part := filepath.Join(dir, "2026-10-27 Episode 2.mp3.part")
	m4a := append([]byte{0, 0, 0, 0x20}, []byte("ftypM4A \x00\x00\x00\x00M4A mp42isom")...)
	os.WriteFile(part, append(m4a, make([]byte, 64)...), 0644)
	if ext, ok := sniffAudio(part); ext != ".m4a" || !ok {
		t.Fatalf("sniffed %s %v", ext, ok)
	}
	isOnion = true
	defer func() { isOnion = false }()
	p := podcast{Name: "Café Stories", Feed: "https://example.org/feed"}
	err := podFinish(p, podEpisode{Title: "Episode 2"}, part, "2026-10-27 Episode 2.mp3", "")
	if err == nil || !strings.Contains(err.Error(), "M4A") {
		t.Errorf("podFinish: %v", err)
	}
	if fileExists(part) || fileExists(filepath.Join(dir, "2026-10-27 Episode 2.m4a")) {
		t.Error("the M4A was kept")
	}
}
