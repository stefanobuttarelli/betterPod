package main

// ij_visualizer.go — the Visualizer screensaver: effects that move with the
// music being heard (Settings → Screensaver → Visualizer; LEFT / RIGHT
// change the effect).
//
// The sound comes from the mixer's output (audio.c's tap: the samples being
// heard now, mono). Each frame takes the last 1024, a Hann window and an
// FFT give 40 bands on a log scale (40 Hz to 16 kHz), with an automatic gain
// so quiet and loud songs both fill the screen, a fast rise and a slower
// fall (like cava), and a beat on a jump of the bass.
//
// The effects:
//   - Bars: the bands as bars, blue going light, with falling caps and a
//     reflection (drawn at full size from ready-made bar pictures).
//   - Waves: the sound's wave, glowing, leaving trails that drift outwards.
//   - Tunnel: the bands round a ring, the picture turning and sinking into
//     the middle (like the old Infinity).
//   - Stars: sparks thrown from the middle on the beats.
//
// The last three draw at half size (320×220) into a picture that fades each
// frame (the trails), then doubled onto the screen. 30 frames a second;
// nothing is drawn while nothing plays.

/*
int audio_vis_read(float *out, int n);
int audio_vis_rate(void);
*/
import "C"

import (
	"image"
	"math"
	"math/cmplx"
	"math/rand"
	"time"
	"unsafe"
)

const (
	vizN     = 1024 // samples per frame
	vizBands = 40
	vizW     = 320 // the half-size picture (the area under the bar, halved)
	vizH     = 220
	vizTop   = 40 // the bar's height (px)
)

var vizNames = []string{"Bars", "Waves", "Tunnel", "Stars"}

// saverEffect: the effect chosen (kept in the prefs).
var saverEffect = 0

type vizParticle struct {
	x, y, vx, vy, life, hue float64
}

type visualizer struct {
	samples [vizN]float32
	win     [vizN]float64
	twid    []complex128
	rev     []int
	spec    []complex128
	bandLo  [vizBands]int
	bandHi  [vizBands]int
	rate    int

	bands, peaks [vizBands]float64
	fall         [vizBands]float64
	norm         float64 // the gain's reference (the recent loudest band, dB)
	bass, bassAv float64
	level        float64
	beat         bool
	beatAt       time.Time
	fresh        bool
	quietSince   time.Time

	hue     float64
	last    time.Time
	nameAt  time.Time // the effect's name shows from here (on start and on a change)
	buf     []byte    // the half-size picture (RGBA) and the one it's made from
	buf2    []byte
	maps    [3][]int32 // the tunnel's moves (source pixel of each pixel)
	mapCur  int
	mapAt   time.Time
	zoomOut []int32 // the waves' drift
	parts   []vizParticle
	rng     *rand.Rand
	bar     *image.RGBA // a bar, light at its top (Bars)
	barRefl *image.RGBA // its reflection
	palette [1024][3]uint8
}

func newVisualizer() *visualizer {
	v := &visualizer{rng: rand.New(rand.NewSource(time.Now().UnixNano())), norm: -60, nameAt: time.Now()}
	for i := range v.win {
		v.win[i] = 0.5 - 0.5*math.Cos(2*math.Pi*float64(i)/float64(vizN-1))
	}
	// the FFT's twiddles and bit-reversed order, once
	v.twid = make([]complex128, vizN/2)
	for i := range v.twid {
		v.twid[i] = cmplx.Exp(complex(0, -2*math.Pi*float64(i)/vizN))
	}
	v.rev = make([]int, vizN)
	bits := 0
	for 1<<bits < vizN {
		bits++
	}
	for i := range v.rev {
		r := 0
		for b := 0; b < bits; b++ {
			if i&(1<<b) != 0 {
				r |= 1 << (bits - 1 - b)
			}
		}
		v.rev[i] = r
	}
	v.spec = make([]complex128, vizN)
	v.setRate(int(C.audio_vis_rate()))
	for i := range v.palette {
		r, g, b := hsvRGB(float64(i)/1024, 0.85, 1)
		v.palette[i] = [3]uint8{r, g, b}
	}
	v.buf = make([]byte, vizW*vizH*4)
	v.buf2 = make([]byte, vizW*vizH*4)
	v.makeMaps()
	v.makeBars()
	return v
}

