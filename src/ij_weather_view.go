package main

// ij_weather_view.go — the forecast of one place, in the spirit of the
// Wii's Forecast Channel drawn by the iPod's designers:
//
//	┌──────────────────────────────────────────┐
//	│ Milan                          Wed 01:24 │  sky that follows the
//	│ Lombardy, Italy · Updated just now       │  weather (and the night)
//	│  ☀      18°  H 21°                       │
//	│ (big)       L 12°                        │  the big icon is animated
//	│        Partly Cloudy                     │
//	│        Feels like 16° · Wind 12 km/h NW  │
//	│ ┌────┐┌────┐┌────┐┌────┐┌────┐           │  glossy glass tiles, one
//	│ │TODAY││THU ││FRI ││SAT ││SUN │ ›         │  a day, 10 days
//	│ │ ☀  ││ ☁  ││ ☂  ││ ☀  ││ ☁  │           │
//	│ │21 12││19 11││..  ││    ││    │           │
//	└──────────────────────────────────────────┘
//
// The wheel / d-pad picks a day (the top shows it); the centre button
// fetches the forecast again; B goes back.

import (
	"fmt"
	"image"
	"math"
	"strings"
	"time"

	"golang.org/x/image/font"
)

type ForecastView struct {
	place   *wxPlace
	loading bool
	err     string
	sel     int     // the day shown on top (0 = now)
	first   float64 // the strip's first visible day, eased
	firstTo int
	firstA  *Anim

	layer    *image.RGBA // everything but the animated icon
	layerKey string
	frameAt  time.Time
	frame    int
}

const (
	wxTiles  = 5
	wxTileW  = 57.0
	wxTileH  = 86.0
	wxTileG  = 5.0
	wxStripY = 128.0
	wxHeroSz = 84.0
	wxTileIc = 38.0
)

func (ui *IpodUI) forecastView(p *wxPlace) *View {
	fv := &ForecastView{place: p, firstA: newAnim(0)}
	v := &View{ID: "forecast", Title: "Weather", Kind: KindFull, Custom: fv}
	if p.Forecast == nil || time.Since(p.Forecast.Fetched) > wxStale {
		fv.refresh(ui)
	}
	return v
}

func (fv *ForecastView) refresh(ui *IpodUI) {
	if fv.loading {
		return
	}
	fv.loading, fv.err = true, ""
	p := fv.place
	go func() {
		f, err := fetchForecast(p)
		ui.post(func() {
			fv.loading = false
			if err != nil {
				fv.err = iaErrText(err)
				logMsg("Weather: " + p.Name + ": " + err.Error())
			} else {
				p.Forecast = f
				p.thumb = nil
				wxSave()
				if fv.sel >= f.days() {
					fv.sel = 0
				}
			}
			ui.dirty = true
		})
	}()
	ui.dirty = true
}

func (fv *ForecastView) Animating(now time.Time) bool {
	return fv.firstA.Running() || now.Sub(fv.frameAt) >= time.Second/time.Duration(wxFrames/wxLoopSecs)
}

func (fv *ForecastView) OnCenter(ui *IpodUI) { fv.refresh(ui) }

func (fv *ForecastView) OnScroll(ui *IpodUI, dir int, velocity float64) {
	f := fv.place.Forecast
	if f == nil {
		return
	}
	n := f.days()
	ns := max(0, min(n-1, fv.sel+dir))
	if ns == fv.sel {
		return
	}
	fv.sel = ns
	to := fv.firstTo
	if ns < to {
		to = ns
	} else if ns >= to+wxTiles {
		to = ns - wxTiles + 1
	}
	if to != fv.firstTo {
		fv.firstTo = to
		fv.firstA.To(float64(to), 260*time.Millisecond, easeCSS, 0)
	}
	ui.dirty = true
}

// ---- looks ----

// wxSky: the background for the weather (top, middle, bottom).
func wxSky(kind wxKind, day bool) []GradStop {
	g := func(a, b, c string) []GradStop {
		return []GradStop{{0, hex(a)}, {0.55, hex(b)}, {1, hex(c)}}
	}
	if !day {
		switch kind {
		case wxMoon, wxSun, wxPartlyMoon, wxPartlySun:
			return g("#06112b", "#13306a", "#2b4f8e")
		case wxSnow, wxSleet:
			return g("#1a2740", "#3a4f70", "#62789a")
		default:
			return g("#141b2b", "#2c3850", "#4a5a76")
		}
	}
	switch kind {
	case wxSun:
		return g("#1f6fd0", "#58a8ec", "#bfe4fb")
	case wxPartlySun:
		return g("#3a82d2", "#7ab6ea", "#cfe7f8")
	case wxCloudy, wxFog:
		return g("#5c7593", "#8ea4bd", "#c9d5e2")
	case wxDrizzle, wxRain:
		return g("#3d506b", "#66809e", "#9fb2c7")
	case wxThunder:
		return g("#232c3d", "#46546d", "#77869e")
	case wxSnow, wxSleet:
		return g("#7f9fc0", "#b4cbe0", "#eef4fa")
	}
	return g("#3a82d2", "#7ab6ea", "#cfe7f8")
}

