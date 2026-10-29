package main

// ij_vizcolors.go — the Visualizer's colours (A changes them, for every
// effect) and the Spectrogram effect.
//
// A scheme is a run of colours from dark to light. Each effect takes from
// it what it needs: the Bars their gradient (bottom to top), the Spectrogram
// its heat (silence to loud), Waves, Tunnel and Stars a cycle of hues going
// along the scheme and back. "Classic" keeps each effect's own colours (blue
// bars, rainbow hues, a magma heat); "Album" is made from the cover of the
// song that plays, and follows it.

import (
	"image"
	"math"
	"sort"
	"time"
)

type vizScheme struct {
	name  string
	stops []Col
}

var vizSchemes = []vizScheme{
	{"Classic", nil},
	{"Album", nil},
	{"Magma", []Col{{0, 0, 4, 1}, {40, 11, 84, 1}, {101, 21, 110, 1}, {159, 42, 99, 1}, {212, 72, 66, 1}, {245, 125, 21, 1}, {250, 193, 39, 1}, {252, 255, 164, 1}}},
	{"Aurora", []Col{{2, 8, 20, 1}, {0, 70, 90, 1}, {0, 170, 140, 1}, {90, 230, 160, 1}, {150, 120, 255, 1}, {230, 205, 255, 1}}},
	{"Sunset", []Col{{10, 4, 30, 1}, {90, 20, 110, 1}, {220, 50, 120, 1}, {255, 120, 80, 1}, {255, 200, 90, 1}, {255, 245, 205, 1}}},
	{"Ocean", []Col{{2, 6, 30, 1}, {10, 40, 120, 1}, {30, 110, 230, 1}, {110, 200, 255, 1}, {220, 245, 255, 1}}},
	{"Phosphor", []Col{{0, 8, 2, 1}, {0, 60, 20, 1}, {20, 160, 60, 1}, {110, 255, 140, 1}, {225, 255, 225, 1}}},
	{"Mono", []Col{{0, 0, 0, 1}, {55, 56, 60, 1}, {150, 151, 156, 1}, {245, 245, 248, 1}}},
}

// saverColors: the scheme chosen (kept in the prefs).
var saverColors = 0

// the magma heat Classic gives the Spectrogram
var vizMagma = vizSchemes[2].stops

// schemeAt: the colour at t (0-1) along stops.
func schemeAt(stops []Col, t float64) Col {
	t = math.Max(0, math.Min(1, t)) * float64(len(stops)-1)
	i := min(int(t), len(stops)-2)
	return lerpCol(stops[i], stops[i+1], t-float64(i))
}

// applyColors makes the effects' colours from the scheme (and al's cover,
// for Album: nil while it isn't ready, then Classic's colours meanwhile).
func (v *visualizer) applyColors(stops []Col) {
	classic := stops == nil
	heat := stops
	if classic {
		heat = vizMagma
	}
	for i := range v.heat {
		c := schemeAt(heat, float64(i)/255)
		v.heat[i] = [3]uint8{uint8(c.R), uint8(c.G), uint8(c.B)}
	}
	// the hues: Classic's rainbow, or along the scheme (its light half
	// and a little below) and back, so the cycle has no seam
	for i := range v.palette {
		t := float64(i) / 1024
		if classic {
			r, g, b := hsvRGB(t, 0.85, 1)
			v.palette[i] = [3]uint8{r, g, b}
			continue
		}
		u := 1 - math.Abs(2*t-1)
		c := schemeAt(stops, 0.35+0.65*u)
		v.palette[i] = [3]uint8{uint8(c.R), uint8(c.G), uint8(c.B)}
	}
	if classic {
		v.barBottom, v.barTop, v.barCap = Col{30, 110, 230, 1}, Col{150, 230, 255, 1}, Col{230, 245, 255, 1}
	} else {
		v.barBottom, v.barTop = schemeAt(stops, 0.4), schemeAt(stops, 0.92)
		v.barCap = lerpCol(schemeAt(stops, 1), Col{255, 255, 255, 1}, 0.4)
	}
	v.makeBars()
}

// coverStops: a scheme from a cover: its colours by brightness (the darker
// quarter left out), livelier, from near black to a light tint.
func coverStops(img *image.RGBA) []Col {
	if img == nil {
		return nil
	}
	w, h := img.Bounds().Dx(), img.Bounds().Dy()
	type px struct {
		c   Col
		lum float64
	}
	var all []px
	const n = 14
	for j := 0; j < n; j++ {
		for i := 0; i < n; i++ {
			o := img.PixOffset(i*w/n+w/(2*n), j*h/n+h/(2*n))
			c := Col{float64(img.Pix[o]), float64(img.Pix[o+1]), float64(img.Pix[o+2]), 1}
			all = append(all, px{c, 0.3*c.R + 0.59*c.G + 0.11*c.B})
		}
	}
	sort.Slice(all, func(a, b int) bool { return all[a].lum < all[b].lum })
	pick := func(q float64) Col {
		// the average of the colours round quantile q, more saturated
		i0 := int(q * float64(len(all)-1))
		var c Col
		k := 0
		for i := max(0, i0-6); i <= min(len(all)-1, i0+6); i++ {
			c.R += all[i].c.R
			c.G += all[i].c.G
			c.B += all[i].c.B
			k++
		}
		c = Col{c.R / float64(k), c.G / float64(k), c.B / float64(k), 1}
		m := (c.R + c.G + c.B) / 3
		s := func(x float64) float64 { return math.Max(0, math.Min(255, m+(x-m)*1.45)) }
		c = Col{s(c.R), s(c.G), s(c.B), 1}
		// never too dark to be seen
		if l := 0.3*c.R + 0.59*c.G + 0.11*c.B; l < 60 {
			f := 60 / math.Max(l, 1)
			c = Col{math.Min(255, c.R*f), math.Min(255, c.G*f), math.Min(255, c.B*f), 1}
		}
		return c
	}
	c1, c2, c3, c4 := pick(0.3), pick(0.55), pick(0.75), pick(0.93)
	dark := Col{c1.R * 0.12, c1.G * 0.12, c1.B * 0.12, 1}
	return []Col{dark, c1, c2, c3, c4, lerpCol(c4, Col{255, 255, 255, 1}, 0.6)}
}

