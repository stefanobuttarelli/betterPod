package main

// ij_screensaver.go — the screensaver: a mosaic of album covers, like the
// old Mac's album artwork screensaver.
//
//	┌──────────────────────────────────────┐ ← 40 px bar: ▶, the clock,
//	│ ▶            20:36              ▭ │   the battery, white on black
//	├─────────────┬──────┬──────┬──────┬───┤
//	│             │      │      │      │   │ ← 110 px covers, 4 rows;
//	│  playing    ├──────┼──────┼──────┼───┤   the 6th column runs off
//	│  (2×2)      │      │      │      │   │   the screen
//	├──────┬──────┼──────┼──────┼──────┼───┤
//	│      │      │      │      │      │   │
//	└──────┴──────┴──────┴──────┴──────┴───┘
//
// The album playing is the big cover; with nothing playing all are the
// same size. Every 5 s a few covers turn over and show another album.
// It starts after the time chosen in Settings → Screensaver, or with
// SELECT + Y; from Now Playing its cover flies into the big square.
//
// The arrows pick a cover (it grows a little, with a white border), A
// turns it over in the middle of the screen with its songs, like Cover
// Flow; choosing one plays it and the cover goes to the big square. B
// turns it back, or leaves the screensaver. X / START play and pause,
// L / R change song, L2 / R2 the brightness (the screensaver's own when
// Dim the Screen is on; shown in the bar). In Hold the keys do nothing (UP / DOWN still set
// Hold's brightness, shown in the bar).

import (
	"image"
	"math"
	"math/rand"
	"strconv"
	"time"
)

const (
	ssBar  = 40  // the bar's height (px)
	ssCell = 110 // a cover (px)
	ssRows = 4
	ssCols = 6 // the last runs off the screen
	ssNav  = 6 // columns the arrows reach (the 6th, cut by the edge, comes in whole when picked)
)

// settings (prefs)
var (
	saverStyle = "cover" // "cover": Cover Flip (the mosaic); "viz": the Visualizer (ij_visualizer.go)
	saverAfter = 0       // minutes without a key before it starts; 0: only by hand
	saverDim   = false   // lower the brightness while it shows
	saverLevel = 0       // the brightness it lowers to (%); 0: a third of the current
)

type ssTile struct {
	al, next *Album
	flip     *Anim // 0 → 1 while it turns over to next
	in       *Anim // its arrival when the screensaver starts
}

type screensaver struct {
	tiles  [ssRows * ssCols]*ssTile
	big    *Album // the album playing (nil: all the same size)
	bigOld *Album // turning over from it to big
	bigFlp *Anim
	bigOn  *Anim // 0: four small covers, 1: the big one

	fade    *Anim // the whole screensaver in and out
	exiting bool
	fly     *Anim // Now Playing's cover flying into the big square
	flyFrom image.Rectangle

	selR, selC int // the cursor (-1: none)
	selAt      time.Time
	selGrow    *Anim

	// the cover turned over with its songs
	card     *ssTile
	cardR    int // its cell (r*ssCols+c), -1 for the big one
	cardAl   *Album
	cardOpen *Anim // 0 at its cell, 1 turned over in the middle
	cardBack *View
	cardHome *Anim // after a song is chosen: flying to the big square
	cardPlay bool

	ready   bool      // the first covers are ready: it shows
	readyBy time.Time // (or at this time anyway)

	songIn  *Anim // the song in the bar: 1 playing, 0 paused (slid down out of it)
	songChg *Anim // a new song coming in (0 → 1)
	songOld *Track
	songNow *Track
	songDir int       // 1 next (from below), -1 back (from above)
	subAlb  *Anim     // the grey part after the title: 0 the artist, 1 the album
	subNext time.Time // when it changes over

	viz    *visualizer     // the Visualizer instead of the mosaic
	clockA *Anim           // the Visualizer's big clock (START)
	lowA   *Anim           // the Visualizer's song at the bottom (UP / DOWN)
	lowPin bool            // shown with UP / DOWN: stays until UP / DOWN
	lowOff time.Time       // shown by itself (a new song, playing again): goes then
	lowArm bool            // past the first frame (what is playing then is no news)
	npView *NowPlayingView // Now Playing under it while its cover flies (its own hidden)
	lib    *Library        // the library its albums are from
	hideAt time.Time       // when the flying cover takes over from Now Playing's (once it is flat)

	selImg *image.RGBA // the cursor's cover, grown
	selFor *Album

	cardSince time.Time // the card's titles scroll from here
	cardMarq  bool      // a title on the card is scrolling

	nextFlip   time.Time
	lastClock  string
	dimmed     bool
	levelDirty bool // its brightness changed: saved when L2 / R2 is let go
	savedBri   int
	flyTilt    bool // flying back to a playing Now Playing: turned on the way
	briShown   time.Time
	rng        *rand.Rand
}

// ---- starting and leaving ----

func (ui *IpodUI) startSaver() {
	if ui.saver != nil && !ui.saver.exiting {
		return
	}
	if old := ui.saver; old != nil && old.npView != nil {
		ui.npFlyHide(old.npView, false) // (one still leaving: Now Playing's cover back)
		old.npView.untilt = false
	}
	now := animNow()
	s := &screensaver{fade: newAnim(0), bigFlp: newAnim(1), bigOn: newAnim(0), fly: newAnim(1),
		selR: -1, selC: -1, selGrow: newAnim(0), songIn: newAnim(0), songChg: newAnim(1), subAlb: newAnim(0), clockA: newAnim(0), lowA: newAnim(0), cardOpen: newAnim(0), cardHome: newAnim(0),
		rng: rand.New(rand.NewSource(now.UnixNano())), nextFlip: now.Add(3 * time.Second)}
	s.lib = ui.app.Library
	s.big = ui.saverPlaying()
	if s.big != nil {
		s.bigOn.Set(1)
	}
	if ui.saverSongPlaying() {
		s.songIn.Set(1)
	}
	if p := ui.app.Playing; p != nil {
		s.songNow = p.Track
	}
	for r := 0; r < ssRows; r++ {
		for c := 0; c < ssCols; c++ {
			s.tiles[r*ssCols+c] = &ssTile{flip: newAnim(1), in: newAnim(1)}
		}
	}
	if saverStyle == "viz" && !isOnion { // (no Visualizer on the Miyoo, see onion.go)
		// the Visualizer: nothing to get ready
		s.viz = newVisualizer()
		s.big = nil
		s.ready = true
		s.fade.To(1, 400*time.Millisecond, easeCSS, 0)
		ui.saver = s
		ui.sheets = nil
		for k := range ui.pressStart {
			delete(ui.pressStart, k)
		}
		ui.saverDim(s, now)
		logMsg("Screensaver on (Visualizer)")
		ui.dirty = true
		return
	}
	ui.ssWarm()
	for i := range s.tiles {
		s.tiles[i].in.Set(0)
		s.tiles[i].al = s.pick(ui, i)
	}
	// from Now Playing: its cover will fly into the big square
	if s.big != nil && ui.nowPlayingOnScreen() && ui.coverUp() == nil {
		if np, ok := ui.topScreen().Custom.(*NowPlayingView); ok {
			np.closeCardNow() // (its cover, not its card, becomes the big one)
			s.flyFrom = npCoverRect()
			s.fly.Set(0)
			s.npView = np
		}
	}
	// it shows when its covers are ready (asked for now), so the mosaic
	// appears whole
	s.readyBy = now.Add(1500 * time.Millisecond)
	s.covers(ui)
	ui.saver = s
	ui.sheets = nil
	for k := range ui.pressStart {
		delete(ui.pressStart, k) // keys down now count no more (no long press under it)
	}
	ui.saverDim(s, now)
	logMsg("Screensaver on")
	ui.dirty = true
}

