//go:build ignore

// Shared UI, library and Software Update tests: run with tools/unittest.sh.

package main

import (
	"archive/zip"
	"errors"
	"image"
	"image/color"
	"math"
	"math/rand"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/fogleman/gg"
	"golang.org/x/image/draw"
	"golang.org/x/image/vector"
)

// Software Update accepts the Knulli launcher and app, never conf/.
func TestUnpackUpdate(t *testing.T) {
	dir := t.TempDir()
	zp := filepath.Join(dir, "u.zip")
	f, _ := os.Create(zp)
	zw := zip.NewWriter(f)
	for _, n := range []string{"betterpod/", "betterPod.sh", "betterpod/betterpod",
		"betterpod/conf/ipodjs_settings.json", "betterpod/libs/libSDL2.so",
		"betterpod/assets/ipod/play.png", "Other/x", "README.md", "App/old/x"} {
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
	want := []string{"betterPod.sh", "betterpod/assets/ipod/play.png", "betterpod/betterpod", "betterpod/libs/libSDL2.so"}
	if !reflect.DeepEqual(files, want) {
		t.Fatalf("files %v, want %v", files, want)
	}
	if b, _ := os.ReadFile(filepath.Join(dir, "new", "betterpod/libs/libSDL2.so")); string(b) != "betterpod/libs/libSDL2.so" {
		t.Fatalf("content %q", b)
	}
	// a path escaping the app folder is refused
	zp2 := filepath.Join(dir, "bad.zip")
	f2, _ := os.Create(zp2)
	zw2 := zip.NewWriter(f2)
	w, _ := zw2.Create("betterpod/../../etc/x")
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
		// Patch releases sort before the next minor version.
		{"0.2", "0.29", true}, {"0.29", "0.29.1", true}, {"0.29.1", "0.30", true}, {"0.29.1", "0.29", false},
		// betas (1.0 on): before their release, after what came before,
		// in their own order
		{"1.0", "1.1-beta.1", true}, {"1.0.3", "1.1-beta.1", true}, {"1.1-beta.1", "1.1-beta.2", true},
		{"1.1-beta.2", "1.1", true}, {"1.1-beta.8", "1.1", true}, {"1.1", "1.1-beta.8", false}, {"1.1", "1.1-beta.3", false}, {"1.1-beta.2", "1.0.9", false},
		{"1.1-beta.2", "1.1-beta.2", false}, {"1.1-beta.10", "1.1-beta.9", false}, {"0.91.1", "1.0-beta.1", true}} {
		if got := isNewerVersion(c.cur, c.rem); got != c.newer {
			t.Errorf("isNewerVersion(%s, %s) = %v", c.cur, c.rem, got)
		}
	}
}

