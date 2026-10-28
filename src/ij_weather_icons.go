package main

// ij_weather_icons.go — the Weather icons, drawn the way they would have
// been in 2006: glossy, a little puffy, with a highlight on top and a soft
// shadow under them (think of the iPod's own icons and the Wii's Forecast
// Channel).
//
// Every icon is drawn in a 100 × 100 box and scaled; t (0..1) is the phase
// of its loop, for the big animated icon: the sun's rays turn, clouds
// drift, rain and snow fall from the cloud, lightning flashes. Static icons
// use a fixed phase.

import (
	"image"
	"image/color"
	"math"
	"sync"

	"github.com/fogleman/gg"
)

type wxKind int

const (
	wxSun wxKind = iota
	wxMoon
	wxPartlySun
	wxPartlyMoon
	wxCloudy
	wxFog
	wxDrizzle
	wxRain
	wxSnow
	wxSleet
	wxThunder
)

// wxKindFor maps a WMO weather code (Open-Meteo) to an icon.
func wxKindFor(code int, day bool) wxKind {
	switch {
	case code == 0 || code == 1:
		if day {
			return wxSun
		}
		return wxMoon
	case code == 2:
		if day {
			return wxPartlySun
		}
		return wxPartlyMoon
	case code == 3:
		return wxCloudy
	case code == 45 || code == 48:
		return wxFog
	case code >= 51 && code <= 55:
		return wxDrizzle
	case code == 56 || code == 57 || code == 66 || code == 67:
		return wxSleet
	case code >= 61 && code <= 65, code >= 80 && code <= 82:
		return wxRain
	case code >= 71 && code <= 77, code == 85 || code == 86:
		return wxSnow
	case code >= 95:
		return wxThunder
	}
	return wxCloudy
}

// wxLabel: the condition in words.
func wxLabel(code int) string {
	switch code {
	case 0:
		return "Clear"
	case 1:
		return "Mostly Clear"
	case 2:
		return "Partly Cloudy"
	case 3:
		return "Cloudy"
	case 45:
		return "Fog"
	case 48:
		return "Freezing Fog"
	case 51:
		return "Light Drizzle"
	case 53:
		return "Drizzle"
	case 55:
		return "Heavy Drizzle"
	case 56, 57:
		return "Freezing Drizzle"
	case 61:
		return "Light Rain"
	case 63:
		return "Rain"
	case 65:
		return "Heavy Rain"
	case 66, 67:
		return "Freezing Rain"
	case 71:
		return "Light Snow"
	case 73:
		return "Snow"
	case 75:
		return "Heavy Snow"
	case 77:
		return "Snow Grains"
	case 80:
		return "Showers"
	case 81:
		return "Rain Showers"
	case 82:
		return "Heavy Showers"
	case 85, 86:
		return "Snow Showers"
	case 95:
		return "Thunderstorm"
	case 96, 99:
		return "Thunderstorm, Hail"
	}
	return "Cloudy"
}

// pen draws in the icon's 100-unit box: p(x) is device px.
type pen struct {
	dc *gg.Context
	s  float64
}

func (p pen) u(v float64) float64 { return v * p.s }

func c8(c Col) color.Color {
	return color.NRGBA{uint8(c.R), uint8(c.G), uint8(c.B), uint8(c.A*255 + 0.5)}
}

func (p pen) linear(x0, y0, x1, y1 float64, stops ...GradStop) gg.Gradient {
	g := gg.NewLinearGradient(p.u(x0), p.u(y0), p.u(x1), p.u(y1))
	for _, s := range stops {
		g.AddColorStop(s.Pos, c8(s.C))
	}
	return g
}

func (p pen) radial(cx, cy, r0, x1, y1, r1 float64, stops ...GradStop) gg.Gradient {
	g := gg.NewRadialGradient(p.u(cx), p.u(cy), p.u(r0), p.u(x1), p.u(y1), p.u(r1))
	for _, s := range stops {
		g.AddColorStop(s.Pos, c8(s.C))
	}
	return g
}

func (p pen) circle(x, y, r float64) { p.dc.DrawCircle(p.u(x), p.u(y), p.u(r)) }

func (p pen) fill(paint gg.Pattern) {
	p.dc.SetFillStyle(paint)
	p.dc.Fill()
}

func (p pen) solid(c Col) gg.Pattern { return gg.NewSolidPattern(c8(c)) }

