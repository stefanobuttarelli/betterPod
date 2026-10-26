package main

// ij_swar.go — SWAR pixel helpers: process the R,B and G,A channel pairs of
// a packed RGBA pixel (little-endian uint32) with one multiply each.

import (
	"image"
	"unsafe"
)

// (the H700 is little-endian, so byte 0 of a pixel is the low byte)

// pix32 views the pixels of img as packed uint32 values; the second result
// is the stride in pixels.
func pix32(img *image.RGBA) ([]uint32, int) {
	if len(img.Pix) < 4 {
		return nil, 0
	}
	return unsafe.Slice((*uint32)(unsafe.Pointer(&img.Pix[0])), len(img.Pix)/4), img.Stride / 4
}

// lerp32 blends a → b by t/256 (t in 0..256), all four channels.
func lerp32(a, b, t uint32) uint32 {
	it := 256 - t
	rb := ((a&0xff00ff)*it + (b&0xff00ff)*t) >> 8 & 0xff00ff
	ag := ((a>>8&0xff00ff)*it + (b>>8&0xff00ff)*t) & 0xff00ff00
	return rb | ag
}

// scale32 multiplies all four channels by s/256.
func scale32(c, s uint32) uint32 {
	rb := (c & 0xff00ff) * s >> 8 & 0xff00ff
	ag := (c >> 8 & 0xff00ff) * s & 0xff00ff00
	return rb | ag
}

// over32 composites premultiplied src over dst.
func over32(src, dst uint32) uint32 {
	a := src >> 24
	if a >= 255 {
		return src
	}
	if a == 0 {
		return dst
	}
	return src + scale32(dst, 256-(a+a>>7))
}