// Internet Archive offers every supported lossless and compressed format.
func TestIAGroups(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Write([]byte(`{"metadata":{"title":"T"},"files":[
   {"name":"1.flac","format":"Flac","size":"10"},
   {"name":"1.m4a","format":"Apple Lossless Audio","size":"10"},
   {"name":"1.mp3","format":"VBR MP3","size":"5"}]}`))
	}))
	defer srv.Close()
	t.Setenv("BETTERPOD_IA_BASE", srv.URL)
	it, err := iaMetadata("x")
	if err != nil {
		t.Fatal(err)
	}
	for _, format := range []string{"FLAC", "ALAC", "MP3"} {
		if len(it.Groups[format]) != 1 {
			t.Errorf("%s: groups %v", format, it.Groups)
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
	for _, e := range []string{"BETTERPOD_MB_BASE", "BETTERPOD_CAA_BASE", "BETTERPOD_ITUNES_BASE", "BETTERPOD_DEEZER_BASE", "BETTERPOD_WIKI_BASE"} {
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
	defer func(was bool) { cDraw = was }(cDraw)
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
		// dimRect, also over see-through pixels (a layer)
		da := []float64{0.32, 0.45, 0.6, 0.01, 0.999, 0.17}[iter%6]
		for _, d := range []*image.RGBA{dst, src} {
			w, h := 1+r.Intn(640), 1+r.Intn(480)
			g, c = both(d, func(d *image.RGBA) { dimRect(d, dx, dy, w, h, da) })
			if n, m := diffPixels(g, c); n != 0 {
				t.Fatalf("dimRect iter %d: %d bytes differ, max %d", iter, n, m)
			}
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
	defer func(was bool) { cDraw = was }(cDraw)
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

// --- benchmarks: Go loops against cdraw.c ---------------------------------

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
	defer func(was bool) { cDraw = was }(cDraw)
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

// Settings retain the full feature set on supported consoles.
func TestSettings(t *testing.T) {
	ui := &IpodUI{app: &BetterPod{}}
	labels := map[string]bool{}
	for _, it := range ui.settingsView().Build() {
		labels[it.Label] = true
	}
	for _, label := range []string{"YouTube", "Bluetooth", "Effects", "Podcasts", "Screensaver", "Software Update"} {
		if !labels[label] {
			t.Errorf("Settings missing %s: %v", label, labels)
		}
	}
	labels = map[string]bool{}
	for _, it := range ui.saverSettingsView().Build() {
		labels[it.Label] = true
	}
	for _, label := range []string{"Style", "Dim the Screen", "Start Now"} {
		if !labels[label] {
			t.Errorf("Screensaver settings missing %s: %v", label, labels)
		}
	}
}

// Reusing the polygon rasterizer avoids allocating a whole screen per draw.
func TestAaPolyMemory(t *testing.T) {
	pts := [][2]float64{{60, 50}, {260, 50}, {260, 106}, {60, 142}}
	a := image.NewRGBA(image.Rect(0, 0, 640, 480))
	aaPoly(a, pts, Col{255, 255, 255, 0.5})
	// (the best of three: TotalAlloc counts the whole process, and other
	// tests' goroutines can allocate meanwhile)
	per := uint64(1 << 62)
	for try := 0; try < 3; try++ {
		var m0, m1 runtime.MemStats
		runtime.ReadMemStats(&m0)
		for i := 0; i < 20; i++ {
			aaPoly(a, pts, Col{255, 255, 255, 0.15})
		}
		runtime.ReadMemStats(&m1)
		per = min(per, (m1.TotalAlloc-m0.TotalAlloc)/20)
	}
	if per > 16<<10 {
		t.Errorf("aaPoly allocates %d bytes a call", per)
	}
	// against the whole screen rasterized, as it was
	b := image.NewRGBA(image.Rect(0, 0, 640, 480))
	var ras vector.Rasterizer
	ras.Reset(640, 480)
	ras.MoveTo(float32(pts[0][0]), float32(pts[0][1]))
	for _, p := range pts[1:] {
		ras.LineTo(float32(p[0]), float32(p[1]))
	}
	ras.ClosePath()
	ras.Draw(b, b.Bounds(), image.NewUniform(c8(Col{255, 255, 255, 0.5})), image.Point{})
	c := image.NewRGBA(image.Rect(0, 0, 640, 480))
	aaPoly(c, pts, Col{255, 255, 255, 0.5})
	for i := range b.Pix {
		if d := int(b.Pix[i]) - int(c.Pix[i]); d > 1 || d < -1 {
			t.Fatalf("byte %d: %d, was %d", i, c.Pix[i], b.Pix[i])
		}
	}
}

// Share rejections identify unsupported formats and retain their reason.
func TestShareUnplayable(t *testing.T) {
	if got := shareUnplayable([]string{".m4a", ".mp3", ".alac"}); len(got) != 0 {
		t.Errorf("supported formats: %v", got)
	}
	if got := shareUnplayable([]string{".mp3", ".xyz", ".XYZ"}); !reflect.DeepEqual(got, []string{"XYZ (.xyz)"}) {
		t.Errorf("unsupported formats: %v", got)
	}
	r := shareUnplayableReason([]string{"XYZ (.xyz)"})
	names, on, ok := shareParseUnplayable(errors.New("TransferError: " + r))
	if !ok || on != "Knulli" || !reflect.DeepEqual(names, []string{"XYZ (.xyz)"}) {
		t.Errorf("%q read back as %v on %q (%v)", r, names, on, ok)
	}
	if _, _, ok := shareParseUnplayable(errors.New("TransferError: transfer rejected")); ok {
		t.Error("ordinary rejection read as format one")
	}
	if got := shExtOf(names[0]); got != ".xyz" {
		t.Errorf("tag extension: %q", got)
	}
}

// A big cover is averaged down before Catmull-Rom (shrinkImage): what comes
// out looks as Catmull-Rom alone, an edge stays where it was, and the 480 px
// preview of a 3000 px cover no longer takes a 46 MB buffer.
func TestShrinkImage(t *testing.T) {
	const W, H = 3001, 2999
	ycc := image.NewYCbCr(image.Rect(0, 0, W, H), image.YCbCrSubsampleRatio420)
	rgba := image.NewRGBA(ycc.Rect)
	nrgba := image.NewNRGBA(ycc.Rect)
	gray := image.NewGray(ycc.Rect)
	for y := 0; y < H; y++ {
		for x := 0; x < W; x++ {
			// rings and a gradient, and a sharp edge at x = 1700
			v := 128 + 60*math.Sin(math.Hypot(float64(x-1200), float64(y-1400))/40) + float64(y)/50
			if x >= 1700 {
				v = 250
			}
			c := uint8(v)
			ycc.Y[ycc.YOffset(x, y)] = c
			ycc.Cb[ycc.COffset(x, y)] = uint8(x / 12)
			ycc.Cr[ycc.COffset(x, y)] = uint8(255 - y/12)
			rgba.SetRGBA(x, y, color.RGBA{c, uint8(x / 12), uint8(y / 12), 255})
			nrgba.SetNRGBA(x, y, color.NRGBA{c, uint8(x / 12), uint8(y / 12), 255})
			gray.SetGray(x, y, color.Gray{c})
		}
	}
	for _, src := range []image.Image{ycc, rgba, nrgba, gray} {
		for _, w := range []int{96, 256, 480} {
			ref := newCanvas(w, w)
			draw.CatmullRom.Scale(ref, ref.Bounds(), src, src.Bounds(), draw.Src, nil)
			runtime.GC()
			var m0, m1 runtime.MemStats
			runtime.ReadMemStats(&m0)
			got := scaleImage(src, w, w)
			runtime.ReadMemStats(&m1)
			var sum, worst float64
			for i := range ref.Pix {
				d := math.Abs(float64(ref.Pix[i]) - float64(got.Pix[i]))
				sum += d
				worst = math.Max(worst, d)
			}
			mean := sum / float64(len(ref.Pix))
			if mean > 1 {
				t.Errorf("%T at %d px: %.2f from Catmull-Rom alone on average (at most %.0f)", src, w, mean, worst)
			}
			if a := m1.TotalAlloc - m0.TotalAlloc; a > 12<<20 {
				t.Errorf("%T at %d px: %d MB allocated", src, w, a>>20)
			}
			// the edge at x = 1700: where the middle row crosses 210 (between
			// the rings' grey there and the white), within a quarter pixel
			// of Catmull-Rom alone
			cross := func(img *image.RGBA) float64 {
				y, e := w/2, 1700*w/W
				for x := e - 2; x <= e+3; x++ {
					a, b := float64(img.Pix[img.PixOffset(x-1, y)]), float64(img.Pix[img.PixOffset(x, y)])
					if a < 210 && b >= 210 {
						return float64(x-1) + (210-a)/(b-a)
					}
				}
				return -1
			}
			if r, g := cross(ref), cross(got); math.Abs(r-g) > 0.25 {
				t.Errorf("%T at %d px: the edge at %.2f, %.2f alone", src, w, g, r)
			}
		}
	}
	// small pictures and other types are scaled as they were
	small := image.NewYCbCr(image.Rect(0, 0, 500, 500), image.YCbCrSubsampleRatio420)
	if shrinkImage(small, 256, 256) != image.Image(small) {
		t.Error("a 500 px picture was shrunk for 256 px")
	}
	pal := image.NewPaletted(ycc.Rect, color.Palette{color.Black, color.White})
	if shrinkImage(pal, 96, 96) != image.Image(pal) {
		t.Error("a paletted picture was shrunk")
	}
}
