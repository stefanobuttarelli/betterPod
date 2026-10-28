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
// L / R change song, L2 / R2 the brightness (when Dim the Screen is on;
// shown in the bar). In Hold the keys do nothing (UP / DOWN still set
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
	ssNav  = 5 // columns the arrows reach (the 6th is cut)
)

// settings (prefs)
var (
	saverAfter = 0     // minutes without a key before it starts; 0: only by hand
	saverDim   = false // lower the brightness while it shows
	saverLevel = 0     // the brightness it lowers to (%); 0: a third of the current
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

	selImg *image.RGBA // the cursor's cover, grown
	selFor *Album

	cardSince time.Time // the card's titles scroll from here
	cardMarq  bool      // a title on the card is scrolling

	nextFlip  time.Time
	lastClock string
	dimmed    bool
	savedBri  int
	briShown  time.Time
	rng       *rand.Rand
}

// ---- starting and leaving ----

func (ui *IpodUI) startSaver() {
	if ui.saver != nil && !ui.saver.exiting {
		return
	}
	now := animNow()
	s := &screensaver{fade: newAnim(0), bigFlp: newAnim(1), bigOn: newAnim(0), fly: newAnim(1),
		selR: -1, selC: -1, selGrow: newAnim(0), cardOpen: newAnim(0), cardHome: newAnim(0),
		rng: rand.New(rand.NewSource(now.UnixNano())), nextFlip: now.Add(3 * time.Second)}
	s.big = ui.saverPlaying()
	if s.big != nil {
		s.bigOn.Set(1)
	}
	ui.ssWarm()
	for r := 0; r < ssRows; r++ {
		for c := 0; c < ssCols; c++ {
			s.tiles[r*ssCols+c] = &ssTile{flip: newAnim(1), in: newAnim(0)}
		}
	}
	for i := range s.tiles {
		s.tiles[i].al = s.pick(ui, i)
	}
	// from Now Playing: its cover will fly into the big square
	if s.big != nil && ui.nowPlayingOnScreen() && ui.coverUp() == nil {
		if np, ok := ui.topScreen().Custom.(*NowPlayingView); ok {
			cx, cy, side := 84.0, 20+np.H*0.35, 118.0
			s.flyFrom = image.Rect(px(cx-side/2), px(cy-side/2), px(cx+side/2), px(cy+side/2))
			s.fly.Set(0)
		}
	}
	// it shows when its covers are ready (asked for now), so the mosaic
	// appears whole
	s.readyBy = now.Add(1500 * time.Millisecond)
	s.covers(ui)
	ui.saver = s
	ui.sheets = nil
	if saverDim && !hold.on {
		s.savedBri = getBacklightPercent()
		lvl := saverLevel
		if lvl <= 0 {
			lvl = max(3, s.savedBri/3)
		}
		if s.savedBri > 0 && lvl < s.savedBri {
			s.dimmed = true
			setBacklightPercent(lvl)
			s.briShown = now.Add(1500 * time.Millisecond)
		}
	}
	logMsg("Screensaver on")
	ui.dirty = true
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
func (s *screensaver) begin(now time.Time) {
	s.ready = true
	for r := 0; r < ssRows; r++ {
		for c := 0; c < ssCols; c++ {
			s.tiles[r*ssCols+c].in.To(1, 380*time.Millisecond, easeCSS, time.Duration(r+c)*45*time.Millisecond)
		}
	}
	if s.fly.Get(now) < 1 {
		s.fly.To(1, 600*time.Millisecond, easeInOut, 60*time.Millisecond)
	}
	s.fade.To(1, 300*time.Millisecond, easeCSS, 0)
	s.nextFlip = now.Add(3 * time.Second)
}

func (ui *IpodUI) stopSaver() {
	s := ui.saver
	if s == nil || s.exiting {
		return
	}
	s.exiting = true
	s.fade.To(0, 250*time.Millisecond, easeCSS, 0)
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
		if s.exiting && !s.fade.Running() {
			ui.saver = nil
			ui.dirty = true
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

// saverPlaying: the album of the song playing or paused (nil: none).
func (ui *IpodUI) saverPlaying() *Album {
	p := ui.app.Playing
	if p == nil || p.Track == nil || p.State == StateStopped {
		return nil
	}
	return ui.trackAlbum(p.Track)
}

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
	for _, a := range all {
		if shown[a] {
			continue
		}
		if ssArtReady(a, ssCell) {
			ready = append(ready, a)
		} else {
			other = append(other, a)
		}
	}
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
			if t.al == al && !s.covered(i/ssCols, i%ssCols) {
				s.turn(ui, i, old, 150*time.Millisecond)
			}
		}
	}
	if al != nil {
		s.big = al
	} else {
		s.big = nil
	}
}

// ---- keys ----

