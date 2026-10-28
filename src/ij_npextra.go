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

// toggleCard turns the cover over to its album's songs, or back, as Cover
// Flow does: from where it is (turned while playing) a half turn to the
// back pose, the card coming in bigger and stronger.
func (np *NowPlayingView) toggleCard(ui *IpodUI) {
	now := animNow()
	if np.flipped {
		np.flipped = false
		np.backA.To(0, 250*time.Millisecond, easeOutCubic, 0)
		np.flip.To(0, 500*time.Millisecond, easeCSS, 0)
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
	np.back, np.backAl, np.flipped = ui.backsideList(al), al, true
	for i, it := range np.back.Items {
		if it.Tracks[it.TrackIdx] == p.Track {
			np.back.Index = i
		}
	}
	np.backSince = now
	np.flip.To(1, 500*time.Millisecond, easeCSS, 0)
	np.backA.Set(0)
	np.backA.To(1, 500*time.Millisecond, easeInOut, 0)
	ui.dirty = true
}

// flipFrame: turned back to the cover: the card goes.
func (np *NowPlayingView) flipFrame(now time.Time) {
	if !np.flipped && np.back != nil && !np.flip.Running() && !np.backA.Running() {
		np.back, np.backAl = nil, nil
	}
}

// OnKey: START turns the cover over; on the card the arrows pick a song,
// on the options row they pick an option.
func (np *NowPlayingView) OnKey(ui *IpodUI, k Key) bool {
	if k == START {
		np.toggleCard(ui)
		return true
	}
	if np.flipped {
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
	v := np.back
	if v == nil {
		return
	}
	if ni := v.Index + d; ni >= 0 && ni < len(v.Items) {
		v.Index = ni
		v.scrolledAt = animNow()
		v.dirty = true
		ui.dirty = true
	}
}

// cardPlay: A on the card: the song plays and the cover turns back.
func (np *NowPlayingView) cardPlay(ui *IpodUI) {
	if np.flip.Target() != 1 || np.back == nil || len(np.back.Items) == 0 {
		return
	}
	it := np.back.Items[np.back.Index]
	ui.playSong(it.Tracks, it.TrackIdx, false)
	np.toggleCard(ui)
}

// OnMenu (B): the cover turns back; the options row goes; else back out.
func (np *NowPlayingView) OnMenu(ui *IpodUI) {
	if np.flipped {
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

// npStage draws Now Playing's cover as Cover Flow draws its own: the 3D
// cover at npCoverPose (turned 35° while playing, flat and smaller when
// paused) with its reflection, cross-fading when the album changes, and
// turning over (flip) to the card on its back (backA).
func (ui *IpodUI) npStage(dst *image.RGBA, np *NowPlayingView, now time.Time) {
	p := ui.app.Playing
	if p == nil || p.Track == nil {
		return
	}
	t := p.Track
	target := 0.0
	if p.State == StatePlaying && !np.untilt {
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
	f := np.flip.Get(now)
	if !np.flyHide {
		base := npCoverPose(np.tilt.Get(now))
		base.A -= 360 // (a full turn back: it turns over the same way as Cover Flow's)
		m := coverMatrix(lerpX(base, backPose(), f))
		front := facesViewer(m)
		draw := func(al *Album, op float64) {
			art := ui.art(al, px(128))
			if art == nil {
				art = ui.artFallback(al, px(128))
			}
			if art == nil {
				art = ui.defaultArtSized(px(128))
			}
			if !front {
				art = coverBackFace()
			}
			sc := float64(art.Bounds().Dx()) / 128
			tm := texMap{img: art, su: sc, sv: sc, u1: 128, v1: 128}
			if r := 0.2 * (1 - f); r > 0.001 {
				tm.v1 = 166.4
				tm.reflFrom, tm.reflMax, tm.reflStart = 128, r, 0.7
			}
			drawTex(dst, m, tm, op)
		}
		al := ui.trackAlbum(t)
		if k := np.change.Get(now); k < 1 && np.oldT != nil && ui.trackAlbum(np.oldT) != al {
			draw(ui.trackAlbum(np.oldT), 1)
			draw(al, k)
		} else {
			draw(al, 1)
		}
	}
	if np.back != nil {
		bm := coverMatrix(backPose())
		drawBacksideCard(dst, bm, bm, np.backA.Get(now), func(w, h float64) *image.RGBA {
			return ui.backsideTex(&np.backBuf, np.backAl, np.back, w, h, np.backSince, now)
		})
	}
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
