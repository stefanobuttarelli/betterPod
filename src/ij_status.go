package main

// ij_status.go — the header's status icons beside the battery: the
// headphones (Bluetooth ones, or wired ones in the jack) and the Wi-Fi with
// its signal (three arcs, filled as it gets stronger).
//
// The Wi-Fi is read every 5 s in the background (/sys/class/net, and the
// signal from /proc/net/wireless, iw or wpa_cli), so the header only reads
// a number.

import (
	"context"
	"fmt"
	"image"
	"os"
	"os/exec"
	"runtime"
	"strconv"
	"strings"
	"sync/atomic"
	"time"

	"github.com/fogleman/gg"
)

var wifiState int32 = -1 // -1 not connected, else 0..5 bars

func wifiLevel() int { return int(atomic.LoadInt32(&wifiState)) }

// startWifiWatch: the signal every 5 s.
func startWifiWatch() {
	go func() {
		for {
			if !onionScreenOff() { // (the Miyoo with the screen off: not read, onion_sleep.go)
				atomic.StoreInt32(&wifiState, int32(readWifi()))
			}
			time.Sleep(5 * time.Second)
		}
	}()
}

// readWifi: the connected wireless interface and its signal in bars (0-5),
// or -1. The interface is any with a wireless/ or phy80211 entry in
// /sys/class/net; connected means it has a carrier and isn't down or
// dormant. The signal comes from /proc/net/wireless, else from iw, else
// from wpa_cli; connected with no signal to read shows all five.
func readWifi() int {
	if v := os.Getenv("MIYOOPOD_FAKE_WIFI"); v != "" { // (tests)
		n, _ := strconv.Atoi(v)
		return n
	}
	read := func(p string) string {
		b, _ := os.ReadFile(p)
		return strings.TrimSpace(string(b))
	}
	entries, _ := os.ReadDir("/sys/class/net")
	note := ""
	for _, e := range entries {
		dir := "/sys/class/net/" + e.Name()
		if !fileExists(dir+"/wireless") && !fileExists(dir+"/phy80211") {
			continue
		}
		iface := e.Name()
		state, carrier := read(dir+"/operstate"), read(dir+"/carrier")
		note += fmt.Sprintf(" %s:%s/%s", iface, state, carrier)
		if carrier != "1" || state == "down" || state == "dormant" || state == "lowerlayerdown" {
			continue
		}
		if dbm, ok := wifiDBM(iface); ok {
			n := wifiBars(dbm)
			wifiNote(note + fmt.Sprintf(" signal %d/5", n)) // (not the dBm: it moves all the time)
			return n
		}
		wifiNote(note + " (no signal readable)")
		return 5
	}
	wifiNote(note)
	return -1
}

// wifiDBM: the interface's signal in dBm.
func wifiDBM(iface string) (float64, bool) {
	// the kernel's table (drivers with wireless extensions)
	if data, err := os.ReadFile("/proc/net/wireless"); err == nil {
		for _, l := range strings.Split(string(data), "\n") {
			i := strings.IndexByte(l, ':')
			if i < 0 || strings.TrimSpace(l[:i]) != iface {
				continue
			}
			f := strings.Fields(l[i+1:])
			if len(f) >= 3 {
				link, _ := strconv.ParseFloat(strings.TrimSuffix(f[1], "."), 64)
				level, _ := strconv.ParseFloat(strings.TrimSuffix(f[2], "."), 64)
				if level < 0 && level > -110 {
					return level, true
				}
				if level > 0 && level <= 100 && link > 0 { // (a 0-100 scale)
					return -100 + level/2, true
				}
				if link > 0 {
					return -100 + link/70*60, true
				}
			}
		}
	}
	// iw dev wlan0 link: "signal: -55 dBm"
	if bin := findBin("iw"); bin != "" {
		if out, err := runQuick(bin, "dev", iface, "link"); err == nil {
			for _, l := range strings.Split(out, "\n") {
				l = strings.TrimSpace(l)
				if strings.HasPrefix(l, "signal:") {
					if f := strings.Fields(l); len(f) >= 2 {
						if v, err := strconv.ParseFloat(f[1], 64); err == nil {
							return v, true
						}
					}
				}
			}
		}
	}
	// wpa_cli signal_poll: "RSSI=-55"
	if bin := findBin("wpa_cli"); bin != "" {
		if out, err := runQuick(bin, "-i", iface, "signal_poll"); err == nil {
			for _, l := range strings.Split(out, "\n") {
				if strings.HasPrefix(l, "RSSI=") {
					if v, err := strconv.ParseFloat(strings.TrimSpace(l[5:]), 64); err == nil && v < 0 {
						return v, true
					}
				}
			}
		}
	}
	return 0, false
}

