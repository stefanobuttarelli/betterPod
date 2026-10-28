package main

// ij_ui.go — the ipod.js interface (github.com/tvillarete/ipod-classic-js)
// drawn natively: view stacks, panels, header, input and the main loop.
//
// Layout, as in ipod.js: a 320x240 CSS screen drawn at DPR 2 into the
// 640x480 framebuffer. The menu stack lives in the left half with a preview
// on the right ("split" views); full-screen views (Now Playing, Cover Flow,
// games) slide over everything. Everything is drawn on the main thread;
// animations are time based (ij_anim.go).

import (
	"bytes"
	"fmt"
	"image"
	"image/png"
	"math"
	"os"
	"runtime/pprof"
	"sync"
	"time"
	"unsafe"
)

type IpodUI struct {
	app *MiyooPod
	fb  *image.RGBA

	split    []*View // menu stack (left half)
	full     []*View // full-screen stack
	gone     []*View // views sliding out
	sheets   []*Sheet
	cover    *CoverFlow
	keyboard *Keyboard

	headerTitle string
	hdrActive   bool   // the header being drawn is the top screen's (the song ticker)
	prevFB      []byte // last presented frame (only changed rows are uploaded)
	noMarquee   bool
	frameNo     int

	leftX        *Anim // menu panel position (0 shown, -1 off to the left)
	fullX        *Anim // full-screen panel (0 shown, 1 off to the right)
	previewHide  *Anim
	preview      PreviewKind
	previewTimer *time.Timer
	pendingPrev  PreviewKind
	musicPrev    *Anim
	kb           *KenBurns

	assets   map[string]*image.RGBA
	assetsMu sync.Mutex
	artc     *artCache
	shadow   []float64 // box-shadow profile of the menu panel

	// wheel acceleration
	velocity float64
	lastTick time.Time
	lastDir  int

	repeatKey   Key
	repeatStart time.Time
	repeatLast  time.Time
	pressStart  map[Key]time.Time
	longFired   map[Key]bool

	volume      float64
	shuffleMode string // "off", "songs", "albums"

	dirty  bool
	events chan func()
}

// Timings (durSlide, durPanel…) and easings are in ij_anim.go, matching the
// ipod.js transitions. Rendering happens on the main thread (SDL needs it);
// other goroutines post work through ui.events / ui.post. A frame is drawn
// only when something is dirty or animating, and only the rows that changed
// are uploaded (refreshFramebufferRows).
//

// MIYOOPOD_PERF=1 logs frame timings once a second (render, present,
// late and unchanged frames).
var perfLog = os.Getenv("MIYOOPOD_PERF") != ""
var (
	perfFrames         int
	perfTotal, perfMax time.Duration
	perfStart          = time.Now()
)

func newIpodUI(app *MiyooPod) *IpodUI {
	ui := &IpodUI{
		app:         app,
		fb:          newCanvas(640, 480),
		leftX:       newAnim(0),
		fullX:       newAnim(1),
		previewHide: newAnim(0),
		musicPrev:   newAnim(0),
		preview:     PrevMusic,
		assets:      map[string]*image.RGBA{},
		artc:        newArtCache(),
		pressStart:  map[Key]time.Time{},
		repeatKey:   NONE,
		longFired:   map[Key]bool{},
		volume:      0.5,
		shuffleMode: "off",
		events:      make(chan func(), 64),
		dirty:       true,
	}
	ui.shadow = boxShadowProfile(48, 1)
	if q := app.Queue; q != nil && q.Shuffle {
		ui.shuffleMode = "songs"
	}
	ui.loadPrefs()
	ui.updateTheme()
	if v := initSystemVolume(); v >= 0 {
		// the console volume is the player volume; SDL_mixer stays at 100%
		// (the wheel and the console keys move the same value)
		ui.volume = math.Min(math.Max(v, 0), 1)
		audioSetVolume(100)
		logMsg(fmt.Sprintf("Using system volume (%d%%)", int(v*100+0.5)))
	} else {
		ui.applyVolume()
	}
	// no auto-lock: the screen stays on while browsing, like the iPod
	// (the screen can still be locked by hand with the MENU/function button)
	app.AutoLockMinutes = 0
	ui.kb = newKenBurns()
	// new artwork found in the background: the covers drawn without it
	// (the default cover) are dropped and loaded again
	onArtExtracted = func() {
		ui.post(func() {
			ui.artc.clear()
			ui.kb.initialized = false
			ui.onArtLoaded()
		})
	}
	startBluetoothWatch()
	if isKnulli {
		startVolumeKeyWatch()
	}
	home := ui.homeView()
	home.firstIn = true
	home.enter = newAnim(0)
	home.hide = newAnim(0)
	ui.split = []*View{home}
	ui.headerTitle = home.Title
	return ui
}

// post runs f on the UI goroutine (from callbacks and background work).
func (ui *IpodUI) post(f func()) {
	select {
	case ui.events <- f:
	default:
	}
}

func (ui *IpodUI) assetSafe(name string) *image.RGBA { return ui.asset(name) }