// saverDim: Dim the Screen: the brightness lowered while it shows.
func (ui *IpodUI) saverDim(s *screensaver, now time.Time) {
	if saverDim && !hold.on {
		s.savedBri = getBacklightPercent()
		lvl := saverLevel
		if lvl <= 0 {
			lvl = max(3, s.savedBri/3)
		}
		if s.savedBri > 0 && lvl < s.savedBri {
			s.dimmed = true
			setBacklightPercent(lvl)
			briOSD.level = lvl
			s.briShown = now.Add(1500 * time.Millisecond)
		}
	}
}

// covers asks for the covers on the screen and reports whether all are
// ready.
func (s *screensaver) covers(ui *IpodUI) bool {
	ok := true
	for i, t := range s.tiles {
		if t.al != nil && !s.covered(i/ssCols, i%ssCols) && ui.ssArtGet(t.al, ssCell) == nil {
			ok = false
		}
	}
	if s.big != nil && ui.ssArtGet(s.big, 2*ssCell) == nil {
		ok = false
	}
	return ok
}

// begin: the covers are ready: the wave from the top-left corner, Now
// Playing's cover flying in.
func (s *screensaver) begin(ui *IpodUI, now time.Time) {
	s.ready = true
	for r := 0; r < ssRows; r++ {
		for c := 0; c < ssCols; c++ {
			s.tiles[r*ssCols+c].in.To(1, 380*time.Millisecond, easeCSS, time.Duration(r+c)*45*time.Millisecond)
		}
	}
	if s.fly.Get(now) < 1 {
		// Now Playing's cover becomes the big one: turned (playing), it
		// first turns flat, then the flying one takes over from it and
		// flies there while the mosaic fades in around it
		var delay time.Duration
		if np := s.npView; np != nil {
			np.untilt = true
			if np.tilt.Get(now) > 0.01 {
				np.tilt.To(0, 280*time.Millisecond, easeInOut, 0)
				delay = 280 * time.Millisecond
			}
		}
		s.hideAt = now.Add(delay)
		s.fly.To(1, 650*time.Millisecond, easeInOut, delay)
		s.fade.To(1, 450*time.Millisecond, easeCSS, delay)
	} else {
		s.fade.To(1, 300*time.Millisecond, easeCSS, 0)
	}
	s.nextFlip = now.Add(3 * time.Second)
}

// flyTop: Now Playing's cover is flying in or out while the mosaic fades:
// it is drawn over everything at full strength (ssFlyDraw), not in the
// fading mosaic.
func (s *screensaver) flyTop(now time.Time) bool {
	return s.npView != nil && s.npView.flyHide && s.fly.Get(now) < 1 && (s.exiting || s.fade.Get(now) < 1)
}

// ssFlyDraw: the flying cover, over the screen; arriving at Now Playing it
// hands over to Now Playing's own cover (which fades in under it).
func (ui *IpodUI) ssFlyDraw(dst *image.RGBA, s *screensaver, now time.Time) {
	f := s.fly.Get(now)
	if s.exiting && s.flyTilt {
		// back to Now Playing with the song playing: the cover turns to Now
		// Playing's angle while it flies there, and lands already turned
		ui.ssFlyTilted(dst, s, f)
		return
	}
	// (Now Playing's cover is held flat, just where the flying one lands:
	// it takes over at the end, then turns if the song plays)
	r := lerpRect(s.flyFrom, cellRect(0, 0, 2), f)
	img := ui.ssArtGet(s.big, 2*ssCell)
	if img == nil {
		img = ui.ssArtGet(s.big, ssCell)
	}
	drawNN(dst, img, r, 0, 1)
}

// saverToNowPlaying: B with a song on: back to Now Playing, the big cover
// flying to its place there while the mosaic fades out.
func (ui *IpodUI) saverToNowPlaying() {
	s := ui.saver
	ui.goNowPlaying()
	ui.finishSlides()
	np, ok := ui.topScreen().Custom.(*NowPlayingView)
	if !ok {
		ui.stopSaver()
		return
	}
	np.closeCardNow()
	np.untilt = true
	np.tilt.Set(0)
	ui.npFlyHide(np, true)
	s.npView = np
	// playing: it lands turned, as Now Playing shows it (paused: flat)
	p := ui.app.Playing
	s.flyTilt = p != nil && p.Track != nil && p.State == StatePlaying
	s.flyFrom = npCoverRect()
	s.fly.Set(1)
	s.fly.To(0, 650*time.Millisecond, easeInOut, 0)
	ui.stopSaverIn(450 * time.Millisecond)
}

func (ui *IpodUI) stopSaver() { ui.stopSaverIn(250 * time.Millisecond) }

// stopSaverIn: it fades out over d (brightness back as it was).
func (ui *IpodUI) stopSaverIn(d time.Duration) {
	s := ui.saver
	if s == nil || s.exiting {
		return
	}
	s.exiting = true
	s.fade.To(0, d, easeCSS, 0)
	// still flying in from Now Playing: back to it
	if s.npView != nil && s.fly.Target() == 1 && s.fly.Get(animNow()) < 1 {
		s.fly.To(0, 400*time.Millisecond, easeInOut, 0)
		p := ui.app.Playing
		s.flyTilt = p != nil && p.Track != nil && p.State == StatePlaying
	}
	if s.dimmed && !hold.on {
		setBacklightPercent(s.savedBri)
	}
	s.dimmed = false
	ui.app.LastActivityTime = animNow()
	ui.dirty = true
}

// saverOnHold: Hold turned on while it shows: Hold takes the brightness
// (and gives back the one from before the screensaver).
func (ui *IpodUI) saverOnHold() int {
	if s := ui.saver; s != nil && s.dimmed {
		s.dimmed = false
		return s.savedBri
	}
	return 0
}

// saverTick (each frame): starts it after the time without a key.
func (ui *IpodUI) saverTick(now time.Time) {
	if s := ui.saver; s != nil {
		if s.lib != ui.app.Library && !s.exiting {
			ui.stopSaver() // a new library: its albums aren't these any more
		}
		if s.exiting && !s.fade.Running() && !s.fly.Running() {
			ui.saver = nil
			ui.dirty = true
		}
		if np := s.npView; np != nil {
			// the flying cover takes over from Now Playing's (once flat)
			if !s.exiting && !np.flyHide && !now.Before(s.hideAt) && s.fly.Get(now) < 1 {
				ui.npFlyHide(np, true)
			}
			// Now Playing's cover shows again once the flying one is there
			// (back on it: it turns again if the song plays)
			if !s.fly.Running() && (s.exiting || s.fly.Get(now) >= 1) {
				ui.npFlyHide(np, false)
				np.untilt = false
				if s.exiting && s.flyTilt {
					np.tilt.Set(1) // (it landed turned: no turn left to do)
				}
				s.npView = nil
				ui.dirty = true
			}
		}
		return
	}
	if saverAfter <= 0 || ui.app.Locked || ui.keyboard != nil {
		return
	}
	idle := time.Since(ui.app.LastActivityTime)
	if idle >= time.Duration(saverAfter)*time.Minute-20*time.Second {
		ui.ssWarm() // the covers get ready a little before (once per library)
	}
	if idle >= time.Duration(saverAfter)*time.Minute {
		ui.startSaver()
	}
}

