package main

// ij_columns.go — two-column menus (Settings): the left column is the
// menu, the right one the menu of the row the cursor is on, under its own
// header bar (the left column lies over it, its shadow falling on it).
//
//	A / RIGHT on a row with a column: the cursor moves into it (the left
//	cursor turns grey); B / LEFT: back to the left column.
//	Rows that open a page of their own (About, Bluetooth, Software Update)
//	show a summary there; A opens the page as before.
//
// The column is an ordinary menu pushed on the split stack (colParent set),
// drawn on the right instead of sliding in on the left, so its keys, its
// rows' actions and refreshBuilt work as for any menu.

import (
	"image"
	"time"
)

var columnBuf *image.RGBA

// colPreview: the column shown for the parent's row idx (made again when
// the row changes, its items each time it is drawn).
var colPreview struct {
	parent *View
	idx    int
	v      *View
}

// columnNow: the two-column menu on screen, if any: the parent, the column
// and whether the cursor is in it.
func (ui *IpodUI) columnNow() (parent, col *View, focused bool) {
	if len(ui.full) > 0 || len(ui.split) == 0 {
		return nil, nil, false
	}
	top := ui.split[len(ui.split)-1]
	if top.colParent != nil {
		return top.colParent, top, true
	}
	if !top.columns || top.Index < 0 || top.Index >= len(top.Items) {
		return nil, nil, false
	}
	it := &top.Items[top.Index]
	if it.Column == nil {
		return top, nil, false
	}
	if colPreview.parent != top || colPreview.idx != top.Index || colPreview.v == nil {
		colPreview.parent, colPreview.idx, colPreview.v = top, top.Index, it.Column()
	} else if b := colPreview.v.Build; b != nil {
		colPreview.v.Items = b()
	}
	return top, colPreview.v, false
}

// openColumn: the cursor moves into col, the column of the menu on top.
func (ui *IpodUI) openColumn(col *View) {
	if len(ui.split) == 0 {
		return
	}
	parent := ui.split[len(ui.split)-1]
	if col.Build != nil {
		col.Items = col.Build()
	}
	col.Kind = KindSplit
	col.colParent = parent
	col.enter, col.hide = newAnim(0), newAnim(0)
	col.dirty = true
	parent.inactive, parent.dirty = true, true
	ui.split = append(ui.split, col)
	colPreview.v = nil
	ui.dirty = true
}

// closeColumn: B / LEFT in the column: the cursor goes back to the left.
func (ui *IpodUI) closeColumn(col *View) {
	ui.split = ui.split[:len(ui.split)-1]
	p := col.colParent
	if col.onClose != nil {
		col.onClose()
	}
	p.inactive, p.dirty = false, true
	if p.Build != nil {
		idx := p.Index
		p.Items = p.Build()
		p.Index = max(0, min(idx, len(p.Items)-1))
	}
	colPreview.v = nil
	ui.headerTitle = p.Title
	ui.dirty = true
}

// columnKey: RIGHT into the column, LEFT back out of it.
func (ui *IpodUI) columnKey(k Key) bool {
	if (k != LEFT && k != RIGHT) || ui.keyboard != nil || len(ui.sheets) > 0 || ui.coverUp() != nil {
		return false
	}
	parent, col, focused := ui.columnNow()
	if parent == nil {
		return false
	}
	switch {
	case k == LEFT && focused:
		ui.closeColumn(col)
		return true
	case k == RIGHT && !focused:
		if it := &parent.Items[parent.Index]; it.ColumnOpen && it.OnSelect != nil {
			it.OnSelect()
			return true
		}
	}
	return false
}

// drawColumn: the right column at x (device px): its header bar and title,
// its menu under it (with the cursor only when it's in it).
func (ui *IpodUI) drawColumn(dst *image.RGBA, x int, col *View, focused bool, now time.Time) {
	hb := headerBackground()
	blitOpaque(dst, hb.SubImage(image.Rect(0, 0, 320, px(20))).(*image.RGBA), x, 0)
	f := face(W700, 13)
	_, asc := lineBox(13)
	drawText(dst, f, ellipsize(f, col.Title, 300*DPR/2), float64(x)+6*DPR, (1.5+asc)*DPR, th.headerText)
	buf := newCanvasCached(&columnBuf, 320, 480-px(20))
	col.noCursor = !focused
	ui.renderList(col, buf)
	col.noCursor = false
	blitOpaque(dst, buf, x, px(20))
}

// headerBackground: the header bar's picture (made once per theme).
func headerBackground() *image.RGBA {
	if headerBg == nil {
		headerBg = newCanvas(640, px(20))
		fillVGradient(headerBg, 0, 0, 640, px(20)-px(1), th.header)
		fillRect(headerBg, 0, px(20)-px(1), 640, px(1), th.headerBorder)
	}
	return headerBg
}

// ---- Settings' columns that aren't menus of their own ----

