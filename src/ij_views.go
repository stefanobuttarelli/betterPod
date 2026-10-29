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
	// Column: the menu the right column shows for this row (two-column
	// menus); ColumnOpen: A / RIGHT move into it (else A does OnSelect)
	Column     func() *View
	ColumnOpen bool

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
	OnLongPress    func() // instead of LongPress's sheet (Y or holding A)
	Value          string // right-aligned value (settings)
	Checked        bool   // checkmark (submenus)
	NoArrow        bool   // no chevron on the selected row
	Duration       string // track length, right-aligned (Cover Flow track list)
	QueueMark      string // queue icon before the title ("queue_next"/"queue_last"); songs look it up
	SearchField    bool   // the search bar on top: Label is the text, Placeholder shows when empty, Value a note
	Gear           func() // (search bar) a round gear button at its right, reached with RIGHT
	Placeholder    string
	Progress       float64 // > 0: a progress bar under the text (0..1), for a loading row
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
	gearOn           bool   // the cursor is on the search bar's gear button
	refresh          func() // reload its data (views that read it in the background)
	onClose          func() // when it leaves the stack (stop its background work)
	ID               string
	Title            string
	Kind             ViewKind
	Items            []Item
	Build            func() []Item // rebuilds Items (refreshBuilt)
	Empty            string        // message shown when there are no items
	DisableLongPress bool
	TrackList        bool      // Cover Flow back: durations, play mark, scrollbar
	scrolledAt       time.Time // last scroll (the TrackList scrollbar shows for a moment)
	Index            int
	Scroll           float64 // CSS px
	Custom           CustomView

	enter     *Anim
	hide      *Anim
	exiting   bool
	firstIn   bool
	cache     *image.RGBA
	marqueeOn bool // the selected row is scrolling its title
	miniRes   bool // the cache was drawn leaving room for the Now Playing block
	marqIdx   int
	marqSince time.Time
	cacheW    int
	dirty     bool

	lastRender time.Time

	// two columns (Settings): the right one shows the selected row's own
	// menu (Item.Column); colParent: this menu is that column, with the
	// cursor in it (ij_columns.go)
	columns   bool
	colParent *View
	noCursor  bool // drawn without the cursor (the column it isn't in)
	inactive  bool // its cursor grey (the cursor is in the other column)
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
	if it.SearchField {
		return searchBarH
	}
	if it.Progress > 0 {
		return 68
	}
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