// wifiBars: the signal in five steps (the dots under the headphones).
func wifiBars(dbm float64) int {
	switch {
	case dbm >= -55:
		return 5
	case dbm >= -63:
		return 4
	case dbm >= -70:
		return 3
	case dbm >= -77:
		return 2
	case dbm >= -85:
		return 1
	}
	return 0
}

// wifiArcs: the five steps as the Wi-Fi icon's three arcs.
func wifiArcs(bars int) int { return (bars*3 + 4) / 5 }

// runQuick: a small program's output, given at most 2 s.
func runQuick(bin string, args ...string) (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	out, err := exec.CommandContext(ctx, bin, args...).Output()
	return string(out), err
}

// wifiNote: what was found, in the log when it changes (to see why an
// icon shows or not on a given console).
var wifiLast string

func wifiNote(s string) {
	if s == "" {
		s = " no wireless interface"
	}
	if s != wifiLast {
		wifiLast = s
		logMsg("Wi-Fi:" + s)
	}
}

// statusKey: what the icons show (the header is drawn again when it changes).
func statusKey() int {
	k := wifiLevel() + 1
	if btConnected() {
		k |= 8
	}
	if headphoneJack() == 1 {
		k |= 16
	}
	return k
}

var wifiIcons = map[string]*image.RGBA{}

// wifiIcon: the dot and three arcs, the first n solid and the rest faint.
func wifiIcon(n int, c Col) *image.RGBA {
	key := fmt.Sprint(n, c)
	if img := wifiIcons[key]; img != nil {
		return img
	}
	// 18 × 14 CSS px: the widest arc (radius 10.2, at ±45°) needs 16.2
	// px plus its stroke, so it isn't cut at the sides
	S := px(14)
	img := image.NewRGBA(image.Rect(0, 0, px(18), S))
	p := pen{gg.NewContextForRGBA(img), float64(S) / 14}
	dc := p.dc
	// its top at the icon's top and the dot's bottom at 11.5 px, as the
	// play / pause sign and the battery (it hung lower, and its arcs looked
	// higher than them)
	cx, cy := 9.0, 10.15
	on, off := p.solid(c), p.solid(Col{c.R, c.G, c.B, c.A * 0.3})
	// the dot (always there: it is the first step)
	dc.SetFillStyle(on)
	dc.DrawCircle(p.u(cx), p.u(cy), p.u(1.4))
	dc.Fill()
	dc.SetLineCap(gg.LineCapRound)
	dc.SetLineWidth(p.u(1.6))
	for i, r := range []float64{3.85, 6.6, 9.35} {
		if i < n {
			dc.SetStrokeStyle(on)
		} else {
			dc.SetStrokeStyle(off)
		}
		dc.DrawArc(p.u(cx), p.u(cy), p.u(r), gg.Radians(-135), gg.Radians(-45))
		dc.Stroke()
	}
	wifiIcons[key] = img
	return img
}

// drawStatusIcons draws, leftwards from left (CSS px), the Wi-Fi and the
// headphones (with the Bluetooth mark when they are Bluetooth ones) in c;
// it returns where the next icon goes.
func (ui *IpodUI) drawStatusIcons(dst *image.RGBA, left, y float64, c Col, white bool) float64 {
	const gap = 4.0
	if !white {
		c = iconTint(ui.themed("bluetooth")) // (the grey of the Bluetooth mark)
	}
	put := func(ic *image.RGBA) {
		iw := float64(ic.Bounds().Dx()) / DPR
		blit(dst, ic, px(left-iw), px(y), 1)
		left -= iw + gap
	}
	bt := btConnected()
	if bt || headphonesOut() {
		// headphones (Bluetooth: with its mark) and the Wi-Fi as five dots
		// under them: one icon for both
		put(headsetIcon(wifiLevel(), bt, c))
		return left
	}
	if n := wifiLevel(); n >= 0 {
		put(wifiIcon(wifiArcs(n), c))
	}
	return left
}