// shadowText: white text with a soft dark shadow, like on a TV channel.
func shadowText(dst *image.RGBA, f font.Face, s string, x, base float64, c Col) {
	drawText(dst, f, s, x+DPR*0.5, base+DPR, rgba(0, 20, 50, 0.45*c.A))
	drawText(dst, f, s, x, base, c)
}

var wxHeroDays = []string{"Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"}

func (fv *ForecastView) heroKind() (wxKind, bool) {
	f := fv.place.Forecast
	if f == nil {
		return wxCloudy, true
	}
	if fv.sel == 0 {
		return wxKindFor(f.Current.Code, f.Current.IsDay == 1), f.Current.IsDay == 1
	}
	return wxKindFor(f.Daily.Code[fv.sel], true), true
}

func (fv *ForecastView) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	fv.first = fv.firstA.Get(now)
	f := fv.place.Forecast
	kind, day := fv.heroKind()
	local := time.Now()
	if f != nil {
		local = f.localNow()
	}
	key := fmt.Sprintf("%p|%d|%.3f|%v|%s|%s|%v|%d|%v", f, fv.sel, fv.first, fv.loading, fv.err, local.Format("15:04"), wxLoad().Fahrenheit, W, 0)
	if fv.layer == nil || fv.layer.Bounds().Dx() != W || fv.layer.Bounds().Dy() != H {
		fv.layer = newCanvas(W, H)
		fv.layerKey = ""
	}
	if key != fv.layerKey || (f != nil && time.Since(f.Fetched) > time.Minute && now.Second() == 0) {
		fv.drawLayer(fv.layer, kind, day, local)
		fv.layerKey = key
	}
	blitOpaque(dst, fv.layer, 0, 0)
	// the big icon, animated
	fv.frame = int(math.Mod(float64(now.UnixNano())/1e9, wxLoopSecs) / wxLoopSecs * wxFrames)
	fv.frameAt = now
	if f != nil || fv.loading {
		ic := wxIcon(kind, px(wxHeroSz), fv.frame)
		blit(dst, ic, px(8), px(32), 1)
	}
}