func (ui *IpodUI) asset(name string) *image.RGBA {
	ui.assetsMu.Lock()
	defer ui.assetsMu.Unlock()
	if a, ok := ui.assets[name]; ok {
		return a
	}
	f, err := os.Open("./assets/ipod/" + name + ".png")
	if err != nil {
		logMsg("WARNING: missing asset " + name)
		ui.assets[name] = newCanvas(1, 1)
		return ui.assets[name]
	}
	defer f.Close()
	img, err := png.Decode(f)
	if err != nil {
		ui.assets[name] = newCanvas(1, 1)
		return ui.assets[name]
	}
	rgbaImg := toRGBA(img)
	ui.assets[name] = rgbaImg
	return rgbaImg
}

func toRGBA(img image.Image) *image.RGBA {
	if r, ok := img.(*image.RGBA); ok && r.Bounds().Min == (image.Point{}) {
		return r
	}
	b := img.Bounds()
	out := newCanvas(b.Dx(), b.Dy())
	for y := 0; y < b.Dy(); y++ {
		for x := 0; x < b.Dx(); x++ {
			out.Set(x, y, img.At(b.Min.X+x, b.Min.Y+y))
		}
	}
	return out
}

// thumb returns album art at size CSS px.
func (ui *IpodUI) thumb(a *Album, size float64) *image.RGBA {
	return ui.artOrDefault(a, px(size))
}

func (ui *IpodUI) isPlayingTrack(t *Track) bool {
	p := ui.app.Playing
	return p != nil && p.Track == t && p.State == StatePlaying
}

// isCurrentTrack: t is the loaded track, playing or paused.
func (ui *IpodUI) isCurrentTrack(t *Track) bool {
	p := ui.app.Playing
	return p != nil && p.Track == t && p.State != StateStopped
}

// topScreen is the view on top (full-screen first): the one that gets the
// keys.
func (ui *IpodUI) topScreen() *View {
	if len(ui.full) > 0 {
		return ui.full[len(ui.full)-1]
	}
	return ui.split[len(ui.split)-1]
}

func (ui *IpodUI) showView(v *View, headerTitle string) {
	if v.Build != nil {
		v.Items = v.Build()
	}
	_ = animNow()

	v.hide = newAnim(0)
	v.dirty = true
	stack := &ui.split
	if v.Kind == KindFull {
		stack = &ui.full
	}
	if len(*stack) == 0 {
		v.firstIn = true
		v.enter = newAnim(0)
	} else {
		v.enter = newAnim(1)
		v.enter.To(0, durSlide, easeInOut, 0)
		prev := (*stack)[len(*stack)-1]
		prev.hide.To(-1, durSlide, easeCSS, 0)
	}
	*stack = append(*stack, v)
	ui.previewForSplit()
	if headerTitle != "" {
		ui.headerTitle = headerTitle
	} else {
		ui.headerTitle = v.Title
	}
	ui.updatePanels()
	ui.dirty = true
}

func (ui *IpodUI) hideView() {
	if kb := ui.keyboard; kb != nil && !kb.exiting {
		ui.hideKeyboard()
		return
	}
	if cf := ui.cover; cf != nil && !cf.exiting {
		cf.menu(ui)
		return
	}
	if len(ui.sheets) > 0 {
		ui.hideSheet()
		return
	}
	if len(ui.split)+len(ui.full) <= 1 {
		return
	}
	var v *View
	if n, m := len(ui.full), len(ui.split); n > 0 {
		v = ui.full[n-1]
		ui.full = ui.full[:n-1]
		if n-1 > 0 {
			ui.full[n-2].hide.To(0, durSlide, easeCSS, 0)
			ui.full[n-2].dirty = true
		}
	} else {
		// back in the menu stack: the previous menu slides back in
		v = ui.split[m-1]
		ui.split = ui.split[:m-1]
		ui.split[m-2].hide.To(0, durSlide, easeCSS, 0)
		ui.split[m-2].dirty = true
		if b := ui.split[m-2]; b.Build != nil {
			b.Items = b.Build()
		}
	}
	if v.firstIn {
		// the first full-screen view leaves with the panel
		v.enter.Set(0)
	} else {
		v.enter.To(1, durSlide, easeInOut, 0)
	}
	v.exiting = true
	ui.gone = append(ui.gone, v)
	top := ui.topScreen()
	if top.Custom == nil && top.Build != nil && top.Kind == KindFull {
		top.Items = top.Build()
	}
	ui.headerTitle = top.Title
	if t, ok := top.titleOverride(); ok {
		ui.headerTitle = t
	}
	ui.previewForSplit()
	ui.updatePanels()
	ui.dirty = true
}

// previewForSplit: the preview follows the selected row of the menu on
// top (when a menu opens or comes back).
func (ui *IpodUI) previewForSplit() {
	if len(ui.full) > 0 || len(ui.split) == 0 {
		return
	}
	v := ui.split[len(ui.split)-1]
	if v.Index >= 0 && v.Index < len(v.Items) {
		ui.schedulePreview(v.Items[v.Index].Preview)
	}
}

func (v *View) titleOverride() (string, bool) { return "", false }

// resetViews: MENU long press returns to the home screen.
func (ui *IpodUI) resetViews() {
	ui.hideKeyboard()
	if cf := ui.cover; cf != nil && !cf.exiting {
		ui.closeCoverFlow()
	}
	for len(ui.sheets) > 0 {
		ui.hideSheet()
	}
	for len(ui.split)+len(ui.full) > 1 {
		ui.hideView()
	}
}