// saverSongPlaying: a song is playing (not paused).
func (ui *IpodUI) saverSongPlaying() bool {
	p := ui.app.Playing
	return p != nil && p.Track != nil && p.State == StatePlaying
}

// npCoverRect: where Now Playing's cover is on the screen (px): held flat
// while a cover flies to or from it.
func npCoverRect() image.Rectangle {
	// flat (it is held flat while a cover flies to or from it): 203 px,
	// as measured on the screen
	side := 203.0
	cx, cy := 152.5, 226.5
	return image.Rect(int(cx-side/2), int(cy-side/2), int(cx+side/2), int(cy+side/2))
}

// npFlyHide hides (or shows again) Now Playing's own cover while the
// screensaver's flies to or from it.
func (ui *IpodUI) npFlyHide(np *NowPlayingView, hide bool) {
	if np == nil || np.flyHide == hide {
		return
	}
	np.flyHide = hide
	ui.dirty = true
}

// saverPlaying: the album of the song playing or paused (nil: none).
func (ui *IpodUI) saverPlaying() *Album {
	p := ui.app.Playing
	if p == nil || p.Track == nil || p.State == StateStopped {
		return nil
	}
	// (asked every frame: the album is looked up once per song)
	if p.Track != ssPlayT || ui.app.Library != ssPlayLib {
		ssPlayT, ssPlayLib, ssPlayAl = p.Track, ui.app.Library, ui.trackAlbum(p.Track)
	}
	return ssPlayAl
}

var (
	ssPlayT   *Track
	ssPlayLib *Library
	ssPlayAl  *Album
)

// ---- the covers ----

// pick: an album for cell i that is nowhere on the screen (no cover is
// ever shown twice), one whose cover is ready if there is one; nil when
// every album is showing (that cell stays dark).
func (s *screensaver) pick(ui *IpodUI, i int) *Album {
	all := ui.library().Albums
	shown := map[*Album]bool{s.big: true, s.bigOld: true, s.cardAl: true}
	for _, t := range s.tiles {
		if t != nil {
			shown[t.al], shown[t.next] = true, true
		}
	}
	var ready, other []*Album
	ssArt.Lock() // (once, not per album)
	for _, a := range all {
		if shown[a] {
			continue
		}
		if ssArt.img[artKey{a, ssCell}] != nil {
			ready = append(ready, a)
		} else {
			other = append(other, a)
		}
	}
	ssArt.Unlock()
	switch {
	case len(ready) > 0:
		return ready[s.rng.Intn(len(ready))]
	case len(other) > 0:
		return other[s.rng.Intn(len(other))]
	}
	return nil
}

// covered: cell (r, c) is under the big cover.
func (s *screensaver) covered(r, c int) bool { return s.big != nil && r < 2 && c < 2 }

// turn over a cover to another album
func (s *screensaver) turn(ui *IpodUI, i int, to *Album, delay time.Duration) {
	t := s.tiles[i]
	given := to != nil
	if !given {
		to = s.pick(ui, i)
	}
	if to == nil || to == t.al {
		return
	}
	if ui.ssArtGet(to, ssCell) == nil && !given {
		return // not ready yet: another time
	}
	t.next = to
	t.flip.Set(0)
	t.flip.To(1, 650*time.Millisecond, easeInOut, delay)
}

// follow: the song changed (or stopped): the big cover follows.
func (s *screensaver) follow(ui *IpodUI) {
	al := ui.saverPlaying()
	if al == s.big || s.cardHome.Running() {
		return
	}
	old := s.big
	switch {
	case al == nil:
		s.bigOn.To(0, 350*time.Millisecond, easeCSS, 0)
		s.bigOld = old // keeps showing while it fades
	case old == nil:
		s.big = al
		s.bigOld = nil
		s.bigFlp.Set(1)
		s.bigOn.To(1, 350*time.Millisecond, easeCSS, 0)
		for i, t := range s.tiles {
			if (t.al == al || t.next == al) && !s.covered(i/ssCols, i%ssCols) {
				t.al, t.next = nil, nil // (never twice)
				s.turn(ui, i, nil, 0)
			}
		}
	default:
		// the big cover turns over to the new album; a small one showing
		// it turns over to the old one
		s.big, s.bigOld = al, old
		s.bigFlp.Set(0)
		s.bigFlp.To(1, 700*time.Millisecond, easeInOut, 0)
		for i, t := range s.tiles {
			if (t.al == al || t.next == al) && !s.covered(i/ssCols, i%ssCols) {
				s.turn(ui, i, old, 150*time.Millisecond)
			}
		}
	}
	s.big = al
}

// ---- keys ----

// saverKey handles a key while it shows (true: taken).
func (ui *IpodUI) saverKey(ev int) bool {
	s := ui.saver
	if s == nil || s.exiting {
		return false
	}
	if len(ui.sheets) > 0 {
		return false // an options sheet over it takes the keys
	}
	if ev < 0 {
		k := Key(-ev - 1)
		if ui.repeatKey == k {
			ui.repeatKey = NONE // released: stops repeating
		}
		// (a key pressed before it started: not held any more, so it
		// doesn't count as a long press)
		delete(ui.pressStart, k)
		delete(ui.longFired, k)
		if (k == L2 || k == R2) && s.levelDirty {
			s.levelDirty = false
			ui.savePrefs()
		}
		return true // presses only
	}
	now := animNow()
	k := Key(ev)
	if !s.ready {
		// not shown yet: a key leaves it (the brightness back as it was)
		if s.dimmed && !hold.on {
			setBacklightPercent(s.savedBri)
		}
		ui.saver = nil
		return true
	}
	if s.viz != nil {
		return ui.vizKey(s, k, now)
	}
	if s.card != nil {
		return ui.saverCardKey(s, k, now)
	}
	switch k {
	case UP, DOWN, LEFT, RIGHT, WHEEL_CW, WHEEL_CCW:
		if k == WHEEL_CW {
			k = RIGHT
		} else if k == WHEEL_CCW {
			k = LEFT
		}
		s.moveSel(k, now)
		if Key(ev) != WHEEL_CW && Key(ev) != WHEEL_CCW { // (the wheel never releases: no repeat)
			ui.repeatKey, ui.repeatStart, ui.repeatLast = Key(ev), now, now
		}
	case A:
		if s.selR < 0 {
			s.moveSel(NONE, now)
		} else {
			ui.saverOpenCard(s, now)
		}
	case B, MENU:
		if s.selR >= 0 {
			s.selR, s.selC = -1, -1
			s.selGrow.Set(0)
		} else if ui.saverSongPlaying() && s.big != nil && ui.coverUp() == nil {
			// to Now Playing only while a song plays (paused: back to where
			// it was)
			ui.saverToNowPlaying()
		} else {
			ui.stopSaver()
		}
	case Y:
		// the picked album's options (Play Next…), as anywhere
		if s.selR < 0 {
			s.moveSel(NONE, now)
		} else if al := s.selAlbum(); al != nil {
			ui.showSheetTitled(al.Name, ui.albumOptions(al))
		}
	case X, START:
		ui.app.togglePlayPause()
	case L:
		skipDir = -1
		go func() { ui.app.prevTrack(); ui.post(ui.onTrackChanged) }()
	case R:
		skipDir = 1
		go func() { ui.app.nextTrack(); ui.post(ui.onTrackChanged) }()
	case L2, R2:
		ui.saverBrightness(k == R2)
		ui.repeatKey, ui.repeatStart, ui.repeatLast = k, now, now
	}
	ui.dirty = true
	return true
}

