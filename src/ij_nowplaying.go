package main

// ij_nowplaying.go — Now Playing: tilted cover with reflection, track info
// and the progress / volume / scrub rows.

import (
	"fmt"
	"image"
	"math"
	"time"

	"golang.org/x/image/font"
)

type NowPlayingView struct {
	scrubbing   bool
	volActive   bool
	volTimer    *time.Timer
	volEnabled  bool
	scrubTime   float64
	scrubActive bool // a seek is pending
	seekTimer   *time.Timer
	mainX       *Anim // progress row (0 shown, -1.1 out to the left)
	volX        *Anim // volume row (0 shown, 1.1 out to the right)
	scrubX      *Anim // scrub row
	optsX       *Anim // options row (Shuffle, Repeat: ij_npextra.go)
	opts        bool
	optSel      int
	optsAt      time.Time
	// the cover turned over to its album's songs (START), as in Cover Flow
	flip      *Anim // the cover's turn: 0 Now Playing's pose, 1 Cover Flow's back pose
	backA     *Anim // the card: 0 → 1 as it comes
	back      *View
	backAl    *Album
	backSince time.Time
	flipped   bool
	backBuf   *image.RGBA
	backList  *image.RGBA
	infoBuf   *image.RGBA
	stageBuf  *image.RGBA // the cover as last drawn (redrawn only when it changes)
	stageKey  npStageKey
	H         float64
	hideArt   bool  // Cover Flow's Now Playing: only the lines and the rows (the flow draws the cover)
	flyHide   bool  // the cover is flying to or from the screensaver: not drawn here (the rest is)
	untilt    bool  // held flat (the screensaver's cover flies from / lands on a flat one)
	tilt      *Anim // 1 playing (cover tilted), 0 paused (flat)
	tiltInit  bool
	lastFrame int
	// song change: the old song slides out while the new one comes in
	shownT *Track
	oldT   *Track
	change *Anim
	dir    float64
	// long lines scroll while playing (from since); marqOn while they do
	since    time.Time
	marqOn   bool
	lastDraw time.Time
	drawnAt  int // ui.frameNo of the last draw (on screen while it follows the frames)
	wasPause bool
	// the lossless badge's bars: 1 moving, 0 still (paused)
	barsLive *Anim
	barsOn   bool
}

func newNowPlayingView() *NowPlayingView {
	return &NowPlayingView{
		H:          220,
		volEnabled: true,
		tilt:       newAnim(1),
		mainX:      newAnim(0),
		volX:       newAnim(1.1),
		scrubX:     newAnim(1.1),
		optsX:      newAnim(1.1),
		flip:       newAnim(0),
		backA:      newAnim(0),
		change:     newAnim(1),
		dir:        1,
		barsLive:   newAnim(0),
	}
}

func (ui *IpodUI) showNowPlaying() {
	// only one Now Playing at a time
	for _, v := range ui.full {
		if v.ID == "nowPlaying" {
			return
		}
	}
	v := &View{ID: "nowPlaying", Title: "Now Playing", Kind: KindFull, Custom: newNowPlayingView()}
	ui.showView(v, "")
}

// Animating: a control row is sliding or the cover is tilting.
func (np *NowPlayingView) Animating(now time.Time) bool {
	if np.mainX.Running() || np.volX.Running() || np.scrubX.Running() || np.optsX.Running() || np.tilt.Running() || np.change.Running() {
		return true
	}
	if np.flip.Running() || np.backA.Running() {
		return true
	}
	if np.back != nil && np.flipped && (marqSyncNeedsFrame(now) || np.back.marqueeOn) {
		return true
	}
	// a scrolling line or the moving bars keep the view redrawing while it
	// is on screen
	// on screen: drawn in the last frame (frames stop while nothing moves,
	// so this holds through a marquee's pause)
	if ipodUI == nil || np.drawnAt < ipodUI.frameNo-1 {
		return false
	}
	return np.barsOn || (np.marqOn && marqSyncNeedsFrame(now))
}

// updateControls slides the control rows for the current mode.
func (np *NowPlayingView) updateControls() {
	// progress row unless the volume or scrub row is up
	if np.scrubbing || np.volActive || np.opts {
		np.mainX.To(-1.1, durSlide, easeCSS, 0)
	} else {
		np.mainX.To(0, durSlide, easeCSS, 0)
	}
	if np.scrubbing || np.opts || !np.volActive {
		np.volX.To(1.1, durSlide, easeCSS, 0)
	} else {
		np.volX.To(0, durSlide, easeCSS, 0)
	}
	if np.scrubbing {
		np.scrubX.To(0, durSlide, easeCSS, 0)
	} else if np.opts {
		np.scrubX.To(-1.1, durSlide, easeCSS, 0) // (out to the left, the options coming in)
	} else {
		np.scrubX.To(1.1, durSlide, easeCSS, 0)
	}
	if np.opts {
		np.optsX.To(0, durSlide, easeCSS, 0)
	} else {
		np.optsX.To(1.1, durSlide, easeCSS, 0)
	}
}

