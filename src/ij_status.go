package main

// ij_status.go — the header's status icons beside the battery: the
// headphones (Bluetooth ones, or wired ones in the jack) and the Wi-Fi with
// its signal (three arcs, filled as it gets stronger).
//
// The Wi-Fi is read every 5 s in the background from the kernel
// (/sys/class/net, /proc/net/wireless: no programs run), so the header
// only reads a number.

import (
	"fmt"
	"image"
	"os"
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
			atomic.StoreInt32(&wifiState, int32(readWifi()))
			time.Sleep(5 * time.Second)
		}
	}()
}

// readWifi: the wireless interface that is up, and its signal in arcs.
func readWifi() int {
	if v := os.Getenv("MIYOOPOD_FAKE_WIFI"); v != "" { // (tests)
		n, _ := strconv.Atoi(v)
		return n
	}
	data, err := os.ReadFile("/proc/net/wireless")
	if err != nil {
		return -1
	}
	// Inter-| sta-|   Quality        |   Discarded packets ...
	//  face | tus | link level noise |  nwid  crypt   frag ...
	// wlan0: 0000   54.  -56.  -256        0      0      0 ...
	for _, l := range strings.Split(string(data), "\n") {
		i := strings.IndexByte(l, ':')
		if i < 0 {
			continue
		}
		iface := strings.TrimSpace(l[:i])
		st, _ := os.ReadFile("/sys/class/net/" + iface + "/operstate")
		if strings.TrimSpace(string(st)) != "up" {
			continue
		}
		f := strings.Fields(l[i+1:])
		if len(f) < 3 {
			continue
		}
		link, _ := strconv.ParseFloat(strings.TrimSuffix(f[1], "."), 64)
		level, _ := strconv.ParseFloat(strings.TrimSuffix(f[2], "."), 64)
		if level < 0 && level > -110 { // dBm
			switch {
			case level >= -60:
				return 3
			case level >= -70:
				return 2
			case level >= -80:
				return 1
			}
			return 0
		}
		q := link / 70 // (the usual scale)
		switch {
		case q >= 0.6:
			return 3
		case q >= 0.4:
			return 2
		case q >= 0.2:
			return 1
		}
		return 0
	}
	return -1
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
	S := px(14)
	img := image.NewRGBA(image.Rect(0, 0, S, S))
	p := pen{gg.NewContextForRGBA(img), float64(S) / 14}
	dc := p.dc
	cx, cy := 7.0, 11.6
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
