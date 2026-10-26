package main

// ij_sheet.go — the ipod.js action sheet (options shown on a long press),
// sliding up from the bottom with a "Cancel" option at the end.

import (
	"image"
	"time"

	"golang.org/x/image/font"
)

type SheetOption struct {
	Label    string
	Selected bool
	OnSelect func()
}

type Sheet struct {
	opts    []SheetOption
	index   int
	anim    *Anim // 0 shown, 1 hidden below the screen
	exiting bool
	cache   *image.RGBA
	cacheIx int
}

// Each option is a 60 CSS px row (a rounded, gradient "button" with a
// shadowed label), over a dark gradient; the selected option is blue.
//
//
//
//

func (ui *IpodUI) showSheet(opts []SheetOption) {
	all := append([]SheetOption{}, opts...)
	all = append(all, SheetOption{Label: "Cancel"})
	s := &Sheet{opts: all, anim: newAnim(1), cacheIx: -1}
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

// heightCSS: 60 CSS px per option plus 16 px of padding.
func (s *Sheet) heightCSS() float64 { return float64(len(s.opts))*60 + 16 }

// action sheet gradients (ipod.js ActionSheet)
var (
	sheetBgGrad     = []GradStop{{0, hex("#bbbcbf")}, {0.1169, hex("#626770")}, {1, hex("#626770")}}
	sheetOptGrad    = []GradStop{{0, hex("#ffffff")}, {0.4844, hex("#cbccce")}, {1, hex("#dfdfdf")}}
	sheetOptSelGrad = []GradStop{{0, hex("#707379")}, {0.5, hex("#3b3f46")}, {1, hex("#363c44")}}
)

func (s *Sheet) render(ui *IpodUI) *image.RGBA {
	if s.cache != nil && s.cacheIx == s.index {
		return s.cache
	}
	hCSS := s.heightCSS()
	img := newCanvas(640, px(hCSS))
	fillVGradient(img, 0, 0, 640, img.Bounds().Dy(), sheetBgGrad)
	f := face(W700, 16)
	lh, asc := lineBox(16)
	y := 8.0
	for i, o := range s.opts {
		// option: margin 4px 19px, border 3px, padding 8px,
		// border-radius 12px
		//
		x0, x1 := 19.0, 301.0
		y0 := (y + 4 + 3 + 4) * DPR
		y1 := y0 + (lh+16+6)*DPR
		grad, border, tc := th.sheetOpt, th.sheetBorder, th.sheetText
		if i == s.index {
			grad, border, tc = selGrad, hex("#ececec"), colWhite
		}
		fillRoundedGradient(img, x0*DPR, y0, x1*DPR, y1, 24, grad, 1)
		strokeRounded(img, x0*DPR, y0, x1*DPR, y1, 24, 6, border, 1)
		tw := textWidth(f, o.Label)
		tx := 320 - tw/2
		by := (11+asc)*DPR + y0
		drawTextShadow(img, f, o.Label, tx, by, tc, th.sheetShadow)
		y += 60
	}
	s.cache = img
	s.cacheIx = s.index
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
		img := s.render(ui)
		h := img.Bounds().Dy()
		y := 480 - h + int(float64(h)*1.1*v+0.5)
		// box-shadow above the sheet
		prof := boxShadowProfile(8, 1)
		for i, a := range prof {
			fillRect(dst, 0, y-1-i, 640, 1, Col{0, 0, 0, a})
		}
		blit(dst, img, 0, y, 1)
	}
	ui.sheets = keep
}