// selGradInactive: the cursor of the column the cursor isn't in
var selGradInactive = []GradStop{{0, rgb(168, 176, 186)}, {1, rgb(122, 131, 142)}}
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
	tops, total := v.rowTops()
	scrollable := total > hCSS+0.5
	dfont := face(W600, 14)
	capH := lineCapHeight(14)
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
		sel := i == v.Index && !v.noCursor
		if it.SearchField {
			drawSearchBar(dst, it, y, wCSS, sel && !(v.gearOn && it.Gear != nil), sel && v.gearOn && it.Gear != nil)
			continue
		}
		if it.Progress > 0 {
			// a loading row: the text, and the bar under it
			f := face(W700, 14)
			_, a14 := lineBox(14)
			_, a12 := lineBox(12)
			drawText(dst, f, ellipsize(f, it.Label, (wCSS-24)*DPR), 12*DPR, (y+8+a14)*DPR, th.text)
			sf := face(W400, 12)
			drawText(dst, sf, ellipsize(sf, it.Sublabel, (wCSS-24)*DPR), 12*DPR, (y+27+a12)*DPR, th.sub)
			ui.drawProgressBar(dst, 12, y+46, wCSS-24, it.Progress*100, false)
			continue
		}
		if v.TrackList && i%2 == 1 && !sel {
			// every other row on a very light grey, like the iPod
			fillRect(dst, 0, px(y), px(wCSS), px(y+rh)-px(y), th.stripe)
		}
		if sel {
			g := selGrad
			if v.inactive {
				g = selGradInactive
			}
			fillVGradient(dst, 0, px(y), px(wCSS), px(y+rh)-px(y), g)
		}
		x := 0.0
		if v.TrackList {
			x = trackListInset // the text lines up with the album title above
		}
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
		if v.TrackList && scrollable {
			right -= sbW
		}
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
			// the label is never cut for the value: the value takes what
			// the label leaves, shortened, and goes when too little is left
			valueW = textWidth(vfont, it.Value)/DPR + 12
			room := right - x - textWidth(lfont, it.Label)/DPR - 16
			if valueW > room {
				valueW = room
				if valueW < 40 { // (too little for "Ab…")
					valueW = 0
				}
			}
			right -= valueW
		}
		playing := !sel && it.isSong() && ui.isPlayingTrack(it.Tracks[it.TrackIdx])
		durW := 0.0
		mark := false
		if v.TrackList {
			// the playing track gets a play mark before its title instead
			// of the speaker at the right
			playing = false
			if it.isSong() && ui.isCurrentTrack(it.Tracks[it.TrackIdx]) {
				mark = true
				x += playMarkW
			}
			if it.Duration != "" {
				durW = textWidth(dfont, it.Duration)/DPR + 10
				right -= durW
			}
		}
		if playing {
			right -= 24
		}
		// a song added to the queue by hand: its icon before the title
		qmark, qx := it.QueueMark, x
		if qmark == "" && it.isSong() {
			qmark = queuedMarks[it.Tracks[it.TrackIdx]]
		}
		if qmark != "" {
			x += 28
		}
		padRight := 16.0
		if showArrow || it.Value != "" || it.Checked || durW > 0 {
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
		if qmark != "" {
			drawQueueMark(dst, qmark, qx+6, ty+4+lasc*0.62, tc)
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
			if it.Value != "" && valueW > 0 {
				vc := th.sub
				if sel {
					vc = colWhite
				}
				val := ellipsize(vfont, it.Value, (valueW-12)*DPR)
				vw := textWidth(vfont, val) / DPR
				drawText(dst, vfont, val, (rx-vw)*DPR, (ty+4+lasc)*DPR, vc)
			}
		}
		if v.TrackList {
			if mark {
				// play mark: a small triangle, black (white when selected);
				// paused, two bars instead
				cy := ty + 4 + lasc - capH/2
				if p := ui.app.Playing; p != nil && p.State == StatePaused {
					x0 := x - playMarkW + 5
					bar := []GradStop{{0, tc}, {1, tc}}
					fillRoundedGradient(dst, x0*DPR, (cy-4.5)*DPR, (x0+2.6)*DPR, (cy+4.5)*DPR, 0, bar, 1)
					fillRoundedGradient(dst, (x0+5)*DPR, (cy-4.5)*DPR, (x0+7.6)*DPR, (cy+4.5)*DPR, 0, bar, 1)
				} else {
					fillTriangle(dst, (x-playMarkW+5)*DPR, (cy-4.5)*DPR, (x-playMarkW+5)*DPR, (cy+4.5)*DPR, (x-playMarkW+12.5)*DPR, cy*DPR, tc)
				}
			}
			if it.Duration != "" {
				rx := right + durW - 8
				dw := textWidth(dfont, it.Duration) / DPR
				drawText(dst, dfont, it.Duration, (rx-dw)*DPR, (ty+4+lasc)*DPR, tc)
			}
		}
		if showArrow {
			blit(dst, ui.asset("arrow_right"), px(wCSS-24), px(y+(rh-24)/2)+1, 1)
		}
		if playing {
			blit(dst, ui.themed("volume_full_16"), px(wCSS-8-16), px(y+(rh-16)/2), 1)
		}
	}
	if v.TrackList && scrollable {
		if a := v.scrollbarAlpha(animNow()); a > 0 {
			drawScrollbar(dst, wCSS-sbW, 0, sbW, hCSS, v.Scroll, total, a)
		}
	}
}

// Cover Flow track list: width of the play mark before the playing title
// and of the scrollbar column (CSS px).
const (
	playMarkW      = 14.0
	sbW            = 9.0
	trackListInset = 4.0
)

// The scrollbar shows while the list scrolls: fully for 0.9 s after the
// last move, then it fades out over 0.35 s.
const (
	sbHold = 900 * time.Millisecond
	sbFade = 350 * time.Millisecond
)

func (v *View) scrollbarAlpha(now time.Time) float64 {
	if v.scrolledAt.IsZero() {
		return 0
	}
	d := now.Sub(v.scrolledAt)
	if d <= sbHold {
		return 1
	}
	if d >= sbHold+sbFade {
		return 0
	}
	return 1 - easeCSS(float64(d-sbHold)/float64(sbFade))
}

// scrollbarAnimating: the scrollbar is still on screen (frames are needed
// until it has faded out).
func (v *View) scrollbarAnimating(now time.Time) bool {
	return !v.scrolledAt.IsZero() && now.Sub(v.scrolledAt) < sbHold+sbFade
}

