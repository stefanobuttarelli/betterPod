package main

// ij_gfx.go — software rendering primitives for the ipod.js UI: colours,
// CSS-like gradients, alpha blending, rounded rectangles and text.

import (
	"image"
	"image/color"
	"math"

	"golang.org/x/image/draw"
	"golang.org/x/image/font"
	"golang.org/x/image/math/fixed"
)

// DPR is the device pixel ratio: ipod.js is laid out at 320x240 CSS px and
// rendered at 640x480 device px.
const DPR = 2.0

// px converts CSS px to device px.
func px(css float64) int { return int(math.Round(css * DPR)) }

// Col is a straight RGBA colour: R, G, B in 0..255, A in 0..1.
type Col struct {
	R, G, B, A float64
}

// rgb / rgba / hex build colours like the CSS sources do.
func rgb(r, g, b int) Col             { return Col{float64(r), float64(g), float64(b), 1} }
func rgba(r, g, b int, a float64) Col { return Col{float64(r), float64(g), float64(b), a} }
func hex(s string) Col {
	r, g, b, _ := parseHexColor(s)
	return rgb(int(r), int(g), int(b))
}

var (
	colWhite       = rgb(255, 255, 255)
	colBlack       = rgb(0, 0, 0)
	colTransparent = Col{}
)

// GradStop is a CSS gradient colour stop (Pos in 0..1).
type GradStop struct {
	Pos float64
	C   Col
}

// lerpCol interpolates in premultiplied space, like browsers do.
func lerpCol(a, b Col, t float64) Col {
	if a.A == b.A {
		return Col{a.R + (b.R-a.R)*t, a.G + (b.G-a.G)*t, a.B + (b.B-a.B)*t, a.A}
	}
	pa := Col{a.R * a.A, a.G * a.A, a.B * a.A, a.A}
	pb := Col{b.R * b.A, b.G * b.A, b.B * b.A, b.A}
	al := a.A + (b.A-a.A)*t
	if al <= 0 {
		return Col{}
	}
	return Col{(pa.R + (pb.R-pa.R)*t) / al, (pa.G + (pb.G-pa.G)*t) / al, (pa.B + (pb.B-pa.B)*t) / al, al}
}

func gradAt(stops []GradStop, t float64) Col {
	if len(stops) == 0 {
		return colTransparent
	}
	if t <= stops[0].Pos {
		return stops[0].C
	}
	for i := 1; i < len(stops); i++ {
		if t <= stops[i].Pos {
			a, b := stops[i-1], stops[i]
			if b.Pos == a.Pos {
				return b.C
			}
			return lerpCol(a.C, b.C, (t-a.Pos)/(b.Pos-a.Pos))
		}
	}
	return stops[len(stops)-1].C
}

// blendPx composites colour c with alpha a over one RGBA pixel.
func blendPx(p []uint8, c Col, a float64) {
	if a <= 0 {
		return
	}
	if a >= 1 {
		p[0], p[1], p[2], p[3] = uint8(c.R+0.5), uint8(c.G+0.5), uint8(c.B+0.5), 255
		return
	}
	ia := 1 - a
	p[0] = uint8(c.R*a + float64(p[0])*ia + 0.5)
	p[1] = uint8(c.G*a + float64(p[1])*ia + 0.5)
	p[2] = uint8(c.B*a + float64(p[2])*ia + 0.5)
	p[3] = uint8(255*a + float64(p[3])*ia + 0.5)
}

func newCanvas(w, h int) *image.RGBA { return image.NewRGBA(image.Rect(0, 0, w, h)) }

func clipRect(dst *image.RGBA, x, y, w, h int) (int, int, int, int, bool) {
	b := dst.Bounds()
	x0, y0, x1, y1 := x, y, x+w, y+h
	if x0 < b.Min.X {
		x0 = b.Min.X
	}
	if y0 < b.Min.Y {
		y0 = b.Min.Y
	}
	if x1 > b.Max.X {
		x1 = b.Max.X
	}
	if y1 > b.Max.Y {
		y1 = b.Max.Y
	}
	if x1 <= x0 || y0 >= y1 {
		return 0, 0, 0, 0, false
	}
	return x0, y0, x1, y1, true
}