// saverRepeat: a held arrow (or L2 / R2) repeats (from checkRepeat).
func (ui *IpodUI) saverRepeat(k Key) {
	s := ui.saver
	now := animNow()
	switch {
	case s.viz != nil && k != L2 && k != R2:
		// (the Visualizer's keys don't repeat, but the brightness)
	case s.card != nil && (k == UP || k == DOWN):
		ui.saverCardKey(s, k, now)
	case s.card == nil && (k == UP || k == DOWN || k == LEFT || k == RIGHT):
		s.moveSel(k, now)
	case k == L2 || k == R2:
		ui.saverBrightness(k == R2)
	}
}

// moveSel moves the cursor a cover (NONE: shows it where it was, or on
// the first cover); the big cover counts as one.
func (s *screensaver) moveSel(k Key, now time.Time) {
	if s.selR < 0 {
		s.selR, s.selC = 0, 0 // (the first cover, or the big one)
	} else {
		dr, dc := 0, 0
		switch k {
		case UP:
			dr = -1
		case DOWN:
			dr = 1
		case LEFT:
			dc = -1
		case RIGHT:
			dc = 1
		}
		r, c := s.selR, s.selC
		// from the big cover: leave it from its edge
		if s.covered(r, c) {
			if dc > 0 {
				c = 1
			}
			if dr > 0 {
				r = 1
			}
		}
		r, c = r+dr, c+dc
		if s.covered(r, c) {
			r, c = 0, 0
		}
		if r < 0 || r >= ssRows || c < 0 || c >= ssNav {
			return
		}
		s.selR, s.selC = r, c
	}
	if s.covered(s.selR, s.selC) {
		s.selR, s.selC = 0, 0
	}
	s.selAt = now
	s.selGrow.Set(0)
	s.selGrow.To(1, 180*time.Millisecond, easeCSS, 0)
}

func (ui *IpodUI) saverBrightness(up bool) {
	s := ui.saver
	if hold.on {
		return // (Hold has its own)
	}
	if !saverDim && !s.dimmed {
		// as anywhere else: the console's brightness
		ui.stepBrightness(up)
		s.briShown = briOSD.until
		return
	}
	// Dim the Screen: the screensaver's own brightness, kept for next
	// time; the one from before comes back when it closes
	cur := getBacklightPercent()
	if !s.dimmed {
		s.savedBri, s.dimmed = cur, true
	}
	cur = briStep(cur, up)
	setBacklightPercent(cur)
	saverLevel = cur
	briOSD.level = cur
	s.briShown = animNow().Add(1500 * time.Millisecond)
	s.levelDirty = true // (saved when the key is let go)
}

// ---- the cover turned over ----

// selAlbum: the album of the picked cover.
func (s *screensaver) selAlbum() *Album {
	if s.selR < 0 {
		return nil
	}
	if s.covered(s.selR, s.selC) {
		return s.big
	}
	t := s.tiles[s.selR*ssCols+s.selC]
	if t.next != nil && t.flip.Running() {
		return t.next
	}
	return t.al
}

func (ui *IpodUI) saverOpenCard(s *screensaver, now time.Time) {
	var al *Album
	idx := -1
	if s.covered(s.selR, s.selC) {
		al = s.big
	} else {
		idx = s.selR*ssCols + s.selC
		t := s.tiles[idx]
		al = t.al
		if t.flip.Running() {
			al = t.next
		}
	}
	if al == nil {
		return
	}
	s.card, s.cardR, s.cardAl = &ssTile{}, idx, al
	ui.art(al, px(128)) // Now Playing's cover, ready if a song is chosen
	s.cardBack = ui.backsideList(al)
	// on the song playing, if it is this album's
	if p := ui.app.Playing; p != nil && p.Track != nil && ui.trackAlbum(p.Track) == al {
		for i, it := range s.cardBack.Items {
			if it.Tracks[it.TrackIdx] == p.Track {
				s.cardBack.Index = i
			}
		}
	}
	s.cardPlay = false
	s.cardSince = now.Add(600 * time.Millisecond)
	s.cardHome.Set(0)
	s.cardOpen.Set(0)
	s.cardOpen.To(1, 550*time.Millisecond, easeInOut, 0)
}

func (ui *IpodUI) saverCardKey(s *screensaver, k Key, now time.Time) bool {
	if s.cardHome.Target() == 1 {
		return true // flying to the big square
	}
	v := s.cardBack
	switch k {
	case UP, DOWN, WHEEL_CW, WHEEL_CCW:
		d := 1
		if k == UP || k == WHEEL_CCW {
			d = -1
		}
		if ni := v.Index + d; ni >= 0 && ni < len(v.Items) {
			v.Index = ni
			v.scrolledAt = now
			v.dirty = true
		}
		if k == UP || k == DOWN {
			ui.repeatKey, ui.repeatStart, ui.repeatLast = k, now, now
		}
	case A:
		if s.cardOpen.Target() != 1 || len(v.Items) == 0 {
			return true
		}
		it := v.Items[v.Index]
		ui.playSong(it.Tracks, it.TrackIdx, false)
		// back to a cover, flying to the big square
		s.cardPlay = true
		s.cardHome.Set(0)
		s.cardHome.To(1, 650*time.Millisecond, easeInOut, 0)
	case Y:
		// the song's options (Play Next…), as anywhere
		if s.cardOpen.Target() == 1 && len(v.Items) > 0 {
			it := v.Items[v.Index]
			ui.showSheetTitled(it.Label, ui.mediaOptions(it.Tracks[it.TrackIdx:it.TrackIdx+1]))
		}
	case B, MENU:
		s.cardOpen.To(0, 450*time.Millisecond, easeInOut, 0)
	case X, START:
		ui.app.togglePlayPause()
	}
	ui.dirty = true
	return true
}

// cardDone: the card is back (at its cell, or in the big square).
func (ui *IpodUI) saverCardDone(s *screensaver) {
	if s.cardPlay {
		old := s.big
		s.big, s.bigOld = s.cardAl, nil
		s.bigFlp.Set(1)
		s.bigOn.Set(1)
		// its cell shows the album that was big (or another)
		if s.cardR >= 0 && !s.covered(s.cardR/ssCols, s.cardR%ssCols) {
			t := s.tiles[s.cardR]
			if old != nil && old != s.big {
				t.al = old
			} else {
				t.al = s.pick(ui, s.cardR)
			}
			t.next = nil
			t.flip.Set(1)
		}
		s.selR, s.selC = 0, 0
		s.selAt = animNow()
	}
	s.card, s.cardBack = nil, nil
	ui.dirty = true
}

// ---- frames ----