// gloss: the shine on the top of a shape (an ellipse fading down).
func (p pen) gloss(cx, cy, rx, ry, a float64) {
	p.dc.DrawEllipse(p.u(cx), p.u(cy), p.u(rx), p.u(ry))
	p.fill(p.linear(0, cy-ry, 0, cy+ry, GradStop{0, rgba(255, 255, 255, a)}, GradStop{1, rgba(255, 255, 255, 0)}))
}

// ---- the parts ----

func (p pen) sun(cx, cy, r, ray, angle float64) {
	dc := p.dc
	// soft glow
	p.circle(cx, cy, r+ray+3)
	p.fill(p.radial(cx, cy, r, cx, cy, r+ray+3, GradStop{0, rgba(255, 214, 70, 0.45)}, GradStop{1, rgba(255, 200, 60, 0)}))
	// rays: 12 rounded spikes
	for i := 0; i < 12; i++ {
		a := angle + float64(i)*math.Pi/6
		w := 0.17
		r0 := r + 1.5
		r1 := r + ray
		if i%2 == 1 {
			r1 = r + ray*0.72
		}
		dc.MoveTo(p.u(cx+r0*math.Cos(a-w)), p.u(cy+r0*math.Sin(a-w)))
		dc.QuadraticTo(p.u(cx+(r1+1.5)*math.Cos(a)), p.u(cy+(r1+1.5)*math.Sin(a)), p.u(cx+r0*math.Cos(a+w)), p.u(cy+r0*math.Sin(a+w)))
		dc.ClosePath()
	}
	dc.SetFillStyle(p.radial(cx, cy, r, cx, cy, r+ray, GradStop{0, hex("#ffc93a")}, GradStop{1, hex("#ff8a12")}))
	dc.FillPreserve()
	dc.SetStrokeStyle(p.solid(rgba(214, 110, 0, 0.75)))
	dc.SetLineWidth(p.u(0.9))
	dc.Stroke()
	// the disc
	p.circle(cx, cy+1.2, r)
	p.fill(p.solid(rgba(150, 70, 0, 0.25)))
	p.circle(cx, cy, r)
	dc.SetFillStyle(p.radial(cx-r*0.3, cy-r*0.35, 0, cx, cy, r*1.05,
		GradStop{0, hex("#fffbe0")}, GradStop{0.35, hex("#ffe35a")}, GradStop{0.8, hex("#ffb21e")}, GradStop{1, hex("#ff9612")}))
	dc.FillPreserve()
	dc.SetStrokeStyle(p.solid(hex("#e07800")))
	dc.SetLineWidth(p.u(1.3))
	dc.Stroke()
	p.gloss(cx, cy-r*0.42, r*0.66, r*0.42, 0.85)
}

func (p pen) moon(cx, cy, r float64) {
	dc := p.dc
	ix, iy, ir := cx+r*0.5, cy-r*0.32, r*0.86
	// the crescent: the disc minus the inner circle (the glow too, so the
	// dark side stays clear)
	p.circle(ix, iy, ir)
	dc.Clip()
	dc.InvertMask()
	p.circle(cx, cy, r+6)
	p.fill(p.radial(cx, cy, r*0.9, cx, cy, r+6, GradStop{0, rgba(255, 248, 200, 0.4)}, GradStop{1, rgba(255, 248, 200, 0)}))
	p.circle(cx, cy+1.2, r)
	p.fill(p.solid(rgba(60, 50, 0, 0.25)))
	p.circle(cx, cy, r)
	dc.SetFillStyle(p.radial(cx-r*0.4, cy-r*0.2, 0, cx, cy, r,
		GradStop{0, hex("#fffdf0")}, GradStop{0.5, hex("#fbeaa0")}, GradStop{1, hex("#e2c25a")}))
	dc.FillPreserve()
	dc.SetStrokeStyle(p.solid(hex("#b89530")))
	dc.SetLineWidth(p.u(1.3))
	dc.Stroke()
	p.gloss(cx-r*0.35, cy-r*0.25, r*0.32, r*0.5, 0.7)
	dc.ResetClip()
	// the inner edge, inside the disc
	p.circle(cx, cy, r)
	dc.Clip()
	p.circle(ix, iy, ir)
	dc.SetStrokeStyle(p.solid(hex("#c9a640")))
	dc.SetLineWidth(p.u(1.1))
	dc.Stroke()
	dc.ResetClip()
}

