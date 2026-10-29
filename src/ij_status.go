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
	"strconv"
	"strings"
	"sync/atomic"
	"time"

	"github.com/fogleman/gg"
)

var wifiState int32 = -1 // -1 not connected, else 0..3 arcs

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

// readWifi: the connected wireless interface and its signal in arcs (0-3),
// or -1. The interface is any with a wireless/ or phy80211 entry in
// /sys/class/net; connected means it has a carrier and isn't down or
// dormant. The signal comes from /proc/net/wireless, else from iw, else
// from wpa_cli; connected with no signal to read shows all three arcs.
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
			n := wifiArcs(dbm)
			wifiNote(note + fmt.Sprintf(" signal %d/3", n)) // (not the dBm: it moves all the time)
			return n
		}
		wifiNote(note + " (no signal readable)")
		return 3
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

func wifiArcs(dbm float64) int {
	switch {
	case dbm >= -60:
		return 3
	case dbm >= -70:
		return 2
	case dbm >= -80:
		return 1
	}
	return 0
}

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
	cx, cy := 9.0, 11.6
	on, off := p.solid(c), p.solid(Col{c.R, c.G, c.B, c.A * 0.3})
	// the dot (always there: it is the first step)
	dc.SetFillStyle(on)
	dc.DrawCircle(p.u(cx), p.u(cy), p.u(1.5))
	dc.Fill()
	dc.SetLineCap(gg.LineCapRound)
	dc.SetLineWidth(p.u(1.7))
	for i, r := range []float64{4.2, 7.2, 10.2} {
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
	if n := wifiLevel(); n >= 0 {
		put(wifiIcon(n, c))
	}
	bt := btConnected()
	if headphonesOut() {
		put(volIcon(true, c))
	}
	if bt {
		if white {
			put(whiteIcon(ui, "bluetooth"))
		} else {
			put(ui.themed("bluetooth"))
		}
	}
	return left
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