// setRate places the bands on the FFT's bins for the device's rate.
func (v *visualizer) setRate(rate int) {
	if rate <= 0 {
		rate = 44100
	}
	v.rate = rate
	lo, hi := 40.0, math.Min(16000, float64(rate)/2*0.95)
	binHz := float64(rate) / vizN
	prev := 1
	for b := 0; b < vizBands; b++ {
		f0 := lo * math.Pow(hi/lo, float64(b)/vizBands)
		f1 := lo * math.Pow(hi/lo, float64(b+1)/vizBands)
		i0 := max(prev, int(f0/binHz))
		i1 := max(i0+1, int(f1/binHz)+1)
		i1 = min(i1, vizN/2)
		v.bandLo[b], v.bandHi[b] = i0, i1
		prev = i1
		if prev >= vizN/2 {
			prev = vizN/2 - 1
		}
	}
}

// ---- listening ----

func (v *visualizer) analyse(now time.Time) {
	dt := now.Sub(v.last).Seconds()
	if v.last.IsZero() || dt > 0.2 {
		dt = 1.0 / 30
	}
	v.last = now
	if r := int(C.audio_vis_rate()); r != v.rate {
		v.setRate(r)
	}
	v.fresh = C.audio_vis_read((*C.float)(unsafe.Pointer(&v.samples[0])), vizN) != 0
	// windowed, into the FFT in bit-reversed order
	var sum float64
	for i := 0; i < vizN; i++ {
		x := float64(v.samples[i])
		sum += x * x
		v.spec[v.rev[i]] = complex(x*v.win[i], 0)
	}
	v.level = math.Sqrt(sum / vizN)
	if v.level < 1e-4 {
		v.fresh = false // (paused: the mixer goes on with silence)
	}
	for size := 2; size <= vizN; size <<= 1 {
		half, step := size>>1, vizN/size
		for i := 0; i < vizN; i += size {
			for j := 0; j < half; j++ {
				t := v.twid[j*step] * v.spec[i+j+half]
				u := v.spec[i+j]
				v.spec[i+j], v.spec[i+j+half] = u+t, u-t
			}
		}
	}
	// the bands: the loudest bin of each in decibels (as it's heard), the
	// high ones lifted a little (they carry less), against the recent
	// loudest band: the top 42 dB fill the height
	var raw [vizBands]float64
	top := -200.0
	for b := 0; b < vizBands; b++ {
		m := 0.0
		for i := v.bandLo[b]; i < v.bandHi[b]; i++ {
			if a := real(v.spec[i])*real(v.spec[i]) + imag(v.spec[i])*imag(v.spec[i]); a > m {
				m = a
			}
		}
		db := 10*math.Log10(m+1e-12) + 9*float64(b)/vizBands
		raw[b] = db
		top = math.Max(top, db)
	}
	if top > v.norm {
		v.norm += (top - v.norm) * 0.5
	} else {
		v.norm = math.Max(-60, v.norm-(v.norm-top)*0.12*dt)
	}
	k := dt * 30 // (per 30 fps frame)
	for b := 0; b < vizBands; b++ {
		x := math.Max(0, math.Min(1, 1+(raw[b]-v.norm-2)/42))
		if !v.fresh {
			x = 0
		}
		if x > v.bands[b] {
			v.bands[b] += (x - v.bands[b]) * math.Min(1, 0.65*k)
			v.fall[b] = 0
		} else {
			v.fall[b] += 0.004 * k
			v.bands[b] = math.Max(x, v.bands[b]-v.fall[b]*k)
		}
		if v.bands[b] >= v.peaks[b] {
			v.peaks[b] = v.bands[b]
		} else {
			v.peaks[b] = math.Max(v.bands[b], v.peaks[b]-0.012*k)
		}
	}
	// the bass (the first quarter) and its beats
	bass := 0.0
	for b := 0; b < vizBands/4; b++ {
		bass += v.bands[b]
	}
	bass /= vizBands / 4
	v.beat = v.fresh && bass > 0.3 && bass > v.bassAv*1.35 && now.Sub(v.beatAt) > 220*time.Millisecond
	if v.beat {
		v.beatAt = now
	}
	v.bassAv += (bass - v.bassAv) * math.Min(1, 0.08*k)
	v.bass = bass
	v.hue = math.Mod(v.hue+(0.0025+0.01*bass)*k, 1)
	if v.fresh || v.energy() > 0.01 {
		v.quietSince = time.Time{}
	} else if v.quietSince.IsZero() {
		v.quietSince = now
	}
}

