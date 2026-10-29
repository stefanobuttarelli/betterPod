//go:build ignore

// Tests for the Onion layer and Software Update: run with tools/onion/unittest.sh.

package main

import (
	"archive/zip"
	"image"
	"math/rand"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/fogleman/gg"
)

// Onion's update zip is unpacked at the root of the SD card (App/betterpod/…):
// Software Update takes what is under App/betterpod, never conf/.
func TestUnpackUpdate(t *testing.T) {
	defer func(o bool) { isOnion = o }(isOnion)
	isOnion = true
	dir := t.TempDir()
	zp := filepath.Join(dir, "u.zip")
	f, _ := os.Create(zp)
	zw := zip.NewWriter(f)
	for _, n := range []string{"App/", "App/betterpod/", "App/betterpod/launch.sh", "App/betterpod/betterpod",
		"App/betterpod/conf/ipodjs_settings.json", "App/betterpod/libs/libSDL2.so",
		"App/betterpod/assets/ipod/play.png", "App/Other/x", "README.md", "betterpod/x"} {
		if n[len(n)-1] == '/' {
			zw.Create(n)
			continue
		}
		w, _ := zw.Create(n)
		w.Write([]byte(n))
	}
	zw.Close()
	f.Close()
	files, err := unpackUpdate(zp, filepath.Join(dir, "new"))
	if err != nil {
		t.Fatal(err)
	}
	sort.Strings(files)
	want := []string{"betterpod/assets/ipod/play.png", "betterpod/betterpod", "betterpod/launch.sh", "betterpod/libs/libSDL2.so"}
	if !reflect.DeepEqual(files, want) {
		t.Fatalf("files %v, want %v", files, want)
	}
	if b, _ := os.ReadFile(filepath.Join(dir, "new", "betterpod/libs/libSDL2.so")); string(b) != "App/betterpod/libs/libSDL2.so" {
		t.Fatalf("content %q", b)
	}
	// a path escaping the app folder is refused
	zp2 := filepath.Join(dir, "bad.zip")
	f2, _ := os.Create(zp2)
	zw2 := zip.NewWriter(f2)
	w, _ := zw2.Create("App/betterpod/../../etc/x")
	w.Write([]byte("x"))
	zw2.Close()
	f2.Close()
	if _, err := unpackUpdate(zp2, filepath.Join(dir, "new2")); err == nil {
		t.Fatal("escaping path accepted")
	}
}

func TestVersions(t *testing.T) {
	for _, c := range []struct {
		cur, rem string
		newer    bool
	}{{"0.1", "0.2", true}, {"0.1", "0.1", false}, {"0.1", "0.1.1", true}, {"0.10", "0.9", false}, {"0.2", "0.1", false},
		// a release made for one system only adds a number (0.29.1), and the
		// next common version (0.30) is newer
		{"0.2", "0.29", true}, {"0.29", "0.29.1", true}, {"0.29.1", "0.30", true}, {"0.29.1", "0.29", false}} {
		if got := isNewerVersion(c.cur, c.rem); got != c.newer {
			t.Errorf("isNewerVersion(%s, %s) = %v", c.cur, c.rem, got)
		}
	}
}

func TestOnionCurves(t *testing.T) {
	// Onion's volume.h shape, scaled to end at 0 dB (no clipping)
	for _, c := range []struct {
		v   float64
		raw int
	}{{0, -60}, {0.05, -46}, {0.5, -13}, {1, 0}} {
		if got := onionVolumeRaw(c.v); got != c.raw {
			t.Errorf("onionVolumeRaw(%v) = %d, want %d", c.v, got, c.raw)
		}
	}
	// display.h: level 0 -> 3, level 10 -> 100, and back
	if onionLevelRaw(0) != 3 || onionLevelRaw(10) != 100 {
		t.Errorf("levels: %d %d", onionLevelRaw(0), onionLevelRaw(10))
	}
	for l := 0; l <= 10; l++ {
		if got := onionLevelFromRaw(onionLevelRaw(l)); got != l {
			t.Errorf("level %d -> raw %d -> %d", l, onionLevelRaw(l), got)
		}
	}
	if onionLevelFromRaw(-1) != 0 || onionLevelFromRaw(50) != 8 {
		t.Errorf("from raw: %d %d", onionLevelFromRaw(-1), onionLevelFromRaw(50))
	}
}

