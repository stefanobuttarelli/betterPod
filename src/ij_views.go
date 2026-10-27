package main

// ij_views.go — menu views and list rendering, as in ipod.js
// (components/views: SelectableList, SelectableListOption).

import (
	"image"
	"math"
	"time"
)

type ViewKind int

const (
	KindSplit     ViewKind = iota // menu on the left half, preview on the right
	KindFull                      // full-screen list / screen
	KindCoverFlow                 // the Cover Flow screen
)

// Item is one row of a list (SelectableListOption in ipod.js).
type Item struct {
	Label          string
	Sublabel       string
	Album          *Album // thumbnail (album rows)
	Icon           string // thumbnail asset instead of album art
	Preview        PreviewKind
	OnSelect       func()
	Tracks         []*Track // song rows: the list to play…
	TrackIdx       int      // …and this row's track
	ShowNowPlaying bool
	LongPress      []SheetOption
	Value          string // right-aligned value (settings)
	Checked        bool   // checkmark (submenus)
	NoArrow        bool   // no chevron on the selected row
}

// How a row behaves when selected (centerClick):
//   - Tracks set: play Tracks from TrackIdx, then show Now Playing if
//     ShowNowPlaying (holding A opens the LongPress sheet instead)
//   - otherwise OnSelect runs (it usually pushes a view or opens a sheet;
//     a view's Build is called again when it comes back on top)
//
// Preview is what the right half shows while the row is selected (split
// views only). Value and Checked are for settings rows; NoArrow hides the
// chevron of rows that act in place. Album and Icon are the row's
// thumbnail (album art, or an asset for the menus that have icons), and
// Sublabel is the grey second line.
//
// isSong reports whether selecting the row plays a track.
func (it *Item) isSong() bool { return it.Tracks != nil }

// CustomView is a full-screen view that draws itself (Now Playing, games…).
type CustomView interface {
	Animating(now time.Time) bool
	OnCenter(ui *IpodUI)
	OnScroll(ui *IpodUI, dir int, velocity float64)
	Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time)
}

type View struct {
	ID               string
	Title            string
	Kind             ViewKind
	Items            []Item
	Build            func() []Item // rebuilds Items (refreshBuilt)
	Empty            string        // message shown when there are no items
	DisableLongPress bool
	Index            int
	Scroll           float64 // CSS px
	Custom           CustomView

	enter     *Anim
	hide      *Anim
	exiting   bool
	firstIn   bool
	cache     *image.RGBA
	marqueeOn bool // the selected row is scrolling its title
	marqIdx   int
	marqSince time.Time
	cacheW    int
	dirty     bool

	lastRender time.Time
}

// row metrics (CSS px)
//
// .option { padding: 4px 8px; min-height: 48px with an image }
// .label  { font: 700 14px Inter }
// .sublabel { font: 400 12px Inter; margin-top: 4px }
//
// The list scrolls so the selected row stays fully visible.
//
// Selected row: blue gradient, white text, chevron at the right; the
// playing song shows a speaker icon instead.
//
// Values (settings) are right-aligned and take at most 60% of the free
// width; a checkmark marks the current choice in submenus.
//
// Long titles of the selected row scroll (marquee) when they don't fit.
//
//
//

// hasImages reports whether the list shows album thumbnails.
func (v *View) hasImages() bool {
	for i := range v.Items {
		if v.Items[i].Album != nil {
			return true
		}
	}
	return false
}

// rowHeight returns the height of a row in CSS px.
func rowHeight(it *Item) float64 {
	lh, _ := lineBox(14)
	h := 8 + lh
	if it.Sublabel != "" {
		slh, _ := lineBox(12)
		h += slh + 4 - 4
	}
	if (it.Album != nil || it.Icon != "") && h < 48 {
		h = 48
	}
	return h
}

func (v *View) rowTops() ([]float64, float64) {
	tops := make([]float64, len(v.Items))
	y := 0.0
	for i := range v.Items {
		tops[i] = y
		y += rowHeight(&v.Items[i])
	}
	return tops, y
}

// ensureVisible clamps the selection and scrolls it into view (block: nearest).
func (v *View) ensureVisible(viewportH float64) {
	if len(v.Items) == 0 {
		v.Scroll = 0
		return
	}
	tops, total := v.rowTops()
	if v.Index >= len(tops) {
		v.Index = len(tops) - 1
	}
	top := tops[v.Index]
	bot := top + rowHeight(&v.Items[v.Index])
	if top < v.Scroll {
		v.Scroll = top
	} else if bot > v.Scroll+viewportH {
		v.Scroll = bot - viewportH
	}
	maxScroll := math.Max(0, total-viewportH)
	if v.Scroll > maxScroll {
		v.Scroll = maxScroll
	}
	if v.Scroll < 0 {
		v.Scroll = 0
	}
}

// selected row
var selGrad = []GradStop{{0, rgb(60, 184, 255)}, {1, rgb(52, 122, 181)}}
var subGray = rgb(100, 100, 100)