// podcastsMenuView: Podcasts in the main menu, Off / On.
func (ui *IpodUI) podcastsMenuView() *View {
	v := &View{ID: "podcastsMenu", Title: "Podcasts", Kind: KindSplit}
	v.Build = func() []Item {
		var items []Item
		for _, on := range []bool{true, false} {
			items = append(items, Item{Label: onOff(on), Checked: podsInMenu == on, NoArrow: true, Preview: PrevSettings,
				Sublabel: map[bool]string{true: "In the main menu", false: "Episodes stay in Music"}[on],
				OnSelect: func() { ui.setPodsInMenu(on) }})
		}
		return items
	}
	v.Items = v.Build()
	for i, it := range v.Items {
		if it.Checked {
			v.Index = i
		}
	}
	return v
}

// setPodsInMenu: Podcasts in the main menu or not (downloaded episodes
// stay in Music); the main menu's cursor stays on Settings.
func (ui *IpodUI) setPodsInMenu(on bool) {
	if podsInMenu == on {
		return
	}
	podsInMenu = on
	ui.savePrefs()
	for _, h := range ui.split {
		if h.ID == "home" && h.Index >= 2 {
			if podsInMenu {
				h.Index++
			} else {
				h.Index--
			}
		}
	}
	ui.refreshBuilt()
}

// summaryView: a column of facts (a page's summary), A opens the page.
func summaryView(title string, items ...Item) *View {
	for i := range items {
		items[i].NoArrow = true
	}
	return &View{ID: "summary", Title: title, Kind: KindSplit, Items: items}
}

func (ui *IpodUI) aboutSummary() *View {
	a := ui.aboutView().Custom.(*AboutView)
	return summaryView("About", a.list.Items...)
}

func (ui *IpodUI) btSummary() *View {
	switch v := btSettingsValue(); v {
	case "":
		return summaryView("Bluetooth", Item{Label: "Bluetooth", Sublabel: "Press A to connect headphones"})
	case "On", "Off":
		return summaryView("Bluetooth", Item{Label: "Bluetooth", Value: v}, Item{Label: "No headphones connected", Sublabel: "Press A to connect them"})
	default:
		return summaryView("Bluetooth", Item{Label: "Bluetooth", Value: "On"}, Item{Label: v, Sublabel: "Connected"})
	}
}

func (ui *IpodUI) updateSummary() *View {
	items := []Item{{Label: "Version", Value: BETTERPOD_VERSION}}
	if upd.manifest != nil && isNewerVersion(BETTERPOD_VERSION, upd.manifest.Version) {
		items = append(items, Item{Label: "Update Available", Value: upd.manifest.Version, Sublabel: "Press A to install it"})
	} else {
		items = append(items, Item{Label: "Check for Updates", Sublabel: "Press A"})
	}
	return summaryView("Software Update", items...)
}

// leftColumn: the split stack without the menus shown in a right column
// (drawn there, not on the left).
func leftColumn(st []*View) []*View {
	for _, v := range st {
		if v.colParent != nil {
			out := make([]*View, 0, len(st))
			for _, w := range st {
				if w.colParent == nil {
					out = append(out, w)
				}
			}
			return out
		}
	}
	return st
}

// columnShadow: the left panel's shadow, lighter and narrower over a right
// column (made once).
var colShadow []float64

func columnShadow(full []float64) []float64 {
	if colShadow == nil {
		n := max(1, len(full)/2)
		colShadow = make([]float64, n)
		for i := range colShadow {
			colShadow[i] = full[i*len(full)/n] * 0.35
		}
	}
	return colShadow
}

// ---- Turn it Apple ----

// appleMode: the main menu says "iPod" instead of "betterPod" (kept in the
// prefs; off by default: the name is Apple's).
var appleMode bool

func homeTitle() string {
	if appleMode {
		return "iPod"
	}
	return "betterPod"
}

// appleView: Turn it Apple, Off / On.
func (ui *IpodUI) appleView() *View {
	v := &View{ID: "apple", Title: "Turn it Apple", Kind: KindSplit}
	v.Build = func() []Item {
		var items []Item
		for _, on := range []bool{false, true} {
			items = append(items, Item{Label: onOff(on), Checked: appleMode == on, NoArrow: true, Preview: PrevSettings,
				Sublabel: map[bool]string{false: "The menu says betterPod", true: "The menu says iPod"}[on],
				OnSelect: func() { ui.setAppleMode(on) }})
		}
		return items
	}
	v.Items = v.Build()
	for i, it := range v.Items {
		if it.Checked {
			v.Index = i
		}
	}
	return v
}

func (ui *IpodUI) setAppleMode(on bool) {
	if appleMode == on {
		return
	}
	appleMode = on
	ui.savePrefs()
	for _, st := range [][]*View{ui.split, ui.full} {
		for _, h := range st {
			if h.ID == "home" {
				h.Title = homeTitle()
			}
		}
	}
	ui.refreshBuilt()
}
