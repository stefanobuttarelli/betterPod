package main

// ij_sheet.go — the ipod.js action sheet (options shown on a long press),
// sliding up from the bottom with a "Cancel" option at the end.

import (
	"image"
	"time"

	"github.com/fogleman/gg"

	"golang.org/x/image/font"
)

type SheetOption struct {
	Label    string
	Icon     string // "queue_next", "queue_last": drawn before the label
	Danger   bool   // a destructive action (Delete): red
	Selected bool
	OnSelect func()
}

type Sheet struct {
	title     string // what the options are about (a song, an album…)
	opts      []SheetOption
	index     int
	anim      *Anim // 0 shown, 1 hidden below the screen
	exiting   bool
	cache     *image.RGBA
	cacheIx   int
	cacheDark bool
}

// The sheet is a small floating card at the bottom of the screen, over a
// dimmed screen: an optional title (what it is about), the options in
// 32 px rows, and Cancel on a card of its own; the selected option is the
// list's blue.

func (ui *IpodUI) showSheet(opts []SheetOption) { ui.showSheetTitled("", opts) }

func (ui *IpodUI) showSheetTitled(title string, opts []SheetOption) {
	all := append([]SheetOption{}, opts...)
	all = append(all, SheetOption{Label: "Cancel"})
	s := &Sheet{title: title, opts: all, anim: newAnim(1), cacheIx: -1}
	for i, o := range all {
		if o.Selected {
			s.index = i
		}
	}
	s.anim.To(0, durSlide, easeInOut, 0)
	ui.sheets = append(ui.sheets, s)
	ui.dirty = true
}

func (ui *IpodUI) hideSheet() {
	for i := len(ui.sheets) - 1; i >= 0; i-- {
		s := ui.sheets[i]
		if !s.exiting {
			s.exiting = true
			s.anim.To(1, durSlide, easeInOut, 0)
			break
		}
	}
	ui.dirty = true
}

func (s *Sheet) scroll(ui *IpodUI, dir int, velocity float64) {
	if s.exiting {
		return
	}
	ni := s.index + dir*skipFor(velocity)
	if ni < 0 {
		ni = 0
	}
	if ni > len(s.opts)-1 {
		ni = len(s.opts) - 1
	}
	if ni != s.index {
		s.index = ni
		ui.dirty = true
	}
}

func (s *Sheet) selectCurrent(ui *IpodUI) {
	if s.exiting {
		return
	}
	f := s.opts[s.index].OnSelect
	ui.hideSheet()
	if f != nil {
		f()
	}
}

const (
	sheetRowH   = 32.0
	sheetTitleH = 26.0
	sheetMargin = 12.0 // from the sides and the bottom
	sheetGap    = 7.0  // above Cancel
)

// heightCSS: the cards, without the bottom margin.
func (s *Sheet) heightCSS() float64 {
	h := float64(len(s.opts)-1)*sheetRowH + sheetGap + sheetRowH
	if s.title != "" {
		h += sheetTitleH
	}
	return h + 4 // the shadow under the last card
}

func sheetCard(img *image.RGBA, x0, y0, x1, y1 float64) {
	d := DPR
	r := 11.0
	// soft shadow
	for i, a := range []float64{0.10, 0.07, 0.04} {
		g := float64(i)
		fillRoundedGradient(img, (x0-g)*d, (y0+1-g)*d, (x1+g)*d, (y1+2+g)*d, (r+g)*d, []GradStop{{0, rgba(0, 0, 0, a)}, {1, rgba(0, 0, 0, a)}}, 1)
	}
	top, bot, edge, hi := hex("#ffffff"), hex("#eceef2"), rgba(0, 0, 0, 0.28), rgba(255, 255, 255, 0.9)
	if th.dark {
		top, bot, edge, hi = hex("#3b3e44"), hex("#2b2e33"), rgba(0, 0, 0, 0.7), rgba(255, 255, 255, 0.10)
	}
	fillRoundedGradient(img, x0*d, y0*d, x1*d, y1*d, r*d, []GradStop{{0, top}, {1, bot}}, 1)
	strokeRounded(img, x0*d, y0*d, x1*d, y1*d, r*d, 1.5, edge, 1)
	fillRect(img, px(x0+r), px(y0)+2, px(x1-x0-2*r), 1, hi)
}