func (v *visualizer) energy() float64 {
	e := 0.0
	for _, b := range v.peaks {
		e += b
	}
	return e / vizBands
}

// animating: it moves while there's sound, and a little after (fading).
func (v *visualizer) animating(now time.Time) bool {
	return v.quietSince.IsZero() || now.Sub(v.quietSince) < 2*time.Second || now.Sub(v.nameAt) < 2*time.Second
}

// ---- drawing ----

func (ui *IpodUI) renderViz(dst *image.RGBA, s *screensaver, now time.Time) {
	v := s.viz
	v.analyse(now)
	fillRect(dst, 0, vizTop, 640, 480-vizTop, colBlack)
	switch saverEffect {
	case 0:
		v.drawBars(dst)
	case 1:
		v.drawWaves()
		v.double(dst)
	case 2:
		v.drawTunnel(now)
		v.double(dst)
	default:
		v.drawStars()
		v.double(dst)
	}
	// nothing playing: a word in the middle
	if !v.fresh && v.energy() < 0.01 {
		f := face(W600, 14)
		msg := "Play a song to see it move"
		_, a := lineBox(14)
		drawText(dst, f, msg, 320-textWidth(f, msg)/2, (vizTop/DPR+110+a/2)*DPR, Col{255, 255, 255, 0.45})
	}
	// the effect's name for a moment (when it starts and when it changes)
	if el := now.Sub(v.nameAt).Seconds(); el < 1.8 {
		op := math.Min(1, (1.8-el)/0.5)
		f := face(W700, 15)
		name := "‹  " + vizNames[saverEffect] + "  ›"
		_, a := lineBox(15)
		tw := textWidth(f, name)
		y := 480 - 34*DPR
		fillRoundedGradient(dst, 320-tw/2-14*DPR, y-12*DPR, 320+tw/2+14*DPR, y+12*DPR, 12*DPR,
			[]GradStop{{0, Col{0, 0, 0, 0.55}}, {1, Col{0, 0, 0, 0.55}}}, op)
		drawText(dst, f, name, 320-tw/2, y+a*DPR/2-1, Col{255, 255, 255, op})
	}
}

// nextEffect: LEFT (-1) / RIGHT (+1).
func (ui *IpodUI) vizEffect(d int) {
	saverEffect = (saverEffect + d + len(vizNames)) % len(vizNames)
	if v := ui.saver.viz; v != nil {
		v.nameAt = time.Now()
		clear(v.buf)
		clear(v.buf2)
		v.parts = v.parts[:0]
	}
	ui.savePrefs()
	ui.dirty = true
}

// Bars: full size, from the ready-made bar pictures.
const (
	vizBarW   = 11
	vizBarGap = 4
	vizBarMax = 270 // px
	vizBase   = vizTop + 30 + vizBarMax
	vizReflH  = 480 - vizBase
)