// drawScrollbar draws the iPod scrollbar in the column x..x+w (CSS px):
// a thin rounded track with a 1 px border and a grey glossy thumb sized
// by the visible fraction of the list.
func drawScrollbar(dst *image.RGBA, x, y, w, h, scroll, total, alpha float64) {
	bw := 6.0
	x0 := x + (w-bw)/2 - 1
	y0, y1 := y+3, y+h-3
	track := []GradStop{{0, hex("#e4e4e4")}, {1, hex("#f7f7f7")}}
	border := hex("#b4b4b4")
	thumb := []GradStop{{0, hex("#a8a8a8")}, {0.5, hex("#8a8a8a")}, {1, hex("#767676")}}
	if th.dark {
		track = []GradStop{{0, hex("#26282c")}, {1, hex("#303237")}}
		border = hex("#45484e")
		thumb = []GradStop{{0, hex("#a3a6ac")}, {0.5, hex("#868990")}, {1, hex("#72757b")}}
	}
	d := DPR
	fillRoundedGradient(dst, x0*d, y0*d, (x0+bw)*d, y1*d, bw/2*d, track, alpha)
	strokeRounded(dst, x0*d, y0*d, (x0+bw)*d, y1*d, bw/2*d, 1, border, alpha)
	th0 := (y1 - y0) * h / total
	if th0 < 14 {
		th0 = 14
	}
	maxS := total - h
	f := 0.0
	if maxS > 0 {
		f = math.Min(1, math.Max(0, scroll/maxS))
	}
	ty := y0 + (y1-y0-th0)*f
	// the thumb's gradient runs across it (left to right), like a rod
	tw := bw - 1
	tx := x0 + 0.5
	fillRoundedGradientH(dst, tx*d, ty*d, (tx+tw)*d, (ty+th0)*d, tw/2*d, thumb, alpha)
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

// ---- the search bar ----

const searchBarH = 46.0

// searchField: the search bar row for a search on *term: the text (with
// the caret while typing), the placeholder when empty, a note on the right
// (how many results).
func (ui *IpodUI) searchField(term *string, placeholder, note string, edit func(), long []SheetOption) Item {
	text := *term
	if ui.keyboardEditing(term) {
		text = ui.withCaret(term)
		note = ""
	}
	return Item{SearchField: true, Label: text, Placeholder: placeholder, Value: note, OnSelect: edit, LongPress: long, NoArrow: true}
}

// drawSearchBar: a bar the width of the list, a little darker than it,
// holding a white rounded field with a magnifying glass, like the search
// bar of the first iPhone. Selected, the field gets a blue ring.
func drawSearchBar(dst *image.RGBA, it *Item, y, wCSS float64, sel, gearSel bool) {
	d := DPR
	h := searchBarH
	top, bot, line := hex("#dde2e8"), hex("#c3cad3"), hex("#a5adb8")
	field, edge, text, dim := colWhite, hex("#9aa3ae"), rgb(20, 22, 26), rgb(150, 155, 162)
	if th.dark {
		top, bot, line = hex("#33363c"), hex("#26282d"), hex("#141518")
		field, edge, text, dim = hex("#1b1c20"), hex("#4a4e56"), rgb(236, 237, 240), rgb(120, 124, 130)
	}
	fillVGradient(dst, 0, px(y), px(wCSS), px(y+h)-px(y), []GradStop{{0, top}, {1, bot}})
	fillRect(dst, 0, px(y+h)-1, px(wCSS), 1, line)
	x0, x1 := 8.0, wCSS-8
	fy0, fy1 := y+8, y+h-8
	r := (fy1 - fy0) / 2
	if it.Gear != nil {
		// the round button at the right, as tall as the field
		x1 -= fy1 - fy0 + 8
		drawGearButton(dst, x1+8+r, (fy0+fy1)/2, r, gearSel)
	}
	if sel {
		// the focus ring
		fillRoundedGradient(dst, (x0-2.5)*d, (fy0-2.5)*d, (x1+2.5)*d, (fy1+2.5)*d, (r+2.5)*d,
			[]GradStop{{0, rgba(90, 170, 255, 0.9)}, {1, rgba(40, 120, 220, 0.9)}}, 1)
	}
	fillRoundedGradient(dst, x0*d, fy0*d, x1*d, fy1*d, r*d, []GradStop{{0, field}, {1, field}}, 1)
	// a soft inner shadow along the top
	fillRoundedGradient(dst, x0*d, fy0*d, x1*d, (fy0+5)*d, r*d, []GradStop{{0, rgba(0, 0, 0, 0.10)}, {1, rgba(0, 0, 0, 0)}}, 1)
	strokeRounded(dst, x0*d, fy0*d, x1*d, fy1*d, r*d, 1.5, edge, 1)
	cy := (fy0 + fy1) / 2
	drawMagnifier(dst, x0+10, cy, 13, dim)
	tx := x0 + 30
	right := x1 - 12
	f := face(W500, 14)
	if it.Value != "" {
		nf := face(W500, 12)
		nw := textWidth(nf, it.Value) / d
		drawText(dst, nf, it.Value, (right-nw)*d, (cy+lineCapHeight(12)/2)*d, dim)
		right -= nw + 10
	}
	maxW := (right - tx) * d
	if it.Label == "" || it.Label == "|" {
		// empty: the placeholder (after the caret while typing)
		px0 := tx
		if it.Label == "|" {
			drawText(dst, f, "|", tx*d, (cy+lineCapHeight(14)/2)*d, text)
			px0 += textWidth(f, "|")/d + 2
		}
		drawText(dst, f, ellipsize(f, it.Placeholder, (right-px0)*d), px0*d, (cy+lineCapHeight(14)/2)*d, dim)
		return
	}
	s := it.Label
	// while typing a long text, its end (where the caret is) stays in view
	for textWidth(f, s) > maxW && len([]rune(s)) > 1 {
		s = string([]rune(s)[1:])
		if textWidth(f, "…"+s) <= maxW {
			s = "…" + s
			break
		}
	}
	drawText(dst, f, s, tx*d, (cy+lineCapHeight(14)/2)*d, text)
}

// drawGearButton: a round glossy button with a gear (CSS px centre and
// radius); selected, it turns blue like the field's ring.
func drawGearButton(dst *image.RGBA, cx, cy, r float64, sel bool) {
	d := DPR
	top, bot, edge, glyph := hex("#fdfdfe"), hex("#d9dee5"), hex("#9aa3ae"), rgb(96, 104, 116)
	if th.dark {
		top, bot, edge, glyph = hex("#4a4e56"), hex("#2c2f35"), hex("#141518"), rgb(220, 222, 226)
	}
	if sel {
		fillRoundedGradient(dst, (cx-r-2.5)*d, (cy-r-2.5)*d, (cx+r+2.5)*d, (cy+r+2.5)*d, (r+2.5)*d,
			[]GradStop{{0, rgba(90, 170, 255, 0.9)}, {1, rgba(40, 120, 220, 0.9)}}, 1)
		top, bot, glyph = hex("#6cb8ff"), hex("#1f74d8"), colWhite
	}
	fillRoundedGradient(dst, (cx-r)*d, (cy-r)*d, (cx+r)*d, (cy+r)*d, r*d, []GradStop{{0, top}, {1, bot}}, 1)
	fillRoundedGradient(dst, (cx-r+1.5)*d, (cy-r+1.5)*d, (cx+r-1.5)*d, cy*d, (r-1.5)*d,
		[]GradStop{{0, rgba(255, 255, 255, 0.45)}, {1, rgba(255, 255, 255, 0.05)}}, 1)
	if !sel {
		strokeRounded(dst, (cx-r)*d, (cy-r)*d, (cx+r)*d, (cy+r)*d, r*d, 1.5, edge, 1)
	}
	drawGear(dst, cx*d, cy*d, r*0.62*d, glyph)
}

// drawGear: a gear of 8 teeth with a hole (device px).
func drawGear(dst *image.RGBA, cx, cy, R float64, c Col) {
	var pts [][2]float64
	const n = 8
	for i := 0; i < n*4; i++ {
		a := (float64(i) + 0.5) / float64(n*4) * 2 * math.Pi
		rr := R
		if (i/2)%2 == 1 {
			rr = R * 0.74
		}
		pts = append(pts, [2]float64{cx + rr*math.Cos(a), cy + rr*math.Sin(a)})
	}
	aaPolyHole(dst, pts, cx, cy, R*0.32, c)
}

// aaPolyHole: a filled polygon with a round hole in it.
func aaPolyHole(dst *image.RGBA, pts [][2]float64, hx, hy, hr float64, c Col) {
	r := polyBox(dst, pts) // (the hole is inside the polygon)
	if r.Empty() {
		return
	}
	ras := aaRaster(r)
	defer aaRasters.Put(ras)
	ox, oy := float64(r.Min.X), float64(r.Min.Y)
	ras.MoveTo(float32(pts[0][0]-ox), float32(pts[0][1]-oy))
	for _, p := range pts[1:] {
		ras.LineTo(float32(p[0]-ox), float32(p[1]-oy))
	}
	ras.ClosePath()
	// the hole, the other way round (non-zero winding leaves it empty)
	const m = 24
	hx, hy = hx-ox, hy-oy
	ras.MoveTo(float32(hx+hr), float32(hy))
	for i := m - 1; i >= 1; i-- {
		a := float64(i) / m * 2 * math.Pi
		ras.LineTo(float32(hx+hr*math.Cos(a)), float32(hy+hr*math.Sin(a)))
	}
	ras.ClosePath()
	ras.Draw(dst, r.Add(dst.Bounds().Min), image.NewUniform(c8(c)), image.Point{})
}