func (ui *IpodUI) updatePanels() {
	hasFull := len(ui.full) > 0
	if hasFull {
		ui.leftX.To(-1, durPanel, easeCSS, delayMenuHide)
		ui.fullX.To(0, durPanel, easeCSS, 0)
	} else {
		ui.leftX.To(0, durPanel, easeCSS, 0)
		ui.fullX.To(1, durPanel, easeCSS, 0)
	}
}

// schedulePreview switches the right-hand preview after the selection has
// rested on an item for a moment (ipod.js uses a 750 ms debounce).
func (ui *IpodUI) schedulePreview(p PreviewKind) {
	if p == PrevNone {
		return
	}
	if ui.previewTimer != nil {
		ui.previewTimer.Stop()
	}
	ui.previewTimer = time.AfterFunc(750*time.Millisecond, func() {
		ui.post(func() { ui.setPreview(p) })
	})
}

func (ui *IpodUI) setPreview(p PreviewKind) {
	if p == ui.preview {
		return
	}
	if p == PrevMusic {
		ui.musicPrev.Set(1)
		ui.musicPrev.To(0, durPreview, easeInOut, 0)
	} else if ui.preview == PrevMusic {
		ui.musicPrev.To(1, durPreview, easeInOut, 0)
	}
	if p == PrevWeather {
		nextWxPreview() // a different weather each time
	}
	ui.preview = p
	ui.dirty = true
}

// wheelTick: one detent of the click wheel (here: a d-pad step), with the
// ipod.js acceleration.
func (ui *IpodUI) wheelTick(dir int) {
	now := animNow()
	dt := now.Sub(ui.lastTick)
	if dir != ui.lastDir || ui.lastTick.IsZero() || dt > 250*time.Millisecond {
		ui.velocity = 0
	} else {
		raw := 1000 / float64(dt.Milliseconds()+1)
		ui.velocity = (ui.velocity*15 + raw) / 16
	}
	ui.lastTick = now
	ui.lastDir = dir
	ui.scroll(dir, ui.velocity)
}

func skipFor(velocity float64) int {
	skip := 1
	for _, s := range [...]struct {
		v float64
		n int
	}{{0, 1}, {6, 3}, {10, 5}, {14, 8}} {
		if velocity >= s.v {
			skip = s.n
		}
	}
	return skip
}

func (ui *IpodUI) scroll(dir int, velocity float64) {
	if kb := ui.keyboard; kb != nil && !kb.exiting {
		kb.step(dir)
		ui.dirty = true
		return
	}
	if cf := ui.cover; cf != nil && !cf.exiting {
		cf.scroll(ui, dir, velocity)
		return
	}
	if len(ui.sheets) > 0 {
		ui.sheets[len(ui.sheets)-1].scroll(ui, dir, velocity)
		return
	}
	v := ui.topScreen()
	if v.Custom != nil {
		v.Custom.OnScroll(ui, dir, velocity)
		ui.dirty = true
		return
	}
	if len(v.Items) == 0 {
		return
	}
	skip := skipFor(velocity)
	ni := v.Index + dir*skip
	if ni < 0 {
		ni = 0
	}
	if ni > len(v.Items)-1 {
		ni = len(v.Items) - 1
	}
	if ni != v.Index {
		v.Index = ni
		v.dirty = true
		ui.dirty = true
		if v.Kind == KindSplit || v.Kind == KindFull {
			ui.schedulePreview(v.Items[ni].Preview)
		}
	}
}

func (ui *IpodUI) centerClick() {
	if kb := ui.keyboard; kb != nil && !kb.exiting {
		kb.press(ui)
		ui.dirty = true
		return
	}
	if cf := ui.cover; cf != nil && !cf.exiting {
		cf.center(ui)
		return
	}
	if len(ui.sheets) > 0 {
		ui.sheets[len(ui.sheets)-1].selectCurrent(ui)
		return
	}
	v := ui.topScreen()
	if v.Custom != nil {
		v.Custom.OnCenter(ui)
		ui.dirty = true
		return
	}
	if len(v.Items) == 0 {
		return
	}
	it := v.Items[v.Index]
	if it.isSong() {
		ui.playSong(it.Tracks, it.TrackIdx, it.ShowNowPlaying)
		return
	}
	if it.OnSelect != nil {
		it.OnSelect()
	}
}

// menuClick: MENU button. Views with their own menu handling (Solitaire) get it first.
func (ui *IpodUI) menuClick() {
	if kb := ui.keyboard; kb != nil && !kb.exiting {
		kb.backspace() // on the keyboard B deletes
		ui.dirty = true
		return
	}
	if ui.keyboard == nil && ui.cover == nil && len(ui.sheets) == 0 {
		if mh, ok := ui.topScreen().Custom.(interface{ OnMenu(*IpodUI) }); ok {
			mh.OnMenu(ui)
			ui.dirty = true
			return
		}
	}
	ui.hideView()
}