func TestOnionUnsupported(t *testing.T) {
	isOnion = true
	defer func() { isOnion = false }()
	if supportedAudioExt(".m4a") || !supportedAudioExt(".flac") || !supportedAudioExt(".mp3") {
		t.Error("supportedAudioExt on Onion")
	}
}

// Internet Archive items: ALAC is not offered on the Miyoo, the rest is.
func TestIAGroupsOnion(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Write([]byte(`{"metadata":{"title":"T"},"files":[
			{"name":"1.flac","format":"Flac","size":"10"},
			{"name":"1.m4a","format":"Apple Lossless Audio","size":"10"},
			{"name":"1.mp3","format":"VBR MP3","size":"5"}]}`))
	}))
	defer srv.Close()
	t.Setenv("MIYOOPOD_IA_BASE", srv.URL)
	for _, onion := range []bool{false, true} {
		isOnion = onion
		it, err := iaMetadata("x")
		isOnion = false
		if err != nil {
			t.Fatal(err)
		}
		if len(it.Groups["FLAC"]) != 1 || len(it.Groups["MP3"]) != 1 || (len(it.Groups["ALAC"]) == 1) == onion {
			t.Errorf("onion %v: groups %v", onion, it.Groups)
		}
	}
}

// Settings → Effects: Light keeps the Weather icons still.
func TestWeatherIconsStillInLight(t *testing.T) {
	defer func(m string) { effectsMode = m }(effectsMode)
	now := time.Unix(1000, 500e6)
	effectsMode = "light"
	if f := wxFrameAt(now); f != -1 {
		t.Errorf("light: frame %d, want -1 (still)", f)
	}
	effectsMode = "full"
	if f := wxFrameAt(now); f < 0 || f >= wxFrames {
		t.Errorf("full: frame %d", f)
	}
}

// --- cover sources (ij_coversources.go) ---------------------------------------

func coverServers(t *testing.T) {
	t.Helper()
	img := func(w http.ResponseWriter) { // a 2×2 PNG
		w.Write([]byte("\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x02\x00\x00\x00\x02\x08\x02\x00\x00\x00\xfd\xd4\x9as\x00\x00\x00\x16IDATx\x9cc\xf8\xcf\xc0\xc0\xf0\x9f\x81\x81\xe1?\x03\x03\x00\x1e\xf6\x03\xfd\x8b\x1a\x9a\xe5\x00\x00\x00\x00IEND\xaeB`\x82"))
	}
	var srv *httptest.Server
	srv = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/ws/2/release-group/": // MusicBrainz: nothing
			w.Write([]byte(`{"release-groups":[]}`))
		case r.URL.Path == "/search" && strings.Contains(r.URL.Query().Get("term"), "Rumours"):
			w.Write([]byte(`{"results":[{"collectionName":"Rumours (Super Deluxe)","artistName":"Fleetwood Mac","releaseDate":"1977-02-04T08:00:00Z","artworkUrl100":"` + srv.URL + `/a/100x100bb.jpg","primaryGenreName":"Rock"},
				{"collectionName":"Dreams - Single","artistName":"Fleetwood Mac","artworkUrl100":"` + srv.URL + `/b/100x100bb.jpg"}]}`))
		case r.URL.Path == "/search":
			w.Write([]byte(`{"results":[]}`))
		case r.URL.Path == "/search/album":
			w.Write([]byte(`{"data":[{"title":"Rumours","cover_medium":"m","cover_xl":"x","md5_image":"","record_type":"album","artist":{"name":"Fleetwood Mac"}}]}`))
		case r.URL.Path == "/w/api.php":
			w.Write([]byte(`{"query":{"pages":[
				{"title":"Colin McRae","index":3,"description":"Scottish rally driver (1968–2007)","original":{"source":"` + srv.URL + `/p.jpg"}},
				{"title":"Colin McRae Rally 2.0","index":1,"description":"2000 video game","thumbnail":{"source":"` + srv.URL + `/t.jpg"},"original":{"source":"` + srv.URL + `/box.png"}},
				{"title":"Rumours (album)","index":2,"description":"1977 studio album by Fleetwood Mac","original":{"source":"` + srv.URL + `/r.png"}}]}}`))
		default:
			img(w)
		}
	}))
	t.Cleanup(srv.Close)
	for _, e := range []string{"MIYOOPOD_MB_BASE", "MIYOOPOD_CAA_BASE", "MIYOOPOD_ITUNES_BASE", "MIYOOPOD_DEEZER_BASE", "MIYOOPOD_WIKI_BASE"} {
		t.Setenv(e, srv.URL)
	}
	itunesLast = time.Time{}
}

