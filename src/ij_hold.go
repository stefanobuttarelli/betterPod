package main

// ij_hold.go — the Hold switch (SELECT toggles it), the Hold / volume
// overlays and the headphone / media keys.

import (
	"fmt"
	"image"
	"math"
	"os"
	"strconv"
	"time"

	"github.com/fogleman/gg"
)

// Hold: SELECT pressed and released on its own toggles it; while on, the
// screen dims, every key is ignored except UP/DOWN (brightness, when
// MIYOOPOD_HOLD_ADJUST isn't 0) and, after 5 s, Now Playing is shown.
// The Hold badge (padlock) shows in the header while it is on.
type holdState struct {
	on, selDown, selCombo bool
	savedLevel            int // brightness before Hold
	badge                 *Anim
	badgeUntil            time.Time
	level                 int // brightness chosen in Hold (saved in prefs)
	cur                   int
	levelShown            time.Time
	adjust                bool
	since                 time.Time
	autoNP                bool
	repKey                Key // UP/DOWN held in Hold (auto-repeat)
	repStart, repLast     time.Time
}

var hold = holdState{badge: newAnim(0), repKey: NONE, adjust: os.Getenv("MIYOOPOD_HOLD_ADJUST") != "0"}

var keyLog = os.Getenv("MIYOOPOD_KEYLOG") != ""

func holdDimPercent() int {
	if v, err := strconv.Atoi(os.Getenv("MIYOOPOD_HOLD_BRIGHTNESS")); err == nil && v >= 1 && v <= 100 {
		return v
	}
	return 0
}

func (ui *IpodUI) setHold(on bool) {
	if hold.on == on {
		return
	}
	hold.on = on
	ui.repeatKey = NONE
	if on {
		hold.savedLevel = getBacklightPercent()
		if b := ui.saverOnHold(); b > 0 {
			hold.savedLevel = b // the screensaver had lowered it
		}
		lvl := hold.level
		if lvl <= 0 {
			lvl = holdDimPercent()
		}
		if lvl <= 0 {
			lvl = hold.savedLevel / 5
			if lvl < 3 {
				lvl = 3
			}
			if hold.savedLevel > 0 && lvl > hold.savedLevel {
				lvl = hold.savedLevel
			}
		}
		hold.cur = lvl
		hold.since = animNow()
		hold.autoNP = false
		setBacklightPercent(lvl)
		logMsg(fmt.Sprintf("Hold on (brightness %d%% -> %d%%)", hold.savedLevel, lvl))
	} else {
		if hold.savedLevel > 0 {
			setBacklightPercent(hold.savedLevel)
		}
		logMsg("Hold off")
	}
	ui.showHoldBadge()
	ui.dirty = true
}

func (ui *IpodUI) showHoldBadge() {
	now := animNow()
	if hold.badge.Get(now) < 1 && hold.badge.Target() != 1 {
		hold.badge.To(1, 150*time.Millisecond, easeCSS, 0)
	}
	hold.badgeUntil = now.Add(1200 * time.Millisecond)
	ui.dirty = true
}

// holdFilter handles SELECT (the Hold switch) and, while Hold is on,
// swallows every other key. ev is a key press, or ^key for a release.
func (ui *IpodUI) holdFilter(ev int) bool {
	// SELECT on its own toggles Hold; SELECT+key combos don't
	switch ev {
	case int(SELECT):
		hold.selDown, hold.selCombo = true, false
		return true
	case -int(SELECT) - 1:
		if hold.selDown && !hold.selCombo {
			ui.setHold(!hold.on)
		}
		hold.selDown = false
		return true
	}
	if ev >= 0 && hold.selDown {
		hold.selCombo = true
	}
	if hold.on {
		if ev >= 0 {
			if hold.adjust && (Key(ev) == UP || Key(ev) == DOWN) {
				// the brightness shows in the header (no badge)
				ui.holdBrightness(Key(ev) == UP)
				now := animNow()
				hold.repKey, hold.repStart, hold.repLast = Key(ev), now, now
				ui.dirty = true
				return true
			}
			ui.showHoldBadge()
		} else if k := Key(-1 - ev); k == hold.repKey {
			hold.repKey = NONE
			ui.savePrefs()
		}
		return true
	}
	hold.repKey = NONE
	return false
}

