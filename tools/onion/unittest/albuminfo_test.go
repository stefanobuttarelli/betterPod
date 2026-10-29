//go:build ignore

// Tests for Album Info: writing tags (tagwrite.go) into real MP3, FLAC and
// Ogg files made by lame, flac and oggenc, pairing songs and the sources
// (ij_infosources.go, ij_albuminfo.go). Run with tools/onion/unittest.sh.

package main

import (
	"bytes"
	"encoding/binary"
	"image"
	"image/jpeg"
	"math"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/dhowden/tag"
)

// sineWAV: a second of 16-bit stereo sine, for the encoders.
func sineWAV(t *testing.T, path string) {
	t.Helper()
	const rate, n = 44100, 44100
	var b bytes.Buffer
	b.WriteString("RIFF")
	binary.Write(&b, binary.LittleEndian, uint32(36+n*4))
	b.WriteString("WAVEfmt ")
	for _, v := range []any{uint32(16), uint16(1), uint16(2), uint32(rate), uint32(rate * 4), uint16(4), uint16(16)} {
		binary.Write(&b, binary.LittleEndian, v)
	}
	b.WriteString("data")
	binary.Write(&b, binary.LittleEndian, uint32(n*4))
	for i := 0; i < n; i++ {
		v := int16(8000 * math.Sin(float64(i)*2*math.Pi*440/rate))
		binary.Write(&b, binary.LittleEndian, []int16{v, v})
	}
	if err := os.WriteFile(path, b.Bytes(), 0644); err != nil {
		t.Fatal(err)
	}
}

func runTool(t *testing.T, name string, args ...string) {
	t.Helper()
	if out, err := exec.Command(name, args...).CombinedOutput(); err != nil {
		t.Fatalf("%s %v: %v\n%s", name, args, err, out)
	}
}

func id3Size(b []byte) int {
	if len(b) < 10 || string(b[:3]) != "ID3" {
		return 0
	}
	return 10 + syncsafe(b[6:10])
}