func (s *Sheet) render(ui *IpodUI) *image.RGBA {
	if s.cache != nil && s.cacheIx == s.index && s.cacheDark == th.dark {
		return s.cache
	}
	d := DPR
	hCSS := s.heightCSS()
	img := newCanvas(640, px(hCSS))
	x0, x1 := sheetMargin, 320-sheetMargin
	sep := rgba(0, 0, 0, 0.09)
	if th.dark {
		sep = rgba(255, 255, 255, 0.07)
	}
	f := face(W600, 15)
	fc := face(W700, 15)
	tf := face(W500, 11)
	n := len(s.opts) - 1 // without Cancel
	y := 0.0
	cardH := float64(n) * sheetRowH
	if s.title != "" {
		cardH += sheetTitleH
	}
	row := func(i int, ry float64, label string, icon string, lf font.Face, last bool) {
		tc := th.text
		danger := s.opts[i].Danger
		if danger {
			tc = hex("#e0352b")
		}
		if i == s.index {
			// the selected option: the list's blue (red for a destructive
			// one), inset in the card
			g := selGrad
			if danger {
				g = []GradStop{{0, hex("#ff6b5e")}, {1, hex("#d42a1e")}}
			}
			fillRoundedGradient(img, (x0+4)*d, (ry+3)*d, (x1-4)*d, (ry+sheetRowH-3)*d, 8*d, g, 1)
			tc = colWhite
		}
		label = ellipsize(lf, label, (x1-x0-24)*d)
		tw := textWidth(lf, label)
		lx := 320 - tw/2
		if icon != "" {
			const iw, gap = 18.0, 8.0
			lx = 320 - (tw+(iw+gap)*d)/2
			drawQueueIcon(img, lx/d, ry+sheetRowH/2, icon == "queue_next", tc)
			lx += (iw + gap) * d
		}
		drawText(img, lf, label, lx, (ry+sheetRowH/2+lineCapHeight(15)/2)*d, tc)
		if !last && i != s.index && i+1 != s.index {
			fillRect(img, px(x0+12), px(ry+sheetRowH)-1, px(x1-x0-24), 1, sep)
		}
	}
	if n > 0 || s.title != "" {
		sheetCard(img, x0, y, x1, y+cardH)
		ry := y
		if s.title != "" {
			t := ellipsize(tf, s.title, (x1-x0-24)*d)
			tw := textWidth(tf, t)
			drawText(img, tf, t, 320-tw/2, (ry+sheetTitleH/2+lineCapHeight(11)/2)*d, th.sub)
			fillRect(img, px(x0), px(ry+sheetTitleH)-1, px(x1-x0), 1, sep)
			ry += sheetTitleH
		}
		for i := 0; i < n; i++ {
			row(i, ry, s.opts[i].Label, s.opts[i].Icon, f, i == n-1)
			ry += sheetRowH
		}
		y += cardH + sheetGap
	}
	sheetCard(img, x0, y, x1, y+sheetRowH)
	row(n, y, s.opts[n].Label, "", fc, true)
	s.cache = img
	s.cacheIx = s.index
	s.cacheDark = th.dark
	return img
}

// drawTextShadow draws text with a soft 1px shadow around it.
func drawTextShadow(dst *image.RGBA, f font.Face, s string, x, y float64, c, sh Col) {
	// text-shadow: 0 1px … approximated by four 1px offsets
	for _, d := range [...][2]float64{{-1, 0}, {1, 0}, {0, -1}, {0, 1}} {
		drawText(dst, f, s, x+d[0], y+d[1], Col{sh.R, sh.G, sh.B, 0.35})
	}
	drawText(dst, f, s, x, y, c)
}

func (ui *IpodUI) drawSheets(dst *image.RGBA, now time.Time) {
	keep := ui.sheets[:0]
	for _, s := range ui.sheets {
		v := s.anim.Get(now)
		if s.exiting && !s.anim.Running() {
			continue
		}
		keep = append(keep, s)
		// the screen dims behind the sheet
		fillRect(dst, 0, 0, 640, 480, rgba(0, 0, 0, 0.32*(1-v)))
		img := s.render(ui)
		h := img.Bounds().Dy()
		rest := 480 - h - px(sheetMargin) + px(4)
		y := rest + int(float64(480-rest)*v+0.5)
		blit(dst, img, 0, y, 1)
	}
	ui.sheets = keep
}