func TestCoverSources(t *testing.T) {
	coverServers(t)
	it, err := itunesSearch("Rumours Fleetwood Mac", 8)
	if err != nil || len(it) != 2 || !strings.HasSuffix(it[0].Full, "/a/1200x1200bb.jpg") || it[0].Year != "1977" ||
		it[1].Title != "Dreams" || it[1].Kind != "Single" {
		t.Errorf("iTunes: %+v %v", it, err)
	}
	dz, err := deezerSearch("Rumours", 8)
	if err != nil || len(dz) != 0 { // no md5_image: Deezer's placeholder, left out
		t.Errorf("Deezer: %+v %v", dz, err)
	}
	wk, err := wikiSearch("Colin McRae Rally 2.0", 8)
	if err != nil || len(wk) != 2 {
		t.Fatalf("Wikipedia: %+v %v", wk, err)
	}
	if c := wk[0]; c.Title != "Colin McRae Rally 2.0" || c.Kind != "Video game" || !c.soundtrack || c.Year != "2000" {
		t.Errorf("Wikipedia game: %+v", c)
	}
	if c := wk[1]; c.Title != "Rumours" || c.Artist != "Fleetwood Mac" || c.Kind != "" || c.Year != "1977" {
		t.Errorf("Wikipedia album: %+v", c)
	}
	for d, want := range map[string]string{"1977 studio album by Fleetwood Mac": "Album", "2000 video game": "Video game",
		"Video game series": "", "1965 song by the Beatles": "", "2019 EP by Phoebe Bridgers": "EP",
		"1997 soundtrack album by Alan Menken": "Soundtrack", "Scottish rally driver": "", "album by Songhoy Blues": "Album"} {
		if got := wikiKind(d); got != want {
			t.Errorf("wikiKind(%q) = %q, want %q", d, got, want)
		}
	}
}

// The automatic search: MusicBrainz has nothing, so the other sources are
// asked, and only a checked result is taken.
func TestFindOtherCover(t *testing.T) {
	coverServers(t)
	game := &Album{Name: "Colin McRae Rally 2.0", Tracks: []*Track{{Title: "Menu", Genre: "Game"}}}
	data, err := findCover(game)
	if err != nil || data == nil {
		t.Errorf("game rip: %v (no cover)", err)
	}
	rock := &Album{Name: "Rumours", Artist: "Fleetwood Mac", Tracks: []*Track{{Artist: "Fleetwood Mac"}}}
	if data, err := findCover(rock); err != nil || data == nil {
		t.Errorf("Rumours: %v (no cover)", err)
	}
	other := &Album{Name: "Rumours", Artist: "Someone Else", Tracks: []*Track{{Artist: "Someone Else"}}}
	if data, _ := findCover(other); data != nil {
		t.Error("Rumours by someone else took Fleetwood Mac's cover")
	}
	if q := newAlbumQuery(game); !q.soundtrack {
		t.Error("genre Game: not a soundtrack")
	}
}

// --- the C pixel loops (cdraw.c) against the Go ones --------------------------

func randImage(r *rand.Rand, w, h int, premul bool) *image.RGBA {
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for i := 0; i < len(img.Pix); i += 4 {
		a := uint8(r.Intn(256))
		switch r.Intn(4) { // plenty of fully opaque and fully clear pixels
		case 0:
			a = 255
		case 1:
			a = 0
		}
		for c := 0; c < 3; c++ {
			v := uint8(r.Intn(256))
			if premul {
				v = uint8(int(v) * int(a) / 255)
			}
			img.Pix[i+c] = v
		}
		img.Pix[i+3] = a
	}
	return img
}

func testClone(img *image.RGBA) *image.RGBA {
	c := image.NewRGBA(img.Rect)
	copy(c.Pix, img.Pix)
	return c
}

// both draws f on a copy of dst with the Go loops and with the C ones.
func both(dst *image.RGBA, f func(d *image.RGBA)) (*image.RGBA, *image.RGBA) {
	g, c := testClone(dst), testClone(dst)
	cDraw = false
	f(g)
	cDraw = true
	f(c)
	return g, c
}