func (ui *IpodUI) centerLongPress() {
	if len(ui.sheets) > 0 {
		return
	}
	// Cover Flow: the album in front, or the song on its back
	if cf := ui.cover; cf != nil {
		if cf.exiting || cf.playing || len(cf.albums) == 0 {
			return
		}
		if cf.selected != nil && cf.back != nil && len(cf.back.Items) > 0 {
			it := cf.back.Items[cf.back.Index]
			ui.showSheetTitled(it.Label, ui.mediaOptions(it.Tracks[it.TrackIdx:it.TrackIdx+1]))
		} else if cf.selected == nil {
			al := cf.albums[cf.active]
			ui.showSheetTitled(al.Name, ui.mediaOptions(albumTracks(al)))
		}
		return
	}
	v := ui.topScreen()
	if lp, ok := v.Custom.(interface{ OnLongPress(*IpodUI) }); ok && ui.keyboard == nil {
		lp.OnLongPress(ui)
		ui.dirty = true
		return
	}
	if v.Custom != nil || len(v.Items) == 0 {
		return
	}
	it := v.Items[v.Index]
	if len(it.LongPress) > 0 {
		ui.showSheetTitled(it.Label, it.LongPress) // titled with what it's about
	}
}

// playSong starts a track list at idx (in the background: loading can take a moment).
func (ui *IpodUI) playSong(tracks []*Track, idx int, showNP bool) {
	app := ui.app
	cur := app.Playing.Track
	if cur != nil && tracks[idx] == cur && app.Playing.State != StateStopped {
		if showNP {
			ui.showNowPlaying()
		}
		return
	}
	skipDir = 1
	// a new list: the songs queued by hand are gone with the old one
	for t := range queuedMarks {
		delete(queuedMarks, t)
	}
	go func() {
		app.Queue.Shuffle = ui.shuffleMode != "off"
		app.playTrackFromList(tracks, idx)
		ui.post(func() {
			if ui.shuffleMode == "albums" {
				ui.applyShuffle()
			}
			ui.onTrackChanged()
			if showNP {
				ui.showNowPlaying()
			}
		})
	}()
}

func (ui *IpodUI) onTrackChanged() {
	// lists showing the speaker icon, the Now Playing item and the
	// previews follow the current track
	//
	ui.refreshBuilt()
}

// HandleKey: ev >= 0 is a key press, -(key+1) a release.
func (ui *IpodUI) HandleKey(ev int) {
	now := animNow()
	ui.app.LastActivityTime = now
	if ev < 0 {
		k := Key(-ev - 1)
		if ui.repeatKey == k {
			ui.repeatKey = NONE
		}
		_, held := ui.pressStart[k]
		delete(ui.pressStart, k)
		long := ui.longFired[k]
		delete(ui.longFired, k)
		if !held || long {
			return
		}
		// A and B act on release, so that holding them can mean something
		// else (long press: options sheet / back to home)
		if k == A {
			ui.centerClick()
		} else if k == B {
			ui.menuClick()
		}
		return
	}
	k := Key(ev)
	// the keyboard takes the d-pad and its own buttons (A and B act on
	// release, below)
	if ui.keyboardKey(k) {
		if k == UP || k == DOWN || k == LEFT || k == RIGHT {
			ui.repeatKey, ui.repeatStart, ui.repeatLast = k, now, now
		}
		return
	}
	switch k {
	case WHEEL_CW:
		ui.wheelTick(1)
	case WHEEL_CCW:
		ui.wheelTick(-1)
	case DOWN, RIGHT:
		ui.scroll(1, 0)
		ui.repeatKey, ui.repeatStart, ui.repeatLast = k, now, now
	case UP, LEFT:
		ui.scroll(-1, 0)
		ui.repeatKey, ui.repeatStart, ui.repeatLast = k, now, now
	case A, B:
		ui.pressStart[k] = now
	case START, X:
		ui.app.togglePlayPause()
		ui.dirty = true
	case L, R:
		if k == R {
			skipDir = 1
			go func() { ui.app.nextTrack(); ui.post(ui.onTrackChanged) }()
		} else {
			skipDir = -1
			go func() { ui.app.prevTrack(); ui.post(ui.onTrackChanged) }()
		}
	case Y:
		ui.centerLongPress()
	}
	ui.dirty = true
}

// checkRepeat repeats a held d-pad direction (after 300 ms, faster after 1.5 s).
// Only UP/DOWN/LEFT/RIGHT repeat; A and B act on release instead.
func (ui *IpodUI) checkRepeat(now time.Time) {
	if ui.repeatKey == NONE || now.Sub(ui.repeatStart) < 300*time.Millisecond {
		return
	}
	rate := 80 * time.Millisecond
	if now.Sub(ui.repeatStart) > 1500*time.Millisecond {
		rate = 40 * time.Millisecond
	}
	if now.Sub(ui.repeatLast) < rate {
		return
	}
	ui.repeatLast = now
	if ui.keyboardKey(ui.repeatKey) {
		return
	}
	if ui.repeatKey == DOWN || ui.repeatKey == RIGHT {
		ui.scroll(1, 0)
	} else {
		ui.scroll(-1, 0)
	}
	ui.dirty = true
}

// checkLongPress: A held = options sheet, B held = back to the home screen.
func (ui *IpodUI) checkLongPress(now time.Time) {
	for k, t := range ui.pressStart {
		if ui.longFired[k] || now.Sub(t) < 500*time.Millisecond {
			continue
		}
		ui.longFired[k] = true
		switch k {
		case A:
			if ui.keyboard == nil {
				ui.centerLongPress()
			}
		case B:
			if kb := ui.keyboard; kb != nil && !kb.exiting {
				ui.hideKeyboard() // held on the keyboard: close it
			} else if top := ui.topScreen(); !top.DisableLongPress || ui.cover != nil {
				ui.resetViews()
			}
		}
		ui.dirty = true
	}
}