// vizColorsFrame: the scheme in use, made again when it changes (A) or, for
// Album, when the album does and once its cover is ready.
func (ui *IpodUI) vizColorsFrame(v *visualizer) {
	al := (*Album)(nil)
	if saverColors == 1 {
		al = ui.saverPlaying()
	}
	ready := saverColors != 1 || v.colorsArt
	if v.colorsFor == saverColors && v.colorsAl == al && ready && v.colorsInit {
		return
	}
	var stops []Col
	v.colorsArt = false
	if saverColors == 1 {
		if al != nil {
			img := ui.artCached(al, px(128))
			if img == nil {
				img = ui.art(al, px(128))
			}
			if stops = coverStops(img); stops != nil {
				v.colorsArt = true
			}
		}
		if al == nil {
			v.colorsArt = true // (nothing playing: Classic's colours, nothing to wait for)
		}
	} else {
		stops = vizSchemes[saverColors].stops
	}
	if v.colorsInit && v.colorsFor == saverColors && v.colorsAl == al && !v.colorsArt {
		return // (still waiting for the cover: keep what shows)
	}
	v.applyColors(stops)
	v.colorsFor, v.colorsAl, v.colorsInit = saverColors, al, true
}

// vizNextColors: A: the next scheme, its name shown for a moment.
func (ui *IpodUI) vizNextColors() {
	saverColors = (saverColors + 1) % len(vizSchemes)
	if v := ui.saver.viz; v != nil {
		v.pill, v.nameAt = "Colors · "+vizSchemes[saverColors].name, time.Now()
	}
	ui.savePrefs()
	ui.dirty = true
}

// ---- the Spectrogram ----

const vizSpecStep = 2 // px (half size) it scrolls per frame: ~5 s across

// specRows: for each row of the half-size picture (the top one highest),
// the FFT bins it covers, on the same 40 Hz-16 kHz scale as the bands.
func (v *visualizer) specRows() {
	lo, hi := 40.0, math.Min(16000, float64(v.rate)/2*0.95)
	binHz := float64(v.rate) / vizN
	for r := 0; r < vizH; r++ {
		f1 := lo * math.Pow(hi/lo, 1-float64(r)/vizH)
		f0 := lo * math.Pow(hi/lo, 1-float64(r+1)/vizH)
		v.rowLo[r] = f0 / binHz
		v.rowHi[r] = f1 / binHz
	}
	v.rowsRate = v.rate
}

// drawSpectro: the picture scrolls left; the newest sound comes in at the
// right, a column of heat per frame (low notes at the bottom).
func (v *visualizer) drawSpectro() {
	if v.rowsRate != v.rate {
		v.specRows()
	}
	parallelRows(0, vizH, func(y0, y1 int) {
		for y := y0; y < y1; y++ {
			row := v.buf[y*vizW*4 : (y+1)*vizW*4]
			copy(row, row[vizSpecStep*4:])
		}
	})
	power := func(b float64) float64 {
		i := int(b)
		if i < 1 || i >= vizN/2-1 {
			return 0
		}
		f := b - float64(i)
		pa := real(v.spec[i])*real(v.spec[i]) + imag(v.spec[i])*imag(v.spec[i])
		pb := real(v.spec[i+1])*real(v.spec[i+1]) + imag(v.spec[i+1])*imag(v.spec[i+1])
		return pa + (pb-pa)*f
	}
	for y := 0; y < vizH; y++ {
		x := 0.0
		if v.fresh {
			lo, hi := v.rowLo[y], v.rowHi[y]
			p := power((lo + hi) / 2)
			for b := int(lo) + 1; float64(b) < hi; b++ { // (a wide row: its loudest bin)
				p = math.Max(p, power(float64(b)))
			}
			db := 10*math.Log10(p+1e-12) + 9*(1-float64(y)/vizH)
			x = math.Max(0, math.Min(1, 1+(db-v.norm-4)/60))
		}
		prev := v.specPrev[y]
		v.specPrev[y] = x
		for k := 0; k < vizSpecStep; k++ {
			// (between the last column and this one, so it flows)
			t := prev + (x-prev)*float64(k+1)/vizSpecStep
			c := v.heat[int(t*255+0.5)]
			o := (y*vizW + vizW - vizSpecStep + k) * 4
			v.buf[o], v.buf[o+1], v.buf[o+2], v.buf[o+3] = c[0], c[1], c[2], 255
		}
	}
}

// spectroScale: the frequencies at the left, small and faint.
func (v *visualizer) spectroScale(dst *image.RGBA) {
	lo, hi := 40.0, math.Min(16000, float64(v.rate)/2*0.95)
	f := face(W600, 10)
	_, a := lineBox(10)
	for _, m := range []struct {
		hz   float64
		text string
	}{{100, "100"}, {1000, "1k"}, {10000, "10k"}} {
		if m.hz >= hi {
			continue
		}
		y := (1 - math.Log(m.hz/lo)/math.Log(hi/lo)) * 480
		fillRect(dst, 0, int(y), px(6), 1, Col{255, 255, 255, 0.55})
		drawText(dst, f, m.text, 9*DPR, y+a*DPR/2-1, Col{255, 255, 255, 0.55})
	}
}