func (fv *ForecastView) drawLayer(dst *image.RGBA, kind wxKind, day bool, local time.Time) {
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	wCSS := float64(W) / DPR
	d := DPR
	f := fv.place.Forecast
	// the sky, with a soft shine on its upper half (and stars at night)
	fillVGradient(dst, 0, 0, W, H, wxSky(kind, day))
	fillVGradient(dst, 0, 0, W, H/2, []GradStop{{0, rgba(255, 255, 255, 0.16)}, {1, rgba(255, 255, 255, 0)}})
	if !day {
		for i := 0; i < 36; i++ {
			x := math.Mod(float64(i)*97.3, wCSS)
			y := math.Mod(float64(i)*41.7, 118)
			a := 0.25 + 0.5*math.Mod(float64(i)*0.37, 1)
			fillRect(dst, px(x), px(y), max(1, px(0.8)), max(1, px(0.8)), rgba(255, 255, 255, a))
		}
	}
	// the place and the time there
	nameF := face(W700, 16)
	timeF := face(W600, 12)
	subF := face(W500, 11)
	tl := local.Format("Mon 15:04")
	tw := textWidth(timeF, tl)
	if f != nil {
		shadowText(dst, timeF, tl, float64(W)-tw-10*d, 18*d, colWhite)
	}
	shadowText(dst, nameF, ellipsize(nameF, fv.place.Name, float64(W)-tw-30*d), 10*d, 19*d, colWhite)
	sub := fv.place.where()
	switch {
	case fv.loading:
		sub = strings.Trim(sub+" · Updating…", " ·")
	case fv.err != "" && f != nil:
		sub = strings.Trim(sub+" · Couldn't update: "+fv.err, " ·")
	case f != nil:
		sub = strings.Trim(sub+" · Updated "+agoText(f.Fetched), " ·")
	}
	shadowText(dst, subF, ellipsize(subF, sub, float64(W)-20*d), 10*d, 33*d, rgba(255, 255, 255, 0.85))

	if f == nil {
		msg, msg2 := "Loading the forecast…", "From Open-Meteo"
		if fv.err != "" && !fv.loading {
			msg, msg2 = "Couldn't load the forecast", fv.err+" · press to try again"
		}
		mf := face(W700, 15)
		shadowText(dst, mf, msg, 104*d, 72*d, colWhite)
		shadowText(dst, subF, ellipsize(subF, msg2, float64(W)-114*d), 104*d, 90*d, rgba(255, 255, 255, 0.85))
		return
	}

	// ---- the hero: the selected day ----
	x := 102.0
	bigF := face(W700, 44)
	var big, hi, lo, cond, line string
	i := fv.sel
	if i == 0 {
		big = wxTemp(f.Current.Temp)
		cond = wxLabel(f.Current.Code)
		line = fmt.Sprintf("Feels %s · Humidity %d%% · %s %s", wxTemp(f.Current.Feels), int(f.Current.Humidity), compass(f.Current.WindDir), wxWind(f.Current.Wind))
	} else {
		dt, _ := time.Parse("2006-01-02", f.Daily.Time[i])
		big = wxTemp(f.Daily.Max[i])
		cond = wxHeroDays[dt.Weekday()] + " · " + wxLabel(f.Daily.Code[i])
		var parts []string
		if r := f.rain(i); r >= 0 {
			parts = append(parts, fmt.Sprintf("Rain %d%%", int(r)))
		}
		if i < len(f.Daily.Sunrise) && len(f.Daily.Sunrise[i]) >= 16 {
			parts = append(parts, "Sunrise "+f.Daily.Sunrise[i][11:16])
		}
		if i < len(f.Daily.Sunset) && len(f.Daily.Sunset[i]) >= 16 {
			parts = append(parts, "Sunset "+f.Daily.Sunset[i][11:16])
		}
		line = strings.Join(parts, " · ")
	}
	hi, lo = "H "+wxTemp(f.Daily.Max[i]), "L "+wxTemp(f.Daily.Min[i])
	shadowText(dst, bigF, big, x*d, 82*d, colWhite)
	bw := textWidth(bigF, big) / d
	// high and low: the Forecast Channel's red and blue, on little pills
	pill := func(s string, y float64, top, bot Col) {
		pf := face(W700, 12)
		w := textWidth(pf, s)/d + 14
		px0 := x + bw + 10
		fillRoundedGradient(dst, px0*d, y*d, (px0+w)*d, (y+17)*d, 8.5*d, []GradStop{{0, top}, {1, bot}}, 1)
		fillRoundedGradient(dst, (px0+1)*d, (y+1)*d, (px0+w-1)*d, (y+8)*d, 7*d, []GradStop{{0, rgba(255, 255, 255, 0.55)}, {1, rgba(255, 255, 255, 0.05)}}, 1)
		strokeRounded(dst, px0*d, y*d, (px0+w)*d, (y+17)*d, 8.5*d, 1.5, rgba(255, 255, 255, 0.7), 1)
		drawText(dst, pf, s, (px0+7)*d, (y+8.5+lineCapHeight(12)/2)*d, colWhite)
	}
	pill(hi, 45, hex("#ff6a55"), hex("#d42a1e"))
	pill(lo, 65, hex("#5aa8ff"), hex("#1f63d0"))
	condF := face(W700, 14)
	shadowText(dst, condF, ellipsize(condF, cond, float64(W)-(x+8)*d), x*d, 103*d, colWhite)
	lineF := face(W500, 10)
	shadowText(dst, lineF, ellipsize(lineF, line, float64(W)-(x+8)*d), x*d, 118*d, rgba(255, 255, 255, 0.88))

	// ---- the days: glass tiles ----
	n := f.days()
	x0 := (wCSS - (wxTiles*wxTileW + (wxTiles-1)*wxTileG)) / 2
	clip := image.Rect(px(x0-2), px(wxStripY-4), px(wCSS-x0+2), px(wxStripY+wxTileH+4))
	strip := dst.SubImage(clip).(*image.RGBA)
	dayF := face(W700, 10)
	hiF := face(W700, 14)
	loF := face(W600, 12)
	rainF := face(W600, 9)
	for j := 0; j < n; j++ {
		tx := x0 + (float64(j)-fv.first)*(wxTileW+wxTileG)
		if tx > wCSS || tx+wxTileW < 0 {
			continue
		}
		ty := wxStripY
		sel := j == fv.sel
		if sel {
			ty -= 2
		}
		x1, y1 := tx+wxTileW, ty+wxTileH
		// shadow, glass, shine, edge
		fillRoundedGradient(strip, tx*d, (ty+2)*d, x1*d, (y1+2)*d, 9*d, []GradStop{{0, rgba(0, 20, 50, 0.18)}, {1, rgba(0, 20, 50, 0.28)}}, 1)
		top, bot := rgba(255, 255, 255, 0.88), rgba(225, 236, 248, 0.78)
		if sel {
			top, bot = rgba(255, 255, 255, 0.98), rgba(214, 234, 255, 0.95)
		}
		fillRoundedGradient(strip, tx*d, ty*d, x1*d, y1*d, 9*d, []GradStop{{0, top}, {1, bot}}, 1)
		fillRoundedGradient(strip, (tx+1.5)*d, (ty+1.5)*d, (x1-1.5)*d, (ty+wxTileH*0.42)*d, 8*d, []GradStop{{0, rgba(255, 255, 255, 0.85)}, {1, rgba(255, 255, 255, 0.15)}}, 1)
		if sel {
			strokeRounded(strip, (tx-1.5)*d, (ty-1.5)*d, (x1+1.5)*d, (y1+1.5)*d, 10.5*d, 3.5, rgba(70, 170, 255, 0.95), 1)
			strokeRounded(strip, tx*d, ty*d, x1*d, y1*d, 9*d, 1.5, rgba(255, 255, 255, 1), 1)
		} else {
			strokeRounded(strip, tx*d, ty*d, x1*d, y1*d, 9*d, 1.5, rgba(255, 255, 255, 0.9), 1)
		}
		// the day
		lab := "TODAY"
		if j > 0 {
			dt, _ := time.Parse("2006-01-02", f.Daily.Time[j])
			lab = strings.ToUpper(dt.Format("Mon"))
		}
		lw := textWidth(dayF, lab) / d
		drawText(strip, dayF, lab, (tx+(wxTileW-lw)/2)*d, (ty+13)*d, hex("#3b4f6b"))
		// the icon
		ic := wxIcon(wxKindFor(f.Daily.Code[j], true), px(wxTileIc), -1)
		blit(strip, ic, px(tx+(wxTileW-wxTileIc)/2), px(ty+16), 1)
		// high in red, low in blue
		hs, ls := wxTemp(f.Daily.Max[j]), wxTemp(f.Daily.Min[j])
		hw, lw2 := textWidth(hiF, hs)/d, textWidth(loF, ls)/d
		gap := 4.0
		sx := tx + (wxTileW-(hw+gap+lw2))/2
		drawText(strip, hiF, hs, sx*d, (ty+68)*d, hex("#d8261b"))
		drawText(strip, loF, ls, (sx+hw+gap)*d, (ty+68)*d, hex("#1f5fc8"))
		// the chance of rain, when there is one
		if r := f.rain(j); r >= 10 {
			rs := fmt.Sprintf("%d%%", int(r))
			rw := textWidth(rainF, rs) / d
			dropW := 6.0
			rx := tx + (wxTileW-(dropW+2+rw))/2
			drop := wxDropIcon(px(dropW + 2))
			blit(strip, drop, px(rx-1), px(ty+72.5), 1)
			drawText(strip, rainF, rs, (rx+dropW+2)*d, (ty+81)*d, hex("#3d6aa6"))
		}
	}
	// more days on either side
	arrow := func(right bool) {
		y := wxStripY + wxTileH/2
		ax := x0 - 4.5
		if right {
			ax = wCSS - x0 + 4.5
		}
		s := 3.5
		if right {
			fillTriangle(dst, (ax-s/2)*d, (y-s)*d, (ax+s/2)*d, y*d, (ax-s/2)*d, (y+s)*d, rgba(255, 255, 255, 0.9))
		} else {
			fillTriangle(dst, (ax+s/2)*d, (y-s)*d, (ax-s/2)*d, y*d, (ax+s/2)*d, (y+s)*d, rgba(255, 255, 255, 0.9))
		}
	}
	if fv.firstTo > 0 {
		arrow(false)
	}
	if fv.firstTo+wxTiles < n {
		arrow(true)
	}
}

// wxDropIcon: the little raindrop of the tiles.
func wxDropIcon(size int) *image.RGBA {
	return wxPartIcon("drop", size)
}