// animating reports whether another frame must be drawn now.
// Idle screens are not redrawn at all: input or a timer marks them dirty.
func (ui *IpodUI) animating(now time.Time) bool {
	if ui.leftX.Running() || ui.fullX.Running() || ui.previewHide.Running() || ui.musicPrev.Running() {
		return true
	}
	if len(ui.gone) > 0 {
		return true
	}
	for _, st := range [][]*View{ui.split, ui.full} {
		for _, v := range st {
			if v.enter.Running() || v.hide.Running() {
				return true
			}
			if v.Custom != nil && v.Custom.Animating(now) {
				return true
			}
		}
	}
	for _, s := range ui.sheets {
		if s.anim.Running() {
			return true
		}
	}
	if ui.keyboard != nil && ui.keyboard.anim.Running() {
		return true
	}
	if ui.cover != nil && ui.cover.animating(now) {
		return true
	}
	if ui.holdAnimating(now) {
		return true
	}
	if hold.on {
		return false
	}
	if ui.kb.animating(ui) {
		return true
	}
	if ui.miniAnimating(now) || tickerAnimating(now) || toastAnimating() || ui.wxPreviewAnimating(now) {
		return true
	}
	return !ui.noMarquee && ui.listMarquee()
}

// listMarquee: the selected row of a visible list is scrolling its title.
func (ui *IpodUI) listMarquee() bool {
	for _, st := range [][]*View{ui.split, ui.full} {
		if len(st) > 0 && st[len(st)-1].marqueeOn && st[len(st)-1].Custom == nil {
			return true
		}
	}
	return false
}

func (ui *IpodUI) viewBuffer(v *View, wDev, hDev int, now time.Time) *image.RGBA {
	if v.cache == nil || v.cache.Bounds().Dx() != wDev || v.cache.Bounds().Dy() != hDev {
		v.cache = newCanvas(wDev, hDev)
		v.dirty = true
	}
	if v.Custom != nil {
		v.Custom.Render(ui, v, v.cache, now)
		return v.cache
	}
	// lists that keep the Now Playing block under them leave its height free
	res := ui.miniReserve(v)
	if res != v.miniRes {
		v.miniRes, v.dirty = res, true
	}
	if v.dirty || v.marqueeOn {
		if res {
			fillRect(v.cache, 0, 0, wDev, hDev, th.bg)
			r := image.Rect(0, 0, wDev, hDev-px(miniPlayerH))
			ui.renderList(v, v.cache.SubImage(r).(*image.RGBA))
		} else {
			ui.renderList(v, v.cache)
		}
		v.dirty = false
	}
	return v.cache
}

// drawStack draws a view stack (and the views sliding out of it) in a panel.
func (ui *IpodUI) drawStack(stack []*View, kind ViewKind, panelX, panelW int, now time.Time) {
	clip := image.Rect(panelX, px(20), panelX+panelW, 480).
		Intersect(ui.fb.Bounds())
	contentH := 480 - px(20)
	draw := func(v *View) {
		e := v.enter.Get(now)
		h := v.hide.Get(now)
		buf := ui.viewBuffer(v, panelW, contentH, now)
		x := panelX + int(float64(panelW)*e+0.5)
		// a view being covered moves left by a full panel width
		cx := x + int(float64(panelW)*h+0.5)

		fillRectClipped(ui.fb, x, px(20), panelW, contentH, th.bg, clip)

		if sub, ok := ui.fb.SubImage(image.Rect(x, px(20), x+panelW, 480).Intersect(clip)).(*image.RGBA); ok && !sub.Bounds().Empty() {
			blitOpaque(sub, buf, cx, px(20))
		}
	}
	for _, v := range stack {
		draw(v)
	}
	for _, v := range ui.gone {
		if v.Kind == kind {
			draw(v)
		}
	}
	// the Now Playing block stays at the bottom of the panel
	var top *View
	if len(stack) > 0 {
		top = stack[len(stack)-1]
	}
	ui.drawMiniPlayer(kind, top, panelX, panelW, now)
}

func fillRectClipped(dst *image.RGBA, x, y, w, h int, c Col, clip image.Rectangle) {
	r := image.Rect(x, y, x+w, y+h).Intersect(clip)
	if r.Empty() {
		return
	}
	fillRect(dst, r.Min.X, r.Min.Y, r.Dx(), r.Dy(), c)
}

