package main

// ij_endqueue.go — End of the Queue: when the last song of the queue ends,
// or NEXT is pressed on it, a card in the middle of the screen says so and
// offers Genius (50 songs at random), Go to Music, or Not Now (B too).
//
// It is a Sheet drawn another way (Sheet.end), so the keys, B and the
// dimmed screen behave as for the other sheets. NEXT on the last song
// doesn't stop it: it plays on under the card, and Not Now leaves it.
// Not shown under the screensaver, Hold, or while a Listen plays.

import (
	"image"
	"time"

	"github.com/fogleman/gg"
	"golang.org/x/image/font"
)

// queueEnded is called by the player (any goroutine) when the queue has
// nothing after the current song: manual for NEXT, else the song ended.
var queueEnded func(manual bool)

// endInfo: what the card says.
type endInfo struct {
	title string // the last song's
	album *Album
}

func (ui *IpodUI) initEndQueue() {
	queueEnded = func(manual bool) {
		ui.post(func() { ui.showEndOfQueue(manual) })
	}
}

func (ui *IpodUI) showEndOfQueue(manual bool) {
	if ui.saver != nil || hold.on || previewActive() || len(ui.library().Tracks) == 0 {
		return
	}
	for _, s := range ui.sheets {
		if s.end != nil && !s.exiting {
			return // already up
		}
	}
	t := ui.app.Playing.Track
	if t == nil {
		return
	}
	info := &endInfo{title: t.Title, album: ui.trackAlbum(t)}
	opts := []SheetOption{
		{Label: "Genius", OnSelect: ui.playGenius},
		{Label: "Go to Music", OnSelect: func() {
			ui.resetViews()
			ui.showView(ui.musicView(), "")
		}},
		{Label: "Not Now"},
	}
	s := &Sheet{opts: opts, anim: newAnim(1), cacheIx: -1, end: info}
	s.anim.To(0, lightDur(durSlide), easeInOut, 0)
	ui.sheets = append(ui.sheets, s)
	ui.dirty = true
	if manual {
		logMsg("End of the queue (NEXT on the last song)")
	}
}

// The card, in CSS px.
const (
	endW     = 250.0
	endH     = 192.0
	endCover = 48.0
)

// renderEnd draws the card (its cover when it is ready: until then the
// cache is made again each frame the sheet is drawn).
func (s *Sheet) renderEnd(ui *IpodUI) *image.RGBA {
	art := ui.art(s.end.album, px(endCover))
	if s.cache != nil && s.cacheIx == s.index && s.cacheDark == th.dark && s.cacheArt == (art != nil) {
		return s.cache
	}
	d := DPR
	img := newCanvas(px(endW+8), px(endH+8))
	x0, y0 := 4.0, 2.0
	x1, y1 := x0+endW, y0+endH
	sheetCard(img, x0, y0, x1, y1)
	cx := (x0 + x1) / 2
	center := func(f font.Face, txt string, y float64, c Col) {
		txt = ellipsize(f, txt, (endW-30)*d)
		drawText(img, f, txt, cx*d-textWidth(f, txt)/2, y*d, c)
	}
	// the last song's cover
	ax, ay := cx-endCover/2, y0+13
	if art != nil {
		blit(img, art, px(ax), px(ay), 1)
	} else {
		fillRect(img, px(ax), px(ay), px(endCover), px(endCover), rgba(128, 128, 128, 0.25))
	}
	strokeRounded(img, ax*d, ay*d, (ax+endCover)*d, (ay+endCover)*d, 2*d, 1, rgba(0, 0, 0, 0.18), 1)
	center(face(W700, 15), "End of the Queue", y0+81+lineCapHeight(15)/2-6, th.text)
	center(face(W400, 11), "Nothing else is lined up after", y0+97, th.sub)
	center(face(W600, 11), "“"+s.end.title+"”.", y0+110, th.text)
	// the buttons: Genius across, Go to Music and Not Now under it
	bx0, bx1 := x0+15, x1-15
	half := (bx1 - bx0 - 6) / 2
	button := func(i int, l, t, r, b float64, label, sub string) {
		sel := i == s.index
		tc, sc := th.text, th.sub
		if sel {
			fillRoundedGradient(img, l*d, t*d, r*d, b*d, 9*d, selGrad, 1)
			tc, sc = colWhite, Col{255, 255, 255, 0.85}
		} else {
			top, bot, edge := hex("#ffffff"), hex("#e9ebef"), rgba(0, 0, 0, 0.22)
			if th.dark {
				top, bot, edge = hex("#4a4d54"), hex("#383b41"), rgba(0, 0, 0, 0.6)
			}
			fillRoundedGradient(img, l*d, t*d, r*d, b*d, 9*d, []GradStop{{0, top}, {1, bot}}, 1)
			strokeRounded(img, l*d, t*d, r*d, b*d, 9*d, 1.5, edge, 1)
		}
		f := face(W600, 14)
		mid := (t + b) / 2
		if sub != "" {
			mid -= 6
		}
		tw := textWidth(f, label)
		lx := (l+r)/2*d - tw/2
		if i == 0 {
			// the sparkle before Genius
			const iw = 14.0
			lx += iw / 2 * d
			drawSparkle(img, lx/d-iw+5, mid, tc)
		}
		drawText(img, f, label, lx, (mid+lineCapHeight(14)/2)*d, tc)
		if sub != "" {
			sf := face(W500, 9.5)
			drawText(img, sf, sub, (l+r)/2*d-textWidth(sf, sub)/2, (mid+16)*d, sc)
		}
	}
	button(0, bx0, y0+119, bx1, y0+153, "Genius", "50 songs from your library, at random")
	button(1, bx0, y0+159, bx0+half, y0+183, "Go to Music", "")
	button(2, bx1-half, y0+159, bx1, y0+183, "Not Now", "")
	s.cache, s.cacheIx, s.cacheDark, s.cacheArt = img, s.index, th.dark, art != nil
	return img
}

// drawSparkle: a four-pointed star with a small one at its top right,
// centred on (cx, cy) in CSS px.
func drawSparkle(img *image.RGBA, cx, cy float64, c Col) {
	d := DPR
	dc := gg.NewContextForRGBA(img)
	dc.SetColor(c8(c))
	star := func(x, y, r float64) {
		k := r * 0.3
		pts := [][2]float64{{0, -r}, {k, -k}, {r, 0}, {k, k}, {0, r}, {-k, k}, {-r, 0}, {-k, -k}}
		for i, p := range pts {
			if i == 0 {
				dc.MoveTo((x+p[0])*d, (y+p[1])*d)
			} else {
				dc.LineTo((x+p[0])*d, (y+p[1])*d)
			}
		}
		dc.ClosePath()
		dc.Fill()
	}
	star(cx, cy+0.5, 5.5)
	star(cx+5, cy-4.5, 2.4)
}

// drawEnd: the card in the middle, fading in (v: 1 hidden, 0 shown).
func (ui *IpodUI) drawEnd(dst *image.RGBA, s *Sheet, v float64, now time.Time) {
	// the screen dims under the status bar, which stays as it is
	fillRect(dst, 0, px(20), 640, 480-px(20), rgba(0, 0, 0, 0.45*(1-v)))
	img := s.renderEnd(ui)
	x := (640 - img.Bounds().Dx()) / 2
	y := px(31) + px(10*v)
	blit(dst, img, x, y, 1-v)
}
