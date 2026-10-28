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
//	◀ ▶ ▲ ▼ Move · L R Zoom · A Use This · Y More
//
// the arrows move the square (held, they keep moving), R zooms in (a
// smaller square), L out, A keeps it, B goes back as it was; Y offers Whole Picture (blurred sides, nothing cut off)
// and Automatic. The cover is made from the choice when the download runs.

import (
	"bytes"
	"context"
	"image"
	"image/jpeg"
	"io"
	"math"
	"net/http"
	"sync"
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
	// remembered for the last picture: finding a framed cover takes a while
	ytAutoMemo.Lock()
	if ytAutoMemo.src == src && src != nil {
		mode, r, why = ytAutoMemo.mode, ytAutoMemo.r, ytAutoMemo.why
		ytAutoMemo.Unlock()
		return
	}
	ytAutoMemo.Unlock()
	mode, r, why = ytCoverAutoFind(src)
	ytAutoMemo.Lock()
	ytAutoMemo.src, ytAutoMemo.mode, ytAutoMemo.r, ytAutoMemo.why = src, mode, r, why
	ytAutoMemo.Unlock()
	return
}

var ytAutoMemo struct {
	sync.Mutex
	src  image.Image
	mode string
	r    image.Rectangle
	why  string
}

func ytCoverAutoFind(src image.Image) (mode string, r image.Rectangle, why string) {
	b := src.Bounds()
	if math.Abs(float64(b.Dx())/float64(b.Dy())-1) < 0.04 {
		return "crop", b, "the square picture"
	}
	if r, ok := framedCover(src); ok {
		return "crop", r, "the cover found in the picture"
	}
	s := min(b.Dx(), b.Dy())
	o := image.Pt(b.Min.X+(b.Dx()-s)/2, b.Min.Y+(b.Dy()-s)/2)
	r = image.Rectangle{Min: o, Max: o.Add(image.Pt(s, s))}
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

	// the picture is got ready away from the screen (scaling a big picture
	// and finding its cover take a moment); meanwhile the stripes move
	ready   bool // choice resolved
	working bool // a goroutine is making fit or prev
	opened  time.Time
}

func (ui *IpodUI) ytCoverEditorView(src image.Image, cur ytCoverChoice, done func(ytCoverChoice)) *View {
	ed := &ytCoverEditor{src: src, choice: cur, done: done, opened: time.Now()}
	return &View{ID: "ytCover", Title: "Cover", Kind: KindFull, Custom: ed}
}

func (ed *ytCoverEditor) Animating(now time.Time) bool { return ed.fit == nil || ed.working }

// prepare makes, away from the screen, what the screen is missing: the
// choice made definite, the picture scaled to fw×fh, the cover of the choice.
func (ed *ytCoverEditor) prepare(ui *IpodUI, fw, fh, prevPx int) {
	ed.working = true
	src, ready, choice := ed.src, ed.ready, ed.choice
	needFit := ed.fit == nil || ed.fitW != fw || ed.fitH != fh
	go func() {
		if !ready {
			choice = ytCoverResolve(src, choice)
		}
		var fit *image.RGBA
		if needFit {
			fit = scaleImage(src, fw, fh)
		}
		prev := ytCoverPreview(src, choice, prevPx)
		ui.postSure(func() {
			ed.working = false
			if !ed.ready {
				ed.choice, ed.ready = choice, true
			}
			if fit != nil {
				ed.fit, ed.fitW, ed.fitH = fit, fw, fh
			}
			ed.prev, ed.prevFor = prev, choice
			ui.dirty = true
		})
	}()
}

// drawLoadStripes: the old iPod's moving 45° stripes, over x,y,w,h (px).
func drawLoadStripes(dst *image.RGBA, x, y, w, h int, now time.Time) {
	a := th.bg
	b := lerpCol(th.bg, th.sub, 0.10)
	if th.dark {
		b = lerpCol(th.bg, th.text, 0.08)
	}
	period := px(24)
	half := period / 2
	off := int(now.Sub(time.Time{}).Seconds()*float64(px(28))) % period
	fillRect(dst, x, y, w, h, a)
	for yy := y; yy < y+h; yy++ {
		// the band start on this row: moving right, slanted 45°
		s := ((yy-off)%period + period) % period
		for xx := x - s; xx < x+w; xx += period {
			x0, x1 := max(xx, x), min(xx+half, x+w)
			if x1 > x0 {
				fillRect(dst, x0, yy, x1-x0, 1, b)
			}
		}
	}
}

// OnScroll (the wheel) moves the square along the picture's long side.
func (ed *ytCoverEditor) OnScroll(ui *IpodUI, dir int, velocity float64) {
	if !ed.ready {
		return // still getting the picture ready
	}
	b := ed.src.Bounds()
	if b.Dx() >= b.Dy() {
		ed.move(ui, dir, 0)
	} else {
		ed.move(ui, 0, dir)
	}
}

// OnKey: the arrows move the square, R zooms in (a smaller square), L out.
func (ed *ytCoverEditor) OnKey(ui *IpodUI, k Key) bool {
	switch k {
	case LEFT, RIGHT, UP, DOWN, L, R:
	default:
		return false
	}
	if !ed.ready {
		return true // still getting the picture ready
	}
	switch k {
	case LEFT:
		ed.move(ui, -1, 0)
	case RIGHT:
		ed.move(ui, 1, 0)
	case UP:
		ed.move(ui, 0, -1)
	case DOWN:
		ed.move(ui, 0, 1)
	case R:
		ed.zoom(ui, 0.92)
	case L:
		ed.zoom(ui, 1/0.92)
	}
	return true
}