func (ui *IpodUI) renderFrame(now time.Time) {
	ui.frameNo++
	marqueeActive = false
	mini.marqOn = false
	artSyncLeft = artSyncBudget
	fb := ui.fb
	fillRect(fb, 0, 0, 640, 480, th.bg)

	// Cover Flow is a separate full screen, fading in and out
	if cf := ui.cover; cf != nil {
		op := cf.fadeIn.Get(now)
		if cf.exiting && !cf.fadeIn.Running() {
			ui.cover = nil
		} else {
			l := newCanvasCached(&coverLayer, 640, 480)
			fillRect(l, 0, 0, 640, 480, th.bg)
			ui.hdrActive = false
			ui.drawHeader(l, 0, 640, now)
			ui.renderCoverFlow(l, now)
			if op >= 1 {
				blitOpaque(fb, l, 0, 0)
			} else {
				blit(fb, l, 0, 0, op)
			}
		}
	}

	// right half: preview
	//
	ph := ui.previewHide.Get(now)
	if ph < 1 {
		pv := newCanvasCached(&previewBuf, 320, 480)
		ui.renderPreview(pv, now)
		blit(fb, pv, 320+int(ph*320+0.5), 0, 1-ph)
	}
	lx := ui.leftX.Get(now)
	leftPx := int(lx*320 - 0.5)
	if lx > -1 {
		ui.hdrActive = len(ui.full) == 0
		ui.drawHeader(fb, leftPx, 320, now)
		ui.drawStack(ui.split, KindSplit, leftPx, 320, now)
		// panel shadow over the preview
		sa := lx + 1
		if sa > 0 {
			prof := ui.shadow
			if sa < 1 {
				prof = scaleProfile(ui.shadow, sa)
			}
			darkenColumns(fb, leftPx+320, 0, 480, prof)
		}
	}

	// full-screen views
	fx := ui.fullX.Get(now)
	if fx < 1 || len(ui.full) > 0 {
		fullPx := int(fx*640 + 0.5)
		if fullPx < 640 {
			fillRect(fb, fullPx, 0, 640-fullPx, 480, th.bg)
			ui.hdrActive = len(ui.full) > 0
			ui.drawHeader(fb, fullPx, 640, now)
			ui.drawStack(ui.full, KindFull, fullPx, 640, now)
		}
	}

	// option sheets
	ui.drawSheets(fb, now)

	// search keyboard
	ui.drawKeyboard(fb, now)
	ui.drawToast(fb, now)

	// hold badge and volume overlay
	ui.drawHoldBadge(fb, now)
	ui.drawVolumeOSD(fb, now)

	// views that finished sliding out
	keep := ui.gone[:0]
	for _, v := range ui.gone {
		if v.enter.Running() {
			keep = append(keep, v)
		}
	}
	ui.gone = keep
}

// newCanvasCached reuses *buf when it has the right size (layers that are
// redrawn every frame: Cover Flow, preview).
// The returned canvas is not cleared.
func newCanvasCached(buf **image.RGBA, w, h int) *image.RGBA {
	if *buf == nil || (*buf).Bounds().Dx() != w || (*buf).Bounds().Dy() != h {
		*buf = newCanvas(w, h)
	}
	return *buf
}

func scaleProfile(p []float64, s float64) []float64 { // the panel shadow, fading while it slides
	out := make([]float64, len(p))
	for i, v := range p {
		out[i] = v * s
	}
	return out
}

var coverLayer, previewBuf, headerBg *image.RGBA

// header gradient (light theme; the themes carry their own), top to bottom
// of the 20 px bar
var headerGrad = []GradStop{{0, rgb(254, 255, 255)}, {1, rgb(177, 182, 185)}}

func (ui *IpodUI) drawHeader(dst *image.RGBA, x, w int, now time.Time) {
	r := image.Rect(x, 0, x+w, px(20)).Intersect(dst.Bounds())
	if r.Empty() {
		return
	}
	sub := dst.SubImage(r).(*image.RGBA)

	if headerBg == nil || headerBg.Bounds().Dx() < w {
		// rendered once at full width: gradient with a 1px bottom border
		// (ipod.js: linear-gradient + border-bottom)
		headerBg = newCanvas(640, px(20))
		fillVGradient(headerBg, 0, 0, 640, px(20)-px(1), th.header)
		fillRect(headerBg, 0, px(20)-px(1), 640, px(1), th.headerBorder)
	}
	blitOpaque(sub, headerBg.SubImage(image.Rect(0, 0, w, px(20))).(*image.RGBA), x, 0)

	// right side, from the right edge leftwards
	right := float64(x)/DPR + float64(w)/DPR - 6
	batW := 26.0
	batX := right - batW
	ui.drawBattery(sub, batX, 3.5)
	iconsLeft := batX - 8
	if hold.on {
		drawPadlock(sub, iconsLeft-8, 3.5, 1, th.padlock, th.padShackle, 1)
		batX -= 16
		iconsLeft = batX - 8
	}
	if btConnected() {
		ic := ui.themed("bluetooth")
		iw := float64(ic.Bounds().Dx()) / DPR
		blit(sub, ic, px(iconsLeft-iw), px(3.5), 1)
		batX -= iw + 8
		iconsLeft = batX - 8
	}
	p := ui.app.Playing
	if p != nil && p.Track != nil && p.State != StateStopped {
		name := "play"
		if p.State == StatePaused {
			name = "pause"
		}
		ic := ui.asset(name)
		iw := float64(ic.Bounds().Dx()) / DPR
		ix := iconsLeft - iw
		blit(sub, ic, px(ix), px(3.5), 1)
		iconsLeft = ix - 8
	}

	if q := ui.app.Queue; q != nil {
		var icons []string
		switch q.Repeat {
		case RepeatOne:
			icons = append(icons, "hdr_repeat_one")
		case RepeatAll:
			icons = append(icons, "hdr_repeat")
		}
		if q.Shuffle {
			icons = append(icons, "hdr_shuffle")
		}
		for _, n := range icons {
			ic := ui.themed(n)
			iw := float64(ic.Bounds().Dx()) / DPR
			ix := iconsLeft + 8 - 6 - iw
			blit(sub, ic, px(ix), px(3), 1)
			iconsLeft = ix - 8
		}
	}
	// title
	f := face(W700, 13)
	_, asc := lineBox(13)
	tx := float64(x)/DPR + 6
	maxW := iconsLeft + 8 - tx

	if hold.on && (x != 0 || w >= 640 || ui.nowPlayingOnScreen()) {
		clock := time.Now().Format("15:04")
		cw := textWidth(f, clock) / DPR
		cx := float64(x)/DPR + 160 - cw/2
		if limit := cx - 8 - tx; limit < maxW {
			maxW = limit
		}
		drawText(sub, f, clock, cx*DPR, (1.5+asc)*DPR, th.headerText)
	}
	ui.drawHeaderTitle(sub, ui.headerTitle, tx, maxW, ui.hdrActive, now)
}

