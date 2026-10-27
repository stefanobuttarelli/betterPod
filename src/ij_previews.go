package main

// ij_previews.go — the right-hand previews of the split menu: Ken Burns
// album artwork for Music, the panning cover for Now Playing and the grey
// icon previews for Games / Settings.

import (
	"image"
	"time"
)

// KenBurns cycles the album covers (slow slide, 5 s cross-fades).
type KenBurns struct {
	stack       []*Album
	animStart   map[*Album]time.Time
	mounted     time.Time
	transStart  time.Time
	lastCycle   time.Time
	initialized bool
}

func newKenBurns() *KenBurns { return &KenBurns{animStart: map[*Album]time.Time{}} }

func (kb *KenBurns) reset(ui *IpodUI, now time.Time) {
	kb.stack = nil
	if lib := ui.app.Library; lib != nil {
		// albums with artwork first; all of them if none has any
		for _, a := range lib.Albums {
			if a.ArtPath != "" || a.ArtImg != nil {
				kb.stack = append(kb.stack, a)
			}
		}
		if len(kb.stack) == 0 {
			kb.stack = append(kb.stack, ui.app.Library.Albums...)
		}
	}
	kb.animStart = map[*Album]time.Time{}
	kb.mounted = now
	kb.lastCycle = now
	kb.transStart = time.Time{}
	if len(kb.stack) > 0 {
		kb.animStart[kb.stack[0]] = now
	}
	kb.initialized = true
}

func (kb *KenBurns) update(now time.Time) {
	if len(kb.stack) == 0 {
		return
	}
	// every 5 s the top cover starts fading out over the next one
	if now.Sub(kb.lastCycle) >= 5*time.Second && kb.transStart.IsZero() {
		kb.lastCycle = kb.lastCycle.Add(5 * time.Second)
		kb.transStart = now
		if len(kb.stack) > 1 {
			if _, ok := kb.animStart[kb.stack[1]]; !ok {
				kb.animStart[kb.stack[1]] = now
			}
		}
	}
	// after the 2 s fade the top cover goes to the bottom of the stack
	if !kb.transStart.IsZero() && now.Sub(kb.transStart) >= 2*time.Second {
		top := kb.stack[0]
		kb.stack = append(kb.stack[1:], top)
		delete(kb.animStart, top)
		kb.transStart = time.Time{}
	}
}

func (kb *KenBurns) animating(ui *IpodUI) bool {
	if ui.preview == PrevNowPlaying && ui.previewHide.Target() < 1 && ui.musicPreviewVisible() {
		if p := ui.app.Playing; p != nil && p.Track != nil && !hold.on {
			return true
		}
	}
	return ui.preview == PrevMusic && ui.previewHide.Target() < 1 && len(kb.stack) > 0 && ui.musicPreviewVisible()
}

// Now Playing preview: the current cover, full height, panning slowly
// to the left over 14 s and fading back to the start over 2 s. The pan
// continues where it was when the preview comes back within 0.5 s.
//
//
//
//
//
//
//
var npPanStart, npPanShown time.Time

func (ui *IpodUI) drawNowPlayingPan(dst, art *image.RGBA, now time.Time) {
	travel := float64(art.Bounds().Dx() - dst.Bounds().Dx())
	if travel <= 0 {
		blit(dst, art, 0, 0, 1)
		return
	}
	if npPanStart.IsZero() || now.Sub(npPanShown) > 500*time.Millisecond {
		npPanStart = now
	}
	npPanShown = now
	el := now.Sub(npPanStart) % (16 * time.Second)
	if el < 14*time.Second {
		t := easeInOut(float64(el) / float64(14*time.Second))
		blitShiftX(dst, art, -t*travel, 0, 1)
		return
	}
	// back to the start
	f := easeCSS(float64(el-14*time.Second) / float64(2*time.Second))
	blitShiftX(dst, art, -travel, 0, 1)
	blit(dst, art, 0, 0, f)
}

// musicPreviewVisible: the split menu (with its preview) is on screen
// or sliding.
func (ui *IpodUI) musicPreviewVisible() bool { return ui.leftX.Target() > -1 || ui.leftX.Running() }

// render draws the stack: the next cover under the top one, which fades
// out during a transition.
func (kb *KenBurns) render(ui *IpodUI, dst *image.RGBA, now time.Time) {
	if !kb.initialized {
		kb.reset(ui, now)
	}
	kb.update(now)
	n := len(kb.stack)
	if n == 0 {
		return
	}
	size := px(240)
	panelW := dst.Bounds().Dx()
	draw := func(a *Album, opacity float64) {
		img := ui.art(a, size)
		if img == nil {
			return
		}
		x := float64(panelW - size)
		if st, ok := kb.animStart[a]; ok {
			p := float64(now.Sub(st)) / float64(20*time.Second)
			if p > 1 {
				p = 1
			}
			x += float64(size) * 0.1 * easeCSS(p)
		}
		blitShiftX(dst, img, x, 0, opacity)
	}
	if n > 1 {
		draw(kb.stack[1], 1)
	}
	op := 1.0
	if !kb.transStart.IsZero() {
		op = 1 - easeCSS(float64(now.Sub(kb.transStart))/float64(2*time.Second))
	}
	draw(kb.stack[0], op)
	// get the one after ready
	if n > 2 {
		ui.art(kb.stack[2], size)
	}
}