// fillRect fills a device-pixel rectangle (opaque fast path).
func fillRect(dst *image.RGBA, x, y, w, h int, c Col) {
	x0, y0, x1, y1, ok := clipRect(dst, x, y, w, h)
	if !ok || c.A <= 0 {
		return
	}
	if c.A >= 1 {
		r, g, b := uint8(c.R+0.5), uint8(c.G+0.5), uint8(c.B+0.5)
		first := dst.PixOffset(x0, y0)
		n := (x1 - x0) * 4
		row := dst.Pix[first : first+n]
		for i := 0; i < n; i += 4 {
			row[i], row[i+1], row[i+2], row[i+3] = r, g, b, 255
		}
		for yy := y0 + 1; yy < y1; yy++ {
			off := dst.PixOffset(x0, yy)
			copy(dst.Pix[off:off+n], row)
		}
		return
	}
	for yy := y0; yy < y1; yy++ {
		off := dst.PixOffset(x0, yy)
		for xx := x0; xx < x1; xx++ {
			blendPx(dst.Pix[off:off+4], c, c.A)
			off += 4
		}
	}
}

// fillVGradient fills a rectangle with a top-to-bottom gradient
// (one colour per device row, sampled at the row centre).
func fillVGradient(dst *image.RGBA, x, y, w, h int, stops []GradStop) {
	if h <= 0 {
		return
	}
	for row := 0; row < h; row++ {
		t := (float64(row) + 0.5) / float64(h)
		fillRect(dst, x, y+row, w, 1, gradAt(stops, t))
	}
}

// fillHGradient: left-to-right gradient.
func fillHGradient(dst *image.RGBA, x, y, w, h int, stops []GradStop) {
	if w <= 0 {
		return
	}
	for col := 0; col < w; col++ {
		t := (float64(col) + 0.5) / float64(w)
		fillRect(dst, x+col, y, 1, h, gradAt(stops, t))
	}
}

// blit composites a premultiplied RGBA image onto dst at (dx, dy).
func blit(dst, src *image.RGBA, dx, dy int, opacity float64) {
	if src == nil || opacity <= 0 {
		return
	}
	sb := src.Bounds()
	x0, y0, x1, y1, ok := clipRect(dst, dx, dy, sb.Dx(), sb.Dy())
	if !ok {
		return
	}
	a := uint32(int(opacity*255 + 0.5))
	for yy := y0; yy < y1; yy++ {
		so := src.PixOffset(sb.Min.X+x0-dx, sb.Min.Y+yy-dy)
		do := dst.PixOffset(x0, yy)
		n := (x1 - x0) * 4
		s := src.Pix[so : so+n]
		d := dst.Pix[do : do+n]
		if a >= 255 {
			for i := 0; i < n; i += 4 {
				sa := uint32(s[i+3])
				if sa == 255 {
					d[i], d[i+1], d[i+2], d[i+3] = s[i], s[i+1], s[i+2], 255
				} else if sa != 0 {
					ia := 255 - sa
					d[i] = uint8(uint32(s[i]) + (uint32(d[i])*ia+127)/255)
					d[i+1] = uint8(uint32(s[i+1]) + (uint32(d[i+1])*ia+127)/255)
					d[i+2] = uint8(uint32(s[i+2]) + (uint32(d[i+2])*ia+127)/255)
					d[i+3] = uint8(sa + (uint32(d[i+3])*ia+127)/255)
				}
			}
		} else {
			for i := 0; i < n; i += 4 {
				sa := uint32(s[i+3]) * a / 255
				if sa == 0 {
					continue
				}
				ia := 255 - sa
				d[i] = uint8((uint32(s[i])*a+127)/255 + (uint32(d[i])*ia+127)/255)
				d[i+1] = uint8((uint32(s[i+1])*a+127)/255 + (uint32(d[i+1])*ia+127)/255)
				d[i+2] = uint8((uint32(s[i+2])*a+127)/255 + (uint32(d[i+2])*ia+127)/255)
				d[i+3] = uint8(sa + (uint32(d[i+3])*ia+127)/255)
			}
		}
	}
}

// blitOpaque copies src rows as they are (no blending).
func blitOpaque(dst, src *image.RGBA, dx, dy int) {
	sb := src.Bounds()
	x0, y0, x1, y1, ok := clipRect(dst, dx, dy, sb.Dx(), sb.Dy())
	if !ok {
		return
	}
	for yy := y0; yy < y1; yy++ {
		so := src.PixOffset(sb.Min.X+x0-dx, sb.Min.Y+yy-dy)
		do := dst.PixOffset(x0, yy)
		n := (x1 - x0) * 4
		copy(dst.Pix[do:do+n], src.Pix[so:so+n])
	}
}