func TestWriteTags(t *testing.T) {
	for _, tool := range []string{"lame", "flac", "oggenc", "ogginfo", "oggdec"} {
		if _, err := exec.LookPath(tool); err != nil {
			t.Skip(tool + " not installed")
		}
	}
	dir := t.TempDir()
	wav, jpg := filepath.Join(dir, "a.wav"), filepath.Join(dir, "c.jpg")
	sineWAV(t, wav)
	var jb bytes.Buffer
	jpeg.Encode(&jb, image.NewRGBA(image.Rect(0, 0, 8, 8)), nil)
	os.WriteFile(jpg, jb.Bytes(), 0644)
	mp3, bare := filepath.Join(dir, "a.mp3"), filepath.Join(dir, "b.mp3")
	runTool(t, "lame", "--quiet", "--id3v2-only", "--tt", "Old", "--ta", "Old Artist", "--ti", jpg, wav, mp3)
	runTool(t, "lame", "--quiet", wav, bare) // no tag at all
	fl := filepath.Join(dir, "a.flac")
	runTool(t, "flac", "--silent", "-T", "TITLE=Old", "-T", "COMMENT=keep me", "--picture=3|image/jpeg|||"+jpg, "-o", fl, wav)
	ogg := filepath.Join(dir, "a.ogg")
	runTool(t, "oggenc", "--quiet", "-t", "Old", "-c", "COMMENT=keep me", "-o", ogg, wav)

	long := strings.Repeat("A long title, ", 1000) // more than the room left (14 KB): the file is written again
	for _, c := range []struct {
		path, title string
		pic         bool
	}{{mp3, "Città — 東京", true}, {mp3, long, true}, {mp3, "Short", true}, {bare, "Città — 東京", false},
		{fl, "Città — 東京", true}, {fl, long, true}, {fl, "Short", true},
		{ogg, "Città — 東京", false}, {ogg, long, false}, {ogg, "Short", false}} {
		name := filepath.Base(c.path)
		before, _ := os.ReadFile(c.path)
		st0, _ := os.Stat(c.path)
		want := songTags{Title: c.title, Artist: "Codemasters", Album: "Colin McRae Rally 2.0", AlbumArtist: "Codemasters",
			Genre: "Soundtrack", Track: 3, TrackTotal: 12, Disc: 1, Year: 2000}
		if err := writeTags(c.path, want); err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		f, _ := os.Open(c.path)
		m, err := tag.ReadFrom(f)
		f.Close()
		if err != nil {
			t.Fatalf("%s: read back: %v", name, err)
		}
		n, tot := m.Track()
		d, _ := m.Disc()
		if m.Title() != want.Title || m.Artist() != want.Artist || m.Album() != want.Album || m.AlbumArtist() != want.AlbumArtist ||
			m.Genre() != want.Genre || n != 3 || tot != 12 || d != 1 || m.Year() != 2000 {
			t.Errorf("%s: read back %q %q %q %q %q %d/%d %d %d", name, m.Title(), m.Artist(), m.Album(),
				m.AlbumArtist(), m.Genre(), n, tot, d, m.Year())
		}
		if c.pic && (m.Picture() == nil || !bytes.Equal(m.Picture().Data, jb.Bytes())) {
			t.Errorf("%s: the cover was lost", name)
		}
		if ext := filepath.Ext(c.path); ext == ".flac" || ext == ".ogg" {
			if raw := m.Raw(); raw["comment"] != "keep me" && raw["COMMENT"] != "keep me" {
				t.Errorf("%s: the other comments were lost: %v", name, raw)
			}
		}
		after, _ := os.ReadFile(c.path)
		if st1, _ := os.Stat(c.path); c.title == "Short" && filepath.Ext(c.path) != ".ogg" && st1.Size() != st0.Size() {
			t.Errorf("%s: a short title should fit in the room the file has (%d → %d bytes)", name, st0.Size(), st1.Size())
		}
		switch filepath.Ext(c.path) {
		case ".mp3": // the audio after the tag, byte for byte
			if !bytes.Equal(before[id3Size(before):], after[id3Size(after):]) {
				t.Errorf("%s: the audio changed", name)
			}
		case ".flac":
			runTool(t, "flac", "--silent", "-t", c.path)
		case ".ogg":
			runTool(t, "ogginfo", "-q", c.path)
			runTool(t, "oggdec", "--quiet", "-o", filepath.Join(dir, "x.wav"), c.path)
		}
		if _, err := os.Stat(c.path + ".tagtmp"); err == nil {
			t.Errorf("%s: the temporary file was left", name)
		}
	}
	if err := writeTags(wav, songTags{Title: "x"}); err != errNoTags {
		t.Errorf("WAV: %v, want errNoTags", err)
	}
}

func TestMatchTracks(t *testing.T) {
	theirs := []infoTrack{{Disc: 1, Num: 1, Title: "Intro", Secs: 60}, {Disc: 1, Num: 2, Title: "Rally", Secs: 120}, {Disc: 1, Num: 3, Title: "Menu", Secs: 90}}
	// by number when the lengths agree (KnulliPod 0.48: never by position)
	if m := matchTracks([]songTags{{Track: 2}, {Track: 3}}, []float64{121, 200}, theirs); m[0] != 1 || m[1] != -1 {
		t.Errorf("by number: %v", m)
	}
	if m := matchTracks([]songTags{{Track: 1}, {Track: 2}, {Track: 3}}, nil, theirs); m[0] != 0 || m[2] != 2 {
		t.Errorf("by number, lengths unknown, as many songs: %v", m)
	}
	if m := matchTracks([]songTags{{}, {}, {}}, nil, theirs); m[0] != -1 || m[1] != -1 || m[2] != -1 {
		t.Errorf("in order: %v", m)
	}
	if m := matchTracks([]songTags{{Title: "menu"}, {Title: "Track 7"}}, []float64{0, 59}, theirs); m[0] != 2 || m[1] != 0 {
		t.Errorf("by title, then by length: %v", m)
	}
}