func diffPixels(a, b *image.RGBA) (n, maxd int) {
	for i := range a.Pix {
		d := int(a.Pix[i]) - int(b.Pix[i])
		if d < 0 {
			d = -d
		}
		if d > 0 {
			n++
		}
		if d > maxd {
			maxd = d
		}
	}
	return
}

func TestCDrawMatchesGo(t *testing.T) {
	defer func() { cDraw = true }()
	r := rand.New(rand.NewSource(1))
	for iter := 0; iter < 40; iter++ {
		dst := randImage(r, 640, 480, false)
		src := randImage(r, 1+r.Intn(500), 1+r.Intn(400), true)
		dx, dy := r.Intn(900)-200, r.Intn(700)-150
		op := []float64{1, 1, 0.5, 0.01, 0.999, 1.3}[iter%6]
		g, c := both(dst, func(d *image.RGBA) { blit(d, src, dx, dy, op) })
		if n, m := diffPixels(g, c); n != 0 {
			t.Fatalf("blit iter %d (op %v): %d bytes differ, max %d", iter, op, n, m)
		}
		x := float64(r.Intn(700)-100) + r.Float64()
		g, c = both(dst, func(d *image.RGBA) { blitShiftX(d, src, x, dy, op) })
		if n, m := diffPixels(g, c); n != 0 {
			t.Fatalf("blitShiftX iter %d: %d bytes differ, max %d", iter, n, m)
		}
		alpha := boxShadowProfile(float64(8+r.Intn(60)), 0.2+0.6*r.Float64())
		g, c = both(dst, func(d *image.RGBA) { darkenColumns(d, dx, dy, 300, alpha) })
		if n, m := diffPixels(g, c); n != 0 {
			t.Fatalf("darkenColumns iter %d: %d bytes differ, max %d", iter, n, m)
		}
	}
	// a clipped sub-image as destination (blitClipped)
	dst := randImage(r, 640, 480, false)
	src := randImage(r, 300, 300, true)
	g, c := both(dst, func(d *image.RGBA) { blitClipped(d, src, 100, 50, 0.7, image.Rect(150, 80, 400, 260)) })
	if n, m := diffPixels(g, c); n != 0 {
		t.Fatalf("blitClipped: %d bytes differ, max %d", n, m)
	}
}

func TestCDrawTexMatchesGo(t *testing.T) {
	defer func() { cDraw = true }()
	r := rand.New(rand.NewSource(2))
	art := randImage(r, 256, 256, true)
	for _, a := range []float64{0, 25, -60, 75, 180} {
		m := mMul(mRotY(a), mScale(0.9))
		full := mMul(mTrans(64, 84, 0), mMul(m, mTrans(-64, -64, 0)))
		proj := mMul(mTrans(160, 110, 0), mMul(mPersp(500), mTrans(-160, -110, 0)))
		m = mMul(proj, full)
		tm := texMap{img: art, su: 2, sv: 2, u1: 128, v1: 166.4, reflFrom: 128, reflMax: 0.2, reflStart: 0.7}
		dst := randImage(r, 640, 480, false)
		g, c := both(dst, func(d *image.RGBA) { drawTex(d, m, tm, 0.8) })
		// the float rounding may differ by an ulp (Go fuses multiply-adds on
		// arm64, cdraw.c doesn't): a handful of texels may shift by one
		n, maxd := diffPixels(g, c)
		if n > len(dst.Pix)/1000 || maxd > 64 {
			t.Fatalf("drawTex angle %v: %d bytes differ, max %d", a, n, maxd)
		}
		t.Logf("drawTex angle %v: %d of %d bytes differ", a, n, len(dst.Pix))
	}
}

// --- benchmarks: Go loops against cdraw.c (tools/onion/bench.sh runs them
// emulated on 32-bit ARM, where the instruction count is what matters) ------

// uiImage: like the interface's layers: opaque rows, an antialiased edge
// and a clear margin on the right, a few antialiased pixels (text) inside.
func uiImage(r *rand.Rand, w, h int) *image.RGBA {
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			a := 255
			switch {
			case x >= w-40:
				a = 0
			case x >= w-50:
				a = 255 * (w - 40 - x) / 10
			case r.Intn(100) < 3:
				a = r.Intn(256)
			}
			i := img.PixOffset(x, y)
			for c := 0; c < 3; c++ {
				img.Pix[i+c] = uint8(r.Intn(256) * a / 255)
			}
			img.Pix[i+3] = uint8(a)
		}
	}
	return img
}