// blitClipped is blit restricted to the clip rectangle of dst.
func blitClipped(dst, src *image.RGBA, dx, dy int, opacity float64, clip image.Rectangle) {
	sub, ok := dst.SubImage(clip).(*image.RGBA)
	if !ok || sub.Bounds().Empty() {
		return
	}
	blit(sub, src, dx, dy, opacity)
}

// darkenColumns multiplies the colour of columns x0+i by (1 - alpha[i])
// (used for the CSS box-shadow of the sliding panels).
func darkenColumns(dst *image.RGBA, x0, y0, h int, alpha []float64) {
	b := dst.Bounds()
	for i, a := range alpha {
		x := x0 + i
		if x < b.Min.X || x >= b.Max.X || a <= 0 {
			continue
		}
		m := uint32(int((1 - a) * 256))
		for y := y0; y < y0+h; y++ {
			if y < b.Min.Y || y >= b.Max.Y {
				continue
			}
			o := dst.PixOffset(x, y)
			dst.Pix[o] = uint8(uint32(dst.Pix[o]) * m >> 8)
			dst.Pix[o+1] = uint8(uint32(dst.Pix[o+1]) * m >> 8)
			dst.Pix[o+2] = uint8(uint32(dst.Pix[o+2]) * m >> 8)
		}
	}
	// alpha is left as it is
}

// boxShadowProfile returns the alpha falloff of a CSS box-shadow blur, one
// value per device px (Gaussian with sigma = blur/2).
func boxShadowProfile(blurDevice, maxAlpha float64) []float64 {
	sigma := blurDevice / 2
	n := int(math.Ceil(sigma * 3))
	out := make([]float64, n)
	for i := 0; i < n; i++ {
		d := float64(i) + 0.5
		out[i] = maxAlpha * 0.5 * math.Erfc(d/(math.Sqrt2*sigma))
	}
	return out
}

// scaleImage resamples src to w x h (Catmull-Rom, like a browser).
func scaleImage(src image.Image, w, h int) *image.RGBA {
	dst := newCanvas(w, h)
	draw.CatmullRom.Scale(dst, dst.Bounds(), src, src.Bounds(), draw.Src, nil)
	return dst
}

// roundedCoverage returns how much of the device pixel centred at (pxc, pyc)
// is inside the rounded rectangle (x0, y0)-(x1, y1) with radius r
// (signed-distance antialiasing, 1 px wide).
// Inside the straight edges it is the plain box coverage.
func roundedCoverage(pxc, pyc, x0, y0, x1, y1, r float64) float64 {
	cx := math.Max(x0+r, math.Min(pxc, x1-r))
	cy := math.Max(y0+r, math.Min(pyc, y1-r))
	dx, dy := pxc-cx, pyc-cy
	var dist float64
	if r > 0 && (pxc < x0+r || pxc > x1-r) && (pyc < y0+r || pyc > y1-r) {
		dist = math.Hypot(dx, dy) - r
	} else {
		dist = math.Max(math.Max(x0-pxc, pxc-x1), math.Max(y0-pyc, pyc-y1))
	}
	c := 0.5 - dist
	if c < 0 {
		return 0
	}
	if c > 1 {
		return 1
	}
	return c
}

// fillRoundedGradient fills a rounded rectangle (device px, fractional) with
// a vertical gradient.
func fillRoundedGradient(dst *image.RGBA, x0, y0, x1, y1, r float64, stops []GradStop, opacity float64) {
	ix0, iy0 := int(math.Floor(x0)), int(math.Floor(y0))
	ix1, iy1 := int(math.Ceil(x1)), int(math.Ceil(y1))
	b := dst.Bounds()
	for y := iy0; y < iy1; y++ {
		if y < b.Min.Y || y >= b.Max.Y {
			continue
		}
		t := (float64(y) + 0.5 - y0) / (y1 - y0)
		c := gradAt(stops, t)
		// interior span of fully covered pixels: no coverage test needed
		fullA, fullB := ix1, ix1
		// (only rows that lie entirely inside the rectangle)
		if float64(y) >= y0 && float64(y)+1 <= y1 {
			inset := 1.0
			if float64(y) < y0+r || float64(y)+1 > y1-r {
				inset = r + 1
			}
			fullA = int(math.Ceil(x0 + inset))
			fullB = int(math.Floor(x1 - inset))
		}
		for x := ix0; x < ix1; x++ {
			if x < b.Min.X || x >= b.Max.X {
				continue
			}
			if x >= fullA && x < fullB {
				o := dst.PixOffset(x, y)
				blendPx(dst.Pix[o:o+4], c, c.A*opacity)
				continue
			}
			cov := roundedCoverage(float64(x)+0.5, float64(y)+0.5, x0, y0, x1, y1, r)
			if cov <= 0 {
				continue
			}
			o := dst.PixOffset(x, y)
			blendPx(dst.Pix[o:o+4], c, c.A*cov*opacity)
		}
	}
}