// cropMode: from the whole picture, the square starts where it was.
func (ed *ytCoverEditor) cropMode() {
	if ed.choice.mode != "crop" {
		ed.choice.mode = "crop"
	}
}

// fitIn keeps r (a square) inside the picture.
func (ed *ytCoverEditor) fitIn(r image.Rectangle) image.Rectangle {
	b := ed.src.Bounds()
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
	return r
}

// move: the square a step (a 20th of its side) across or down.
func (ed *ytCoverEditor) move(ui *IpodUI, dx, dy int) {
	ed.cropMode()
	r := ed.choice.rect
	step := max(1, r.Dx()/20)
	ed.choice.rect = ed.fitIn(r.Add(image.Pt(dx*step, dy*step)))
	ui.dirty = true
}

// zoom: the square's side times f, about its centre; from the whole
// picture's height (or width) down to a sixth of it.
func (ed *ytCoverEditor) zoom(ui *IpodUI, f float64) {
	ed.cropMode()
	b := ed.src.Bounds()
	r := ed.choice.rect
	maxS := min(b.Dx(), b.Dy())
	minS := max(32, maxS/6)
	s := int(math.Round(float64(r.Dx()) * f))
	if f < 1 && s == r.Dx() {
		s--
	} else if f > 1 && s == r.Dx() {
		s++
	}
	s = min(max(s, minS), maxS)
	if s == r.Dx() {
		return
	}
	c := image.Pt((r.Min.X+r.Max.X)/2, (r.Min.Y+r.Max.Y)/2)
	o := image.Pt(c.X-s/2, c.Y-s/2)
	n := image.Rectangle{Min: o, Max: o.Add(image.Pt(s, s))}
	ed.choice.rect = ed.fitIn(n)
	ui.dirty = true
}

// OnCenter: A keeps the cover as shown.
func (ed *ytCoverEditor) OnCenter(ui *IpodUI) {
	if !ed.ready {
		return // still getting the picture ready
	}
	ed.done(ed.choice)
	ui.hideView()
	ui.showToast("Cover chosen")
}

// OnLongPress: Y offers the whole picture and the automatic cover.
func (ed *ytCoverEditor) OnLongPress(ui *IpodUI) {
	if !ed.ready {
		return // still getting the picture ready
	}
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
	if ed.ready && ed.choice.mode == "crop" && (ed.prev == nil || ed.prevFor != ed.choice) {
		// a square is quick: made at once, so moving it never flickers
		ed.prev, ed.prevFor = ytCoverPreview(ed.src, ed.choice, px(prevS)), ed.choice
	}
	if !ed.working && (!ed.ready || ed.fit == nil || ed.fitW != fw || ed.fitH != fh || ed.prev == nil || ed.prevFor != ed.choice) {
		ed.prepare(ui, fw, fh, px(prevS))
	}
	if ed.fit == nil || ed.fitW != fw || ed.fitH != fh || !ed.ready {
		// getting it ready: the whole screen in moving stripes
		drawLoadStripes(dst, 0, 0, W, H, now)
		f := face(W600, 14)
		msg := "Getting the picture ready…"
		_, a14 := lineBox(14)
		tw := textWidth(f, msg)
		bw, bh := tw+28*DPR, 34*DPR
		bx0, by0 := (float64(W)-bw)/2, (float64(H)-bh)/2
		fillRoundedGradient(dst, bx0, by0, bx0+bw, by0+bh, bh/2, []GradStop{{0, th.bg}, {1, th.bg}}, 1)
		strokeRounded(dst, bx0, by0, bx0+bw, by0+bh, bh/2, DPR, th.artBorder, 1)
		drawText(dst, f, msg, (float64(W)-tw)/2, by0+bh/2+a14*DPR/2, th.text)
		return
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
	px0 := (wCSS - pad - prevS) * DPR
	py0 := pad*DPR + (boxH*DPR-float64(px(prevS)))/2
	fillRect(dst, int(px0)-px(1), int(py0)-px(1), px(prevS)+px(2), px(prevS)+px(2), th.artBorder)
	if ed.prev != nil && ed.prevFor == ed.choice {
		blit(dst, ed.prev, int(px0), int(py0), 1)
	} else {
		// the new cover being made (the whole picture's blur takes a moment)
		drawLoadStripes(dst, int(px0), int(py0), px(prevS), px(prevS), now)
	}
	f := face(W600, 11)
	cap := "Cover"
	drawText(dst, f, cap, px0+(float64(px(prevS))-textWidth(f, cap))/2, py0+float64(px(prevS))+14*DPR, th.sub)
	// the keys
	hint := "◀ ▶ ▲ ▼ Move · L R Zoom · A Use This · Y More"
	if ed.choice.mode == "whole" {
		hint = "Whole picture · ◀ ▶ or L R a square instead · A Use This"
	}
	hf := face(W500, 12)
	hint = ellipsize(hf, hint, float64(W)-2*pad*DPR)
	_, a12 := lineBox(12)
	drawText(dst, hf, hint, (float64(W)-textWidth(hf, hint))/2, (hCSS-pad-hintH/2+a12/2)*DPR, th.sub)
}