// blitShiftX blits src at a fractional x offset (bilinear filtering).
func blitShiftX(dst, src *image.RGBA, x float64, y int, opacity float64) {
	ix := int(x)
	if x < 0 {
		ix--
	}
	f := x - float64(ix)
	if f < 0.02 {
		blit(dst, src, ix, y, opacity)
		return
	}
	if f > 0.98 {
		blit(dst, src, ix+1, y, opacity)
		return
	}
	sb := src.Bounds()
	db := dst.Bounds()
	w := sb.Dx()
	wf := uint32(f*256 + 0.5)
	ow := uint32(opacity*256 + 0.5)
	xa, xb := ix, ix+w
	if xa < db.Min.X {
		xa = db.Min.X
	}
	if xb >= db.Max.X {
		xb = db.Max.X - 1
	}
	ya, yb := 0, sb.Dy()
	if y < db.Min.Y {
		ya = db.Min.Y - y
	}
	if y+yb > db.Max.Y {
		yb = db.Max.Y - y
	}
	sp, sst := pix32(src)
	dp, dst32 := pix32(dst)
	op32 := ow
	if op32 > 256 {
		op32 = 256
	}
	parallelRows(ya, yb, func(r0, r1 int) {
		for sy := r0; sy < r1; sy++ {
			srow := sp[sy*sst : sy*sst+w]
			drow := dp[(y+sy)*dst32:]
			for dxp := xa; dxp <= xb; dxp++ {
				sx := dxp - ix
				var p0, p1 uint32
				if sx < w {
					p0 = srow[sx]
				}
				if sx > 0 && sx-1 < w {
					p1 = srow[sx-1]
				}
				c := lerp32(p0, p1, wf)
				if op32 < 256 {
					c = scale32(c, op32)
				}
				drow[dxp] = over32(c, drow[dxp])
			}
		}
	})
}

// grey preview background (the gradient now comes from the theme), and
// the layer the Music preview is drawn into while it slides out
var previewBgGrad = []GradStop{{0, hex("#b1b5c0")}, {1, hex("#686e7a")}}
var greyPreviewBg, musicLayer *image.RGBA

func (ui *IpodUI) renderPreview(dst *image.RGBA, now time.Time) {
	w := dst.Bounds().Dx()
	h := dst.Bounds().Dy()
	fillRect(dst, 0, 0, w, h, th.bg)
	// Games / Settings / Now Playing previews; Music (Ken Burns) below
	switch ui.preview {
	case PrevGames:
		ui.drawIconPreview(dst, "dice", 96, []previewLine{{"Games", 16, W500}})
	case PrevSettings:
		ui.drawIconPreview(dst, "ipod_logo", 32, []previewLine{{"Settings", 16, W500}})
	case PrevNowPlaying:
		if p := ui.app.Playing; p != nil && p.Track != nil {
			art := ui.art(ui.trackAlbum(p.Track), h)
			if art != nil {
				ui.drawNowPlayingPan(dst, art, now)
			}
		}
		// (the Music preview may still be sliding out over it)
	}
	// the Music preview slides out to the right while fading
	m := ui.musicPrev.Get(now)
	if ui.preview == PrevMusic || ui.musicPrev.Running() {
		if ui.preview == PrevMusic && !ui.kb.initialized {
			ui.kb.reset(ui, now)
		}
		if m <= 0 && !ui.musicPrev.Running() {
			ui.kb.render(ui, dst, now)
		} else {
			layer := newCanvasCached(&musicLayer, w, h)
			clearRGBA(layer)
			ui.kb.render(ui, layer, now)
			blit(dst, layer, int(float64(w)*1.3*m+0.5), 0, 1-m)
		}
	} else {
		ui.kb.initialized = false
	}
}

// clearRGBA makes img fully transparent.
//
func clearRGBA(img *image.RGBA) {
	for i := range img.Pix {
		img.Pix[i] = 0
	}
}

// previewLine is a line of text under the icon of an icon preview.
type previewLine struct {
	text   string
	size   float64
	weight FontWeight
}

// drawIconPreview: grey preview, icon and centred lines of text.
func (ui *IpodUI) drawIconPreview(dst *image.RGBA, icon string, iconCSS float64, lines []previewLine) {
	w := dst.Bounds().Dx()
	h := dst.Bounds().Dy()
	if greyPreviewBg == nil || greyPreviewBg.Bounds().Dy() != h {
		greyPreviewBg = newCanvas(w, h)
		fillVGradient(greyPreviewBg, 0, 0, w, h, th.preview)
	}
	blitOpaque(dst, greyPreviewBg, 0, 0)
	total := iconCSS + 16
	for _, l := range lines {
		lh, _ := lineBox(l.size)
		total += lh
	}
	y := (float64(h)/DPR - total) / 2
	img := ui.asset(icon)
	blit(dst, img, (w-img.Bounds().Dx())/2, px(y+8), 1)
	y += iconCSS + 16
	for _, l := range lines {
		f := face(l.weight, l.size)
		lh, asc := lineBox(l.size)
		tw := textWidth(f, l.text)
		drawText(dst, f, l.text, (float64(w)-tw)/2, (y+asc)*DPR, colWhite)
		y += lh
	}
}