// renderList draws a list view at full device resolution into dst: rows
// with optional thumbnail, label, sublabel, value, checkmark and chevron.
func (ui *IpodUI) renderList(v *View, dst *image.RGBA) {
	v.marqueeOn = false
	fillRect(dst, 0, 0, dst.Bounds().Dx(), dst.Bounds().Dy(), th.bg)
	wCSS := float64(dst.Bounds().Dx()) / DPR
	hCSS := float64(dst.Bounds().Dy()) / DPR
	if len(v.Items) == 0 {
		ui.renderEmpty(dst, v.Empty)
		return
	}
	v.ensureVisible(hCSS)
	tops, _ := v.rowTops()
	lfont := face(W700, 14)
	sfont := face(W400, 12)
	_, lasc := lineBox(14)
	slh, sasc := lineBox(12)
	llh, _ := lineBox(14)

	for i := range v.Items {
		it := &v.Items[i]
		rh := rowHeight(it)
		y := tops[i] - v.Scroll
		if y+rh < 0 || y > hCSS {
			continue
		}
		sel := i == v.Index
		if sel {
			fillVGradient(dst, 0, px(y), px(wCSS), px(y+rh)-px(y), selGrad)
		}
		x := 0.0
		if it.Album != nil || it.Icon != "" {
			var thumb *image.RGBA
			if it.Icon != "" {
				thumb = ui.asset(it.Icon)
			} else {
				thumb = ui.thumb(it.Album, 48)
			}
			blit(dst, thumb, 0, px(y+(rh-48)/2), 1)
			x = 52
		}

		right := wCSS
		showArrow := sel && !it.NoArrow
		if showArrow {
			right -= 24
		}
		if it.Checked {
			right -= 22
		}
		valueW := 0.0
		vfont := face(W400, 14)
		if it.Value != "" {
			valueW = textWidth(vfont, it.Value)/DPR + 12
			if valueW > (right-x)*0.6 {
				valueW = (right - x) * 0.6
			}
			right -= valueW
		}
		playing := !sel && it.isSong() && ui.isPlayingTrack(it.Tracks[it.TrackIdx])
		if playing {
			right -= 24
		}
		padRight := 16.0
		if showArrow || it.Value != "" || it.Checked {
			padRight = 0
		}
		textW := right - x - padRight - 8

		// vertically centre the label (+ sublabel) block, like a flex
		// column with justify-content: center
		//
		blockH := 8 + llh
		if it.Sublabel != "" {
			blockH += 4 + slh - 4
		}
		ty := y + (rh-blockH)/2
		tc := th.text
		sc := th.sub
		if sel {
			tc, sc = colWhite, colWhite
		}
		if sel && textWidth(lfont, it.Label)/DPR > textW {
			// selected and too long: scroll the title
			now := animNow()
			if v.marqIdx != i || v.marqSince.IsZero() {
				v.marqIdx, v.marqSince = i, now
			}
			drawMarquee(dst, lfont, it.Label, x+4, ty+4+lasc, textW, tc, v.marqSince, now)
			v.marqueeOn = true
		} else {
			label := ellipsize(lfont, it.Label, textW*DPR)
			drawText(dst, lfont, label, (x+4)*DPR, (ty+4+lasc)*DPR, tc)
		}
		if it.Sublabel != "" {
			sy := ty + llh + 8 - 4
			sub := ellipsize(sfont, it.Sublabel, textW*DPR)
			drawText(dst, sfont, sub, (x+4)*DPR, (sy+sasc)*DPR, sc)
		}
		if it.Value != "" || it.Checked {
			rx := wCSS - 16
			if showArrow {
				rx = wCSS - 24
			}
			if it.Checked {
				ck := "check"
				if sel || th.dark {
					ck = "check_white"
				}
				rx -= 14
				blit(dst, ui.asset(ck), px(rx), px(y+(rh-14)/2), 1)
				rx -= 8
			}
			if it.Value != "" {
				vc := th.sub
				if sel {
					vc = colWhite
				}
				val := ellipsize(vfont, it.Value, (valueW-12)*DPR)
				vw := textWidth(vfont, val) / DPR
				drawText(dst, vfont, val, (rx-vw)*DPR, (ty+4+lasc)*DPR, vc)
			}
		}
		if showArrow {
			blit(dst, ui.asset("arrow_right"), px(wCSS-24), px(y+(rh-24)/2)+1, 1)
		}
		if playing {
			blit(dst, ui.themed("volume_full_16"), px(wCSS-8-16), px(y+(rh-16)/2), 1)
		}
	}
}

// renderEmpty: centred grey message for empty lists.
func (ui *IpodUI) renderEmpty(dst *image.RGBA, msg string) {
	if msg == "" {
		msg = "Nothing to see here"
	}
	f := face(W500, 16)
	w := float64(dst.Bounds().Dx())
	h := float64(dst.Bounds().Dy())
	_, asc := lineBox(16)
	lh, _ := lineBox(16)
	tw := textWidth(f, msg)
	drawText(dst, f, msg, (w-tw)/2, h/2-lh*DPR/2+asc*DPR, th.empty)
}

// marqueeActive is set while a scrolling (marquee) title is on screen, so
// the main loop keeps redrawing at 30 fps.
var marqueeActive bool