func (np *NowPlayingView) OnCenter(ui *IpodUI) {
	if np.flipped {
		np.cardPlay(ui)
		return
	}
	if np.opts {
		np.optPress(ui)
		return
	}
	if np.scrubbing {
		// after the scrubber: the options (Shuffle, Repeat)
		np.scrubbing, np.volActive = false, false
		np.opts, np.optsAt = true, animNow()
	} else {
		// centre: scrub mode (the wheel seeks)
		np.volEnabled = false
		np.volActive = false
		if np.volTimer != nil {
			np.volTimer.Stop()
		}
		np.scrubbing = true
		if p := ui.app.Playing; p != nil {
			np.scrubTime = p.Position
		}
	}
	np.updateControls()
}

// keyDir turns a d-pad key into a wheel step: down / right go forward, like
// turning the wheel clockwise. On Now Playing, where the wheel is the
// volume (or the scrubber), up / right raise it and down / left lower it.
func (ui *IpodUI) keyDir(k Key) int {
	fwd := k == DOWN || k == RIGHT
	if (k == UP || k == DOWN) && ui.nowPlayingTakesScroll() {
		fwd = k == UP
	}
	if fwd {
		return 1
	}
	return -1
}

// nowPlayingTakesScroll: the wheel goes to Now Playing (nothing above it,
// see scroll), or to Cover Flow's own Now Playing.
func (ui *IpodUI) nowPlayingTakesScroll() bool {
	if kb := ui.keyboard; kb != nil && !kb.exiting {
		return false
	}
	if cf := ui.coverUp(); cf != nil {
		return cf.playing
	}
	if len(ui.sheets) > 0 {
		return false
	}
	_, ok := ui.topScreen().Custom.(*NowPlayingView)
	return ok
}

func (np *NowPlayingView) OnScroll(ui *IpodUI, dir int, velocity float64) {
	if np.flipped {
		np.cardMove(ui, dir)
		return
	}
	if np.opts {
		np.optMove(ui, dir)
		return
	}
	p := ui.app.Playing
	if np.scrubbing {
		if p == nil || p.Track == nil {
			return
		}
		nt := np.scrubTime + float64(dir)
		if nt < 0 || p.Duration > 0 && nt > p.Duration {
			return
		}
		np.scrubTime = nt
		np.scrubActive = true
		if np.seekTimer != nil {
			np.seekTimer.Stop()
		}
		np.seekTimer = time.AfterFunc(300*time.Millisecond, func() {
			ui.post(func() {
				audioSeek(np.scrubTime)
				ui.app.Playing.Position = np.scrubTime
				np.scrubActive = false
				ui.dirty = true
			})
		})
		return
	}
	if !np.volEnabled {
		return
	}
	np.activateVolume(ui)
	if dir > 0 {
		if ui.volume < 1 {
			ui.volume = math.Min(ui.volume+0.04, 1)
		}
	} else {
		ui.volume = math.Max(ui.volume-0.04, 0.01)
	}
	ui.applyVolume()
	ui.savePrefs()
	np.updateControls()
}

func (ui *IpodUI) applyVolume() {
	if sysVol.ok {
		sysVol.set(int(ui.volume*100 + 0.5))
		return
	}
	audioSetVolume(int(ui.volume*100 + 0.5))
}

func formatPlaybackTime(sec float64) string {
	if sec < 0 {
		sec = 0
	}
	s := int(sec) % 3600
	return fmt.Sprintf("%02d:%02d", s/60, s%60)
}