// saverKey handles a key while it shows (true: taken).
func (ui *IpodUI) saverKey(ev int) bool {
	s := ui.saver
	if s == nil || s.exiting {
		return false
	}
	if ev < 0 {
		if k := Key(-ev - 1); ui.repeatKey == k {
			ui.repeatKey = NONE // released: stops repeating
		}
		return true // presses only
	}
	now := animNow()
	k := Key(ev)
	if !s.ready {
		ui.saver = nil // not shown yet: a key leaves it
		return true
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
		ui.repeatKey, ui.repeatStart, ui.repeatLast = Key(ev), now, now
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
		} else {
			ui.stopSaver()
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
		logMsg("Screensaver: L2 / R2")
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
		s.selR, s.selC = 0, 0
		if s.big == nil || s.bigOn.Target() == 0 {
			s.selR, s.selC = 0, 0
		}
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
		return // (Hold has its own, with UP / DOWN)
	}
	cur := getBacklightPercent()
	if !s.dimmed {
		s.savedBri, s.dimmed = cur, true
	}
	step := 5
	if cur < 10 || cur == 10 && !up {
		step = 1
	}
	if up {
		cur += step
	} else {
		cur -= step
	}
	cur = min(max(cur, 1), 100)
	setBacklightPercent(cur)
	saverLevel = cur
	s.briShown = animNow().Add(1500 * time.Millisecond)
	ui.savePrefs()
}

// ---- the cover turned over ----

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
	if s.fade.Running() || s.fly.Running() || s.bigFlp.Running() || s.bigOn.Running() || s.selGrow.Running() ||
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
			s.begin(now)
			ui.dirty = true
		}
		return
	}
	s.follow(ui)
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

// faceOf: during a flip the old album shows the first half, the new one
// the second (t 0 → 1, the turn 0 → 1 with edge-on at 0.5).
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
		hide := s.card != nil && s.cardR < 0
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
		var al *Album
		if t == nil {
			al = s.big
		} else if t.next == nil {
			al = t.al
		}
		if al != nil && g >= 1 {
			// grown: scaled properly once
			if s.selImg == nil || s.selFor != al || s.selImg.Bounds().Dx() != r.Dx() {
				src := ui.ssArtGet(al, 2*ssCell)
				if src == nil {
					src = ui.ssArtGet(al, ssCell)
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
	fillRect(dst, 0, ssBar, 640, 480-ssBar, Col{0, 0, 0, 0.6 * dim})
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
	if btConnected() {
		ic := whiteIcon(ui, "bluetooth")
		iw := float64(ic.Bounds().Dx()) / DPR
		blit(dst, ic, px(left-iw), px(3.5), 1)
		left -= iw + gap
	}
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
		lvl := getBacklightPercent()
		if hold.on {
			lvl = hold.cur
		}
		ssBrightness(dst, lvl)
		return
	}
	f := face(W700, 13)
	_, asc := lineBox(13)
	clock := time.Now().Format("15:04")
	drawText(dst, f, clock, 6*DPR, (1.5+asc)*DPR, colWhite)
	// next to it, the song playing: its title, and the artist in grey
	p := ui.app.Playing
	if p == nil || p.Track == nil || p.State == StateStopped {
		return
	}
	x := 6 + textWidth(f, clock)/DPR + 12
	room := left - 10 - x
	if room < 40 {
		return
	}
	title := p.Track.Title
	tw := textWidth(f, title) / DPR
	if tw > room {
		drawText(dst, f, ellipsize(f, title, room*DPR), x*DPR, (1.5+asc)*DPR, colWhite)
		return
	}
	drawText(dst, f, title, x*DPR, (1.5+asc)*DPR, colWhite)
	if p.Track.Artist == "" {
		return
	}
	af := face(W500, 13)
	art := "  " + p.Track.Artist
	if rest := room - tw; rest > 30 {
		drawText(dst, af, ellipsize(af, art, rest*DPR), (x+tw)*DPR, (1.5+asc)*DPR, Col{255, 255, 255, 0.55})
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

func (ui *IpodUI) saverSettingsView() *View {
	v := &View{ID: "screensaver", Title: "Screensaver", Kind: KindSplit}
	v.Build = func() []Item {
		var items []Item
		for _, m := range []int{0, 1, 2, 5, 10} {
			m := m
			l := "Never"
			if m > 0 {
				l = "After " + strconv.Itoa(m) + " min"
			}
			items = append(items, Item{Label: l, Sublabel: map[bool]string{true: "SELECT + Y still starts it"}[m == 0], Checked: saverAfter == m, NoArrow: true, Preview: PrevSettings, OnSelect: func() {
				saverAfter = m
				ui.savePrefs()
				ui.refreshBuilt()
			}})
		}
		items = append(items,
			Item{Label: "Dim the Screen", Value: onOff(saverDim), Sublabel: "L2 / R2 set the brightness while it shows", NoArrow: true, Preview: PrevSettings, OnSelect: func() {
				saverDim = !saverDim
				ui.savePrefs()
				ui.refreshBuilt()
			}},
			Item{Label: "Start Now", Sublabel: "Or SELECT + Y", NoArrow: true, Preview: PrevSettings, OnSelect: func() { ui.startSaver() }},
		)
		return items
	}
	v.Items = v.Build()
	for i, it := range v.Items {
		if it.Checked {
			v.Index = i
		}
	}
	return v
}
