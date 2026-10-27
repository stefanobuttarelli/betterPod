package main

// ij_nowplaying.go — Now Playing: tilted cover with reflection, track info
// and the progress / volume / scrub rows.

import (
	"fmt"
	"image"
	"math"
	"time"
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
	H           float64
	hideArt     bool
	warpTrack   *Track
	warpArt     *image.RGBA
	warpReady   bool
	tilt        *Anim // 1 playing (cover tilted), 0 paused (flat)
	tiltInit    bool
	warpTilt    float64
	warpSrc     *image.RGBA
	warpSrcT    *Track
	lastFrame   int
}

func newNowPlayingView() *NowPlayingView {
	return &NowPlayingView{
		H:          220,
		volEnabled: true,
		tilt:       newAnim(1),
		mainX:      newAnim(0),
		volX:       newAnim(1.1),
		scrubX:     newAnim(1.1),
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
func (np *NowPlayingView) Animating(now time.Time) bool { return np.mainX.Running() || np.volX.Running() || np.scrubX.Running() || np.tilt.Running() }

// updateControls slides the control rows for the current mode.
func (np *NowPlayingView) updateControls() {
	// progress row unless the volume or scrub row is up
	if np.scrubbing || np.volActive {
		np.mainX.To(-1.1, durSlide, easeCSS, 0)
	} else {
		np.mainX.To(0, durSlide, easeCSS, 0)
	}
	if np.scrubbing || !np.volActive {
		np.volX.To(1.1, durSlide, easeCSS, 0)
	} else {
		np.volX.To(0, durSlide, easeCSS, 0)
	}
	if np.scrubbing {
		np.scrubX.To(0, durSlide, easeCSS, 0)
	} else {
		np.scrubX.To(1.1, durSlide, easeCSS, 0)
	}
}

func (np *NowPlayingView) OnCenter(ui *IpodUI) {
	if np.scrubbing {
		np.volEnabled = true
		np.scrubbing, np.volActive = false, false
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

func (np *NowPlayingView) OnScroll(ui *IpodUI, dir int, velocity float64) {
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

// buildWarp renders the cover (with its 1px border) in perspective,
// rotated tilt×35° about its vertical axis like the iPod's Now Playing,
// with a fading reflection below; the result is kept in np.warpArt.
//
// The card is 128 CSS px wide, its centre 84 px from the left edge; the
// view is a 1000 px perspective from x = 160 at the height of the text
// block's centre. Each output pixel is mapped back onto the card plane and
// sampled bilinearly.
//
func (np *NowPlayingView) buildWarp(ui *IpodUI, t *Track, tilt float64) bool {
	if np.warpSrcT != t || np.warpSrc == nil {
		album := ui.trackAlbum(t)
		art := ui.art(album, px(128))
		if art == nil {
			return false
		}
		// cover with a 1px border
		src := newCanvas(px(128), px(128))
		fillRect(src, 0, 0, src.Bounds().Dx(), src.Bounds().Dy(), th.artBorder)
		inner := scaleImage(art, px(126), px(126))
		blit(src, inner, px(1), px(1), 1)
		np.warpSrc, np.warpSrcT = src, t
	}
	src := np.warpSrc
	// output: the left 170 CSS px of the screen, the height of the view
	W := px(320)
	npMetaH := np.H * 0.7
	H := px(np.H)
	out := newCanvasCached(&np.warpArt, W, H)
	for y := 0; y < H; y++ {
		row := out.Pix[y*out.Stride : y*out.Stride+px(170)*4]
		clear(row)
	}
	//
	//
	//
	th := 35 * tilt * math.Pi / 180
	th0 := 35 * math.Pi / 180
	sinT, cosT := math.Sin(th), math.Cos(th)
	const pv = 1000.0
	ox, oy := 160.0, npMetaH/2
	cx, cy := 84.0, npMetaH/2
	// the card's left edge (x, z) at the resting angle th0, then rotated
	// by th about it
	//
	//
	xr, zr := cx+64*math.Cos(th0), -64*math.Sin(th0)
	ax := xr - 64*cosT
	bz := zr + 64*sinT
	parallelRows(0, H, func(r0, r1 int) {
		for dy := r0; dy < r1; dy++ {
			py := float64(dy) + 0.5
			for dx := 0; dx < px(170); dx++ {
				fx := float64(dx) + 0.5
				A := fx/DPR - ox
				den := pv*cosT - A*sinT
				if den <= 0 {
					continue
				}
				X := ((ax-ox)*pv - (pv-bz)*A) / (sinT*A - pv*cosT)
				k := pv / (pv - bz + X*sinT)
				Y := oy + (py/DPR-oy)/k - cy
				u := X + 64
				v := Y + 64
				if u < 0 || u >= 128 || v < 0 || v >= 256 {
					continue
				}
				alpha := 1.0
				vv := v
				if v >= 128 {
					// reflection: mirrored, fading out over the top 30%
					//
					vv = 256 - v
					r := vv / 128
					if r < 0.7 {
						continue
					}
					alpha = (vv/128 - 0.7) * 0.2 / 0.3
				}
				c := sampleBilinear(src, u*DPR-0.5, vv*DPR-0.5)
				o := out.PixOffset(dx, dy)
				a := float64(c[3]) / 255 * alpha
				out.Pix[o] = uint8(float64(c[0]) * alpha)
				out.Pix[o+1] = uint8(float64(c[1]) * alpha)
				out.Pix[o+2] = uint8(float64(c[2]) * alpha)
				out.Pix[o+3] = uint8(a * 255)
			}
		}
	})
	np.warpTrack = t
	np.warpTilt = tilt
	return true
}

func sampleBilinear(img *image.RGBA, x, y float64) [4]uint8 {
	b := img.Bounds()
	if x < 0 {
		x = 0
	}
	if y < 0 {
		y = 0
	}
	x0, y0 := int(x), int(y)
	fx, fy := x-float64(x0), y-float64(y0)
	x1, y1 := x0+1, y0+1
	if x1 >= b.Dx() {
		x1 = b.Dx() - 1
	}
	if y1 >= b.Dy() {
		y1 = b.Dy() - 1
	}
	if x0 >= b.Dx() {
		x0 = b.Dx() - 1
	}
	if y0 >= b.Dy() {
		y0 = b.Dy() - 1
	}
	var out [4]uint8
	for c := 0; c < 4; c++ {
		p00 := float64(img.Pix[img.PixOffset(x0, y0)+c])
		p10 := float64(img.Pix[img.PixOffset(x1, y0)+c])
		p01 := float64(img.Pix[img.PixOffset(x0, y1)+c])
		p11 := float64(img.Pix[img.PixOffset(x1, y1)+c])
		v := p00*(1-fx)*(1-fy) + p10*fx*(1-fy) + p01*(1-fx)*fy + p11*fx*fy
		out[c] = uint8(v + 0.5)
	}
	return out
}

func formatPlaybackTime(sec float64) string {
	if sec < 0 {
		sec = 0
	}
	s := int(sec) % 3600
	return fmt.Sprintf("%02d:%02d", s/60, s%60)
}

func (np *NowPlayingView) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	W := dst.Bounds().Dx()
	if !np.hideArt {
		fillRect(dst, 0, 0, W, dst.Bounds().Dy(), th.bg)
	}
	p := ui.app.Playing
	var t *Track
	if p != nil {
		t = p.Track
	}
	npMetaH := np.H * 0.7
	// cover (tilted while playing) and track info
	if t != nil {
		if !np.hideArt {
			target := 0.0
			if p.State == StatePlaying {
				target = 1
			}
			// snap when the view was not drawn on the previous frame
			if !np.tiltInit || np.lastFrame != ui.frameNo-1 {
				np.tiltInit = true
				np.tilt.Set(target)
			} else if np.tilt.Target() != target {
				np.tilt.To(target, 700*time.Millisecond, easeInOut, 0)
			}
			np.lastFrame = ui.frameNo
			tv := np.tilt.Get(now)
			if np.warpTrack != t || np.warpArt == nil || np.warpTilt != tv {
				np.buildWarp(ui, t, tv)
			}
			if np.warpArt != nil {
				blit(dst, np.warpArt, 0, 0, 1)
			}
		}
		ui.drawNPInfo(dst, t, npMetaH)
	}
	// control rows, sliding horizontally
	//
	//
	cy := npMetaH + (np.H*0.3-16)/2
	ctrlW := 304.0
	mx := np.mainX.Get(now) * ctrlW
	vx := np.volX.Get(now) * ctrlW
	sx := np.scrubX.Get(now) * ctrlW
	//
	//
	if vx < ctrlW {
		ui.drawVolumeBar(dst, vx+8, cy)
	}
	if mx > -ctrlW {
		ui.drawTrackProgress(dst, mx+8, cy, p)
	}
	if sx < ctrlW {
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

func (ui *IpodUI) drawNPInfo(dst *image.RGBA, t *Track, npMetaH float64) {
	// title (wrapped), artist and album (2 lines each), vertically centred
	// in the top 70% of the view, starting at x = 168
	tf := face(W600, 14.72)
	sf := face(W600, 12)
	tlh, tasc := lineBox(14.72)
	slh, sasc := lineBox(12)
	title := wrapLines(tf, t.Title, 288, 99)
	artist := wrapLines(sf, t.Artist, 288, 2)
	album := wrapLines(sf, t.Album, 288, 2)
	if t.Title == "" {
		title = nil
	}
	h := float64(len(title))*tlh + float64(len(artist)+len(album))*slh
	y := (npMetaH - h) / 2
	for _, l := range title {
		drawText(dst, tf, l, 336, (y+tasc)*DPR, th.text)
		y += tlh
	}
	sub := th.npSub
	for _, l := range append(artist, album...) {
		drawText(dst, sf, l, 336, (y+sasc)*DPR, sub)
		y += slh
	}
}

func (ui *IpodUI) drawNPStatus(dst *image.RGBA) {
	q := ui.app.Queue
	if q == nil {
		return
	}
	var icons []string
	if q.Shuffle {
		icons = append(icons, "emoji_shuffle")
	}
	switch q.Repeat {
	case RepeatOne:
		icons = append(icons, "emoji_repeat_one")
	case RepeatAll:
		icons = append(icons, "emoji_repeat")
	}
	x := 308.0
	for i := len(icons) - 1; i >= 0; i-- {
		img := ui.asset(icons[i])
		w := float64(img.Bounds().Dx())
		x -= w / DPR
		blit(dst, img, px(x), px(8), 0.8)
		x -= 8
	}
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
//
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
		for k := 0; k < (x1-x0)*4; k++ {
			dst.Pix[do+k] = uint8(float64(dst.Pix[do+k])*(1-a) + float64(dst.Pix[so+k])*a + 0.5)
		}
	}
}

// reflectStrip is the older reflection (fading in from the bottom).
func reflectStrip(dst *image.RGBA, x0, x1, yTop, h int) {
	b := dst.Bounds()
	for i := 0; i < h; i++ {
		// t: 1 at the row next to the edge, 0 at the far one
		t := (float64(h-1-i) + 0.5) / float64(h)
		if t < 0.6 {
			continue
		}
		a := (t - 0.6) * 0.1 / 0.4
		sy := yTop + h - 1 - i
		dy := yTop + h + i
		if dy >= b.Max.Y || sy < b.Min.Y {
			continue
		}
		for x := x0; x < x1; x++ {
			if x < b.Min.X || x >= b.Max.X {
				continue
			}
			so := dst.PixOffset(x, sy)
			do := dst.PixOffset(x, dy)
			for c := 0; c < 3; c++ {
				dst.Pix[do+c] = uint8(float64(dst.Pix[do+c])*(1-a) + float64(dst.Pix[so+c])*a)
			}
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
// nowPlayingOnScreen: Now Playing is the visible screen.
//go:noinline
func (ui *IpodUI) nowPlayingOnScreen() bool {
	if cf := ui.cover; cf != nil && !cf.exiting && cf.np != nil && cf.npFade.Target() > 0 {
		return true
	}
	if len(ui.full) > 0 {
		if _, ok := ui.full[len(ui.full)-1].Custom.(*NowPlayingView); ok && ui.fullX.Target() == 0 {
			return true
		}
	}
	return false
}