// star: a little four-point sparkle.
func (p pen) star(x, y, r, a float64) {
	if a <= 0.02 {
		return
	}
	dc := p.dc
	dc.MoveTo(p.u(x), p.u(y-r))
	dc.QuadraticTo(p.u(x), p.u(y), p.u(x+r), p.u(y))
	dc.QuadraticTo(p.u(x), p.u(y), p.u(x), p.u(y+r))
	dc.QuadraticTo(p.u(x), p.u(y), p.u(x-r), p.u(y))
	dc.QuadraticTo(p.u(x), p.u(y), p.u(x), p.u(y-r))
	dc.ClosePath()
	p.fill(p.solid(rgba(255, 250, 215, a)))
}

type cloudTone int

const (
	cloudWhite cloudTone = iota
	cloudGrey
	cloudDark
)

// cloud: three puffs on a flat base, w wide from x, top at y.
func (p pen) cloud(x, y, w float64, tone cloudTone) {
	dc := p.dc
	h := w * 0.62
	puffs := [][3]float64{{0.30, 0.56, 0.22}, {0.53, 0.42, 0.29}, {0.76, 0.60, 0.19}}
	shape := func(grow, dy float64) {
		for _, c := range puffs {
			p.circle(x+c[0]*w, y+c[1]*h+dy, c[2]*w+grow)
		}
		rx0, rx1 := x+0.08*w-grow, x+0.94*w+grow
		ry0, ry1 := y+0.52*h-grow+dy, y+0.92*h+grow+dy
		rr := (ry1 - ry0) / 2
		dc.DrawRoundedRectangle(p.u(rx0), p.u(ry0), p.u(rx1-rx0), p.u(ry1-ry0), p.u(rr))
	}
	top, bot, line := hex("#ffffff"), hex("#c7d6e8"), hex("#7f97b4")
	switch tone {
	case cloudGrey:
		top, bot, line = hex("#f2f4f7"), hex("#98a5b6"), hex("#66738a")
	case cloudDark:
		top, bot, line = hex("#d4d9e1"), hex("#5f6a7c"), hex("#454f60")
	}
	// shadow, outline, body
	shape(1.2, 2.2)
	p.fill(p.solid(rgba(10, 30, 60, 0.22)))
	shape(1.3, 0)
	p.fill(p.solid(line))
	shape(0, 0)
	p.fill(p.linear(0, y, 0, y+h*0.95, GradStop{0, top}, GradStop{0.55, lerpCol(top, bot, 0.35)}, GradStop{1, bot}))
	// shine on the big puff
	c := puffs[1]
	p.gloss(x+c[0]*w-w*0.03, y+c[1]*h-c[2]*w*0.45, c[2]*w*0.72, c[2]*w*0.5, 0.9)
}

// drop: a raindrop, s tall (about), its round bottom at y.
func (p pen) drop(x, y, s, a float64) {
	if a <= 0.02 {
		return
	}
	dc := p.dc
	r := s * 0.32
	cy := y - r
	dc.MoveTo(p.u(x), p.u(y-s))
	dc.QuadraticTo(p.u(x+r*1.05), p.u(cy-r*0.9), p.u(x+r), p.u(cy))
	dc.DrawEllipticalArc(p.u(x), p.u(cy), p.u(r), p.u(r), 0, math.Pi)
	dc.QuadraticTo(p.u(x-r*1.05), p.u(cy-r*0.9), p.u(x), p.u(y-s))
	dc.ClosePath()
	dc.SetFillStyle(p.linear(x-r, y-s, x+r, y, GradStop{0, rgba(196, 238, 255, a)}, GradStop{0.5, rgba(64, 160, 240, a)}, GradStop{1, rgba(20, 96, 205, a)}))
	dc.FillPreserve()
	dc.SetStrokeStyle(p.solid(rgba(18, 80, 170, a)))
	dc.SetLineWidth(p.u(0.8))
	dc.Stroke()
	dc.DrawEllipse(p.u(x-r*0.38), p.u(cy-r*0.2), p.u(r*0.22), p.u(r*0.42))
	p.fill(p.solid(rgba(255, 255, 255, 0.8*a)))
}