// Render: the cover is drawn as Cover Flow draws its Now Playing (the same
// 3D cover, turning over to the same card: npStage), then the song's lines
// and the rows over it (renderBody), fading out while the card shows.
// Cover Flow's own Now Playing (hideArt) is only the lines and the rows.
func (np *NowPlayingView) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	if np.hideArt {
		np.renderBody(ui, v, dst, now)
		return
	}
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	np.trackChange(ui, now) // (before the cover: it cross-fades from this frame)
	ui.npStage(dst, np, now)
	// the song's info goes as the cover starts to turn, and comes back as
	// it lands: never with the card's text
	a := math.Max(0, 1-2*np.flip.Get(now))
	if a >= 0.999 {
		np.renderBody(ui, v, dst, now)
	} else if a > 0.002 {
		tmp := newCanvasCached(&np.infoBuf, W, H)
		clearRGBA(tmp)
		np.renderBody(ui, v, tmp, now)
		blit(dst, tmp, 0, 0, a)
	} else {
		// hidden under the card: nothing of it moves
		np.drawnAt = ui.frameNo
		np.barsOn, np.marqOn = false, false
	}
	np.flipFrame(now)
}

// trackChange: a new song: its lines slide in (and the cover cross-fades)
// when the view was on screen.
func (np *NowPlayingView) trackChange(ui *IpodUI, now time.Time) {
	p := ui.app.Playing
	if p == nil || p.Track == nil || p.Track == np.shownT {
		return
	}
	t := p.Track
	if np.shownT != nil && now.Sub(np.lastDraw) < 200*time.Millisecond {
		np.oldT, np.dir = np.shownT, skipDir
		np.change.Set(0)
		np.change.To(1, 420*time.Millisecond, easeLinear, 0) // changePhases eases each phase
	}
	np.shownT, np.since = t, now
}

// renderBody: the song's lines (title, artist, album) and the rows under
// them (progress, volume, scrubber, options).
func (np *NowPlayingView) renderBody(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	p := ui.app.Playing
	var t *Track
	if p != nil {
		t = p.Track
	}
	npMetaH := np.H * 0.7
	if t != nil {
		np.trackChange(ui, now)
		paused := p.State != StatePlaying
		if np.wasPause && !paused {
			np.since = now // scrolling starts again from the beginning
		}
		np.wasPause = paused
		k := np.change.Get(now)
		off := 22 * np.dir
		np.marqOn = false
		np.barsOn = false
		if k < 1 && np.oldT != nil {
			// the old lines leave, then the new ones come in
			out, in := changePhases(k)
			ui.drawNPInfo(dst, np, np.oldT, npMetaH, -off*out, 1-out, false, now)
			ui.drawNPInfo(dst, np, t, npMetaH, off*(1-in), in, false, now)
		} else {
			ui.drawNPInfo(dst, np, t, npMetaH, 0, 1, !paused, now)
		}
		np.lastDraw = now
		np.drawnAt = ui.frameNo
	}
	// control rows, sliding horizontally
	//
	//
	cy := npMetaH + (np.H*0.3-16)/2
	ctrlW := 304.0
	mx := np.mainX.Get(now) * ctrlW
	vx := np.volX.Get(now) * ctrlW
	sx := np.scrubX.Get(now) * ctrlW
	ox := np.optsX.Get(now) * ctrlW
	np.optsTick(now)
	// the options row (drawn after the rows below)
	if ox < ctrlW {
		defer ui.drawOptions(dst, np, ox+8, cy)
	}
	//
	//
	if vx < ctrlW {
		ui.drawVolumeBar(dst, vx+8, cy)
	}
	if mx > -ctrlW {
		ui.drawTrackProgress(dst, mx+8, cy, p)
	}
	if sx < ctrlW && sx > -ctrlW {
		cur := 0.0
		dur := 0.0
		if p != nil {
			cur, dur = p.Position, p.Duration
		}
		if np.scrubActive || np.scrubbing {
			if !np.scrubActive {
				np.scrubTime = cur
			}
			cur = np.scrubTime
		}
		ui.drawScrubber(dst, sx+8, cy, cur, dur, p != nil && p.Track != nil)
	}
}