func (s *screensaver) animating(now time.Time) bool {
	if s.viz != nil {
		return s.fade.Running() || s.songIn.Running() || s.songChg.Running() || s.subAlb.Running() ||
			s.clockA.Running() || s.lowA.Running() || !s.lowOff.IsZero() || now.Before(s.briShown) || s.viz.animating(now)
	}
	if s.fade.Running() || s.songIn.Running() || s.songChg.Running() || s.subAlb.Running() || s.fly.Running() || s.bigFlp.Running() || s.bigOn.Running() || s.selGrow.Running() ||
		s.cardOpen.Running() || s.cardHome.Running() || now.Before(s.briShown) ||
		s.card != nil && (s.cardMarq || s.cardBack != nil && s.cardBack.marqueeOn) {
		return true
	}
	for _, t := range s.tiles {
		if t.flip.Running() || t.in.Running() {
			return true
		}
	}
	return false
}

// saverFrame: the timers (flips, the cursor fading, the clock).
func (ui *IpodUI) saverFrame(now time.Time) {
	s := ui.saver
	if !s.ready {
		if s.covers(ui) || now.After(s.readyBy) {
			s.begin(ui, now)
			ui.dirty = true
		}
		return
	}
	if s.viz == nil {
		s.follow(ui)
	}
	// the song in the bar goes down out of it while paused, back up when it plays
	want := 0.0
	if ui.saverSongPlaying() {
		want = 1
	}
	if s.songIn.Target() != want {
		if want == 1 && s.lowArm {
			s.lowAuto(now) // playing again
		}
		s.songIn.To(want, 380*time.Millisecond, easeInOut, 0)
		ui.dirty = true
	}
	if p := ui.app.Playing; p != nil && p.Track != s.songNow {
		if p.Track != nil && s.lowArm {
			s.lowAuto(now) // a new song
		}
		if s.songNow != nil && p.Track != nil {
			s.songOld, s.songDir = s.songNow, 1
			if skipDir < 0 {
				s.songDir = -1
			}
			s.songChg.Set(0)
			s.songChg.To(1, 420*time.Millisecond, easeInOut, 0)
		}
		s.songNow = p.Track
		s.subAlb.Set(0) // a new song starts with its artist
		s.subNext = now.Add(6 * time.Second)
		ui.dirty = true
	}
	if s.viz != nil {
		s.lowArm = true
		if !s.lowOff.IsZero() && now.After(s.lowOff) {
			s.lowOff = time.Time{}
			s.lowA.To(0, 450*time.Millisecond, easeInOut, 0)
			ui.dirty = true
		}
	}
	// after the title, the artist (6 s) and the album (4 s) take turns
	if s.subNext.IsZero() {
		s.subNext = now.Add(6 * time.Second)
	} else if now.After(s.subNext) {
		if s.subAlb.Target() == 0 {
			s.subAlb.To(1, 450*time.Millisecond, easeInOut, 0)
			s.subNext = now.Add(4 * time.Second)
		} else {
			s.subAlb.To(0, 450*time.Millisecond, easeInOut, 0)
			s.subNext = now.Add(6 * time.Second)
		}
		ui.dirty = true
	}
	if s.viz != nil {
		return // (no covers)
	}
	if s.card != nil {
		if s.cardPlay && !s.cardHome.Running() && s.cardHome.Get(now) >= 1 {
			ui.saverCardDone(s)
		} else if !s.cardPlay && s.cardOpen.Target() == 0 && !s.cardOpen.Running() {
			ui.saverCardDone(s)
		}
	}
	for _, t := range s.tiles {
		if t.next != nil && !t.flip.Running() && t.flip.Get(now) >= 1 {
			t.al, t.next = t.next, nil
		}
	}
	if s.bigOld != nil && !s.bigFlp.Running() && !s.bigOn.Running() {
		s.bigOld = nil
	}
	// every 5 s a few covers turn over
	if now.After(s.nextFlip) {
		s.nextFlip = now.Add(5 * time.Second)
		n := 1 + s.rng.Intn(3)
		for j := 0; j < n; j++ {
			i := s.rng.Intn(len(s.tiles))
			r, c := i/ssCols, i%ssCols
			if s.covered(r, c) || (r == s.selR && c == s.selC) || s.tiles[i].next != nil || (s.card != nil && i == s.cardR) {
				continue
			}
			s.turn(ui, i, nil, time.Duration(j*250)*time.Millisecond)
		}
		ui.dirty = true
	}
	// the cursor goes after 6 s without a key
	if s.selR >= 0 && s.card == nil && now.Sub(s.selAt) > 6*time.Second {
		s.selR, s.selC = -1, -1
		ui.dirty = true
	}
	if c := time.Now().Format("15:04"); c != s.lastClock {
		s.lastClock = c
		ui.dirty = true
	}
}

func cellRect(r, c, span int) image.Rectangle {
	x, y := c*ssCell, ssBar+r*ssCell
	return image.Rect(x, y, x+span*ssCell, y+span*ssCell)
}

// ssCover draws al into r, turned by turn (0 flat, 0.5 edge-on: drawn
// narrower about its centre, darker as it turns). Covers at their own size
// are copied; growing, flying or turning ones are scaled on the fly.
func (ui *IpodUI) ssCover(dst *image.RGBA, al *Album, r image.Rectangle, turn, op float64) {
	if r.Dx() <= 0 || r.Dy() <= 0 || al == nil {
		return
	}
	k := math.Abs(math.Cos(turn * math.Pi))
	w := int(float64(r.Dx())*k + 0.5)
	if w < 1 {
		return
	}
	size := ssCell
	if r.Dy() > ssCell*5/4 {
		size = 2 * ssCell
	}
	img := ui.ssArtGet(al, size)
	if img == nil && size > ssCell {
		img = ui.ssArtGet(al, ssCell) // the small one, until the big is ready
	}
	if img == nil {
		return
	}
	x := r.Min.X + (r.Dx()-w)/2
	if w == img.Bounds().Dx() && r.Dy() == img.Bounds().Dy() {
		if op >= 1 {
			blitOpaque(dst, img, x, r.Min.Y)
		} else {
			blit(dst, img, x, r.Min.Y, op)
		}
		return
	}
	drawNN(dst, img, image.Rect(x, r.Min.Y, x+w, r.Max.Y), (1-k)*0.55, op)
}

// flipFace: during a flip the old album shows the first half (edge-on at
// t = 0.5), the new one the second.
func flipFace(from, to *Album, t float64) (*Album, float64) {
	if t < 0.5 {
		return from, t
	}
	return to, t
}

func lerpRect(a, b image.Rectangle, t float64) image.Rectangle {
	l := func(x, y int) int { return int(float64(x) + (float64(y)-float64(x))*t + 0.5) }
	return image.Rect(l(a.Min.X, b.Min.X), l(a.Min.Y, b.Min.Y), l(a.Max.X, b.Max.X), l(a.Max.Y, b.Max.Y))
}

