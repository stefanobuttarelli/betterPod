package main

// ij_vizclock.go — the Visualizer's big clock as frosted glass: thin digits
// (Inter Display Light) filled with what is behind them, blurred and
// lightened by 30% white, with a bright rim.
//
// The digits' shape (and its rim) is made once a minute; each frame only
// the part of the screen behind them is blurred, at a quarter of the size
// (a few thousand pixels), and only the pixels inside the digits are drawn.

import (
	"image"
)

const (
	vcSize  = 75 // CSS px
	vcWhite = 0.30
	vcRim   = 0.75
	vcQ     = 4 // the blur works at 1/vcQ of the size
	vcPad   = 12
)

var vizClock struct {
	text       string
	w, h       int
	base       int     // baseline from the top of the mask
	mask, rim  []uint8 // w*h
	small, tmp []uint32
}

// vcMake: the digits' mask and rim for text.
func vcMake(text string) {
	c := &vizClock
	f := face(W300, vcSize)
	tw := int(textWidth(f, text) + 0.999)
	asc := int(float64(vcSize)*DPR*0.75 + 0.5) // (digits: cap height ~0.73 em)
	c.w = (tw + 2*vcPad + vcQ - 1) / vcQ * vcQ
	c.h = (asc + 2*vcPad + vcQ - 1) / vcQ * vcQ
	c.base = vcPad + asc
	img := image.NewRGBA(image.Rect(0, 0, c.w, c.h))
	drawText(img, f, text, vcPad, float64(c.base), Col{255, 255, 255, 1})
	n := c.w * c.h
	c.mask = make([]uint8, n)
	for i := range c.mask {
		c.mask[i] = img.Pix[i*4+3]
	}
	// the rim: the mask less its erosion (2 px), softened, 75%
	ero := vcMin(vcMin(c.mask, c.w, c.h, 1, 0), c.w, c.h, 0, 1)
	edge := make([]uint8, n)
	for i := range edge {
		if d := int(c.mask[i]) - int(ero[i]); d > 0 {
			edge[i] = uint8(d)
		}
	}
	c.rim = make([]uint8, n)
	for y := 1; y < c.h-1; y++ {
		for x := 1; x < c.w-1; x++ {
			i := y*c.w + x
			s := 4*int(edge[i]) + 2*(int(edge[i-1])+int(edge[i+1])+int(edge[i-c.w])+int(edge[i+c.w])) +
				int(edge[i-c.w-1]) + int(edge[i-c.w+1]) + int(edge[i+c.w-1]) + int(edge[i+c.w+1])
			c.rim[i] = uint8(min(255, s*int(vcRim*256)/(16*256)))
		}
	}
	c.text = text
	c.small = make([]uint32, n/(vcQ*vcQ)*3)
	c.tmp = make([]uint32, len(c.small))
}

// vcMin: a 5-wide minimum along x (dx=1) or y (dy=1).
func vcMin(src []uint8, w, h, dx, dy int) []uint8 {
	out := make([]uint8, len(src))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			m := uint8(255)
			for k := -2; k <= 2; k++ {
				xx, yy := x+k*dx, y+k*dy
				if xx < 0 || yy < 0 || xx >= w || yy >= h {
					m = 0
					break
				}
				if v := src[yy*w+xx]; v < m {
					m = v
				}
			}
			out[y*w+x] = m
		}
	}
	return out
}

// vizClockFrost draws text centred on x 320, its digits' middle at cy
// (device px), faded by a.
func vizClockFrost(dst *image.RGBA, text string, cy int, a float64) {
	c := &vizClock
	if c.text != text {
		vcMake(text)
	}
	x0 := 320 - (c.w-2*vcPad)/2 - vcPad
	y0 := cy - c.h/2
	x0 -= x0 & (vcQ - 1) // (aligned, so the small grid stays put)
	y0 -= y0 & (vcQ - 1)
	r := image.Rect(x0, y0, x0+c.w, y0+c.h)
	if !r.In(dst.Bounds()) {
		return
	}
	// what is behind, at 1/vcQ (box average)
	sw, sh := c.w/vcQ, c.h/vcQ
	sm := c.small
	for sy := 0; sy < sh; sy++ {
		for sx := 0; sx < sw; sx++ {
			var rr, gg, bb uint32
			for yy := 0; yy < vcQ; yy++ {
				o := dst.PixOffset(x0+sx*vcQ, y0+sy*vcQ+yy)
				p := dst.Pix[o : o+vcQ*4]
				for xx := 0; xx < vcQ*4; xx += 4 {
					rr += uint32(p[xx])
					gg += uint32(p[xx+1])
					bb += uint32(p[xx+2])
				}
			}
			i := (sy*sw + sx) * 3
			sm[i], sm[i+1], sm[i+2] = rr/(vcQ*vcQ), gg/(vcQ*vcQ), bb/(vcQ*vcQ)
		}
	}
	// a box blur (radius 2 small px, twice: ~ a 12 px Gaussian)
	for pass := 0; pass < 2; pass++ {
		vcBox(sm, c.tmp, sw, sh, 1, 0)
		vcBox(c.tmp, sm, sw, sh, 0, 1)
	}
	// inside the digits: blurred * 0.7 + 30% white, then the rim
	ka := uint32(a*256 + 0.5)
	for y := 0; y < c.h; y++ {
		// (bilinear between the small rows/columns)
		fy := (y*256+128)/vcQ - 128
		sy0 := min(max(fy>>8, 0), sh-1)
		sy1 := min(sy0+1, sh-1)
		wy := uint32(fy & 255)
		if fy < 0 {
			wy = 0
		}
		row := dst.Pix[dst.PixOffset(x0, y0+y):]
		mrow := c.mask[y*c.w : (y+1)*c.w]
		rrow := c.rim[y*c.w : (y+1)*c.w]
		for x, m := range mrow {
			if m == 0 {
				continue
			}
			fx := (x*256+128)/vcQ - 128
			sx0 := min(max(fx>>8, 0), sw-1)
			sx1 := min(sx0+1, sw-1)
			wx := uint32(fx & 255)
			if fx < 0 {
				wx = 0
			}
			i00, i01 := (sy0*sw+sx0)*3, (sy0*sw+sx1)*3
			i10, i11 := (sy1*sw+sx0)*3, (sy1*sw+sx1)*3
			d := row[x*4 : x*4+4 : x*4+4]
			al := uint32(m) * ka >> 8 // 0..255
			rm := uint32(rrow[x]) * ka >> 8
			for ch := 0; ch < 3; ch++ {
				t := (sm[i00+ch]*(256-wx)+sm[i01+ch]*wx)>>8*(256-wy) + (sm[i10+ch]*(256-wx)+sm[i11+ch]*wx)>>8*wy
				v := (t>>8)*179>>8 + 77 // 70% + 30% white
				o := (v*al + uint32(d[ch])*(255-al)) / 255
				o += (255 - o) * rm / 255
				d[ch] = uint8(o)
			}
		}
	}
}

// vcBox: a 5-wide box blur along x or y (rgb triples).
func vcBox(src, dst []uint32, w, h, dx, dy int) {
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			var r, g, b uint32
			for k := -2; k <= 2; k++ {
				xx := min(max(x+k*dx, 0), w-1)
				yy := min(max(y+k*dy, 0), h-1)
				i := (yy*w + xx) * 3
				r += src[i]
				g += src[i+1]
				b += src[i+2]
			}
			i := (y*w + x) * 3
			dst[i], dst[i+1], dst[i+2] = r/5, g/5, b/5
		}
	}
}
