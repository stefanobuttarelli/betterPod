package main

/*
#include "cdraw.c"
*/
import "C"

// cdraw.go — the hottest pixel loops of the interface (blitShiftX,
// darkenColumns, dimRect, drawTex, a fading blit) run in C (cdraw.c), with the same
// arithmetic as the Go code they replace (tools/onion/unittest checks it
// pixel for pixel). On the Miyoo's Cortex-A7 the Go loops were most of a
// frame; clang vectorises them and drops the bounds checks
// (tools/onion/bench.sh compares them). MIYOOPOD_GO_DRAW=1 keeps the Go
// versions. Elsewhere the Go loops stay, unless MIYOOPOD_C_DRAW=1: the C
// ones haven't been tried on the H700.

import (
	"image"
	"os"
	"unsafe"
)

var cDraw = os.Getenv("MIYOOPOD_GO_DRAW") != "1" && (isOnion || os.Getenv("MIYOOPOD_C_DRAW") == "1")

func pixPtr(img *image.RGBA, off int) *C.uint8_t {
	return (*C.uint8_t)(unsafe.Pointer(&img.Pix[off]))
}

func pix32Ptr(p []uint32) *C.uint32_t { return (*C.uint32_t)(unsafe.Pointer(&p[0])) }

// cBlit: the clipped w x h area of blit, a = opacity in 0..255.
func cBlit(dst *image.RGBA, do int, src *image.RGBA, so, w, h int, a uint32) {
	C.cd_blit(pixPtr(dst, do), C.int(dst.Stride), pixPtr(src, so), C.int(src.Stride), C.int(w), C.int(h), C.uint32_t(a))
}

// cShiftX: the source rows ya..yb of blitShiftX.
func cShiftX(dp []uint32, dst32 int, sp []uint32, sst, w, y, ix, xa, xb, ya, yb int, wf, op32 uint32) {
	C.cd_shiftx(pix32Ptr(dp), C.int(dst32), pix32Ptr(sp), C.int(sst), C.int(w), C.int(y),
		C.int(ix), C.int(xa), C.int(xb), C.int(ya), C.int(yb), C.uint32_t(wf), C.uint32_t(op32))
}

var darkenBuf []uint16

// cDarken: darkenColumns over the clipped columns cx0..cx1 and rows
// ry0..ry1; alpha[i] belongs to column x0 + i.
func cDarken(dst *image.RGBA, x0, cx0, cx1, ry0, ry1 int, alpha []float64) {
	n := cx1 - cx0
	if n <= 0 || ry1 <= ry0 {
		return
	}
	if cap(darkenBuf) < n {
		darkenBuf = make([]uint16, n)
	}
	m := darkenBuf[:n]
	for i := range m {
		a := alpha[cx0-x0+i]
		if a <= 0 {
			m[i] = 256
		} else {
			m[i] = uint16(uint32(int((1 - a) * 256)))
		}
	}
	C.cd_darken(pixPtr(dst, dst.PixOffset(cx0, ry0)), C.int(dst.Stride), C.int(n), C.int(ry1-ry0),
		(*C.uint16_t)(unsafe.Pointer(&m[0])))
}

// cDrawTex: the rows ya..yb of drawTex.
func cDrawTex(dp []uint32, dst32, x0, x1, ya, yb int, sp []uint32, sst, iw, ih int, hi [9]float64, t texMap, op uint32) {
	tm := [11]float64{t.su, t.ou, t.sv, t.ov, t.u0, t.v0, t.u1, t.v1, t.reflFrom, t.reflMax, t.reflStart}
	C.cd_drawtex(pix32Ptr(dp), C.int(dst32), C.int(x0), C.int(x1), C.int(ya), C.int(yb),
		pix32Ptr(sp), C.int(sst), C.int(iw), C.int(ih),
		(*C.double)(unsafe.Pointer(&hi[0])), (*C.double)(unsafe.Pointer(&tm[0])), C.uint32_t(op), C.double(DPR))
}

// cDim: dimRect over the clipped area x0..x1, y0..y1.
func cDim(dst *image.RGBA, x0, y0, x1, y1 int, m uint32) {
	C.cd_dim(pixPtr(dst, dst.PixOffset(x0, y0)), C.int(dst.Stride), C.int(x1-x0), C.int(y1-y0), C.uint32_t(m))
}