func benchDraw(b *testing.B, useC bool, f func(dst *image.RGBA)) {
	defer func() { cDraw = true }()
	cDraw = useC
	dst := image.NewRGBA(image.Rect(0, 0, 640, 480))
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		f(dst)
	}
}

var benchR = rand.New(rand.NewSource(3))
var benchPanel = uiImage(benchR, 320, 480)
var benchCover = uiImage(benchR, 480, 480)

func BenchmarkBlitGo(b *testing.B) {
	benchDraw(b, false, func(d *image.RGBA) { blit(d, benchPanel, 0, 0, 1) })
}
func BenchmarkBlitC(b *testing.B) {
	benchDraw(b, true, func(d *image.RGBA) { blit(d, benchPanel, 0, 0, 1) })
}
func BenchmarkBlitFadeGo(b *testing.B) {
	benchDraw(b, false, func(d *image.RGBA) { blit(d, benchPanel, 0, 0, 0.5) })
}
func BenchmarkBlitFadeC(b *testing.B) {
	benchDraw(b, true, func(d *image.RGBA) { blit(d, benchPanel, 0, 0, 0.5) })
}
func BenchmarkShiftXGo(b *testing.B) {
	benchDraw(b, false, func(d *image.RGBA) { blitShiftX(d, benchCover, 80.37, 0, 1) })
}
func BenchmarkShiftXC(b *testing.B) {
	benchDraw(b, true, func(d *image.RGBA) { blitShiftX(d, benchCover, 80.37, 0, 1) })
}
func BenchmarkDarkenGo(b *testing.B) {
	p := boxShadowProfile(48, 1)
	benchDraw(b, false, func(d *image.RGBA) { darkenColumns(d, 320, 0, 480, p) })
}
func BenchmarkDarkenC(b *testing.B) {
	p := boxShadowProfile(48, 1)
	benchDraw(b, true, func(d *image.RGBA) { darkenColumns(d, 320, 0, 480, p) })
}

func benchTex() (mat4, texMap) {
	m := mMul(mRotY(60), mScale(1.2))
	full := mMul(mTrans(64, 84, 0), mMul(m, mTrans(-64, -64, 0)))
	proj := mMul(mTrans(160, 110, 0), mMul(mPersp(500), mTrans(-160, -110, 0)))
	return mMul(proj, full), texMap{img: benchCover, su: 480.0 / 128, sv: 480.0 / 128, u1: 128, v1: 166.4, reflFrom: 128, reflMax: 0.2, reflStart: 0.7}
}

func BenchmarkTexGo(b *testing.B) {
	m, t := benchTex()
	benchDraw(b, false, func(d *image.RGBA) { drawTex(d, m, t, 1) })
}
func BenchmarkTexC(b *testing.B) {
	m, t := benchTex()
	benchDraw(b, true, func(d *image.RGBA) { drawTex(d, m, t, 1) })
}

// On Now Playing up / right raise the volume, down / left lower it; in the
// menus down / right go forward, like the click wheel.
func TestKeyDir(t *testing.T) {
	menu := &IpodUI{split: []*View{{ID: "home"}}}
	np := &IpodUI{split: []*View{{ID: "home"}}, full: []*View{{ID: "nowPlaying", Custom: &NowPlayingView{}}}}
	flow := &IpodUI{split: []*View{{ID: "home"}}, cover: &CoverFlow{}}
	flowNP := &IpodUI{split: []*View{{ID: "home"}}, cover: &CoverFlow{playing: true}} // Cover Flow's Now Playing
	for _, c := range []struct {
		ui   *IpodUI
		k    Key
		want int
	}{
		{menu, DOWN, 1}, {menu, RIGHT, 1}, {menu, UP, -1}, {menu, LEFT, -1},
		{np, UP, 1}, {np, RIGHT, 1}, {np, DOWN, -1}, {np, LEFT, -1},
		{flow, DOWN, 1}, {flow, RIGHT, 1}, {flow, UP, -1}, {flow, LEFT, -1},
		{flowNP, UP, 1}, {flowNP, RIGHT, 1}, {flowNP, DOWN, -1}, {flowNP, LEFT, -1},
	} {
		if got := c.ui.keyDir(c.k); got != c.want {
			t.Errorf("keyDir(%d) on %s = %d, want %d", c.k, c.ui.topScreen().ID, got, c.want)
		}
	}
}

