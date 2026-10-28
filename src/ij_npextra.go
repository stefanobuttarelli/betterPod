package main

// ij_npextra.go — two more things on Now Playing:
//
//   - START turns the cover over to the back of the album, the same card as
//     Cover Flow's (size, list, scrolling titles): UP / DOWN pick a song, A
//     plays it (the card turns back to the cover), START or B turn it back,
//     Y the song's options.
//   - A goes through the rows under the song: the progress, the scrubber,
//     then the options: Shuffle and Repeat as two buttons, as wide as the
//     bar. LEFT / RIGHT pick one, A changes it; B (or 8 s without a key)
//     goes back to the progress.

import (
	"image"
	"math"
	"time"
)

// npCardRect: the card turned over, where Cover Flow has it (in the view:
// under the header), down past the bottom of the screen.
var npCardRect = image.Rect(64, 42, 64+px(256), 442)

// npFlatCover: Now Playing's cover held flat, in the view (208 px).
var npFlatCover = image.Rect(47, 50, 255, 258)

type npCard struct {
	al     *Album
	back   *View
	since  time.Time
	marq   bool
	open   *Anim // 0 the cover, 1 the card in the middle
	closed bool  // turning back
}

var npCardBuf, npCardList *image.RGBA

// npCardStart turns the cover over (or back).
func (np *NowPlayingView) toggleCard(ui *IpodUI) {
	now := animNow()
	if c := np.card; c != nil {
		if !c.closed {
			c.closed = true
			c.open.To(0, 450*time.Millisecond, easeInOut, 0)
		}
		ui.dirty = true
		return
	}
	p := ui.app.Playing
	if p == nil || p.Track == nil {
		return
	}
	al := ui.trackAlbum(p.Track)
	if al == nil {
		return
	}
	c := &npCard{al: al, back: ui.backsideList(al), open: newAnim(0)}
	for i, it := range c.back.Items {
		if it.Tracks[it.TrackIdx] == p.Track {
			c.back.Index = i
		}
	}
	// a turned (playing) cover first turns flat, then over
	var delay time.Duration
	np.untilt = true
	if np.tilt.Get(now) > 0.01 {
		np.tilt.To(0, 280*time.Millisecond, easeInOut, 0)
		delay = 280 * time.Millisecond
	}
	c.since = now.Add(delay + 600*time.Millisecond)
	c.open.To(1, 550*time.Millisecond, easeInOut, delay)
	np.card = c
	ui.dirty = true
}

// cardFrame: the card is back at the cover: gone.
func (np *NowPlayingView) cardFrame(now time.Time) {
	if c := np.card; c != nil && c.closed && !c.open.Running() {
		np.card = nil
		np.flyHide = false
		np.untilt = false
	}
}

// OnKey: START turns the cover over; on the card the arrows pick a song,
// on the options row they pick an option.
func (np *NowPlayingView) OnKey(ui *IpodUI, k Key) bool {
	if k == START {
		np.toggleCard(ui)
		return true
	}
	if c := np.card; c != nil && !c.closed {
		switch k {
		case UP, DOWN:
			d := 1
			if k == UP {
				d = -1
			}
			np.cardMove(ui, d)
			return true
		case LEFT, RIGHT:
			return true
		}
		return false
	}
	if np.opts {
		switch k {
		case LEFT, UP:
			np.optMove(ui, -1)
			return true
		case RIGHT, DOWN:
			np.optMove(ui, 1)
			return true
		}
	}
	return false
}

func (np *NowPlayingView) cardMove(ui *IpodUI, d int) {
	v := np.card.back
	if ni := v.Index + d; ni >= 0 && ni < len(v.Items) {
		v.Index = ni
		v.scrolledAt = animNow()
		v.dirty = true
		ui.dirty = true
	}
}

// cardPlay: A on the card: the song plays and the card turns back.
func (np *NowPlayingView) cardPlay(ui *IpodUI) {
	c := np.card
	if c.closed || c.open.Target() != 1 || len(c.back.Items) == 0 {
		return
	}
	it := c.back.Items[c.back.Index]
	ui.playSong(it.Tracks, it.TrackIdx, false)
	np.toggleCard(ui)
}