func (ui *IpodUI) renderSaver(dst *image.RGBA, now time.Time) {
	s := ui.saver
	if s.viz != nil {
		ui.renderViz(dst, s, now)
		ui.vizOverlays(dst, s, now)
		// no bar over it: only for a moment, while the brightness or the
		// volume changes (shown there)
		if now.Before(s.briShown) || hold.on && now.Before(hold.levelShown) || volOSD.a.Get(now) > 0.001 {
			ui.ssBarDraw(dst, s, now)
		}
		return
	}
	fillRect(dst, 0, 0, 640, 480, colBlack)
	bigOn := s.bigOn.Get(now)
	bigAl := s.big
	if bigAl == nil {
		bigAl = s.bigOld
	}
	// the small covers (under the big one while it fades in or out)
	for i, t := range s.tiles {
		r, c := i/ssCols, i%ssCols
		if s.card != nil && i == s.cardR {
			continue // in the air
		}
		under := r < 2 && c < 2 && bigAl != nil
		if under && bigOn >= 1 {
			continue
		}
		if r == s.selR && c == s.selC && s.card == nil {
			continue // drawn last, bigger
		}
		ui.ssTileDraw(dst, t, cellRect(r, c, 1), now, 1)
	}
	// the big cover
	if bigAl != nil && bigOn > 0 {
		br := cellRect(0, 0, 2)
		hide := s.card != nil && s.cardR < 0 || s.flyTop(now)
		if f := s.fly.Get(now); f < 1 {
			br = lerpRect(s.flyFrom, br, f)
		}
		if !hide {
			if s.bigFlp.Get(now) < 1 && s.bigOld != nil {
				al, tt := flipFace(s.bigOld, s.big, s.bigFlp.Get(now))
				ui.ssCover(dst, al, br, tt, bigOn)
			} else {
				ui.ssCover(dst, bigAl, br, 0, bigOn)
			}
		}
	}
	// the cursor's cover: a little bigger, with a white border
	if s.selR >= 0 && s.card == nil {
		g := s.selGrow.Get(now)
		var r image.Rectangle
		var t *ssTile
		if s.covered(s.selR, s.selC) {
			r = cellRect(0, 0, 2)
		} else {
			r = cellRect(s.selR, s.selC, 1)
			t = s.tiles[s.selR*ssCols+s.selC]
		}
		grow := int(float64(r.Dx()) * 0.07 * g)
		r = r.Inset(-grow)
		if r.Min.Y < ssBar {
			r = r.Add(image.Pt(0, ssBar-r.Min.Y))
		}
		if r.Min.X < 0 {
			r = r.Add(image.Pt(-r.Min.X, 0))
		}
		// the last column runs off the screen: picked, it comes in whole
		if r.Max.X > 640 {
			r = r.Add(image.Pt(640-r.Max.X, 0))
		}
		if r.Max.Y > 480 {
			r = r.Add(image.Pt(0, 480-r.Max.Y))
		}
		var al *Album
		if t == nil {
			al = s.big
		} else if t.next == nil {
			al = t.al
		}
		if al != nil && g >= 1 {
			// grown: scaled properly once
			if s.selImg == nil || s.selFor != al || s.selImg.Bounds().Dx() != r.Dx() {
				src := ui.ssArtGet(al, ssCell)
				if t == nil { // (the big one: its own size)
					if b := ui.ssArtGet(al, 2*ssCell); b != nil {
						src = b
					}
				}
				if src != nil {
					s.selImg, s.selFor = scaleImage(src, r.Dx(), r.Dy()), al
				}
			}
			if s.selImg != nil && s.selFor == al {
				blitOpaque(dst, s.selImg, r.Min.X, r.Min.Y)
			}
		} else if t != nil {
			ui.ssTileDraw(dst, t, r, now, 1)
		} else {
			ui.ssCover(dst, s.big, r, 0, 1)
		}
		strokeRounded(dst, float64(r.Min.X)-1, float64(r.Min.Y)-1, float64(r.Max.X)+1, float64(r.Max.Y)+1, 2, 3, colWhite, g)
	}
	// the cover turned over
	if s.card != nil {
		ui.ssCardDraw(dst, s, now)
	}
	ui.ssBarDraw(dst, s, now)
}

func (ui *IpodUI) ssTileDraw(dst *image.RGBA, t *ssTile, r image.Rectangle, now time.Time, op float64) {
	in := t.in.Get(now)
	if in <= 0 {
		return
	}
	if in < 1 {
		sh := int(float64(r.Dx()) * 0.12 * (1 - in))
		r = r.Inset(sh)
		op *= in
	}
	if t.next != nil {
		al, tt := flipFace(t.al, t.next, t.flip.Get(now))
		ui.ssCover(dst, al, r, tt, op)
		return
	}
	ui.ssCover(dst, t.al, r, 0, op)
}

// the card: from its cell (or the big square) to the middle, turning over
// to the songs; after a song is chosen, back to a cover flying to the big
// square.
func (ui *IpodUI) ssCardDraw(dst *image.RGBA, s *screensaver, now time.Time) {
	o := s.cardOpen.Get(now)
	h := s.cardHome.Get(now)
	from := cellRect(0, 0, 2)
	if s.cardR >= 0 {
		from = cellRect(s.cardR/ssCols, s.cardR%ssCols, 1)
	}
	mid := ssCardRect // the card turned over
	// the screen darkens behind it
	dim := o
	if s.cardPlay {
		dim = 1 - h
	}
	dimRect(dst, 0, ssBar, 640, 480-ssBar, 0.6*dim)
	var r image.Rectangle
	var turn float64
	if s.cardPlay {
		// turning back to the cover while flying to the big square
		r = lerpRect(mid, cellRect(0, 0, 2), h)
		turn = 1 - h
	} else {
		r = lerpRect(from, mid, o)
		turn = o
	}
	if turn < 0.5 {
		// the cover side: square, as tall as the card is now
		side := r.Dy()
		if r.Dx() < side {
			side = r.Dx()
		}
		cx, cy := (r.Min.X+r.Max.X)/2, (r.Min.Y+r.Max.Y)/2
		sq := image.Rect(cx-side/2, cy-side/2, cx-side/2+side, cy-side/2+side)
		ui.ssCover(dst, s.cardAl, sq, turn, 1)
		return
	}
	k := math.Abs(math.Cos(turn * math.Pi))
	w := int(float64(r.Dx())*k + 0.5)
	if w < 2 {
		return
	}
	tex := ui.ssCardTexture(s, mid.Dx(), mid.Dy(), now)
	x := r.Min.X + (r.Dx()-w)/2
	if w == tex.Bounds().Dx() && r.Dy() == tex.Bounds().Dy() {
		blitOpaque(dst, tex, x, r.Min.Y)
		return
	}
	drawNN(dst, tex, image.Rect(x, r.Min.Y, x+w, r.Max.Y), (1-k)*0.5, 1)
}

var ssCardBuf *image.RGBA

// ssCardRect: the card turned over, as wide as Cover Flow's (256 px) and
// down past the bottom of the screen like it.
var ssCardRect = image.Rect(64, 82, 64+px(256), 482)

// ssCardTexture: the back of the card, like Cover Flow's: album and
// artist on blue (scrolling when long), then the songs.
func (ui *IpodUI) ssCardTexture(s *screensaver, W, H int, now time.Time) *image.RGBA {
	img := newCanvasCached(&ssCardBuf, W, H)
	fillRect(img, 0, 0, W, H, th.bg)
	al := s.cardAl
	tf := face(W700, 16)
	sf := face(W700, 14)
	tlh, tasc := lineBox(16)
	_, sasc := lineBox(14)
	w := float64(W) / DPR
	inner := w - 2
	infoH := backsideInfoH()
	fillVGradient(img, px(1), px(1), px(inner), px(infoH-1), []GradStop{{0, hex("#6585ad")}, {1, hex("#789ab3")}})
	fillRect(img, px(1), px(infoH), px(inner), px(1), hex("#6d87a3"))
	// album and artist scroll on one cycle
	cyc := math.Max(marqueeCycle(tf, al.Name, inner-16), marqueeCycle(sf, al.Artist, inner-16))
	s.cardMarq = cyc > 0
	y := 5.0
	drawMarqueeSync(img, tf, al.Name, 9, y+tasc, inner-16, colWhite, s.cardSince, now, cyc)
	y += tlh
	drawMarqueeSync(img, sf, al.Artist, 9, y+sasc, inner-16, colWhite, s.cardSince, now, cyc)
	lt := infoH + 1
	if lh := float64(H)/DPR - 1 - lt; lh > 0 {
		lst := newCanvasCached(&ssListBuf, px(inner), px(lh))
		ui.renderList(s.cardBack, lst)
		blitOpaque(img, lst, px(1), px(lt))
	}
	// 1px border
	fillRect(img, 0, 0, W, px(1), th.cardBorder)
	fillRect(img, 0, H-px(1), W, px(1), th.cardBorder)
	fillRect(img, 0, 0, px(1), H, th.cardBorder)
	fillRect(img, W-px(1), 0, px(1), H, th.cardBorder)
	return img
}