// flake: a six-armed snowflake, r in radius, turned by rot.
func (p pen) flake(x, y, r, rot, a float64) {
	if a <= 0.02 {
		return
	}
	dc := p.dc
	dc.SetLineCapRound()
	arms := func() {
		for i := 0; i < 6; i++ {
			an := rot + float64(i)*math.Pi/3
			ex, ey := x+r*math.Cos(an), y+r*math.Sin(an)
			dc.MoveTo(p.u(x), p.u(y))
			dc.LineTo(p.u(ex), p.u(ey))
			mx, my := x+r*0.55*math.Cos(an), y+r*0.55*math.Sin(an)
			for _, d := range []float64{-0.7, 0.7} {
				dc.MoveTo(p.u(mx), p.u(my))
				dc.LineTo(p.u(mx+r*0.32*math.Cos(an+d)), p.u(my+r*0.32*math.Sin(an+d)))
			}
		}
	}
	arms()
	dc.SetStrokeStyle(p.solid(rgba(70, 130, 200, a)))
	dc.SetLineWidth(p.u(3.4))
	dc.Stroke()
	arms()
	dc.SetStrokeStyle(p.solid(rgba(255, 255, 255, a)))
	dc.SetLineWidth(p.u(1.7))
	dc.Stroke()
}

// bolt: a lightning bolt, s tall from (x, y).
func (p pen) bolt(x, y, s, a float64) {
	dc := p.dc
	pts := [][2]float64{{0.52, 0}, {0.08, 0.56}, {0.40, 0.56}, {0.22, 1.0}, {0.86, 0.38}, {0.52, 0.38}, {0.74, 0}}
	path := func(dx, dy float64) {
		for i, q := range pts {
			px, py := p.u(x+q[0]*s*0.7+dx), p.u(y+q[1]*s+dy)
			if i == 0 {
				dc.MoveTo(px, py)
			} else {
				dc.LineTo(px, py)
			}
		}
		dc.ClosePath()
	}
	if a > 0.6 {
		// the flash
		p.circle(x+s*0.33, y+s*0.5, s*0.75)
		p.fill(p.radial(x+s*0.33, y+s*0.5, 0, x+s*0.33, y+s*0.5, s*0.75, GradStop{0, rgba(255, 250, 190, 0.55*(a-0.6)/0.4)}, GradStop{1, rgba(255, 250, 190, 0)}))
	}
	path(0.8, 1.5)
	p.fill(p.solid(rgba(90, 50, 0, 0.25)))
	path(0, 0)
	dc.SetFillStyle(p.linear(x, y, x+s*0.5, y+s, GradStop{0, hex("#fffbb0")}, GradStop{0.5, hex("#ffd52a")}, GradStop{1, hex("#ff9e0d")}))
	dc.FillPreserve()
	dc.SetStrokeStyle(p.solid(hex("#c97800")))
	dc.SetLineJoinRound()
	dc.SetLineWidth(p.u(1.2))
	dc.Stroke()
}

// fogBars: three soft bars.
func (p pen) fogBars(y, t float64) {
	dc := p.dc
	for i, w := range []float64{62, 72, 54} {
		dx := math.Sin(2*math.Pi*(t+float64(i)*0.33)) * 3
		x := 50 - w/2 + dx + float64(i%2)*4 - 2
		yy := y + float64(i)*9.5
		dc.DrawRoundedRectangle(p.u(x), p.u(yy+1.2), p.u(w), p.u(6), p.u(3))
		p.fill(p.solid(rgba(20, 40, 70, 0.18)))
		dc.DrawRoundedRectangle(p.u(x), p.u(yy), p.u(w), p.u(6), p.u(3))
		dc.SetFillStyle(p.linear(0, yy, 0, yy+6, GradStop{0, hex("#ffffff")}, GradStop{1, hex("#b9c6d6")}))
		dc.FillPreserve()
		dc.SetStrokeStyle(p.solid(hex("#8796ab")))
		dc.SetLineWidth(p.u(0.9))
		dc.Stroke()
	}
}

// falling: phase p (0..1) of a drop or flake falling from the cloud;
// its y and its alpha (fading in out of the cloud, out at the bottom).
func falling(t, offset, speed, y0, y1 float64) (float64, float64) {
	ph := math.Mod(t*speed+offset, 1)
	a := math.Min(1, math.Min(ph*5, (1-ph)*4))
	return y0 + (y1-y0)*ph, a
}