// headsetIcon: headphones, with the Bluetooth mark between the cups when
// bt, and under them the Wi-Fi's strength as five dots (bars of 0-5; none
// when there is no Wi-Fi: the headphones are then centred). 15 × 14 CSS px.
var headsetIcons = map[string]*image.RGBA{}

func headsetIcon(bars int, bt bool, c Col) *image.RGBA {
	key := fmt.Sprint(bars, bt, c)
	if img := headsetIcons[key]; img != nil {
		return img
	}
	img := image.NewRGBA(image.Rect(0, 0, px(15), px(14)))
	p := pen{gg.NewContextForRGBA(img), float64(px(14)) / 14}
	dc := p.dc
	s := 9.5 // the headphones' height
	// (from the icon's top to 11.5 px, as the play / pause sign and the
	// battery)
	cx, cy := 7.5, 5.9
	if bars >= 0 {
		cy = 4.75
	}
	on := p.solid(c)
	// the headband
	w := s * 1.05
	dc.SetStrokeStyle(on)
	dc.SetLineWidth(p.u(s * 0.14))
	dc.DrawEllipticalArc(p.u(cx), p.u(cy-s*0.5+s*0.525), p.u(w/2-s*0.07), p.u(s*0.525-s*0.07), gg.Radians(180), gg.Radians(360))
	dc.Stroke()
	// the cups
	cw, ch := s*0.30, s*0.46
	yb := cy + s*0.05
	x0, x1 := cx-w/2, cx+w/2
	dc.SetFillStyle(on)
	for _, x := range []float64{x0 - cw*0.15, x1 - cw*0.85} {
		dc.DrawRoundedRectangle(p.u(x), p.u(yb-ch*0.1), p.u(cw), p.u(ch), p.u(cw*0.35))
		dc.Fill()
	}
	if bt {
		// the Bluetooth rune
		bw, bh := s*0.16, s*0.56
		by := cy + s*0.1
		dc.SetLineWidth(p.u(s * 0.085))
		dc.SetLineCap(gg.LineCapRound)
		dc.SetLineJoin(gg.LineJoinRound)
		pts := [][2]float64{{cx - bw, by - bh*0.27}, {cx + bw, by + bh*0.25}, {cx, by + bh*0.5}, {cx, by - bh*0.5}, {cx + bw, by - bh*0.25}, {cx - bw, by + bh*0.27}}
		dc.MoveTo(p.u(pts[0][0]), p.u(pts[0][1]))
		for _, q := range pts[1:] {
			dc.LineTo(p.u(q[0]), p.u(q[1]))
		}
		dc.Stroke()
	}
	if bars >= 0 {
		off := p.solid(Col{c.R, c.G, c.B, c.A * 0.25})
		for i := 0; i < 5; i++ {
			if i < bars {
				dc.SetFillStyle(on)
			} else {
				dc.SetFillStyle(off)
			}
			dc.DrawCircle(p.u(cx+float64(i-2)*2.6), p.u(10.7), p.u(0.8))
			dc.Fill()
		}
	}
	headsetIcons[key] = img
	return img
}

// iconTint: the colour of an icon (its most solid pixel).
var iconTints = map[*image.RGBA]Col{}

func iconTint(img *image.RGBA) Col {
	if c, ok := iconTints[img]; ok {
		return c
	}
	best := 3
	for i := 3; i < len(img.Pix); i += 4 {
		if img.Pix[i] > img.Pix[best] {
			best = i
		}
	}
	a := int(img.Pix[best])
	if a == 0 {
		return th.headerText
	}
	un := func(v uint8) float64 { return float64(min(255, int(v)*255/a)) }
	c := Col{un(img.Pix[best-3]), un(img.Pix[best-2]), un(img.Pix[best-1]), 1}
	iconTints[img] = c
	return c
}

// startHealthLog writes, every 5 minutes, the memory, the goroutines and
// the threads to the log, so that a betterPod which grows slow over hours
// shows why (a cache that keeps growing, goroutines that pile up).
func startHealthLog() {
	go func() {
		for {
			time.Sleep(5 * time.Minute)
			var m runtime.MemStats
			runtime.ReadMemStats(&m)
			th, _ := os.ReadDir("/proc/self/task")
			logMsg(fmt.Sprintf("Health: heap %d MB (%d MB from the system), %d GCs, %d goroutines, %d threads, %d font faces",
				m.HeapAlloc>>20, m.Sys>>20, m.NumGC, runtime.NumGoroutine(), len(th), faceCount()))
		}
	}()
}
