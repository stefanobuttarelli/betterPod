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
	a := &AboutView{list: &View{ID: "aboutList", Kind: KindFull, Items: []Item{
		{Label: "betterPod " + BETTERPOD_VERSION},
		{Label: "Interface: iPod.js", Sublabel: "Tanner Villarete · MIT"},
		{Label: "Audio engine: MiyooPod", Sublabel: "Danilo Fragoso · MIT"},
		{Label: "Font: Inter", Sublabel: "Rasmus Andersson · OFL"}}}}
	//
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
	// header row: logo + "betterPod", centred, 8px padding
	logo := ui.asset("ipod_logo")
	lw := float64(logo.Bounds().Dx())
	tf := face(W900, 16)
	tw := textWidth(tf, "betterPod")
	rowW := lw/DPR + 16 + tw/DPR
	rowH := 48.0
	x := (float64(W)/DPR - rowW) / 2
	y := 16.0
	blit(dst, logo, px(x+8), px(y+8), 1)
	tlh, tasc := lineBox(16)
	drawText(dst, tf, "betterPod", (x+lw/DPR+16)*DPR, (y+(rowH-tlh)/2+tasc)*DPR, th.text)

	y += rowH
	df := face(W400, 14)
	dlh, dasc := lineBox(14)
	desc := "Based on iPod.js by Tanner Villarete"
	dw := textWidth(df, desc)
	drawText(dst, df, desc, (float64(W)/DPR-dw/DPR)/2*DPR, (y+dasc)*DPR, th.text)
	y += dlh + 16
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
