package main

// ij_about.go — Settings → About: logo, credit line and component list.

import (
	"image"
	"time"
)

type AboutView struct {
	list *View
	buf  *image.RGBA
}

func (ui *IpodUI) aboutView() *View {
	sysName, sysConsoles := platformAbout()
	a := &AboutView{list: &View{ID: "aboutList", Kind: KindFull, Items: []Item{
		{Label: "betterPod " + BETTERPOD_VERSION, Sublabel: "betterpod.xyz · r/betterPod"},
		{Label: sysName, Sublabel: sysConsoles},
		{Label: "Interface: iPod.js", Sublabel: "Tanner Villarete · MIT"},
		{Label: "Audio engine: MiyooPod", Sublabel: "Danilo Fragoso · MIT"},
		{Label: "Puzzles: Portable Puzzle Collection", Sublabel: "Simon Tatham and contributors · MIT"},
		{Label: "2048: the game's rules", Sublabel: "Gabriele Cirulli · MIT"},
		{Label: "Five Letters: the words", Sublabel: "SCOWL (Kevin Atkinson) · ENABLE, public domain"},
		{Label: "Font: Inter", Sublabel: "Rasmus Andersson · OFL"},
		{Label: "An independent fan project", Sublabel: "iPod is a trademark of Apple Inc.; betterPod is not affiliated with Apple"}}}}
	//
	if isOnion { // the Miyoo's SDL2 drivers (onion/betterpod/libs), before the last row
		items := a.list.Items
		last := items[len(items)-1]
		a.list.Items = append(append(items[:len(items)-1:len(items)-1], Item{Label: "SDL2 for the Miyoo", Sublabel: "steward-fu · zlib"}), last)
	}
	a.list.dirty = true
	return &View{ID: "about", Title: "About", Kind: KindFull, Custom: a}
}

func (a *AboutView) Animating(now time.Time) bool { return false }
func (a *AboutView) OnCenter(ui *IpodUI)          {}
func (a *AboutView) OnScroll(ui *IpodUI, dir int, velocity float64) {
	ni := a.list.Index + dir*skipFor(velocity)
	if ni < 0 {
		ni = 0
	}
	if ni >= len(a.list.Items) {
		ni = len(a.list.Items) - 1
	}
	a.list.Index = ni
	a.list.dirty = true
}

func (a *AboutView) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	W := dst.Bounds().Dx()
	fillRect(dst, 0, 0, W, dst.Bounds().Dy(), th.bg)
	// the logo and the name, centred, then what it is
	logo := ui.asset("betterpod_logo_96")
	lw := float64(logo.Bounds().Dx()) / DPR // CSS px
	tf := face(W900, 20)
	tw := (textWidth(tf, "better") + textWidth(tf, "Pod")) / DPR
	rowW := lw + 12 + tw
	rowH := lw
	x := (float64(W)/DPR - rowW) / 2
	y := 14.0
	blit(dst, logo, px(x), px(y), 1)
	tlh, tasc := lineBox(20)
	drawWordmark(dst, tf, (x+lw+12)*DPR, (y+(rowH-tlh)/2+tasc)*DPR)

	y += rowH + 8
	df := face(W500, 11)
	dlh, dasc := lineBox(11)
	desc := "The mid-2000s music player, reborn on retro handhelds"
	dw := textWidth(df, desc)
	drawText(dst, df, desc, (float64(W)/DPR-dw/DPR)/2*DPR, (y+dasc)*DPR, th.sub)
	y += dlh + 12
	// the credits list fills the rest of the screen
	lh := dst.Bounds().Dy() - px(y)
	if a.buf == nil || a.buf.Bounds().Dy() != lh || a.buf.Bounds().Dx() != W {
		a.buf = newCanvas(W, lh)
		a.list.dirty = true
	}
	if a.list.dirty {
		ui.renderList(a.list, a.buf)
		a.list.dirty = false
	}
	blit(dst, a.buf, 0, px(y), 1)
}