func TestWikiClean(t *testing.T) {
	for in, want := range map[string]string{
		"[[Codemasters]]{{efn|Game Boy Advance version developed by Spellbound}}": "Codemasters",
		"{{hlist|[[Hans Zimmer]]|[[Lisa Gerrard]]}}":                              "Hans Zimmer",
		"[[Fleetwood Mac|The Mac]]<ref>x</ref>":                                   "The Mac",
		"Steve Root<br />Someone":                                                 "Steve Root",
	} {
		if got := wikiClean(in); got != want {
			t.Errorf("wikiClean(%q) = %q, want %q", in, got, want)
		}
	}
}

const wikiGame = "{{Infobox video game\n| title = Colin McRae Rally 2.0\n| developer = [[Codemasters]]{{efn|GBA by Spellbound}}\n" +
	"| composer = \n| released = '''PlayStation'''{{vgrelease|UK|9 June 2000}}\n}}"

// Album Info with fake sources: the songs are paired, the album title kept
// when only an edition note differs, the empty artists filled in.
func TestAlbumInfoSources(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/lookup":
			w.Write([]byte(`{"results":[{"wrapperType":"collection"},
				{"wrapperType":"track","trackName":"Intro","artistName":"Codemasters","trackNumber":1,"discNumber":1,"trackTimeMillis":60000},
				{"wrapperType":"track","trackName":"Rally","artistName":"Codemasters","trackNumber":2,"discNumber":1,"trackTimeMillis":120000}]}`))
		case "/w/api.php":
			w.Write([]byte(`{"parse":{"wikitext":` + strings.ReplaceAll(jsonQuote(wikiGame), "\n", `\n`) + `}}`))
		}
	}))
	defer srv.Close()
	t.Setenv("MIYOOPOD_ITUNES_BASE", srv.URL)
	t.Setenv("MIYOOPOD_WIKI_BASE", srv.URL)
	itunesLast = time.Time{}
	al := &Album{Name: "Colin McRae Rally 2.0", Artist: "Unknown Artist", Tracks: []*Track{
		{Path: "/m/b.mp3", Title: "b", Artist: "Unknown Artist", Album: "Colin McRae Rally 2.0", Duration: 121},
		{Path: "/m/a.mp3", Title: "a", Artist: "Unknown Artist", Album: "Colin McRae Rally 2.0", Duration: 60}}}
	e := newAlbumEdit(al)
	if e.songs[0].tags.Artist != "" {
		t.Fatal("Unknown Artist should read as empty")
	}
	it := &albumInfo{Source: "iTunes", ID: "1", Title: "Colin McRae Rally 2.0 (Original Soundtrack)", Artist: "Codemasters", Year: "2000"}
	if err := it.detail(); err != nil || len(it.Tracks) != 2 {
		t.Fatalf("iTunes detail: %v %+v", err, it.Tracks)
	}
	if n := e.apply(it); n != 2 {
		t.Errorf("paired %d of 2", n)
	}
	for _, s := range e.songs {
		g := s.tags
		if g.Album != "Colin McRae Rally 2.0" || g.AlbumArtist != "Codemasters" || g.Artist != "Codemasters" || g.Year != 2000 || g.TrackTotal != 2 {
			t.Errorf("after iTunes: %+v", g)
		}
	}
	// unnumbered, same count: in the album's order (a, b) → Intro, Rally
	if e.songs[0].tags.Title != "Intro" || e.songs[1].tags.Title != "Rally" || e.songs[1].tags.Track != 2 {
		t.Errorf("pairing: %+v / %+v", e.songs[0].tags, e.songs[1].tags)
	}
	wk := &albumInfo{Source: "Wikipedia", ID: "Colin McRae Rally 2.0", Title: "Colin McRae Rally 2.0", Kind: "Video game", soundtrack: true}
	if err := wk.detail(); err != nil || wk.Artist != "Codemasters" || wk.ArtistNote != "developer" || wk.Year != "2000" || wk.Genre != "Soundtrack" {
		t.Errorf("Wikipedia detail: %v %+v", err, wk)
	}
}

func jsonQuote(s string) string {
	b := strings.NewReplacer(`\`, `\\`, `"`, `\"`).Replace(s)
	return `"` + b + `"`
}