// Weather icons are drawn with gg, frame by frame: one frame of the Games
// preview (256 px) and of the forecast's big icon (168 px), for every kind.
func benchWx(b *testing.B, size int) {
	for i := 0; i < b.N; i++ {
		k := wxKind(i % (int(wxThunder) + 1))
		img := image.NewRGBA(image.Rect(0, 0, size, size))
		drawWxIcon(gg.NewContextForRGBA(img), k, float64(size), 0.3)
	}
}
func BenchmarkWxIcon256(b *testing.B) { benchWx(b, 256) }
func BenchmarkWxIcon168(b *testing.B) { benchWx(b, 168) }

// Settings on the Miyoo: Effects is there, YouTube isn't (onionSettings);
// a merge that takes KnulliPod's settingsView whole drops both (0.48).
func TestOnionSettings(t *testing.T) {
	labels := func() map[string]bool {
		ui := &IpodUI{app: &MiyooPod{}}
		m := map[string]bool{}
		for _, it := range ui.settingsView().Build() {
			m[it.Label] = true
		}
		return m
	}
	isOnion = true
	got := labels()
	isOnion = false
	if got["YouTube"] || got["Bluetooth"] || !got["Effects"] || !got["Podcasts"] || !got["Screensaver"] || !got["Software Update"] {
		t.Errorf("Settings on Onion: %v", got)
	}
	if m := labels(); !m["YouTube"] || !m["Bluetooth"] {
		t.Errorf("Settings elsewhere: %v", m)
	}
}

func TestOnionSaverSettings(t *testing.T) {
	labels := func() map[string]bool {
		ui := &IpodUI{app: &MiyooPod{}}
		m := map[string]bool{}
		for _, it := range ui.saverSettingsView().Build() {
			m[it.Label] = true
		}
		return m
	}
	isOnion = true
	got := labels()
	isOnion = false
	if got["Style"] || !got["Dim the Screen"] || !got["Start Now"] {
		t.Errorf("Screensaver settings on Onion: %v", got)
	}
	if !labels()["Style"] {
		t.Error("Screensaver settings elsewhere: no Style")
	}
}

func TestOnionSleepAction(t *testing.T) {
	m := time.Minute
	cases := []struct {
		locked, auto, playing, busy, charging bool
		idle                                  time.Duration
		mins                                  int
		want                                  string
	}{
		{false, false, false, false, false, 4 * m, 5, ""},
		{false, false, false, false, false, 5 * m, 5, "screen off"},
		{false, false, true, false, false, 60 * m, 5, ""},          // music playing
		{false, false, false, true, false, 60 * m, 5, ""},          // a download
		{false, false, false, false, false, 600 * m, 0, ""},        // Onion's timer off
		{true, true, false, false, false, 59 * time.Second, 5, ""}, // the timer turned it off
		{true, true, false, false, false, m, 5, "power off"},       // and one minute later
		{true, false, false, false, false, 5 * m, 5, ""},           // POWER: the timer and one minute
		{true, false, false, false, false, 6 * m, 5, "power off"},
		{true, false, false, false, true, 60 * m, 5, ""},  // charging
		{true, false, true, false, false, 600 * m, 5, ""}, // locked with music
	}
	for i, c := range cases {
		if got := onionSleepAction(c.locked, c.auto, c.playing, c.busy, c.charging, c.idle, c.mins, m); got != c.want {
			t.Errorf("case %d: %q, want %q", i, got, c.want)
		}
	}
}

func TestOnionSleepMinutes(t *testing.T) {
	os.Setenv("MIYOOPOD_SLEEP_MINUTES", "3")
	defer os.Unsetenv("MIYOOPOD_SLEEP_MINUTES")
	if got := onionSleepMinutes(); got != 3 {
		t.Errorf("override: %d", got)
	}
	os.Unsetenv("MIYOOPOD_SLEEP_MINUTES")
	if got := onionSleepMinutes(); got != 5 { // no system.json here: Onion's default
		t.Errorf("default: %d", got)
	}
}
