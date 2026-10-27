package main

// ij_lossless.go — the "Lossless" badge under the song in Now Playing.
//
// For lossless files (FLAC, ALAC, WAV, AIFF) a small outlined pill shows a
// little wave and LOSSLESS, with the bit depth and sample rate on a small
// line under it ("24-bit · 96 kHz"). It is KnulliPod's own
// design, drawn in the theme's secondary text colour (light and dark).
// The format comes from the file headers, read once per track in the
// background.

import (
	"fmt"
	"image"
	"math"
)

const (
	losslessBadgeH   = 13.0 // CSS px
	losslessBadgeGap = 7.0
	// the bit depth / sample rate line under the pill
	losslessDetailGap = 3.0
)

// losslessBadgeHeight: the pill and, when known, the quality line.
func losslessBadgeHeight(af audioFormat) float64 {
	h := losslessBadgeH
	if af.bits > 0 && af.rate > 0 {
		lh, _ := lineBox(10)
		h += losslessDetailGap + lh
	}
	return h
}

var trackFormats = map[*Track]audioFormat{}
var trackFormatPending = map[*Track]bool{}

// trackFormat returns the format of t once it has been read (the first call
// starts the read and redraws when it is done).
func (ui *IpodUI) trackFormat(t *Track) (audioFormat, bool) {
	if t == nil {
		return audioFormat{}, false
	}
	if f, ok := trackFormats[t]; ok {
		return f, true
	}
	if !trackFormatPending[t] {
		trackFormatPending[t] = true
		path := t.Path
		go func() {
			f := probeFormat(path)
			ui.post(func() {
				trackFormats[t] = f
				delete(trackFormatPending, t)
				ui.dirty = true
			})
		}()
	}
	return audioFormat{}, false
}

// drawLosslessBadge draws the badge with its left edge at x, top at y (CSS
// px), within maxW.
func (ui *IpodUI) drawLosslessBadge(dst *image.RGBA, af audioFormat, x, y, maxW, alpha float64) {
	if alpha <= 0.002 {
		return
	}
	label := "LOSSLESS"
	detail := ""
	if af.bits > 0 && af.rate > 0 {
		detail = fmt.Sprintf("%d-bit · %s kHz", af.bits, khz(af.rate))
	}
	c := th.npSub
	lf := face(W700, 8)
	const track = 0.7 // letter spacing (CSS px)
	lw := 0.0
	for _, r := range label {
		lw += textWidth(lf, string(r))/DPR + track
	}
	lw -= track
	pad := 5.0
	const waveW, waveGap = 12.0, 3.5
	pw := pad + waveW + waveGap + lw + pad
	if pw > maxW {
		return
	}
	d := DPR
	strokeRounded(dst, x*d, y*d, (x+pw)*d, (y+losslessBadgeH)*d, 3*d, 1.2*d, c, alpha)
	base := y + losslessBadgeH/2 + lineCapHeight(8)/2
	col := Col{c.R, c.G, c.B, alpha}
	// a little wave, then LOSSLESS
	drawWave(dst, (x+pad)*d, (y+losslessBadgeH/2)*d, waveW*d, 2.3*d, 1.25*d, col)
	tx := x + pad + waveW + waveGap
	for _, r := range label {
		drawText(dst, lf, string(r), tx*d, base*d, col)
		tx += textWidth(lf, string(r))/DPR + track
	}
	if detail != "" {
		// the quality on a small line under the pill
		df := face(W600, 10)
		_, dasc := lineBox(10)
		drawText(dst, df, ellipsize(df, detail, maxW*d), x*d, (y+losslessBadgeH+losslessDetailGap+dasc)*d, col)
	}
}

// khz: 44100 → "44.1", 96000 → "96".
func khz(rate int) string {
	k := float64(rate) / 1000
	if math.Abs(k-math.Round(k)) < 0.05 {
		return fmt.Sprintf("%d", int(math.Round(k)))
	}
	return fmt.Sprintf("%.1f", k)
}

// drawWave strokes one and a half periods of a sine wave, w wide and amp
// high (device px), centred vertically on cy, with a round pen lw thick.
func drawWave(dst *image.RGBA, x0, cy, w, amp, lw float64, c Col) {
	const n = 96
	var pts [n + 1][2]float64
	for i := range pts {
		t := float64(i) / n
		pts[i] = [2]float64{x0 + t*w, cy - amp*math.Sin(t*3*math.Pi)}
	}
	r := lw / 2
	b := dst.Bounds()
	for py := int(cy - amp - lw - 1); py <= int(cy+amp+lw+1); py++ {
		for px := int(x0 - lw - 1); px <= int(x0+w+lw+1); px++ {
			if px < b.Min.X || px >= b.Max.X || py < b.Min.Y || py >= b.Max.Y {
				continue
			}
			fx, fy := float64(px)+0.5, float64(py)+0.5
			best := math.Inf(1)
			for i := 0; i < n; i++ {
				// distance to the segment pts[i]–pts[i+1]
				ax, ay := pts[i][0], pts[i][1]
				bx, by := pts[i+1][0], pts[i+1][1]
				vx, vy := bx-ax, by-ay
				t := ((fx-ax)*vx + (fy-ay)*vy) / (vx*vx + vy*vy)
				t = math.Max(0, math.Min(1, t))
				dx, dy := fx-(ax+t*vx), fy-(ay+t*vy)
				if dd := dx*dx + dy*dy; dd < best {
					best = dd
				}
			}
			cov := r + 0.5 - math.Sqrt(best)
			if cov <= 0 {
				continue
			}
			if cov > 1 {
				cov = 1
			}
			o := dst.PixOffset(px, py)
			blendPx(dst.Pix[o:o+4], c, c.A*cov)
		}
	}
}
