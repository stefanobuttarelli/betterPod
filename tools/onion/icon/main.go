// Command icon draws betterPod's app icon for Onion OS: an iPod classic in
// the sticker style of Onion's default icons (74x74, white border, dark
// outline, the theme's lilac).
//
//	go run ./tools/onion/icon onion/betterpod/icon.png
package main

import (
	"log"
	"os"

	"github.com/fogleman/gg"
)

const size = 74

var (
	white   = [3]int{242, 242, 242}
	lilac   = [3]int{154, 162, 230}
	outline = [3]int{64, 57, 110}
)

func set(dc *gg.Context, c [3]int, a float64) {
	dc.SetRGBA255(c[0], c[1], c[2], int(a*255))
}

func main() {
	out := "icon.png"
	if len(os.Args) > 1 {
		out = os.Args[1]
	}
	dc := gg.NewContext(size, size)

	// the body of the iPod: 40x60, centred
	x0, y0, w, h, r := 17.0, 7.0, 40.0, 60.0, 9.0

	// sticker: soft shadow, then the white border around the body
	set(dc, outline, 0.18)
	dc.DrawRoundedRectangle(x0-4, y0-3, w+8, h+8, r+4)
	dc.Fill()
	set(dc, white, 1)
	dc.DrawRoundedRectangle(x0-4, y0-4, w+8, h+8, r+4)
	dc.Fill()

	// body
	set(dc, lilac, 1)
	dc.DrawRoundedRectangle(x0, y0, w, h, r)
	dc.Fill()
	set(dc, outline, 1)
	dc.SetLineWidth(2.5)
	dc.DrawRoundedRectangle(x0, y0, w, h, r)
	dc.Stroke()

	// screen with two list rows
	sx, sy, sw, sh := x0+5, y0+5, w-10, 22.0
	set(dc, white, 1)
	dc.DrawRoundedRectangle(sx, sy, sw, sh, 3)
	dc.Fill()
	set(dc, outline, 1)
	dc.SetLineWidth(2)
	dc.DrawRoundedRectangle(sx, sy, sw, sh, 3)
	dc.Stroke()
	set(dc, lilac, 1)
	dc.DrawRoundedRectangle(sx+3, sy+4, sw-6, 5, 1.5)
	dc.Fill()
	set(dc, outline, 0.55)
	dc.SetLineWidth(1.6)
	dc.SetLineCapRound()
	dc.DrawLine(sx+5, sy+14, sx+sw-9, sy+14)
	dc.Stroke()
	dc.DrawLine(sx+5, sy+18.5, sx+sw-13, sy+18.5)
	dc.Stroke()

	// click wheel and centre button
	cx, cy := x0+w/2, y0+h-16
	set(dc, white, 1)
	dc.DrawCircle(cx, cy, 11.5)
	dc.Fill()
	set(dc, outline, 1)
	dc.SetLineWidth(2)
	dc.DrawCircle(cx, cy, 11.5)
	dc.Stroke()
	set(dc, lilac, 1)
	dc.DrawCircle(cx, cy, 4.2)
	dc.Fill()
	set(dc, outline, 1)
	dc.SetLineWidth(1.5)
	dc.DrawCircle(cx, cy, 4.2)
	dc.Stroke()

	if err := dc.SavePNG(out); err != nil {
		log.Fatal(err)
	}
}
