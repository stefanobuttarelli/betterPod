package main

// ij_ytcoverpick.go — choosing a YouTube download's square cover before it
// starts. A video's page has a Cover row with the cover it will get (the
// automatic one of ij_ytcover.go at first); choosing it shows the whole
// picture with the square on it:
//
//	┌───────────────────────────┐   ┌────────┐
//	│░░░░░┌───────────┐░░░░░░░░░│   │ Cover  │
//	│░░░░░│           │░░░░░░░░░│   │        │
//	│░░░░░└───────────┘░░░░░░░░░│   └────────┘
//	└───────────────────────────┘
//	◀ ▶ Move the square · A Use This · Y Options
//
// the arrows move the square (held, they keep moving), A keeps it, B goes
// back as it was; Y offers Whole Picture (blurred sides, nothing cut off)
// and Automatic. The cover is made from the choice when the download runs.

import (
	"bytes"
	"context"
	"image"
	"image/jpeg"
	"io"
	"math"
	"net/http"
	"time"
)

type ytCoverChoice struct {
	mode string          // "" automatic, "crop", "whole"
	rect image.Rectangle // the square (source pixels), for "crop"
}

// ytCoverMake: the cover JPEG of a choice.
func ytCoverMake(src image.Image, c ytCoverChoice) ([]byte, error) {
	var out image.Image
	switch c.mode {
	case "crop":
		out = scaleImage(cropTo(src, c.rect), ytCoverSize, ytCoverSize)
	case "whole":
		out = blurFillSquare(src, ytCoverSize)
	default:
		return squareCover(src)
	}
	var buf bytes.Buffer
	if err := jpeg.Encode(&buf, out, &jpeg.Options{Quality: 90}); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

// ytCoverAuto: what the automatic cover does with src: a square ("crop")
// or the whole picture ("whole"); the square is the centre one for
// "whole" (where the editor starts).
func ytCoverAuto(src image.Image) (mode string, r image.Rectangle, why string) {
	b := src.Bounds()
	if math.Abs(float64(b.Dx())/float64(b.Dy())-1) < 0.04 {
		return "crop", b, "the square picture"
	}
	if r, ok := framedCover(src); ok {
		return "crop", r, "the cover found in the picture"
	}
	s := min(b.Dx(), b.Dy())
	r = image.Rect(b.Min.X+(b.Dx()-s)/2, b.Min.Y+(b.Dy()-s)/2, 0, 0)
	r.Max = r.Min.Add(image.Pt(s, s))
	return "whole", r, "whole picture, blurred sides"
}

// ytCoverResolve: the choice made definite (automatic → what it does).
func ytCoverResolve(src image.Image, c ytCoverChoice) ytCoverChoice {
	if c.mode != "" {
		return c
	}
	m, r, _ := ytCoverAuto(src)
	return ytCoverChoice{m, r}
}

// ytCoverPreview: the cover of a choice at size px (for the screens).
func ytCoverPreview(src image.Image, c ytCoverChoice, size int) *image.RGBA {
	c = ytCoverResolve(src, c)
	if c.mode == "crop" {
		return scaleImage(cropTo(src, c.rect), size, size)
	}
	out := blurFillSquare(src, size)
	if r, ok := out.(*image.RGBA); ok {
		return r
	}
	return scaleImage(out, size, size)
}

// ytCoverText: the Cover row's line.
func ytCoverText(src image.Image, c ytCoverChoice) string {
	switch c.mode {
	case "crop":
		return "Your square · press A to change"
	case "whole":
		return "Whole picture, blurred sides · press A to change"
	}
	_, _, why := ytCoverAuto(src)
	return "Automatic: " + why + " · press A to change"
}

// ytCoverSource: the first of urls that loads as a picture (the biggest
// first: the cover is made from it).
func ytCoverSource(ctx context.Context, urls []string) image.Image {
	for _, u := range urls {
		req, err := http.NewRequestWithContext(ctx, "GET", u, nil)
		if err != nil {
			continue
		}
		resp, err := updateClient(30 * time.Second).Do(req)
		if err != nil {
			if ctx.Err() != nil {
				return nil
			}
			continue
		}
		if resp.StatusCode != 200 {
			resp.Body.Close()
			continue
		}
		img, _, err := image.Decode(io.LimitReader(resp.Body, 8<<20))
		resp.Body.Close()
		if err == nil && img.Bounds().Dx() >= 120 {
			return img
		}
	}
	return nil
}

// ---- the editor ----

type ytCoverEditor struct {
	src    image.Image
	choice ytCoverChoice // being edited (definite)
	done   func(ytCoverChoice)

	fit             *image.RGBA // the picture scaled into its box
	fitW, fitH      int
	prev            *image.RGBA // the cover as chosen now
	prevFor         ytCoverChoice
	lastW, lastH    int
	boxX, boxY, scl float64 // where the picture is (px) and its scale
}

func (ui *IpodUI) ytCoverEditorView(src image.Image, cur ytCoverChoice, done func(ytCoverChoice)) *View {
	ed := &ytCoverEditor{src: src, choice: ytCoverResolve(src, cur), done: done}
	return &View{ID: "ytCover", Title: "Cover", Kind: KindFull, Custom: ed}
}

func (ed *ytCoverEditor) Animating(now time.Time) bool { return false }

// OnScroll moves the square: across a wide picture, up and down a tall one.
func (ed *ytCoverEditor) OnScroll(ui *IpodUI, dir int, velocity float64) {
	b := ed.src.Bounds()
	if ed.choice.mode != "crop" {
		// from the whole picture: the square starts where it was
		ed.choice.mode = "crop"
	}
	r := ed.choice.rect
	wide := b.Dx() >= b.Dy()
	span := b.Dx() - r.Dx()
	if !wide {
		span = b.Dy() - r.Dy()
	}
	step := max(1, span/40)
	d := image.Pt(dir*step, 0)
	if !wide {
		d = image.Pt(0, dir*step)
	}
	r = r.Add(d)
	// inside the picture
	if r.Min.X < b.Min.X {
		r = r.Add(image.Pt(b.Min.X-r.Min.X, 0))
	}
	if r.Max.X > b.Max.X {
		r = r.Add(image.Pt(b.Max.X-r.Max.X, 0))
	}
	if r.Min.Y < b.Min.Y {
		r = r.Add(image.Pt(0, b.Min.Y-r.Min.Y))
	}
	if r.Max.Y > b.Max.Y {
		r = r.Add(image.Pt(0, b.Max.Y-r.Max.Y))
	}
	ed.choice.rect = r
	ui.dirty = true
}

// OnCenter: A keeps the cover as shown.
func (ed *ytCoverEditor) OnCenter(ui *IpodUI) {
	ed.done(ed.choice)
	ui.hideView()
	ui.showToast("Cover chosen")
}

// OnLongPress: Y offers the whole picture and the automatic cover.
func (ed *ytCoverEditor) OnLongPress(ui *IpodUI) {
	ui.showSheetTitled("Cover", []SheetOption{
		{Label: "Whole Picture", OnSelect: func() {
			ed.choice.mode = "whole"
			ui.dirty = true
		}},
		{Label: "Automatic", OnSelect: func() {
			ed.choice = ytCoverResolve(ed.src, ytCoverChoice{})
			ed.done(ytCoverChoice{})
			ui.hideView()
			ui.showToast("Automatic cover")
		}},
	})
}

func (ed *ytCoverEditor) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	fillRect(dst, 0, 0, W, H, th.bg)
	wCSS, hCSS := float64(W)/DPR, float64(H)/DPR
	// the picture on the left, the cover on the right, the keys under them
	const pad, gap = 12.0, 12.0
	prevS := 120.0
	hintH := 30.0
	boxW := wCSS - 2*pad - gap - prevS
	boxH := hCSS - 2*pad - hintH
	b := ed.src.Bounds()
	scl := math.Min(boxW/float64(b.Dx()), boxH/float64(b.Dy()))
	fw, fh := int(float64(b.Dx())*scl*DPR+0.5), int(float64(b.Dy())*scl*DPR+0.5)
	if ed.fit == nil || ed.fitW != fw || ed.fitH != fh {
		ed.fit, ed.fitW, ed.fitH = scaleImage(ed.src, fw, fh), fw, fh
	}
	bx := pad*DPR + (boxW*DPR-float64(fw))/2
	by := pad*DPR + (boxH*DPR-float64(fh))/2
	blit(dst, ed.fit, int(bx), int(by), 1)
	k := scl * DPR // source px → screen px
	if ed.choice.mode == "crop" {
		r := ed.choice.rect.Sub(b.Min)
		x0, y0 := bx+float64(r.Min.X)*k, by+float64(r.Min.Y)*k
		x1, y1 := bx+float64(r.Max.X)*k, by+float64(r.Max.Y)*k
		shade := Col{0, 0, 0, 0.55}
		// outside the square: darker
		fillRect(dst, int(bx), int(by), int(x0-bx), fh, shade)
		fillRect(dst, int(x1), int(by), int(bx)+fw-int(x1), fh, shade)
		fillRect(dst, int(x0), int(by), int(x1-x0), int(y0-by), shade)
		fillRect(dst, int(x0), int(y1), int(x1-x0), int(by)+fh-int(y1), shade)
		strokeRounded(dst, x0, y0, x1, y1, 2, 2*DPR/2, colWhite, 1)
	}
	// the cover as it will be
	if ed.prev == nil || ed.prevFor != ed.choice {
		ed.prev, ed.prevFor = ytCoverPreview(ed.src, ed.choice, px(prevS)), ed.choice
	}
	px0 := (wCSS - pad - prevS) * DPR
	py0 := pad*DPR + (boxH*DPR-float64(px(prevS)))/2
	fillRect(dst, int(px0)-px(1), int(py0)-px(1), px(prevS)+px(2), px(prevS)+px(2), th.artBorder)
	blit(dst, ed.prev, int(px0), int(py0), 1)
	f := face(W600, 11)
	cap := "Cover"
	drawText(dst, f, cap, px0+(float64(px(prevS))-textWidth(f, cap))/2, py0+float64(px(prevS))+14*DPR, th.sub)
	// the keys
	hint := "◀ ▶ Move the square · A Use This · Y Whole Picture or Automatic"
	if b.Dx() < b.Dy() {
		hint = "▲ ▼ Move the square · A Use This · Y Whole Picture or Automatic"
	}
	if ed.choice.mode == "whole" {
		hint = "Whole picture · ◀ ▶ a square instead · A Use This"
	}
	hf := face(W500, 12)
	hint = ellipsize(hf, hint, float64(W)-2*pad*DPR)
	_, a12 := lineBox(12)
	drawText(dst, hf, hint, (float64(W)-textWidth(hf, hint))/2, (hCSS-pad-hintH/2+a12/2)*DPR, th.sub)
}