// holdRepeat repeats UP/DOWN in Hold while held (after 300 ms, every
// 70 ms).
func (ui *IpodUI) holdRepeat(now time.Time) {
	if !hold.on || hold.repKey == NONE {
		return
	}
	if now.Sub(hold.repStart) < 300*time.Millisecond || now.Sub(hold.repLast) < 70*time.Millisecond {
		return
	}
	hold.repLast = now
	ui.holdBrightness(hold.repKey == UP)
	ui.dirty = true
}

// holdBrightness steps the Hold brightness up or down.
func (ui *IpodUI) holdBrightness(up bool) {
	// steps of 5%, 1% at the bottom of the range; the level is kept for
	// the next time Hold is turned on
	//
	if cur := getBacklightPercent(); cur > 0 {
		hold.cur = cur
	}
	step := 5
	if hold.cur < 10 || hold.cur == 10 && !up {
		step = 1
	}
	v := hold.cur
	if up {
		v += step
	} else {
		v -= step
	}
	if v < 1 {
		v = 1
	}
	if v > 100 {
		v = 100
	}
	hold.cur = v
	hold.level = v
	setBacklightPercent(v)
	logMsg(fmt.Sprintf("Hold brightness -> %d%%", v))
	hold.levelShown = animNow().Add(1500 * time.Millisecond)
	if hold.repKey == NONE {
		ui.savePrefs()
	}
}

// tickOverlays runs the Hold / volume overlay timers and reports whether
// an overlay is visible or animating.
// Called once per frame from the main loop.
func (ui *IpodUI) tickOverlays(now time.Time) bool {
	vis := false
	ui.holdRepeat(now)
	// Hold for 5 s: show Now Playing (once)
	if hold.on && !hold.autoNP && now.Sub(hold.since) > 5*time.Second {
		hold.autoNP = true
		ui.goNowPlaying()
	}
	if hold.badge.Target() == 1 && now.After(hold.badgeUntil) {
		hold.badge.To(0, 300*time.Millisecond, easeCSS, 0)
	}
	if hold.badge.Running() || hold.badge.Target() == 1 || hold.on && now.Before(hold.levelShown.Add(50*time.Millisecond)) {
		vis = true // (the brightness in the header, until it goes)
	}
	if volOSD.a.Target() == 1 && now.After(volOSD.until) {
		volOSD.a.To(0, 300*time.Millisecond, easeCSS, 0)
	}
	if volOSD.a.Running() || volOSD.a.Target() == 1 {
		vis = true
	}
	return vis
}

func (ui *IpodUI) holdAnimating(now time.Time) bool { return ui.tickOverlays(now) }

// volume overlay (shown when the volume changes)
var volOSD = struct {
	a     *Anim
	until time.Time
}{a: newAnim(0)}

// showVolumeOSD shows the volume overlay for 1.5 s.
func (ui *IpodUI) showVolumeOSD() {
	now := animNow()
	if volOSD.a.Target() != 1 {
		volOSD.a.To(1, 220*time.Millisecond, easeCSS, 0)
	}
	volOSD.until = now.Add(1500 * time.Millisecond)
	ui.dirty = true
}

// drawVolumeOSD: the volume shows in the header now (drawHeaderVolume).
func (ui *IpodUI) drawVolumeOSD(dst *image.RGBA, now time.Time) {}

// headphonesOut: the sound goes to headphones (Bluetooth, or wired ones in
// the jack), not the console's speaker.
func headphonesOut() bool {
	return btConnected() || headphoneJack() == 1 || os.Getenv("MIYOOPOD_HEADPHONES") == "1" // (the last for tests)
}

// volIcons: the header's little speaker and headphones, in the header's
// text colour (made once per colour).
var volIcons = map[string]*image.RGBA{}