var ssListBuf *image.RGBA

// ---- the bar ----

// ssBarDraw: the header's own layout, white on black: the clock on the
// left (or the brightness while it changes), on the right the battery, the
// padlock in Hold and play / pause, where the header has them.
func (ui *IpodUI) ssBarDraw(dst *image.RGBA, s *screensaver, now time.Time) {
	fillRect(dst, 0, 0, 640, ssBar, colBlack)
	const gap = 4.0
	batX := 320.0 - 6 - 26
	// the battery: an outline, the tip, and the level inside
	shell := ui.asset("battery_shell").Bounds()
	bw, bh := float64(shell.Dx()), float64(shell.Dy())
	x0, y0 := batX*DPR, 3.5*DPR
	strokeRounded(dst, x0, y0, x0+bw-5, y0+bh, 4, 2, colWhite, 1)
	fillRect(dst, int(x0+bw-4), int(y0+bh/2-4), 3, 8, colWhite)
	pct, charging := batteryInfo()
	if pct < 0 {
		pct = 100
	}
	pct = max(pct, 8)
	inW := bw - 5 - 8
	fillRect(dst, int(x0)+4, int(y0)+4, int(inW*math.Min(1, float64(pct)/85)+0.5), int(bh)-8, colWhite)
	if charging {
		fillTriangle(dst, x0+bw/2+3, y0+3, x0+bw/2-6, y0+bh/2+1, x0+bw/2, y0+bh/2+1, colBlack)
		fillTriangle(dst, x0+bw/2-3, y0+bh-3, x0+bw/2+6, y0+bh/2-1, x0+bw/2, y0+bh/2-1, colBlack)
	}
	left := batX - gap
	if hold.on {
		drawPadlock(dst, left-8, 3.5, 1, []GradStop{{0, colWhite}, {1, colWhite}}, colWhite, 1)
		left -= 16
	}
	left = ui.drawStatusIcons(dst, left, 3.5, colWhite, true)
	if p := ui.app.Playing; p != nil && p.Track != nil && p.State != StateStopped {
		name := "play"
		if p.State == StatePaused {
			name = "pause"
		}
		ic := whiteIcon(ui, name)
		iw := float64(ic.Bounds().Dx()) / DPR
		blit(dst, ic, px(left-iw), px(3.5), 1)
		left -= iw + gap
	}
	// left: the clock, or the brightness for a moment after it changes
	if now.Before(s.briShown) || hold.on && now.Before(hold.levelShown) {
		lvl := briOSD.level // (set with the brightness: not read from the screen each frame)
		if lvl <= 0 {
			lvl = getBacklightPercent()
		}
		if hold.on {
			lvl = hold.cur
		}
		ssBrightness(dst, lvl)
		return
	}
	// (drawn inside the bar only: the volume slides in from below it)
	bar := dst.SubImage(image.Rect(0, 0, 640, ssBar)).(*image.RGBA)
	maxW := left - 10 - 6
	if va := volOSD.a.Get(now); va > 0.001 {
		// the volume, while it changes: the clock and the song slide up and
		// the volume comes in from below, as in the header
		up := -20 * easeInOut(va)
		ui.ssClockSong(bar, s, left, up)
		ssVolume(bar, ui.volume, 6, maxW, 20+up)
		return
	}
	ui.ssClockSong(bar, s, left, 0)
}

// ssClockSong: the clock and, next to it, the song playing (its title, and
// the artist in grey), off CSS px down, up to left (the icons).
func (ui *IpodUI) ssClockSong(dst *image.RGBA, s *screensaver, left, off float64) {
	f := face(W700, 13)
	_, asc := lineBox(13)
	base := (1.5 + asc + off) * DPR
	clock := time.Now().Format("15:04")
	drawText(dst, f, clock, 6*DPR, base, colWhite)
	p := ui.app.Playing
	now := animNow()
	in := s.songIn.Get(now)
	// (the Visualizer has the song at the bottom instead: only the clock)
	if s.viz != nil || p == nil || p.Track == nil || p.State == StateStopped || in <= 0.001 {
		return
	}
	// paused: down out of the bar (it is clipped to it)
	base += 20 * (1 - in) * DPR
	x := 6 + textWidth(f, clock)/DPR + 12
	room := left - 10 - x
	if room < 40 {
		return
	}
	// a new song: next comes in from below (the old one goes up), back
	// comes down from above
	if k := s.songChg.Get(now); k < 1 && s.songOld != nil {
		d := float64(s.songDir)
		ssSong(dst, s.songOld, x, base-20*d*k*DPR, room, 0)
		ssSong(dst, p.Track, x, base+20*d*(1-k)*DPR, room, 0)
		return
	}
	ssSong(dst, p.Track, x, base, room, s.subAlb.Get(now))
}

// ssSong: a song's title and, in grey after it, its artist or album (alb:
// 0 the artist, 1 the album, between: the album coming in from below as
// the artist goes up), from x (CSS px) on the baseline base (px), within
// room.
func ssSong(dst *image.RGBA, t *Track, x, base, room, alb float64) {
	f := face(W700, 13)
	tw := textWidth(f, t.Title) / DPR
	if tw > room {
		drawText(dst, f, ellipsize(f, t.Title, room*DPR), x*DPR, base, colWhite)
		return
	}
	drawText(dst, f, t.Title, x*DPR, base, colWhite)
	rest := room - tw
	if rest <= 30 {
		return
	}
	af := face(W500, 13)
	grey := Col{255, 255, 255, 0.55}
	sub := func(s string, off float64) {
		if s != "" {
			drawText(dst, af, ellipsize(af, "  "+s, rest*DPR), (x+tw)*DPR, base+off*DPR, grey)
		}
	}
	album := t.Album
	if album == "" || album == t.Artist {
		alb = 0 // nothing else to show
	}
	switch {
	case alb <= 0.001:
		sub(t.Artist, 0)
	case alb >= 0.999:
		sub(album, 0)
	default:
		sub(t.Artist, -20*alb)
		sub(album, 20*(1-alb))
	}
}

