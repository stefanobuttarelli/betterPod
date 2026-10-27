package main

// ij_lossless.go — the "Lossless" badge under the song in Now Playing.
//
// For lossless files (FLAC, ALAC, WAV, AIFF): five small rounded bars, a
// level meter that moves softly while the song plays, and LOSSLESS. It is
// KnulliPod's own
// design, drawn in the theme's secondary text colour (light and dark).
// The format comes from the file headers, read once per track in the
// background.

import (
	"fmt"
	"image"
	"math"
	"time"
)

const (
	losslessBadgeH   = 12.0 // CSS px (the tallest bar)
	losslessBadgeGap = 7.0
)

// losslessBadgeHeight: the pill alone (minimal, no figures under it).
func losslessBadgeHeight(af audioFormat) float64 { return losslessBadgeH }

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
// px), within maxW: five rounded bars, like a small level meter, and
// LOSSLESS. While the song plays the bars rise and fall, each at its own
// pace (two slow sines mixed, so they move with some rhythm rather than in
// step); paused, they settle back to the logo's shape.
func (ui *IpodUI) drawLosslessBadge(dst *image.RGBA, af audioFormat, x, y, maxW, alpha float64, live float64, now time.Time) {
	if alpha <= 0.002 {
		return
	}
	c := th.npSub
	col := Col{c.R, c.G, c.B, alpha}
	d := DPR
	const barW, barGap = 2.2, 1.7
	base := [5]float64{0.42, 0.72, 1, 0.72, 0.42} // the logo's shape
	// each bar: two frequencies (rad/s) and phases
	freq := [5][2]float64{{2.3, 5.1}, {3.1, 1.7}, {1.9, 4.3}, {2.7, 6.1}, {3.6, 2.2}}
	phase := [5][2]float64{{0, 1.3}, {2.1, 0.4}, {4.0, 2.8}, {1.2, 5.0}, {3.3, 0.9}}
	t := float64(now.UnixNano()%int64(600*time.Second)) / 1e9
	cy := y + losslessBadgeH/2
	full := losslessBadgeH
	bx := x
	for i := range base {
		h := base[i]
		if live > 0 {
			m := 0.5 + 0.32*math.Sin(t*freq[i][0]+phase[i][0]) + 0.18*math.Sin(t*freq[i][1]+phase[i][1])
			// half the logo's shape, half free movement (25–100%), so the
			// middle stays the tallest on average
			mv := 0.5*base[i] + 0.5*(0.25+0.75*m)
			h = h*(1-live) + mv*live
		}
		bh := math.Max(barW, full*h)
		fillRoundedGradient(dst, bx*d, (cy-bh/2)*d, (bx+barW)*d, (cy+bh/2)*d, barW/2*d,
			[]GradStop{{0, col}, {1, col}}, 1)
		bx += barW + barGap
	}
	// LOSSLESS, letter-spaced
	lf := face(W700, 9)
	const track = 0.9
	tx := bx + 3.5
	if tx > x+maxW {
		return
	}
	baseLine := cy + lineCapHeight(9)/2
	for _, r := range "LOSSLESS" {
		drawText(dst, lf, string(r), tx*d, baseLine*d, col)
		tx += textWidth(lf, string(r))/DPR + track
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