func (v *visualizer) makeBars() {
	v.bar = image.NewRGBA(image.Rect(0, 0, vizBarW, vizBarMax))
	v.barRefl = image.NewRGBA(image.Rect(0, 0, vizBarW, vizReflH))
	bottom, top := Col{30, 110, 230, 1}, Col{150, 230, 255, 1}
	for y := 0; y < vizBarMax; y++ {
		t := 1 - float64(y)/vizBarMax // 1 at the top
		c := lerpCol(bottom, top, t*t)
		for x := 0; x < vizBarW; x++ {
			// rounded light down the middle
			e := 1 - 0.25*math.Abs(float64(x)-float64(vizBarW-1)/2)/(float64(vizBarW)/2)
			o := v.bar.PixOffset(x, y)
			v.bar.Pix[o], v.bar.Pix[o+1], v.bar.Pix[o+2], v.bar.Pix[o+3] = uint8(c.R*e), uint8(c.G*e), uint8(c.B*e), 255
		}
	}
	// the reflection: the bar's bottom, mirrored, fading into the black
	for y := 0; y < vizReflH; y++ {
		f := 0.28 * (1 - float64(y)/vizReflH)
		for x := 0; x < vizBarW; x++ {
			so := v.bar.PixOffset(x, vizBarMax-1-min(y, vizBarMax-1))
			o := v.barRefl.PixOffset(x, y)
			for k := 0; k < 3; k++ {
				v.barRefl.Pix[o+k] = uint8(float64(v.bar.Pix[so+k]) * f)
			}
			v.barRefl.Pix[o+3] = 255
		}
	}
}

func (v *visualizer) drawBars(dst *image.RGBA) {
	total := vizBands*vizBarW + (vizBands-1)*vizBarGap
	x0 := (640 - total) / 2
	for b := 0; b < vizBands; b++ {
		x := x0 + b*(vizBarW+vizBarGap)
		h := int(v.bands[b] * vizBarMax)
		if h > 0 {
			blitOpaque(dst, v.bar.SubImage(image.Rect(0, vizBarMax-h, vizBarW, vizBarMax)).(*image.RGBA), x, vizBase-h)
			rh := min(h, vizReflH)
			blitOpaque(dst, v.barRefl.SubImage(image.Rect(0, 0, vizBarW, rh)).(*image.RGBA), x, vizBase+2)
		}
		// the cap, falling slower
		py := vizBase - int(v.peaks[b]*vizBarMax) - 5
		fillRect(dst, x, py, vizBarW, 3, Col{230, 245, 255, 1})
	}
}

// ---- the half-size picture ----

// fade darkens the picture by f/256 (the trails).
func vizFade(buf []byte, f uint32) {
	for i := 0; i < len(buf); i += 4 {
		buf[i] = uint8(uint32(buf[i]) * f >> 8)
		buf[i+1] = uint8(uint32(buf[i+1]) * f >> 8)
		buf[i+2] = uint8(uint32(buf[i+2]) * f >> 8)
	}
}

// remap: dst's pixels from src through m, darkened by f/256.
func vizRemap(dst, src []byte, m []int32, f uint32) {
	parallelRows(0, vizH, func(y0, y1 int) {
		for i := y0 * vizW; i < y1*vizW; i++ {
			s, d := int(m[i])*4, i*4
			dst[d] = uint8(uint32(src[s]) * f >> 8)
			dst[d+1] = uint8(uint32(src[s+1]) * f >> 8)
			dst[d+2] = uint8(uint32(src[s+2]) * f >> 8)
			dst[d+3] = 255
		}
	})
}

// add: a soft dot of colour (r, g, b) added at (x, y), 3×3.
func (v *visualizer) add(x, y int, r, g, b uint8) {
	for dy := -1; dy <= 1; dy++ {
		yy := y + dy
		if yy < 0 || yy >= vizH {
			continue
		}
		for dx := -1; dx <= 1; dx++ {
			xx := x + dx
			if xx < 0 || xx >= vizW {
				continue
			}
			sh := uint(0)
			if dx != 0 || dy != 0 {
				sh = 1 // the edge half as strong
				if dx != 0 && dy != 0 {
					sh = 2
				}
			}
			o := (yy*vizW + xx) * 4
			v.buf[o] = sat(v.buf[o], r>>sh)
			v.buf[o+1] = sat(v.buf[o+1], g>>sh)
			v.buf[o+2] = sat(v.buf[o+2], b>>sh)
		}
	}
}