func volIcon(headphones bool, c Col) *image.RGBA {
	key := fmt.Sprintf("%v%v", headphones, c)
	if img := volIcons[key]; img != nil {
		return img
	}
	S := px(14)
	img := image.NewRGBA(image.Rect(0, 0, S, S))
	p := pen{gg.NewContextForRGBA(img), float64(S) / 14}
	dc := p.dc
	dc.SetFillStyle(p.solid(c))
	dc.SetStrokeStyle(p.solid(c))
	dc.SetLineCap(gg.LineCapRound)
	if headphones {
		dc.SetLineWidth(p.u(1.6))
		dc.DrawArc(p.u(7), p.u(8), p.u(5.2), gg.Radians(180), gg.Radians(360))
		dc.Stroke()
		dc.DrawRoundedRectangle(p.u(1.1), p.u(7.4), p.u(3.2), p.u(5.4), p.u(1.2))
		dc.Fill()
		dc.DrawRoundedRectangle(p.u(9.7), p.u(7.4), p.u(3.2), p.u(5.4), p.u(1.2))
		dc.Fill()
	} else {
		// the speaker: box and cone, and two waves
		dc.MoveTo(p.u(1.2), p.u(5))
		dc.LineTo(p.u(3.6), p.u(5))
		dc.LineTo(p.u(7), p.u(2.2))
		dc.LineTo(p.u(7), p.u(11.8))
		dc.LineTo(p.u(3.6), p.u(9))
		dc.LineTo(p.u(1.2), p.u(9))
		dc.ClosePath()
		dc.Fill()
		dc.SetLineWidth(p.u(1.3))
		dc.DrawArc(p.u(7.5), p.u(7), p.u(2.6), gg.Radians(-50), gg.Radians(50))
		dc.Stroke()
		dc.DrawArc(p.u(7.5), p.u(7), p.u(5), gg.Radians(-50), gg.Radians(50))
		dc.Stroke()
	}
	volIcons[key] = img
	return img
}

// drawHeaderVolume draws the volume in a header (tx: the left edge of the
// title's room, maxW: its width, off: how far it is moved down, CSS px):
// the speaker (or headphones) and a bar as wide as the room allows, so it
// is shorter on the half-width menus and a little longer (at most 150 px)
// on full screens.
func (ui *IpodUI) drawHeaderVolume(sub *image.RGBA, tx, maxW, off float64) {
	ic := volIcon(headphonesOut(), th.headerText)
	blit(sub, ic, px(tx), px(3+off), 1)
	bx := tx + 14 + 7
	bw := math.Min(maxW-14-7-6, 150) // at most 150 px: long enough to read, not across a whole screen
	if bw < 30 {
		return
	}
	y := 7.5 + off
	fillRoundedGradient(sub, bx*DPR, y*DPR, (bx+bw)*DPR, (y+5)*DPR, 2.5*DPR,
		[]GradStop{{0, Col{0, 0, 0, 0.22}}, {1, Col{0, 0, 0, 0.12}}}, 1)
	if v := ui.volume; v > 0.001 {
		fw := math.Max(bw*v, 5)
		fillRoundedGradient(sub, bx*DPR, y*DPR, (bx+fw)*DPR, (y+5)*DPR, 2.5*DPR, selGrad, 1)
	}
}

// drawPadlock draws the padlock icon: s is the size of one unit in CSS
// px, the icon is 8x11 units.
func drawPadlock(dst *image.RGBA, x, y, s float64, body []GradStop, shackle Col, op float64) {
	d := s * DPR
	X, Y := x*DPR, y*DPR
	// shackle, then the body over its lower half
	strokeRounded(dst, X+1.2*d, Y, X+6.8*d, Y+8*d, 2.8*d, 1.4*d, shackle, op)
	fillRoundedGradient(dst, X, Y+4.6*d, X+8*d, Y+11*d, 1.2*d, body, op)
}

var padlockDarkGrad = []GradStop{{0, rgb(120, 126, 130)}, {1, rgb(40, 44, 48)}}
var padlockBadgeGrad = []GradStop{{0, rgb(255, 255, 255)}, {1, rgb(190, 196, 200)}}

// drawHoldBadge: the "Hold" / "Unlocked" badge (brightness bar while adjusting).
func (ui *IpodUI) drawHoldBadge(dst *image.RGBA, now time.Time) {
	a := hold.badge.Get(now)
	if a <= 0.001 {
		return
	}
	//
	x0 := 100.0
	// same box as the volume overlay
	fillRoundedGradient(dst, x0*DPR, 64*DPR, (x0+120)*DPR, 176*DPR, 14*DPR,
		[]GradStop{{0, Col{30, 30, 30, 0.82}}, {1, Col{0, 0, 0, 0.82}}}, a)
	drawPadlock(dst, x0+44, 80, 4, padlockBadgeGrad, rgb(225, 228, 230), a)
	label := "Hold"
	hint := "SELECT to unlock"
	if !hold.on {
		label, hint = "Unlocked", ""
	}
	f := face(W700, 16)
	tw := textWidth(f, label)
	drawText(dst, f, label, 320-tw/2, 284, Col{255, 255, 255, a})
	if hint != "" {
		f2 := face(W500, 11)
		tw2 := textWidth(f2, hint)
		drawText(dst, f2, hint, 320-tw2/2, 320, Col{200, 200, 200, a})
	}
}