// OnMenu (B): the card turns back; the options row goes; else back out.
func (np *NowPlayingView) OnMenu(ui *IpodUI) {
	if c := np.card; c != nil && !c.closed {
		np.toggleCard(ui)
		return
	}
	if np.opts {
		np.opts = false
		np.volEnabled = true
		np.updateControls()
		return
	}
	ui.hideView()
}

// ---- the options row ----

const npOptions = 2 // Shuffle, Repeat

func (np *NowPlayingView) optMove(ui *IpodUI, d int) {
	np.optSel = min(max(np.optSel+d, 0), npOptions-1)
	np.optsAt = animNow()
	ui.dirty = true
}

// optPress: A on an option: its next value.
func (np *NowPlayingView) optPress(ui *IpodUI) {
	np.optsAt = animNow()
	switch np.optSel {
	case 0:
		next := map[string]string{"off": "songs", "songs": "albums", "albums": "off"}[ui.shuffleMode]
		if next == "" {
			next = "songs"
		}
		ui.setShuffle(next)
	case 1:
		switch ui.repeatMode() {
		case RepeatOff:
			ui.setRepeat(RepeatAll)
		case RepeatAll:
			ui.setRepeat(RepeatOne)
		default:
			ui.setRepeat(RepeatOff)
		}
	}
	ui.refreshBuilt()
	ui.dirty = true
}

// optsTick: 8 s without a key on the options row: back to the progress.
func (np *NowPlayingView) optsTick(now time.Time) {
	if np.opts && now.Sub(np.optsAt) > 8*time.Second {
		np.opts = false
		np.volEnabled = true
		np.updateControls()
	}
}

// drawOptions: the two buttons, as wide as the bar (x, y: the row, CSS).
func (ui *IpodUI) drawOptions(dst *image.RGBA, np *NowPlayingView, x, y float64) {
	const gap = 8.0
	bw := (280 - gap) / 2
	y0, y1 := y-5, y+21
	for i := 0; i < npOptions; i++ {
		bx := x + 12 + float64(i)*(bw+gap)
		sel := i == np.optSel
		var icon, label, value string
		switch i {
		case 0:
			icon, label, value = "hdr_shuffle", "Shuffle", shuffleLabel(ui.shuffleMode)
		case 1:
			icon, label = "hdr_repeat", "Repeat"
			value = repeatLabel(ui.repeatMode())
			if ui.repeatMode() == RepeatOne {
				icon = "hdr_repeat_one"
			}
		}
		on := value != "Off"
		tc, vc := th.text, th.sub
		if sel {
			fillRoundedGradient(dst, bx*DPR, y0*DPR, (bx+bw)*DPR, y1*DPR, 6*DPR, selGrad, 1)
			tc, vc = colWhite, Col{255, 255, 255, 0.8}
		} else {
			fillRoundedGradient(dst, bx*DPR, y0*DPR, (bx+bw)*DPR, y1*DPR, 6*DPR, th.sheetOpt, 1)
			strokeRounded(dst, bx*DPR, y0*DPR, (bx+bw)*DPR, y1*DPR, 6*DPR, DPR, Col{th.sheetBorder.R, th.sheetBorder.G, th.sheetBorder.B, 0.35}, 1)
		}
		// the icon (dimmer when off), the name and the value
		var ic *image.RGBA
		if sel {
			ic = whiteIcon(ui, icon)
		} else {
			ic = ui.themed(icon)
		}
		iw, ih := float64(ic.Bounds().Dx())/DPR, float64(ic.Bounds().Dy())/DPR
		op := 1.0
		if !on {
			op = 0.45
		}
		blit(dst, ic, px(bx+9), px((y0+y1)/2-ih/2), op)
		f := face(W700, 12)
		vf := face(W500, 12)
		_, asc := lineBox(12)
		base := ((y0+y1)/2 + asc/2 - 0.5) * DPR
		tx := bx + 9 + iw + 6
		drawText(dst, f, label, tx*DPR, base, tc)
		lw := textWidth(f, label) / DPR
		vw := textWidth(vf, value) / DPR
		vx := math.Max(tx+lw+6, bx+bw-9-vw)
		drawText(dst, vf, ellipsize(vf, value, (bx+bw-9-vx)*DPR), vx*DPR, base, vc)
	}
}

