package main

// ij_loading.go — the loading screen shown while the library is scanned:
// an iOS-style activity spinner (12 fading spokes) on the theme
// background, drawn straight to the framebuffer until done is closed.

import (
	"image"
	"math"
	"os"
	"time"
	"unsafe"
)

// ipod.js UI by default (MIYOOPOD_CLASSIC_UI=1: classic screens)
var useIpodUI = os.Getenv("MIYOOPOD_CLASSIC_UI") == ""

// drawSpinner draws the spinner centred at (cx, cy) device px, sizeCSS wide.
func drawSpinner(dst *image.RGBA, cx, cy, sizeCSS float64, now time.Time) {
	opac := []float64{1, 0.917, 0.833, 0.75, 0.667, 0.583, 0.5, 0.417, 0.333, 0.25, 0.167, 0.083}
	step := int(float64(now.UnixNano()%833333333)/833333333*12) % 12
	scale := sizeCSS * DPR / 2400
	stroke := 200 * scale
	for i := 0; i < 12; i++ {
		// spoke i, rotated 30° per step
		ang := (float64(-i*30) + float64(step*30)) * math.Pi / 180
		r0, r1 := 600*scale, 1100*scale
		x0 := cx + math.Sin(ang)*r0
		y0 := cy - math.Cos(ang)*r0
		x1 := cx + math.Sin(ang)*r1
		y1 := cy - math.Cos(ang)*r1
		drawThickLine(dst, x0, y0, x1, y1, stroke, Col{0, 0, 0, opac[i]})
	}
}

// drawThickLine draws an antialiased line segment w px thick.
func drawThickLine(dst *image.RGBA, x0, y0, x1, y1, w float64, c Col) {
	minX := int(math.Floor(math.Min(x0, x1) - w))
	maxX := int(math.Ceil(math.Max(x0, x1) + w))
	minY := int(math.Floor(math.Min(y0, y1) - w))
	maxY := int(math.Ceil(math.Max(y0, y1) + w))
	dx, dy := x1-x0, y1-y0
	l2 := dx*dx + dy*dy
	b := dst.Bounds()
	for y := minY; y <= maxY; y++ {
		if y < b.Min.Y || y >= b.Max.Y {
			continue
		}
		for x := minX; x <= maxX; x++ {
			if x < b.Min.X || x >= b.Max.X {
				continue
			}
			px, py := float64(x)+0.5, float64(y)+0.5
			t := ((px-x0)*dx + (py-y0)*dy) / l2
			t = math.Max(0, math.Min(1, t))
			d := math.Hypot(px-(x0+t*dx), py-(y0+t*dy)) - w*0.5
			cov := math.Max(0, math.Min(1, 0.5-d))
			if cov > 0 {
				o := dst.PixOffset(x, y)
				blendPx(dst.Pix[o:o+4], c, c.A*cov)
			}
		}
	}
}

// ipodLoadingScreen animates the spinner until done is closed.
func ipodLoadingScreen(app *MiyooPod, done chan struct{}) {
	fb := newCanvas(640, 480)
	for {
		select {
		case <-done:
			return
		default:
		}
		for C_GetKeyPress() != -1 {
		}
		fillRect(fb, 0, 0, 640, 480, th.bg)
		drawSpinner(fb, 320, 240, 24, time.Now())
		refreshFramebuffer(unsafe.Pointer(&fb.Pix[0]))
		time.Sleep(40 * time.Millisecond)
	}
}