func sat(a, b uint8) uint8 {
	if s := uint16(a) + uint16(b); s < 255 {
		return uint8(s)
	}
	return 255
}

func (v *visualizer) color(h float64) (uint8, uint8, uint8) {
	c := v.palette[int(math.Mod(h+1, 1)*1023)&1023]
	return c[0], c[1], c[2]
}

// line: from (x0, y0) to (x1, y1), dots all along.
func (v *visualizer) line(x0, y0, x1, y1 int, r, g, b uint8) {
	dx, dy := abs(x1-x0), -abs(y1-y0)
	sx, sy := 1, 1
	if x0 > x1 {
		sx = -1
	}
	if y0 > y1 {
		sy = -1
	}
	e := dx + dy
	for n := 0; n < 400; n++ {
		v.add(x0, y0, r, g, b)
		if x0 == x1 && y0 == y1 {
			return
		}
		e2 := 2 * e
		if e2 >= dy {
			e += dy
			x0 += sx
		}
		if e2 <= dx {
			e += dx
			y0 += sy
		}
	}
}

// double: the half-size picture onto the screen, each pixel 2×2.
func (v *visualizer) double(dst *image.RGBA) {
	parallelRows(0, vizH, func(y0, y1 int) {
		for y := y0; y < y1; y++ {
			o := dst.PixOffset(0, vizTop+2*y)
			row := dst.Pix[o : o+640*4]
			src := v.buf[y*vizW*4 : (y+1)*vizW*4]
			for x := 0; x < vizW; x++ {
				s := src[x*4 : x*4+3]
				d := row[x*8 : x*8+8]
				d[0], d[1], d[2], d[3] = s[0], s[1], s[2], 255
				d[4], d[5], d[6], d[7] = s[0], s[1], s[2], 255
			}
			o2 := dst.PixOffset(0, vizTop+2*y+1)
			copy(dst.Pix[o2:o2+640*4], row)
		}
	})
}

// the moves (where each pixel comes from), made once
func (v *visualizer) makeMaps() {
	mk := func(zoom, rot float64) []int32 {
		m := make([]int32, vizW*vizH)
		cx, cy := float64(vizW)/2, float64(vizH)/2
		sn, cs := math.Sin(rot), math.Cos(rot)
		for y := 0; y < vizH; y++ {
			for x := 0; x < vizW; x++ {
				dx, dy := float64(x)-cx, float64(y)-cy
				sx := cx + (dx*cs-dy*sn)*zoom
				sy := cy + (dx*sn+dy*cs)*zoom
				ix := min(max(int(sx+0.5), 0), vizW-1)
				iy := min(max(int(sy+0.5), 0), vizH-1)
				m[y*vizW+x] = int32(iy*vizW + ix)
			}
		}
		return m
	}
	// sinking into the middle, turning one way or the other, or pushed out
	v.maps[0] = mk(1.035, 0.025)
	v.maps[1] = mk(1.035, -0.025)
	v.maps[2] = mk(0.975, 0.012)
	v.zoomOut = mk(0.985, 0)
}

// Waves: the wave, glowing, its trails drifting outwards.
func (v *visualizer) drawWaves() {
	vizRemap(v.buf2, v.buf, v.zoomOut, 226)
	v.buf, v.buf2 = v.buf2, v.buf
	if !v.fresh {
		return // (silence: the trails fade away)
	}
	g := 0.0
	for _, s := range v.samples {
		g = math.Max(g, math.Abs(float64(s)))
	}
	amp := 80 / math.Max(0.08, g) // (the wave fills the height whatever the volume)
	r, gg, b := v.color(v.hue)
	r2, g2, b2 := v.color(v.hue + 0.5)
	step := vizN / vizW
	px, py, qy := 0, vizH/2, vizH/2
	for x := 0; x < vizW; x++ {
		s := float64(v.samples[x*step]) * amp
		y := vizH/2 + int(s)
		y2 := vizH/2 - int(s*0.6)
		if x > 0 {
			v.line(px, py, x, y, r, gg, b)
			v.line(px, qy, x, y2, r2/2, g2/2, b2/2)
		}
		px, py, qy = x, y, y2
	}
}