// drawWxIcon draws kind in a 100-unit box scaled to size device px.
func drawWxIcon(dc *gg.Context, kind wxKind, size, t float64) {
	p := pen{dc, size / 100}
	drift := math.Sin(2*math.Pi*t) * 2.2
	switch kind {
	case wxSun:
		p.sun(50, 50, 22, 17, t*math.Pi/6)
	case wxMoon:
		p.moon(48, 52, 27)
		for i, s := range [][3]float64{{80, 22, 5}, {86, 48, 3.5}, {66, 12, 3}} {
			tw := 0.55 + 0.45*math.Sin(2*math.Pi*(t+float64(i)*0.37))
			p.star(s[0], s[1], s[2], tw)
		}
	case wxPartlySun:
		p.sun(37, 36, 17, 12, t*math.Pi/6)
		p.cloud(20+drift, 38, 74, cloudWhite)
	case wxPartlyMoon:
		p.moon(38, 36, 20)
		p.star(78, 18, 4, 0.55+0.45*math.Sin(2*math.Pi*t))
		p.cloud(20+drift, 40, 72, cloudWhite)
	case wxCloudy:
		p.cloud(40-drift*0.6, 18, 54, cloudGrey)
		p.cloud(6+drift, 36, 80, cloudWhite)
	case wxFog:
		p.cloud(14+drift*0.5, 14, 72, cloudGrey)
		p.fogBars(64, t)
	case wxDrizzle, wxRain, wxThunder, wxSleet:
		n, size, speed := 4, 16.0, 2.0
		if kind == wxDrizzle {
			n, size = 3, 11
		}
		xs := []float64{30, 47, 64, 79}
		offs := []float64{0.0, 0.55, 0.25, 0.8}
		for i := 0; i < n; i++ {
			x := xs[i]
			if n == 3 {
				x = []float64{34, 52, 70}[i]
			}
			y, a := falling(t, offs[i], speed, 68, 99)
			if kind == wxSleet && i%2 == 1 {
				p.flake(x, y-6, 6.5, t*math.Pi, a)
				continue
			}
			p.drop(x-drift*0.3, y, size, a)
		}
		tone := cloudGrey
		if kind == wxThunder {
			tone = cloudDark
		}
		p.cloud(8+drift, 12, 84, tone)
		if kind == wxThunder {
			// a flash about once a loop
			ph := math.Mod(t*2, 1)
			a := 0.55
			if ph < 0.12 {
				a = 1
			}
			p.bolt(42, 50, 40, a)
		}
	case wxSnow:
		xs := []float64{30, 50, 70}
		offs := []float64{0, 0.4, 0.75}
		for i, x := range xs {
			y, a := falling(t, offs[i], 1, 66, 100)
			p.flake(x+math.Sin(2*math.Pi*(t+offs[i]))*3, y-6, 7.5, t*math.Pi*2/3+float64(i), a)
		}
		p.cloud(8+drift, 12, 84, cloudWhite)
	}
}

// ---- caches ----

type wxIconKey struct {
	kind  wxKind
	size  int
	frame int // -1: static
}

var (
	wxIconMu    sync.Mutex
	wxIconCache = map[wxIconKey]*image.RGBA{}
)

// wxStaticPhase: where a still icon catches its rain and snow.
const wxStaticPhase = 0.3

// wxIcon returns the icon at size device px: static (frame < 0), or one
// of wxFrames frames of its loop.
func wxIcon(kind wxKind, size, frame int) *image.RGBA {
	k := wxIconKey{kind, size, frame}
	wxIconMu.Lock()
	img := wxIconCache[k]
	wxIconMu.Unlock()
	if img != nil {
		return img
	}
	img = image.NewRGBA(image.Rect(0, 0, size, size))
	dc := gg.NewContextForRGBA(img)
	t := wxStaticPhase
	if frame >= 0 {
		t = float64(frame) / wxFrames
	}
	drawWxIcon(dc, kind, float64(size), t)
	wxIconMu.Lock()
	// the animated frames of one kind and size at a time
	if frame >= 0 {
		for kk := range wxIconCache {
			if kk.frame >= 0 && (kk.kind != kind || kk.size != size) {
				delete(wxIconCache, kk)
			}
		}
	}
	wxIconCache[k] = img
	wxIconMu.Unlock()
	return img
}

// wxFrames: frames in the animated icon's 3-second loop (20 a second).
const (
	wxFrames   = 60
	wxLoopSecs = 3.0
)

// wxPartIcon: one part on its own (the tiles' little raindrop).
func wxPartIcon(part string, size int) *image.RGBA {
	k := wxIconKey{wxKind(-1), size, -2}
	wxIconMu.Lock()
	img := wxIconCache[k]
	wxIconMu.Unlock()
	if img != nil {
		return img
	}
	img = image.NewRGBA(image.Rect(0, 0, size, size))
	p := pen{gg.NewContextForRGBA(img), float64(size) / 100}
	p.drop(50, 96, 90, 1)
	wxIconMu.Lock()
	wxIconCache[k] = img
	wxIconMu.Unlock()
	return img
}