// mediaKey handles headphone / media keys (they work in Hold too).
// It returns false for keys that are not media keys.
func (ui *IpodUI) mediaKey(ev int) bool {
	k := Key(ev)
	if ev < 0 {
		k = Key(-1 - ev)
	}
	if k < MEDIA_PLAYPAUSE ||
		k > MEDIA_VOLDN {
		// not a media key
		return false
	}
	if ev < 0 {
		return true
	}
	p := ui.app.Playing
	playing := p != nil && p.Track != nil && p.State == StatePlaying
	logMsg(fmt.Sprintf("Media key %d (playing=%v)", int(k), playing))
	switch k {
	case MEDIA_PLAYPAUSE:
		ui.app.togglePlayPause()
	case MEDIA_PLAY:
		if !playing {
			ui.app.togglePlayPause()
		}
	case MEDIA_PAUSE, MEDIA_STOP:
		if playing {
			ui.app.togglePlayPause()
		}
	case HP_UNPLUG:
		ui.pauseIfPlaying("Headphones unplugged")
	case MEDIA_HOOK:
		ui.hookClick()
	case MEDIA_VOLUP, MEDIA_VOLDN:
		step := 0.04
		if k == MEDIA_VOLDN {
			step = -step
		}
		ui.volume = math.Min(math.Max(ui.volume+step, 0.01), 1)
		ui.applyVolume()
		ui.savePrefs()
		ui.showVolumeOSD()
	case MEDIA_NEXT:
		skipDir = 1
		go func() { ui.app.nextTrack(); ui.post(ui.onTrackChanged) }()
	case MEDIA_PREV:
		skipDir = -1
		go func() { ui.app.prevTrack(); ui.post(ui.onTrackChanged) }()
	}
	ui.dirty = true
	return true
}

func (ui *IpodUI) pauseIfPlaying(why string) {
	if p := ui.app.Playing; p != nil && p.Track != nil && p.State == StatePlaying {
		ui.app.togglePlayPause()
		logMsg(why + ": paused")
		ui.dirty = true
	}
}

// goNowPlaying brings Now Playing to the front (Hold for 5 s), closing
// whatever is on top of it.
func (ui *IpodUI) goNowPlaying() {
	if p := ui.app.Playing; p == nil || p.Track == nil {
		return
	}
	if ui.nowPlayingOnScreen() {
		return
	}
	ui.sheets = nil
	if kb := ui.keyboard; kb != nil && !kb.exiting {
		ui.hideKeyboard()
	}
	if cf := ui.cover; cf != nil && !cf.exiting {
		ui.closeCoverFlow()
	}
	has := false
	for _, v := range ui.full {
		if v.ID == "nowPlaying" {
			has = true
		}
	}
	if has {
		for len(ui.full) > 0 && ui.full[len(ui.full)-1].ID != "nowPlaying" {
			ui.hideView()
		}
	} else {
		ui.showNowPlaying()
	}
	ui.dirty = true
}

// The button of wired earphones, as on the iPod's: one click plays or
// pauses, two go to the next song, three to the previous one. The clicks
// are counted until 450 ms pass without another.
var hook struct {
	clicks int
	gen    int
}

func (ui *IpodUI) hookClick() {
	hook.clicks++
	hook.gen++
	gen := hook.gen
	time.AfterFunc(450*time.Millisecond, func() {
		ui.post(func() {
			if gen != hook.gen {
				return // another click came
			}
			n := hook.clicks
			hook.clicks = 0
			logMsg(fmt.Sprintf("Headset button: %d %s", n, pluralize(n, "click", "clicks")))
			switch {
			case n == 1:
				ui.app.togglePlayPause()
			case n == 2:
				skipDir = 1
				go func() { ui.app.nextTrack(); ui.post(ui.onTrackChanged) }()
			default:
				skipDir = -1
				go func() { ui.app.prevTrack(); ui.post(ui.onTrackChanged) }()
			}
			ui.dirty = true
		})
	})
}