// Tunnel: the bands round a ring, the picture turning into the middle.
func (v *visualizer) drawTunnel(now time.Time) {
	if v.beat && now.Sub(v.mapAt) > 3*time.Second {
		v.mapCur, v.mapAt = (v.mapCur+1+v.rng.Intn(2))%3, now
	}
	vizRemap(v.buf2, v.buf, v.maps[v.mapCur], 236)
	v.buf, v.buf2 = v.buf2, v.buf
	cx, cy := vizW/2, vizH/2
	base := 26 + 24*v.bass
	px, py := 0, 0
	const n = 2 * vizBands
	for i := 0; i <= n; i++ {
		b := i % n
		if b >= vizBands {
			b = n - 1 - b // (mirrored: the ring closes)
		}
		a := float64(i)/n*2*math.Pi + v.hue*2*math.Pi
		rr := base + v.bands[b]*70
		r, g, bl := v.color(v.hue + float64(b)/vizBands*0.5)
		x, y := cx+int(math.Cos(a)*rr), cy+int(math.Sin(a)*rr)
		if i > 0 {
			v.line(px, py, x, y, r, g, bl)
		}
		px, py = x, y
	}
}

// Stars: sparks from the middle, many on the beats.
func (v *visualizer) drawStars() {
	vizFade(v.buf, 200)
	emit := func(n int, speed float64) {
		for i := 0; i < n && len(v.parts) < 700; i++ {
			a := v.rng.Float64() * 2 * math.Pi
			sp := speed * (0.4 + v.rng.Float64())
			v.parts = append(v.parts, vizParticle{x: vizW / 2, y: vizH / 2, vx: math.Cos(a) * sp, vy: math.Sin(a) * sp,
				life: 1, hue: v.hue + v.rng.Float64()*0.25})
		}
	}
	treble := 0.0
	for b := vizBands / 2; b < vizBands; b++ {
		treble += v.bands[b]
	}
	treble /= vizBands / 2
	if v.fresh {
		emit(int(2+treble*14+v.bass*6), 1.2+2.5*treble)
	}
	if v.beat {
		emit(90, 3+3*v.bass)
	}
	k := 0
	for _, p := range v.parts {
		p.x += p.vx
		p.y += p.vy
		p.vx *= 1.02
		p.vy *= 1.02
		p.life -= 0.012
		if p.life <= 0 || p.x < 0 || p.y < 0 || p.x >= vizW || p.y >= vizH {
			continue
		}
		r, g, b := v.color(p.hue)
		f := p.life
		v.add(int(p.x), int(p.y), uint8(float64(r)*f), uint8(float64(g)*f), uint8(float64(b)*f))
		v.parts[k] = p
		k++
	}
	v.parts = v.parts[:k]
}

// hsvRGB: a colour from hue (0-1), saturation and value.
func hsvRGB(h, s, v float64) (uint8, uint8, uint8) {
	h = math.Mod(h, 1) * 6
	i := math.Floor(h)
	f := h - i
	p, q, t := v*(1-s), v*(1-s*f), v*(1-s*(1-f))
	var r, g, b float64
	switch int(i) {
	case 0:
		r, g, b = v, t, p
	case 1:
		r, g, b = q, v, p
	case 2:
		r, g, b = p, v, t
	case 3:
		r, g, b = p, q, v
	case 4:
		r, g, b = t, p, v
	default:
		r, g, b = v, p, q
	}
	return uint8(r * 255), uint8(g * 255), uint8(b * 255)
}

// ---- the Visualizer's keys and what shows over it ----