// ---- toast: a short confirmation at the bottom ("Playing Next") ----

var toast = struct {
	text  string
	a     *Anim
	until time.Time
}{a: newAnim(0)}

func (ui *IpodUI) showToast(text string) {
	toast.text = text
	toast.until = time.Now().Add(1600 * time.Millisecond)
	toast.a.To(1, 200*time.Millisecond, easeCSS, 0)
	ui.dirty = true
}

func toastAnimating() bool { return toast.a.Running() }

// toastTick fades the toast out when its time is up (from the loop, so the
// screen needn't be redrawn while it just sits there).
func (ui *IpodUI) toastTick(now time.Time) {
	if toast.a.Target() == 1 && now.After(toast.until) {
		toast.a.To(0, 250*time.Millisecond, easeCSS, 0)
		ui.dirty = true
	}
}

func (ui *IpodUI) drawToast(dst *image.RGBA, now time.Time) {
	a := toast.a.Get(now)
	if a <= 0.001 {
		return
	}
	d := DPR
	f := face(W600, 13)
	tw := textWidth(f, toast.text) / d
	w := tw + 28
	x0 := 160 - w/2
	y0 := 240 - miniPlayerH - 14 - 28 + (1-a)*8 // above the Now Playing block
	fillRoundedGradient(dst, x0*d, y0*d, (x0+w)*d, (y0+28)*d, 14*d, []GradStop{{0, Col{48, 50, 54, 0.9}}, {1, Col{12, 13, 15, 0.9}}}, a)
	strokeRounded(dst, x0*d, y0*d, (x0+w)*d, (y0+28)*d, 14*d, 1.5, rgba(255, 255, 255, 0.18*a), 1)
	drawText(dst, f, toast.text, (x0+14)*d, (y0+14+lineCapHeight(13)/2)*d, Col{255, 255, 255, a})
}

// drawQueueIcon: three lines with an arrow on their left, 18 × 14 CSS px
// from x, centred on cy. Play Next: the first line is the bold one and the
// arrow points up; End of Queue: the last one, the arrow down. Drawn once
// per kind and colour, then blitted.
func drawQueueIcon(dst *image.RGBA, x, cy float64, next bool, c Col) {
	img := queueIconImg(next, c)
	blit(dst, img, px(x), px(cy-8), 1)
}

type queueIconKey struct {
	next bool
	c    Col
}

var queueIcons = map[queueIconKey]*image.RGBA{}

func queueIconImg(next bool, c Col) *image.RGBA {
	k := queueIconKey{next, c}
	if img := queueIcons[k]; img != nil {
		return img
	}
	d := DPR
	img := image.NewRGBA(image.Rect(0, 0, px(20), px(16)))
	dc := gg.NewContextForRGBA(img)
	dc.SetColor(c8(c))
	dc.SetLineCapRound()
	cy := 8.0
	for i := 0; i < 3; i++ {
		y := cy - 5.5 + float64(i)*5.5
		bold := (next && i == 0) || (!next && i == 2)
		w := 1.4
		if bold {
			w = 2.8
		}
		dc.SetLineWidth(w * d)
		dc.MoveTo(8*d, y*d)
		dc.LineTo(18*d, y*d)
		dc.Stroke()
	}
	// the arrow
	ax := 2.5
	y0, y1 := cy+5.5, cy-5.5 // from, to (up)
	dir := 1.0
	if !next {
		y0, y1, dir = y1, y0, -1
	}
	dc.SetLineWidth(1.6 * d)
	dc.MoveTo(ax*d, y0*d)
	dc.LineTo(ax*d, y1*d)
	dc.Stroke()
	dc.SetLineJoinRound()
	dc.MoveTo((ax-2.4)*d, (y1+2.6*dir)*d)
	dc.LineTo(ax*d, y1*d)
	dc.LineTo((ax+2.4)*d, (y1+2.6*dir)*d)
	dc.Stroke()
	queueIcons[k] = img
	return img
}