// ---- the card ----

// drawCard: the cover turning over to the card (or back), over the view.
func (ui *IpodUI) drawNPCard(dst *image.RGBA, np *NowPlayingView, now time.Time) {
	c := np.card
	o := c.open.Get(now)
	if o <= 0.001 {
		if !c.closed {
			np.flyHide = false // (still waiting for the cover to turn flat)
		}
		return
	}
	np.flyHide = true // its own cover is the one turning
	r := lerpRect(npFlatCover, npCardRect, o)
	if o < 0.5 {
		side := min(r.Dx(), r.Dy())
		cx, cy := (r.Min.X+r.Max.X)/2, (r.Min.Y+r.Max.Y)/2
		sq := image.Rect(cx-side/2, cy-side/2, cx-side/2+side, cy-side/2+side)
		k := math.Abs(math.Cos(o * math.Pi))
		w := int(float64(sq.Dx())*k + 0.5)
		src := ui.art(c.al, px(128))
		if src == nil {
			src = ui.artFallback(c.al, px(128))
		}
		if src != nil && w > 0 {
			x := sq.Min.X + (sq.Dx()-w)/2
			drawNN(dst, src, image.Rect(x, sq.Min.Y, x+w, sq.Max.Y), (1-k)*0.4, 1)
		}
		return
	}
	k := math.Abs(math.Cos(o * math.Pi))
	w := int(float64(r.Dx())*k + 0.5)
	if w < 2 {
		return
	}
	tex := ui.albumCardTexture(&npCardBuf, &npCardList, c.al, c.back, npCardRect.Dx(), npCardRect.Dy(), c.since, now, &c.marq)
	x := r.Min.X + (r.Dx()-w)/2
	if w == tex.Bounds().Dx() && r.Dy() == tex.Bounds().Dy() {
		blitOpaque(dst, tex, x, r.Min.Y)
		return
	}
	drawNN(dst, tex, image.Rect(x, r.Min.Y, x+w, r.Max.Y), (1-k)*0.5, 1)
}

// albumCardTexture: the back of an album's card, like Cover Flow's: album
// and artist on blue (scrolling when long, marq says so), then the songs.
func (ui *IpodUI) albumCardTexture(buf, listBuf **image.RGBA, al *Album, back *View, W, H int, since, now time.Time, marq *bool) *image.RGBA {
	img := newCanvasCached(buf, W, H)
	fillRect(img, 0, 0, W, H, th.bg)
	tf := face(W700, 16)
	sf := face(W700, 14)
	tlh, tasc := lineBox(16)
	_, sasc := lineBox(14)
	w := float64(W) / DPR
	inner := w - 2
	infoH := backsideInfoH()
	fillVGradient(img, px(1), px(1), px(inner), px(infoH-1), []GradStop{{0, hex("#6585ad")}, {1, hex("#789ab3")}})
	fillRect(img, px(1), px(infoH), px(inner), px(1), hex("#6d87a3"))
	cyc := math.Max(marqueeCycle(tf, al.Name, inner-16), marqueeCycle(sf, al.Artist, inner-16))
	*marq = cyc > 0
	y := 5.0
	drawMarqueeSync(img, tf, al.Name, 9, y+tasc, inner-16, colWhite, since, now, cyc)
	y += tlh
	drawMarqueeSync(img, sf, al.Artist, 9, y+sasc, inner-16, colWhite, since, now, cyc)
	lt := infoH + 1
	if lh := float64(H)/DPR - 1 - lt; lh > 0 {
		lst := newCanvasCached(listBuf, px(inner), px(lh))
		ui.renderList(back, lst)
		blitOpaque(img, lst, px(1), px(lt))
	}
	fillRect(img, 0, 0, W, px(1), th.cardBorder)
	fillRect(img, 0, H-px(1), W, px(1), th.cardBorder)
	fillRect(img, 0, 0, px(1), H, th.cardBorder)
	fillRect(img, W-px(1), 0, px(1), H, th.cardBorder)
	return img
}
