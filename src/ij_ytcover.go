package main

// ij_ytcover.go — a square cover from a video's (16:9) thumbnail:
//
//  1. the cover is in the picture with bars around it (a still image of
//     the album on a black or plain background, as full-album uploads
//     often are): it is cut out exactly;
//  2. otherwise: the whole picture, sharp, in the middle of a square made
//     of the same picture enlarged, blurred and darkened (nothing is cut
//     off: titles and faces stay).
//
// (A square thumbnail, as YouTube Music has for its tracks, is used as it
// is: the caller picks it first.)

import (
	"bytes"
	"image"
	"image/draw"
	"image/jpeg"
	"math"
)

const ytCoverSize = 800

// squareCover returns the cover as JPEG.
func squareCover(src image.Image) ([]byte, error) {
	var out image.Image
	b := src.Bounds()
	if r, ok := framedCover(src); ok {
		out = scaleImage(cropTo(src, r), ytCoverSize, ytCoverSize)
	} else if math.Abs(float64(b.Dx())/float64(b.Dy())-1) < 0.04 {
		out = scaleImage(src, ytCoverSize, ytCoverSize) // already square
	} else {
		out = blurFillSquare(src, ytCoverSize)
	}
	var buf bytes.Buffer
	if err := jpeg.Encode(&buf, out, &jpeg.Options{Quality: 90}); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func cropTo(src image.Image, r image.Rectangle) image.Image {
	dst := image.NewRGBA(image.Rect(0, 0, r.Dx(), r.Dy()))
	draw.Draw(dst, dst.Bounds(), src, r.Min, draw.Src)
	return dst
}

// framedCover finds a roughly square picture on a plain background (the
// colour of the edges): its rectangle, made exactly square.
func framedCover(src image.Image) (image.Rectangle, bool) {
	b := src.Bounds()
	w, h := b.Dx(), b.Dy()
	if w < 64 || h < 64 {
		return image.Rectangle{}, false
	}
	at := func(x, y int) (int, int, int) {
		r, g, bb, _ := src.At(b.Min.X+x, b.Min.Y+y).RGBA()
		return int(r >> 8), int(g >> 8), int(bb >> 8)
	}
	// the background: the average of the left and right edge columns,
	// which must be plain
	var sr, sg, sb, n int
	for y := 0; y < h; y += 2 {
		for _, x := range []int{1, w - 2} {
			r, g, bl := at(x, y)
			sr, sg, sb, n = sr+r, sg+g, sb+bl, n+1
		}
	}
	br, bg, bb := sr/n, sg/n, sb/n
	differs := func(x, y int) bool {
		r, g, bl := at(x, y)
		d := abs(r-br) + abs(g-bg) + abs(bl-bb)
		return d > 48
	}
	// the edges must be plain (else it's a normal picture)
	edgeOdd := 0
	for y := 0; y < h; y += 2 {
		if differs(1, y) || differs(w-2, y) {
			edgeOdd++
		}
	}
	if edgeOdd > h/2/20 {
		return image.Rectangle{}, false
	}
	// columns and rows with content
	colHas := func(x int) bool {
		c := 0
		for y := 0; y < h; y += 3 {
			if differs(x, y) {
				c++
			}
		}
		return c > h/3/25
	}
	rowHas := func(y, x0, x1 int) bool {
		c := 0
		for x := x0; x < x1; x += 3 {
			if differs(x, y) {
				c++
			}
		}
		return c > (x1-x0)/3/25
	}
	x0, x1 := 0, w-1
	for x0 < w && !colHas(x0) {
		x0++
	}
	for x1 > x0 && !colHas(x1) {
		x1--
	}
	if x1-x0 < w/4 {
		return image.Rectangle{}, false
	}
	y0, y1 := 0, h-1
	for y0 < h && !rowHas(y0, x0, x1) {
		y0++
	}
	for y1 > y0 && !rowHas(y1, x0, x1) {
		y1--
	}
	cw, ch := x1-x0+1, y1-y0+1
	ratio := float64(cw) / float64(ch)
	// a square-ish picture that leaves clear bars
	if ratio < 0.9 || ratio > 1.12 || float64(cw) > 0.85*float64(w) {
		return image.Rectangle{}, false
	}
	// exactly square, centred on the found picture
	s := min(cw, ch)
	cx, cy := x0+cw/2, y0+ch/2
	r := image.Rect(cx-s/2, cy-s/2, cx-s/2+s, cy-s/2+s).Add(b.Min)
	return r.Intersect(b), true
}

func abs(v int) int {
	if v < 0 {
		return -v
	}
	return v
}

// blurFillSquare: the picture whole, in the middle of a square of itself
// enlarged, blurred and darkened.
func blurFillSquare(src image.Image, size int) image.Image {
	b := src.Bounds()
	// the background: cover-crop to a square, shrink a lot, enlarge back
	// (a cheap, very soft blur), darken
	s := min(b.Dx(), b.Dy())
	sq := image.Rect(b.Min.X+(b.Dx()-s)/2, b.Min.Y+(b.Dy()-s)/2, b.Min.X+(b.Dx()-s)/2+s, b.Min.Y+(b.Dy()-s)/2+s)
	small := scaleImage(cropTo(src, sq), 12, 12)
	bg := scaleImage(scaleImage(small, 48, 48), size, size)
	fillRect(bg, 0, 0, size, size, Col{0, 0, 0, 0.28})
	// the picture, 92% of the width, with a soft shadow
	fw := int(float64(size) * 0.92)
	fh := int(float64(fw) * float64(b.Dy()) / float64(b.Dx()))
	if fh > fw {
		fh, fw = fw, int(float64(fw)*float64(b.Dx())/float64(b.Dy()))
	}
	fx, fy := (size-fw)/2, (size-fh)/2
	for i, a := range []float64{0.10, 0.08, 0.06, 0.04, 0.02} {
		g := i + 1
		fillRect(bg, fx-g, fy-g+3, fw+2*g, fh+2*g, Col{0, 0, 0, a})
	}
	pic := scaleImage(src, fw, fh)
	draw.Draw(bg, image.Rect(fx, fy, fx+fw, fy+fh), pic, image.Point{}, draw.Src)
	return bg
}