func (ui *IpodUI) drawNPInfo(dst *image.RGBA, np *NowPlayingView, t *Track, npMetaH, dx, alpha float64, live bool, now time.Time) {
	// title, artist and album on one line each, vertically centred in the
	// top 70% of the view, starting at x = 168; lines too long to fit
	// scroll while the song plays (marquee), and stop at their start when
	// paused
	if alpha <= 0.002 {
		return
	}
	tf := face(W600, 14.72)
	sf := face(W600, 12)
	tlh, tasc := lineBox(14.72)
	slh, sasc := lineBox(12)
	const x0, maxW = 168.0, 144.0
	type line struct {
		s   string
		f   font.Face
		asc float64
		lh  float64
		c   Col
	}
	var lines []line
	if t.Title != "" {
		lines = append(lines, line{t.Title, tf, tasc, tlh, th.text})
	}
	if t.Artist != "" {
		lines = append(lines, line{t.Artist, sf, sasc, slh, th.npSub})
	}
	if t.Album != "" {
		lines = append(lines, line{t.Album, sf, sasc, slh, th.npSub})
	}
	h := 0.0
	for _, l := range lines {
		h += l.lh
	}
	af, hasFmt := ui.trackFormat(t)
	badge := hasFmt && af.lossless
	if badge {
		h += losslessBadgeGap + losslessBadgeHeight(af)
	}
	y := (npMetaH - h) / 2
	b := dst.Bounds()
	// title, artist and album scroll on one cycle: they start together and
	// the shorter ones wait for the longest before starting again
	cyc := 0.0
	for _, l := range lines {
		cyc = math.Max(cyc, marqueeCycle(l.f, l.s, maxW))
	}
	for _, l := range lines {
		c := Col{l.c.R, l.c.G, l.c.B, l.c.A * alpha}
		x := x0 + dx
		if live && textWidth(l.f, l.s)/DPR > maxW {
			drawMarqueeSync(dst, l.f, l.s, x, y+l.asc, maxW, c, np.since, now, cyc)
			np.marqOn = true
		} else {
			s := ellipsize(l.f, l.s, maxW*DPR)
			// clipped to the text column (it slides during a song change)
			r := image.Rect(px(x0-24), b.Min.Y, px(x0+maxW+24), b.Max.Y).Intersect(b)
			if sub, ok := dst.SubImage(r).(*image.RGBA); ok {
				drawText(sub, l.f, s, x*DPR, (y+l.asc)*DPR, c)
			}
		}
		y += l.lh
	}
	if badge {
		// the bars move while the song plays and settle when it pauses
		target := 0.0
		if p := ui.app.Playing; p != nil && p.State == StatePlaying {
			target = 1
		}
		if np.barsLive.Target() != target {
			np.barsLive.To(target, 450*time.Millisecond, easeInOut, 0)
		}
		ui.drawLosslessBadge(dst, af, x0+dx, y+losslessBadgeGap, maxW, alpha, np.barsLive.Get(now), now)
		np.barsOn = target == 1 || np.barsLive.Running()
	}
}

// changePhases splits a song change (k: 0 → 1) into the old song leaving
// (out: 0 → 1 over the first 45%) and the new one arriving (in: 0 → 1
// over the last 55%), each eased.
func changePhases(k float64) (out, in float64) {
	out = math.Min(1, k/0.45)
	in = math.Max(0, (k-0.45)/0.55)
	return easeInOut(out), easeInOut(in)
}

// drawProgressBar draws a glossy bar (iPod style) w CSS px wide, filled
// to percent, with the scrubber diamond instead of the fill if scrubber.
func (ui *IpodUI) drawProgressBar(dst *image.RGBA, x, y, w, percent float64, scrubber bool) {
	percent = math.Max(0, math.Min(100, percent))
	track := ui.themed("gloss_track")
	tw := px(w)
	if tw > track.Bounds().Dx() {
		tw = track.Bounds().Dx()
	}
	blit(dst, track.SubImage(image.Rect(0, 0, tw, track.Bounds().Dy())).(*image.RGBA), px(x), px(y), 1)
	fw := w * percent / 100
	if !scrubber {
		blue := ui.asset("gloss_blue")
		bw := px(fw)
		if bw > 0 {
			blit(dst, blue.SubImage(image.Rect(0, 0, bw, blue.Bounds().Dy())).(*image.RGBA), px(x), px(y), 1)
		}
	} else {
		d := ui.asset("scrubber")
		blit(dst, d, px(x+fw-8), px(y), 1)
	}

	reflectBar(dst, px(x-8), px(x+w+8), px(y), px(16))
}

// reflectBar mirrors the h rows above yTop below it, fading out (the
// reflection under the progress bar), from x0 to x1 (device px).
// The rows are read from dst itself, so the bar must be drawn first.
func reflectBar(dst *image.RGBA, x0, x1, yTop, h int) {
	b := dst.Bounds()
	if x0 < b.Min.X {
		x0 = b.Min.X
	}
	if x1 > b.Max.X {
		x1 = b.Max.X
	}
	fade := float64(h) * 0.75
	for i := 0; i < h; i++ {
		a := (1 - (float64(i)+0.5)/fade) * 0.35
		if a <= 0 {
			break
		}
		sy, dy := yTop+h-1-i, yTop+h+i
		if dy >= b.Max.Y || sy < b.Min.Y {
			continue
		}
		so, do := dst.PixOffset(x0, sy), dst.PixOffset(x0, dy)
		for k := 0; k < (x1-x0)*4; k += 4 {
			// mix premultiplied pixels; the colour can never exceed the
			// alpha (rounding would make it overflow when composited)
			al := uint8(float64(dst.Pix[do+k+3])*(1-a) + float64(dst.Pix[so+k+3])*a + 0.5)
			for c := 0; c < 3; c++ {
				v := uint8(float64(dst.Pix[do+k+c])*(1-a) + float64(dst.Pix[so+k+c])*a + 0.5)
				if v > al {
					v = al
				}
				dst.Pix[do+k+c] = v
			}
			dst.Pix[do+k+3] = al
		}
	}
}