// ssVolume: the header's volume (speaker or headphones, and a bar), in
// white, off CSS px down.
func ssVolume(dst *image.RGBA, vol, tx, maxW, off float64) {
	blit(dst, volIcon(headphonesOut(), colWhite), px(tx), px(3+off), 1)
	bx := tx + 14 + 7
	bw := math.Min(maxW-14-7-6, 150)
	if bw < 30 {
		return
	}
	y := 7.5 + off
	fillRoundedGradient(dst, bx*DPR, y*DPR, (bx+bw)*DPR, (y+5)*DPR, 2.5*DPR,
		[]GradStop{{0, Col{255, 255, 255, 0.28}}, {1, Col{255, 255, 255, 0.28}}}, 1)
	if vol > 0.001 {
		fw := math.Max(bw*vol, 5)
		fillRoundedGradient(dst, bx*DPR, y*DPR, (bx+fw)*DPR, (y+5)*DPR, 2.5*DPR, []GradStop{{0, colWhite}, {1, colWhite}}, 1)
	}
}

// ssBrightness: a sun and a bar where the clock is (as in the header).
func ssBrightness(dst *image.RGBA, lvl int) {
	sx, sy := 13.0, 10.0 // (fillCircle is in CSS px)
	fillCircle(dst, sx, sy, 2.6, colWhite)
	for i := 0; i < 8; i++ {
		a := float64(i) * math.Pi / 4
		fillCircle(dst, sx+math.Cos(a)*4.6, sy+math.Sin(a)*4.6, 0.75, colWhite)
	}
	bx, bw, y := 27.0, 150.0, 7.5
	fillRoundedGradient(dst, bx*DPR, y*DPR, (bx+bw)*DPR, (y+5)*DPR, 2.5*DPR,
		[]GradStop{{0, Col{255, 255, 255, 0.28}}, {1, Col{255, 255, 255, 0.28}}}, 1)
	w := math.Max(5, bw*float64(lvl)/100)
	fillRoundedGradient(dst, bx*DPR, y*DPR, (bx+w)*DPR, (y+5)*DPR, 2.5*DPR, []GradStop{{0, colWhite}, {1, colWhite}}, 1)
}

// whiteIcon: a header icon in white (its shape, made once).
var whiteIcons = map[string]*image.RGBA{}

func whiteIcon(ui *IpodUI, name string) *image.RGBA {
	if w := whiteIcons[name]; w != nil {
		return w
	}
	src := ui.asset(name)
	w := image.NewRGBA(src.Bounds())
	for i := 0; i+3 < len(src.Pix); i += 4 {
		a := src.Pix[i+3]
		w.Pix[i], w.Pix[i+1], w.Pix[i+2], w.Pix[i+3] = a, a, a, a
	}
	whiteIcons[name] = w
	return w
}

// ---- Settings → Screensaver ----

func saverLabel() string {
	if saverAfter <= 0 {
		return "Never"
	}
	return strconv.Itoa(saverAfter) + " min"
}

// saverSettingsView: a page (Settings' rule: more than a few choices).
// What it does first, then when it starts.
func (ui *IpodUI) saverSettingsView() *View {
	v := &View{ID: "screensaver", Title: "Screensaver", Kind: KindFull}
	v.Build = func() []Item {
		style, sub := "Cover Flip", "Covers turning over"
		if saverStyle == "viz" {
			style, sub = "Visualizer", "Moves with the music · [◀] [▶] change it"
		}
		items := []Item{
			{Label: "Start Now", Sublabel: "Anywhere: [SELECT] + [Y]", NoArrow: true, OnSelect: func() { ui.startSaver() }},
			{Label: "Style", Value: style, Sublabel: sub, NoArrow: true, OnSelect: func() {
				if saverStyle == "viz" {
					saverStyle = "cover"
				} else {
					saverStyle = "viz"
				}
				ui.savePrefs()
				ui.refreshBuilt()
			}},
			{Label: "Dim the Screen", Value: onOff(saverDim), Sublabel: "[L2] / [R2] set it while it shows", NoArrow: true, OnSelect: func() {
				saverDim = !saverDim
				ui.savePrefs()
				ui.refreshBuilt()
			}},
		}
		for _, m := range []int{1, 2, 5, 10, 0} {
			l := "Start After " + strconv.Itoa(m) + " min"
			if m == 0 {
				l = "Never Start by Itself"
			}
			items = append(items, Item{Label: l, Checked: saverAfter == m, NoArrow: true, OnSelect: func() {
				saverAfter = m
				ui.savePrefs()
				ui.refreshBuilt()
			}})
		}
		return onionSaverSettings(items) // (no Visualizer on the Miyoo, see onion.go)
	}
	v.Items = v.Build()
	return v
}

// lowAuto: the Visualizer's song comes up by itself for 5 s (a new song,
// or playing again), unless it is up already for good (UP / DOWN).
func (s *screensaver) lowAuto(now time.Time) {
	if s.viz == nil || s.lowPin {
		return
	}
	if s.lowA.Target() != 1 {
		s.lowA.To(1, 450*time.Millisecond, easeInOut, 0)
	}
	s.lowOff = now.Add(5 * time.Second)
}

// ssFlyTilted: the flying cover drawn as Now Playing draws its own, at the
// angle of 1−f (f: 1 in the mosaic, 0 at Now Playing), and moved and
// scaled on the screen from the mosaic's big cell (f = 1) to its own place
// (f = 0), with its reflection coming in as it lands.
func (ui *IpodUI) ssFlyTilted(dst *image.RGBA, s *screensaver, f float64) {
	img := ui.ssArtGet(s.big, 2*ssCell)
	if img == nil {
		img = ui.ssArtGet(s.big, ssCell)
	}
	if img == nil {
		return
	}
	// where Now Playing's flat cover projects (CSS px of its stage), and
	// where that is on the screen (npCoverRect, measured)
	proj := func(m mat4) (x0, y0, x1, y1 float64) {
		H := planeH(m)
		x0, y0, x1, y1 = math.Inf(1), math.Inf(1), math.Inf(-1), math.Inf(-1)
		for _, c := range [4][2]float64{{0, 0}, {128, 0}, {0, 128}, {128, 128}} {
			w := H[6]*c[0] + H[7]*c[1] + H[8]
			x := (H[0]*c[0] + H[1]*c[1] + H[2]) / w
			y := (H[3]*c[0] + H[4]*c[1] + H[5]) / w
			x0, x1, y0, y1 = math.Min(x0, x), math.Max(x1, x), math.Min(y0, y), math.Max(y1, y)
		}
		return
	}
	fx0, fy0, fx1, _ := proj(coverMatrix(npCoverPose(0)))
	nr := npCoverRect()
	ox, oy := float64(nr.Min.X)/DPR-fx0, float64(nr.Min.Y)/DPR-fy0
	// the mosaic's big cell, as a scale and a shift of the flat cover
	cr := cellRect(0, 0, 2)
	k1 := float64(cr.Dx()) / DPR / (fx1 - fx0)
	tx1 := float64(cr.Min.X)/DPR - k1*(fx0+ox)
	ty1 := float64(cr.Min.Y)/DPR - k1*(fy0+oy)
	k := 1 + (k1-1)*f
	tx, ty := tx1*f, ty1*f
	scr := mat4{k, 0, 0, tx + k*ox, 0, k, 0, ty + k*oy, 0, 0, 1, 0, 0, 0, 0, 1}
	m := mMul(scr, coverMatrix(npCoverPose(1-f)))
	sc := float64(img.Bounds().Dx()) / 128
	tm := texMap{img: img, su: sc, sv: sc, u1: 128, v1: 128}
	if r := 0.2 * (1 - f); r > 0.001 {
		tm.v1 = 166.4
		tm.reflFrom, tm.reflMax, tm.reflStart = 128, r, 0.7
	}
	drawTex(dst, m, tm, 1)
}