// vizKey: LEFT / RIGHT the effect, START the big clock, UP / DOWN the song
// at the bottom, B leaves (to Now Playing while a song plays), X plays and
// pauses, L / R change song, L2 / R2 the brightness.
func (ui *IpodUI) vizKey(s *screensaver, k Key, now time.Time) bool {
	toggle := func(a *Anim) {
		if a.Target() > 0.5 {
			a.To(0, 350*time.Millisecond, easeInOut, 0)
		} else {
			a.To(1, 450*time.Millisecond, easeInOut, 0)
		}
	}
	switch k {
	case LEFT, WHEEL_CCW:
		ui.vizEffect(-1)
	case RIGHT, WHEEL_CW:
		ui.vizEffect(1)
	case START:
		toggle(s.clockA)
	case UP, DOWN:
		toggle(s.lowA)
	case B, MENU:
		if ui.saverSongPlaying() && ui.coverUp() == nil {
			// to Now Playing (shown at once under it as it fades)
			ui.goNowPlaying()
			ui.finishSlides()
		}
		ui.stopSaverIn(350 * time.Millisecond)
	case X:
		ui.app.togglePlayPause()
	case L:
		skipDir = -1
		go func() { ui.app.prevTrack(); ui.post(ui.onTrackChanged) }()
	case R:
		skipDir = 1
		go func() { ui.app.nextTrack(); ui.post(ui.onTrackChanged) }()
	case L2, R2:
		ui.saverBrightness(k == R2)
		ui.repeatKey, ui.repeatStart, ui.repeatLast = k, now, now
	}
	ui.dirty = true
	return true
}

var vizShade *image.RGBA // the lower third's shade (made once)

// vizOverlays: the big clock (START) and the song at the bottom (UP /
// DOWN), as Now Playing's block shows it, on a shade instead of its
// background.
func (ui *IpodUI) vizOverlays(dst *image.RGBA, s *screensaver, now time.Time) {
	if a := s.clockA.Get(now); a > 0.002 {
		t := time.Now()
		big := face(W400, 72)
		small := face(W500, 15)
		hm := t.Format("15:04")
		day := t.Format("Monday 2 January")
		_, ab := lineBox(72)
		_, as := lineBox(15)
		cy := (vizTop/DPR + (240-vizTop/DPR)/2) - 8 + 6*(1-a)
		// a soft dark halo so it reads on any effect
		fillRoundedGradient(dst, 320-200, (cy-56)*DPR, 320+200, (cy+40)*DPR, 40*DPR,
			[]GradStop{{0, Col{0, 0, 0, 0.35}}, {1, Col{0, 0, 0, 0.35}}}, a)
		drawText(dst, big, hm, 320-textWidth(big, hm)/2, (cy+ab/2-12)*DPR, Col{255, 255, 255, a})
		drawText(dst, small, day, 320-textWidth(small, day)/2, (cy+40+as/2)*DPR, Col{255, 255, 255, 0.8 * a})
	}
	if a := s.lowA.Get(now); a > 0.002 {
		t := ui.miniPlayerTrack()
		if t == nil {
			return
		}
		// the shade: from clear to dark, over the bottom of the screen
		const sh = 150 // px
		if vizShade == nil {
			vizShade = image.NewRGBA(image.Rect(0, 0, 640, sh))
			for y := 0; y < sh; y++ {
				k := float64(y) / sh
				al := uint8(255 * 0.82 * k * k * (3 - 2*k))
				for x := 0; x < 640; x++ {
					vizShade.Pix[(y*640+x)*4+3] = al // (black, premultiplied)
				}
			}
		}
		top := 480 - sh + int(float64(sh)*(1-a))
		blit(dst, vizShade, 0, top, a)
		// Now Playing's block, without its background, sliding up
		y := 240 - miniPlayerH - 4 + 30*(1-a)
		clip := dst.SubImage(image.Rect(0, vizTop, 640, 480)).(*image.RGBA)
		ui.updateMini(t, now) // (its cover's pause mark follows the song)
		ui.drawMiniContent(clip, t, 0, y, 320, 0, a, true, false, now)
	}
}