// strokeRounded draws the border (width bw) of a rounded rectangle.
func strokeRounded(dst *image.RGBA, x0, y0, x1, y1, r, bw float64, c Col, opacity float64) {
	ix0, iy0 := int(math.Floor(x0)), int(math.Floor(y0))
	ix1, iy1 := int(math.Ceil(x1)), int(math.Ceil(y1))
	ri := math.Max(0, r-bw)
	b := dst.Bounds()
	for y := iy0; y < iy1; y++ {
		if y < b.Min.Y || y >= b.Max.Y {
			continue
		}
		for x := ix0; x < ix1; x++ {
			if x < b.Min.X || x >= b.Max.X {
				continue
			}
			fx, fy := float64(x)+0.5, float64(y)+0.5
			outer := roundedCoverage(fx, fy, x0, y0, x1, y1, r)
			if outer <= 0 {
				continue
			}
			inner := roundedCoverage(fx, fy, x0+bw, y0+bw, x1-bw, y1-bw, ri)
			cov := outer - inner
			if cov <= 0 {
				continue
			}
			o := dst.PixOffset(x, y)
			blendPx(dst.Pix[o:o+4], c, c.A*cov*opacity)
		}
	}
}

// drawText draws s with its baseline at (x, baseline), device px.
//
// The colour alpha is applied by the font rasterizer.
func drawText(dst *image.RGBA, face font.Face, s string, x, baseline float64, c Col) {
	if s == "" {
		return
	}
	d := &font.Drawer{
		Dst:  dst,
		Src:  image.NewUniform(color.NRGBA{uint8(c.R), uint8(c.G), uint8(c.B), uint8(c.A*255 + 0.5)}),
		Face: face,
		Dot:  fixed.Point26_6{X: fixed.Int26_6(x * 64), Y: fixed.Int26_6(baseline * 64)},
	}
	d.DrawString(s)
}

func textWidth(face font.Face, s string) float64 {
	return float64(font.MeasureString(face, s)) / 64
}

// ellipsize shortens s with "…" so it fits maxW (like text-overflow: ellipsis).
func ellipsize(face font.Face, s string, maxW float64) string {
	if maxW <= 0 {
		return ""
	}
	if textWidth(face, s) <= maxW {
		return s
	}
	r := []rune(s)
	ell := "…"
	ew := textWidth(face, ell)
	lo, hi := 0, len(r)
	for lo < hi {
		mid := (lo + hi + 1) / 2
		if textWidth(face, string(r[:mid]))+ew <= maxW {
			lo = mid
		} else {
			hi = mid - 1
		}
	}
	for lo > 0 && r[lo-1] == ' ' {
		lo--
	}
	return string(r[:lo]) + ell
}

// wrapLines breaks s into at most maxLines lines of width maxW; the last
// line is ellipsized.
func wrapLines(face font.Face, s string, maxW float64, maxLines int) []string {
	words := splitWords(s)
	var lines []string
	cur := ""
	for i := 0; i < len(words); i++ {
		w := words[i]
		try := w
		if cur != "" {
			try = cur + " " + w
		}
		if textWidth(face, try) <= maxW || cur == "" {
			cur = try
			continue
		}
		lines = append(lines, cur)
		cur = w
		if len(lines) == maxLines-1 {
			rest := cur
			for _, ww := range words[i+1:] {
				rest += " " + ww
			}
			lines = append(lines, ellipsize(face, rest, maxW))
			return lines
		}
	}
	if cur != "" {
		lines = append(lines, ellipsize(face, cur, maxW))
	}
	return lines
}

func splitWords(s string) []string {
	var words []string
	cur := []rune{}
	for _, r := range s {
		if r == ' ' {
			if len(cur) > 0 {
				words = append(words, string(cur))
				cur = cur[:0]
			}
			continue
		}
		cur = append(cur, r)
	}
	if len(cur) > 0 {
		words = append(words, string(cur))
	}
	return words
}