func (ui *IpodUI) drawTimeLabel(dst *image.RGBA, s string, x, w, y float64, right bool) {
	f := face(W700, 12)
	_, asc := lineBox(12)
	tx := x
	if right {
		tx = x + w - textWidth(f, s)/DPR
	}
	drawText(dst, f, s, tx*DPR, (y+0.5+asc)*DPR, th.text)
}

func (ui *IpodUI) drawTrackProgress(dst *image.RGBA, x, y float64, p *NowPlaying) {
	has := p != nil && p.Track != nil
	pct, cur, rem := 0.0, "--:--", "--:--"
	if has {
		cur = formatPlaybackTime(p.Position)
		if p.Duration > 0 {
			rem = "-" + formatPlaybackTime(math.Max(0, p.Duration-p.Position))
		}
		if p.Duration > 0 {
			pct = p.Position / p.Duration * 100
		}
	}
	// elapsed | bar | remaining
	ui.drawTimeLabel(dst, cur, x+12, 40, y, false)
	ui.drawProgressBar(dst, x+12+40+8, y, 184, pct, false)
	ui.drawTimeLabel(dst, rem, x+304-12-40, 40, y, false)
}

func (ui *IpodUI) drawVolumeBar(dst *image.RGBA, x, y float64) {
	blit(dst, ui.themed("volume_mute_bar"), px(x+12), px(y), 1)
	ui.drawProgressBar(dst, x+12+30+8, y, 204, ui.volume*100, false)
	blit(dst, ui.themed("volume_full_bar"), px(x+304-12-30), px(y), 1)
}

func (ui *IpodUI) drawScrubber(dst *image.RGBA, x, y, cur, dur float64, has bool) {
	cs, rs := "--:--", "--:--"
	pct := 0.0
	if has {
		cs = formatPlaybackTime(cur)
		rs = "-" + formatPlaybackTime(dur-cur)
		if dur > 0 {
			pct = math.Round(cur / dur * 100)
		}
	}
	// elapsed | bar with the scrubber | remaining
	ui.drawTimeLabel(dst, cs, x+12, 30, y, false)
	ui.drawProgressBar(dst, x+12+30+8+8, y, 188, pct, true)
	ui.drawTimeLabel(dst, rs, x+304-12-40, 40, y, false)
}

// activateVolume shows the volume row; it hides 3 s after the last change.
func (np *NowPlayingView) activateVolume(ui *IpodUI) {
	np.volActive = true
	if np.volTimer != nil {
		np.volTimer.Stop()
	}
	np.volTimer = time.AfterFunc(3*time.Second, func() {
		ui.post(func() { np.volActive = false; np.updateControls(); ui.dirty = true })
	})
}

// flashVolume shows the volume row for a moment (volume keys).
func (np *NowPlayingView) flashVolume(ui *IpodUI) {
	if np.volEnabled && !np.scrubbing {
		np.activateVolume(ui)
	}
	np.updateControls()
}

// nowPlayingViews returns every Now Playing view (menus and Cover Flow).
func (ui *IpodUI) nowPlayingViews() []*NowPlayingView {
	var out []*NowPlayingView
	for _, st := range [][]*View{ui.split, ui.full} {
		for _, v := range st {
			if np, ok := v.Custom.(*NowPlayingView); ok {
				out = append(out, np)
			}
		}
	}
	if ui.cover != nil && ui.cover.np != nil {
		out = append(out, ui.cover.np)
	}
	return out
}

//go:noinline
func (ui *IpodUI) nowPlayingOnScreen() bool { // Now Playing is the visible screen
	if cf := ui.coverUp(); cf != nil && cf.np != nil && cf.npFade.Target() > 0 {
		return true
	}
	if len(ui.full) > 0 {
		if _, ok := ui.full[len(ui.full)-1].Custom.(*NowPlayingView); ok && ui.fullX.Target() == 0 {
			return true
		}
	}
	return false
}