// drawBattery: ipod.js battery (shell, fill by level, bolt/plug when charging).
func (ui *IpodUI) drawBattery(dst *image.RGBA, x, y float64) {
	blit(dst, ui.themed("battery_shell"), px(x), px(y), 1)
	pct, charging := batteryInfo()
	if pct < 0 {
		pct = 100
	}
	if pct <= 15 {
		pct = 15
	}
	fill := ui.asset("battery_fill_ok")
	if pct <= 20 && !charging {
		fill = ui.asset("battery_fill_warn")
	}
	// the icon reads full down to 85%, then empties in proportion (80%
	// shows a small gap), so a nearly full battery looks full
	shown := math.Min(1, float64(pct)/85)
	w := int(float64(fill.Bounds().Dx())*shown + 0.5)
	sub := fill.SubImage(image.Rect(0, 0, w, fill.Bounds().Dy())).(*image.RGBA)
	blit(dst, sub, px(x+1), px(y+1), 1)

	if charging {
		icon := "battery_bolt"
		if pct >= 100 {
			icon = "battery_plug"
		}
		blit(dst, ui.asset(icon), px(x+1), px(y+1), 1)
	}
}

var (
	batMu      sync.Mutex
	batPct     = -1
	batChg     bool
	batChecked time.Time
)

// batteryInfo is read at most every 10 s.
func batteryInfo() (int, bool) {
	batMu.Lock()
	defer batMu.Unlock()
	if time.Since(batChecked) > 10*time.Second {
		batPct = getBatteryLevel()
		batChg = batteryCharging()
		batChecked = time.Now()
	}
	return batPct, batChg
}

func batteryCharging() bool {
	for _, p := range []string{"/sys/class/power_supply/axp2202-battery/status", "/sys/class/power_supply/battery/status"} {
		if b, err := os.ReadFile(p); err == nil {
			s := string(b)
			return len(s) >= 8 && s[:8] == "Charging"
		}
	}
	return false
}

// onlyKenBurns reports whether the Ken Burns preview is the only animation.
func (ui *IpodUI) onlyKenBurns(now time.Time) bool {
	saved := ui.kb
	ui.kb = &KenBurns{}
	ui.noMarquee = true
	other := ui.animating(now)
	ui.noMarquee = false
	ui.kb = saved
	return !other
}

// present uploads the frame. Only the rows that changed since the last
// frame are uploaded (most frames change a list row or the progress bar),
// and identical frames are skipped.
// It returns false when nothing was uploaded (the frame was identical to
// the previous one).
func (ui *IpodUI) present() bool {
	pix := ui.fb.Pix
	stride := ui.fb.Stride
	h := len(pix) / stride
	if len(ui.prevFB) != len(pix) {
		ui.prevFB = make([]byte, len(pix))
		copy(ui.prevFB, pix)
		refreshFramebuffer(unsafe.Pointer(&pix[0]))
		return true
	}
	y0, y1 := -1, -1
	for y := 0; y < h; y++ {
		o := y * stride
		if !bytes.Equal(pix[o:o+stride], ui.prevFB[o:o+stride]) {
			if y0 < 0 {
				y0 = y
			}
			y1 = y + 1
		}
	}
	if y0 < 0 {
		return false
	}
	copy(ui.prevFB[y0*stride:y1*stride], pix[y0*stride:y1*stride])
	refreshFramebufferRows(unsafe.Pointer(&pix[0]), y0, y1)
	return true
}

func (ui *IpodUI) Run() {
	app := ui.app
	if pf := os.Getenv("MIYOOPOD_PPROF"); pf != "" {
		if f, err := os.Create(pf); err == nil {
			pprof.StartCPUProfile(f)
			defer pprof.StopCPUProfile()
		}
	}
	vsync := displayVsync()
	var perfPresent time.Duration
	var perfLate, perfSkipped int
	var lastFrame time.Time
	lastMinute := -1
	lastState := PlayState(-1)
	logMsg(fmt.Sprintf("Display vsync: %v", vsync))
	lastSecond := -1
	lastBT := false
	var prefetched *Library
	for app.Running {
		// thumbnails of a newly loaded library are prefetched
		if lib := app.Library; lib != nil && lib != prefetched && !app.LibScanRunning && len(lib.Albums) > 0 {
			prefetched = lib
			go ui.prefetchArt(lib)
		}
		// input
		for {
			ev := C_GetKeyPress()
			if ev == -1 {
				break
			}
			if ev == int(LOCK) {
				app.handlePowerButtonPress()
				continue
			} else if ev == -(int(LOCK) + 1) {
				app.handlePowerButtonRelease()
				continue
			}
			if app.Locked {
				continue
			}
			if keyLog {
				logMsg(fmt.Sprintf("KEY %d", ev))
			}
			// SELECT + START quits (kept from MiyooPod); SELECT alone = Hold
			if Key(ev) == START && hold.selDown {
				app.Running = false
				break
			}
			if ui.mediaKey(ev) { // headphone buttons work in Hold too, like an iPod remote
				continue
			}
			if ui.holdFilter(ev) {
				continue
			}
			ui.HandleKey(ev)
		}
		// posted callbacks
		for done := false; !done; {
			select {
			case f := <-ui.events:
				f()
			default:
				done = true
			}
		}
		now := animNow()
		if bt := btConnected(); bt != lastBT {
			lastBT = bt
			ui.dirty = true
		}
		if btLostTake() {
			ui.pauseIfPlaying("Bluetooth headphones disconnected")
		}
		if sysVol.ok {
			if v := sysVol.takeChange(); v >= 0 {
				// volume changed on the console or the headphones: move the
				// bar and show it, like turning the wheel
				ui.volume = math.Min(math.Max(float64(v)/100, 0), 1)
				ui.dirty = true
				for _, npv := range ui.nowPlayingViews() {
					npv.flashVolume(ui)
				}
				if !ui.nowPlayingOnScreen() {
					ui.showVolumeOSD()
				}
			}
		}
		if volKeyTake() {
			// a volume key: the overlay (or Now Playing's bar) at once;
			// the level follows when the system has changed it
			ui.dirty = true
			for _, npv := range ui.nowPlayingViews() {
				npv.flashVolume(ui)
			}
			if !ui.nowPlayingOnScreen() {
				ui.showVolumeOSD()
			}
		}
		ui.autoCheckTick(now)
		ui.followPause(now)
		ui.followTicker(now)
		ui.tickOverlays(now)
		ui.checkLongPress(now)
		ui.checkRepeat(now)
		if app.TrackEndPending {
			app.TrackEndPending = false
			skipDir = 1
			app.handleTrackEnd()
			ui.onTrackChanged()
		}
		// play/pause changes (from the poller, headphones…) redraw at once
		if p := app.Playing; p != nil && p.State != lastState {
			lastState = p.State
			ui.dirty = true
		}
		// redraw every second while playing (progress, header icon)
		if p := app.Playing; p != nil && p.State == StatePlaying {
			if s := int(p.Position); s != lastSecond {
				lastSecond = s
				ui.dirty = true
			}
		}
		if m := time.Now().Minute(); m != lastMinute {
			lastMinute = m
			if hold.on {
				ui.dirty = true // the clock in the header
			}
			ui.updateTheme()
		}
		anim := ui.animating(now)
		presentBlocked := false
		if ui.dirty || anim {
			ui.dirty = false
			t0 := time.Now()
			if app.Locked {
				fillRect(ui.fb, 0, 0, 640, 480, colBlack)
			} else {
				ui.renderFrame(now)
			}
			t1 := time.Now()
			presented := ui.present()
			tp := time.Since(t1)
			// with vsync, a present that took a while waited for the display
			presentBlocked = presented && vsync && tp > 3*time.Millisecond
			if perfLog {
				perfFrames++
				perfTotal += t1.Sub(t0)
				perfPresent += tp
				if d := time.Since(t0); d > perfMax {
					perfMax = d
				}
				if anim && !lastFrame.IsZero() && t0.Sub(lastFrame) > 24*time.Millisecond && t0.Sub(lastFrame) < 200*time.Millisecond {
					perfLate++
				}
				lastFrame = t0
				if !presented {
					perfSkipped++
				}
				if time.Since(perfStart) > time.Second {
					logMsg(fmt.Sprintf("PERF: %d frames (%d unchanged), render %.1fms, present %.1fms avg, max %.1fms, late %d",
						perfFrames, perfSkipped, float64(perfTotal.Microseconds())/1000/float64(perfFrames),
						float64(perfPresent.Microseconds())/1000/float64(perfFrames), float64(perfMax.Microseconds())/1000, perfLate))
					perfFrames, perfTotal, perfMax, perfStart = 0, 0, 0, time.Now()
					perfPresent, perfSkipped, perfLate = 0, 0, 0
				}
			}
		}
		// frame pacing: ~60 fps while animating, 40 fps idle polling;
		// the Ken Burns preview alone runs at 20 fps (30 while fading or
		// scrolling a title)
		frame := 16 * time.Millisecond
		if !anim {
			frame = 25 * time.Millisecond
		} else if ui.onlyKenBurns(now) {
			frame = 50 * time.Millisecond // slow pan: fewer frames save battery
			if !ui.kb.transStart.IsZero() || ui.listMarquee() || marqueeActive {
				frame = 33 * time.Millisecond
			}
		}

		if presentBlocked && frame == 16*time.Millisecond {
			continue // vsync already paced this frame
		}
		if el := time.Since(now); el < frame {
			time.Sleep(frame - el)
		}
	}
}
