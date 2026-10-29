package main

// ij_bluetooth.go — Settings → Bluetooth: connect the headphones without
// leaving betterPod.
//
//	Bluetooth                 On
//	▣ Sony WH-1000XM4         Connected · 80%
//	▣ AirPods Pro             Not Connected
//	  Find New Devices        Put the headphones in pairing mode
//	▣ JBL Flip 5              New · press A to pair and connect
//
// It drives BlueZ, Knulli's Bluetooth, with bluetoothctl (one command at a
// time, each in the background with a time limit): the paired devices and
// whether they are connected (read again every 4 s while the screen is
// open, so a change made elsewhere shows), A on one connects or
// disconnects it, Find New Devices searches for 12 s and A on a new one
// pairs, trusts and connects it. Once connected, the sound goes to it
// (PipeWire's default output, through pactl). Each device gets a picture
// for its kind (over-ear headphones, earbuds, speaker, game controller),
// from what it says it is and its name.

import (
	"bufio"
	"context"
	"fmt"
	"image"
	"os/exec"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/fogleman/gg"
)

type btDevice struct {
	MAC, Name, Icon   string
	Paired, Connected bool
	Battery           int // percent, -1 unknown
}

var (
	reBTMac  = regexp.MustCompile(`^[0-9A-Fa-f]{2}(:[0-9A-Fa-f]{2}){5}$`)
	reANSI   = regexp.MustCompile("\x1b\\[[0-9;?]*[A-Za-z]|\x01|\x02|\r")
	reBTDev  = regexp.MustCompile(`Device ([0-9A-Fa-f:]{17}) ?(.*)$`)
	reBTBatt = regexp.MustCompile(`\((\d+)\)`)
)

// btMu: one bluetoothctl command at a time (the search runs beside them).
var btMu sync.Mutex

// btLast: what the last look found, for the Settings row (UI goroutine).
var btLast struct {
	connected string // the name of a connected device
	powered   bool
	known     bool
}

func btBin() string { return findBin("bluetoothctl") }

// btctl runs bluetoothctl with args; the output without colours and
// prompts.
func btctl(timeout time.Duration, args ...string) (string, error) {
	btMu.Lock()
	defer btMu.Unlock()
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	c := exec.CommandContext(ctx, btBin(), args...)
	c.Env = toolEnv()
	out, err := c.CombinedOutput()
	s := reANSI.ReplaceAllString(string(out), "")
	if ctx.Err() == context.DeadlineExceeded {
		return s, fmt.Errorf("no answer from Bluetooth")
	}
	if err != nil {
		if l := btErrLine(s); l != "" {
			return s, fmt.Errorf("%s", l)
		}
	}
	return s, err
}

// btErrLine: the line that says what went wrong ("Failed to connect: …"),
// in words.
func btErrLine(s string) string {
	l := btRawErr(s)
	low := strings.ToLower(l)
	switch {
	case l == "":
		return ""
	case strings.Contains(low, "page-timeout"), strings.Contains(low, "hostdown"), strings.Contains(low, "not available"):
		return "not found · is it on and close by? (It may be connected to another device)"
	case strings.Contains(low, "authentication"), strings.Contains(low, "rejected"):
		return "pairing refused · put it in pairing mode and try again"
	case strings.Contains(low, "inprogress"), strings.Contains(low, "busy"):
		return "already trying · wait a moment"
	case strings.Contains(low, "profile-unavailable"), strings.Contains(low, "protocol"):
		return "it doesn't offer audio to this console"
	case strings.Contains(low, "not ready"), strings.Contains(low, "notready"):
		return "Bluetooth is off"
	}
	return l
}

func btRawErr(s string) string {
	for _, l := range strings.Split(s, "\n") {
		l = strings.TrimSpace(l)
		if strings.HasPrefix(l, "Failed") || strings.Contains(l, "not available") || strings.HasPrefix(l, "Invalid") {
			l = strings.TrimPrefix(l, "Failed to ")
			if i := strings.Index(l, "org.bluez.Error."); i >= 0 {
				l = l[:i] + strings.TrimPrefix(l[i:], "org.bluez.Error.")
			}
			return l
		}
	}
	return ""
}

func btPowered() bool {
	out, err := btctl(5*time.Second, "show")
	return err == nil && strings.Contains(out, "Powered: yes")
}

func btSetPower(on bool) error {
	v := "off"
	if on {
		v = "on"
	}
	_, err := btctl(8*time.Second, "power", v)
	return err
}

// btInfo: what BlueZ knows of one device.
func btInfo(mac string) btDevice {
	d := btDevice{MAC: mac, Battery: -1}
	out, _ := btctl(5*time.Second, "info", mac)
	for _, l := range strings.Split(out, "\n") {
		k, v, ok := strings.Cut(strings.TrimSpace(l), ":")
		if !ok {
			continue
		}
		v = strings.TrimSpace(v)
		switch k {
		case "Alias":
			d.Name = v
		case "Name":
			if d.Name == "" {
				d.Name = v
			}
		case "Icon":
			d.Icon = v
		case "Paired":
			d.Paired = v == "yes"
		case "Connected":
			d.Connected = v == "yes"
		case "Battery Percentage":
			if m := reBTBatt.FindStringSubmatch(v); m != nil {
				d.Battery, _ = strconv.Atoi(m[1])
			}
		}
	}
	if d.Name == "" {
		d.Name = mac
	}
	return d
}

// btDevices: the devices of a "devices" listing (paired: only those).
func btDevices(paired bool) ([]btDevice, error) {
	args := []string{"devices"}
	if paired {
		args = append(args, "Paired")
	}
	out, err := btctl(5*time.Second, args...)
	if paired && (err != nil || strings.Contains(out, "Invalid")) {
		out, err = btctl(5*time.Second, "paired-devices") // BlueZ before 5.65
	}
	if err != nil {
		return nil, err
	}
	var ds []btDevice
	seen := map[string]bool{}
	for _, l := range strings.Split(out, "\n") {
		m := reBTDev.FindStringSubmatch(strings.TrimSpace(l))
		if m == nil || seen[m[1]] {
			continue
		}
		seen[m[1]] = true
		d := btInfo(m[1])
		if paired && !d.Paired {
			continue
		}
		ds = append(ds, d)
	}
	// by name only: a row doesn't move when it connects (the cursor stays
	// on it)
	sort.SliceStable(ds, func(i, j int) bool { return strings.ToLower(ds[i].Name) < strings.ToLower(ds[j].Name) })
	return ds, nil
}

// btScan searches for secs seconds; found is called for each new device.
func btScan(ctx context.Context, secs int, found func(mac, name string)) error {
	c := exec.CommandContext(ctx, btBin(), "--timeout", strconv.Itoa(secs), "scan", "on")
	c.Env = toolEnv()
	out, err := c.StdoutPipe()
	if err != nil {
		return err
	}
	c.Stderr = c.Stdout
	if err := c.Start(); err != nil {
		return err
	}
	sc := bufio.NewScanner(out)
	for sc.Scan() {
		l := reANSI.ReplaceAllString(sc.Text(), "")
		if !strings.Contains(l, "NEW") {
			continue
		}
		if m := reBTDev.FindStringSubmatch(l); m != nil {
			found(m[1], strings.TrimSpace(m[2]))
		}
	}
	return c.Wait()
}

// btPairConnect pairs, trusts (so it reconnects by itself next time) and
// connects a new device.
func btPairConnect(mac string) error {
	if _, err := btctl(40*time.Second, "--agent", "NoInputNoOutput", "pair", mac); err != nil {
		if !strings.Contains(err.Error(), "AlreadyExists") {
			// an older bluetoothctl without --agent: once more without it
			if _, err2 := btctl(40*time.Second, "pair", mac); err2 != nil && !strings.Contains(err2.Error(), "AlreadyExists") {
				return err
			}
		}
	}
	_, _ = btctl(8*time.Second, "trust", mac)
	return btConnect(mac)
}

// btConnect connects and sends the sound to it.
func btConnect(mac string) error {
	if _, err := btctl(25*time.Second, "connect", mac); err != nil {
		return err
	}
	btUseSink(mac)
	return nil
}

func btDisconnect(mac string) error {
	_, err := btctl(10*time.Second, "disconnect", mac)
	return err
}

func btForget(mac string) error {
	_, err := btctl(10*time.Second, "remove", mac)
	return err
}

// btUseSink makes the device's PipeWire output the default one (it shows
// a moment after the connection).
func btUseSink(mac string) {
	if findBin("pactl") == "" {
		return
	}
	id := strings.ReplaceAll(strings.ToUpper(mac), ":", "_")
	for i := 0; i < 16; i++ {
		out, err := pactlCmd("list", "short", "sinks").Output()
		if err == nil {
			for _, l := range strings.Split(string(out), "\n") {
				f := strings.Fields(l)
				if len(f) >= 2 && strings.HasPrefix(f[1], "bluez_") && strings.Contains(strings.ToUpper(f[1]), id) {
					if err := pactlCmd("set-default-sink", f[1]).Run(); err == nil {
						logMsg("Bluetooth: the sound goes to " + f[1])
					}
					return
				}
			}
		}
		time.Sleep(500 * time.Millisecond)
	}
	logMsg("Bluetooth: no output for " + mac + " (the sound stays where it was)")
}

// ---- the screen ----

func (ui *IpodUI) bluetoothView() *View {
	st := &struct {
		loaded, powered, scanning bool
		status                    string
		devs, found               []btDevice
		doing                     map[string]string // MAC → "Connecting…"
		scanLeft                  int
		closed                    bool
		stop                      context.CancelFunc
	}{doing: map[string]string{}}
	v := &View{ID: "bluetooth", Title: "Bluetooth", Kind: KindFull}
	have := btBin() != ""
	var refresh func()
	busy := false
	refresh = func() {
		if busy || !have {
			return
		}
		busy = true
		go func() {
			on := btPowered()
			var ds []btDevice
			var err error
			if on {
				ds, err = btDevices(true)
			}
			ui.postSure(func() {
				busy = false
				st.loaded, st.powered = true, on
				if err == nil {
					st.devs = ds
				}
				btLast.known, btLast.powered, btLast.connected = true, on, ""
				for _, d := range ds {
					if d.Connected {
						btLast.connected = d.Name
						break
					}
				}
				// the new ones that got paired leave "New"
				var keep []btDevice
				for _, f := range st.found {
					paired := false
					for _, d := range ds {
						paired = paired || d.MAC == f.MAC
					}
					if !paired {
						keep = append(keep, f)
					}
				}
				st.found = keep
				ui.refreshView(v)
			})
		}()
	}
	// while open: read again every 4 s (a change made elsewhere shows)
	tick := time.NewTicker(4 * time.Second)
	go func() {
		for range tick.C {
			ui.post(func() {
				if !st.closed && len(st.doing) == 0 {
					refresh()
				}
			})
		}
	}()
	v.onClose = func() {
		st.closed = true
		tick.Stop()
		if st.stop != nil {
			st.stop()
		}
	}
	act := func(d btDevice, label string, f func() error) {
		if st.doing[d.MAC] != "" {
			return
		}
		st.doing[d.MAC] = label
		st.status = ""
		ui.refreshView(v)
		go func() {
			err := f()
			ui.postSure(func() {
				delete(st.doing, d.MAC)
				if err != nil {
					st.status = d.Name + ": " + err.Error()
					if l, sub, ok := strings.Cut(st.status, " · "); ok {
						st.status = l + " · " + sub
					}
					logMsg("Bluetooth: " + d.MAC + ": " + err.Error())
				}
				if !st.closed {
					refresh()
				}
				ui.refreshView(v)
			})
		}()
	}
	scan := func() {
		if st.scanning {
			return
		}
		const secs = 12
		ctx, cancel := context.WithCancel(context.Background())
		st.stop, st.scanning, st.scanLeft, st.status = cancel, true, secs, ""
		ui.refreshView(v)
		end := time.Now().Add(secs * time.Second)
		go func() {
			for t := time.NewTicker(time.Second); time.Now().Before(end) && ctx.Err() == nil; <-t.C {
				left := int(time.Until(end).Seconds() + 0.5)
				ui.post(func() { st.scanLeft = left; ui.refreshView(v) })
			}
		}()
		go func() {
			var macs []string // the ones seen (this goroutine's own list)
			err := btScan(ctx, secs, func(mac, name string) {
				if name == "" || strings.ReplaceAll(name, "-", ":") == mac {
					return // nameless ones (most aren't headphones)
				}
				macs = append(macs, mac)
				ui.post(func() {
					for _, d := range append(st.devs, st.found...) {
						if d.MAC == mac {
							return
						}
					}
					st.found = append(st.found, btDevice{MAC: mac, Name: name, Battery: -1})
					ui.refreshView(v)
				})
			})
			// the kinds of the new ones, now that the search has seen them
			var infos []btDevice
			for _, m := range macs {
				infos = append(infos, btInfo(m))
			}
			ui.postSure(func() {
				st.scanning, st.stop = false, nil
				for i := range st.found {
					for _, in := range infos {
						if in.MAC == st.found[i].MAC && in.Icon != "" {
							st.found[i].Icon = in.Icon
						}
					}
				}
				if err != nil && ctx.Err() == nil && len(st.found) == 0 {
					st.status = "Couldn't search: " + shortErr(err)
				} else if len(st.found) == 0 && ctx.Err() == nil {
					st.status = "Nothing new found · put the headphones in pairing mode and search again"
				}
				ui.refreshView(v)
			})
		}()
	}
	v.Build = func() []Item {
		if !have {
			return []Item{{Label: "Bluetooth isn't available", Sublabel: "bluetoothctl wasn't found on this system", NoArrow: true}}
		}
		if !st.loaded {
			return []Item{{Label: "Bluetooth", Value: "…", NoArrow: true}}
		}
		items := []Item{{Label: "Bluetooth", Value: onOff(st.powered), NoArrow: true, OnSelect: func() {
			on := !st.powered
			st.powered = on
			ui.refreshView(v)
			go func() {
				err := btSetPower(on)
				ui.postSure(func() {
					if err != nil {
						st.status = "Couldn't turn it " + map[bool]string{true: "on", false: "off"}[on] + ": " + err.Error()
					}
					refresh()
				})
			}()
		}}}
		if st.status != "" {
			l, sub, _ := strings.Cut(st.status, " · ")
			items = append(items, Item{Label: l, Sublabel: sub, NoArrow: true})
		}
		if !st.powered {
			return items
		}
		if len(st.devs) == 0 {
			items = append(items, Item{Label: "No devices paired yet", Sublabel: "Find New Devices below", NoArrow: true})
		}
		for _, d := range st.devs {
			d := d
			sub := "Not Connected · press A to connect"
			if d.Connected {
				sub = "Connected"
				if d.Battery >= 0 {
					sub += fmt.Sprintf(" · Battery %d%%", d.Battery)
				}
			}
			if s := st.doing[d.MAC]; s != "" {
				sub = s
			}
			toggle := func() {
				if d.Connected {
					act(d, "Disconnecting…", func() error { return btDisconnect(d.MAC) })
				} else {
					act(d, "Connecting…", func() error { return btConnect(d.MAC) })
				}
			}
			label := "Connect"
			if d.Connected {
				label = "Disconnect"
			}
			items = append(items, Item{Label: d.Name, Sublabel: sub, Icon: ui.btIcon(d), OnSelect: toggle,
				LongPress: []SheetOption{
					{Label: label, OnSelect: toggle},
					{Label: "Forget This Device", Danger: true, OnSelect: func() {
						ui.showSheetTitled("Forget "+d.Name+"? It will have to be paired again.", []SheetOption{
							{Label: "Forget This Device", Danger: true, OnSelect: func() {
								act(d, "Forgetting…", func() error { return btForget(d.MAC) })
							}}})
					}},
				}})
		}
		find := Item{Label: "Find New Devices", Sublabel: "Put the headphones in pairing mode first", OnSelect: scan}
		if st.scanning {
			find = Item{Label: "Searching…", Sublabel: fmt.Sprintf("%d s · new devices show below", st.scanLeft), NoArrow: true,
				Progress: 1 - float64(st.scanLeft)/12}
		}
		items = append(items, find)
		for _, d := range st.found {
			d := d
			sub := "New · press A to pair and connect"
			if s := st.doing[d.MAC]; s != "" {
				sub = s
			}
			items = append(items, Item{Label: d.Name, Sublabel: sub, Icon: ui.btIcon(d), OnSelect: func() {
				if st.stop != nil {
					st.stop() // the search stops: pairing goes better without it
				}
				act(d, "Pairing…", func() error { return btPairConnect(d.MAC) })
			}})
		}
		return items
	}
	v.Items = v.Build()
	refresh()
	return v
}

// btSettingsValue: the Settings row's value (the device connected).
func btSettingsValue() string {
	switch {
	case !btLast.known:
		return ""
	case btLast.connected != "":
		return btLast.connected
	case btLast.powered:
		return "On"
	}
	return "Off"
}

// ---- pictures ----

// btKind: what a device is, from its BlueZ icon and its name.
func btKind(d btDevice) string {
	n := strings.ToLower(d.Name)
	for _, w := range []string{"airpods", "buds", "pods", "earbud", "tws", "wf-", "elite", "free", "fit pro", "studio buds", "beats fit", "powerbeats", "liberty", "soundcore life p", "ear (", "nothing ear"} {
		if strings.Contains(n, w) && !strings.Contains(n, "max") {
			return "earbuds"
		}
	}
	switch {
	case strings.HasPrefix(d.Icon, "audio-head"):
		return "headphones"
	case d.Icon == "audio-card" || strings.Contains(n, "speaker") || strings.Contains(n, "flip") || strings.Contains(n, "boom") || strings.Contains(n, "soundlink"):
		return "speaker"
	case d.Icon == "input-gaming" || strings.Contains(n, "controller") || strings.Contains(n, "gamepad") || strings.Contains(n, "xbox") || strings.Contains(n, "dualsense"):
		return "gamepad"
	case strings.Contains(n, "wh-") || strings.Contains(n, "headphone") || strings.Contains(n, "qc") || strings.Contains(n, "max"):
		return "headphones"
	}
	return "other"
}

// btIcon: the row picture of d's kind (made once per kind).
func (ui *IpodUI) btIcon(d btDevice) string {
	name := "bt_" + btKind(d)
	ui.assetsMu.Lock()
	_, ok := ui.assets[name]
	ui.assetsMu.Unlock()
	if !ok {
		img := btIconImage(btKind(d))
		ui.assetsMu.Lock()
		ui.assets[name] = img
		ui.assetsMu.Unlock()
	}
	return name
}

// btIconImage: a glossy tile with a white picture of the kind, 48 px.
func btIconImage(kind string) *image.RGBA {
	S := px(48)
	img := image.NewRGBA(image.Rect(0, 0, S, S))
	p := pen{gg.NewContextForRGBA(img), float64(S) / 100}
	dc := p.dc
	tops := map[string][2]string{"headphones": {"#5ec2ff", "#1769d6"}, "earbuds": {"#6fe0d0", "#14958a"},
		"speaker": {"#ffb35c", "#e2620f"}, "gamepad": {"#b58cff", "#6a3fd1"}, "other": {"#9fb2c8", "#56697f"}}
	c := tops[kind]
	// the tile
	dc.DrawRoundedRectangle(p.u(8), p.u(8), p.u(84), p.u(84), p.u(20))
	p.fill(p.linear(0, 8, 0, 92, GradStop{0, hex(c[0])}, GradStop{1, hex(c[1])}))
	dc.DrawRoundedRectangle(p.u(10), p.u(9.5), p.u(80), p.u(38), p.u(18))
	p.fill(p.linear(0, 9, 0, 48, GradStop{0, rgba(255, 255, 255, 0.45)}, GradStop{1, rgba(255, 255, 255, 0.05)}))
	white := p.solid(rgba(255, 255, 255, 1))
	shade := p.solid(rgba(0, 0, 0, 0.18))
	draw := func(dy float64, paint gg.Pattern) {
		dc.Push()
		dc.Translate(0, p.u(dy))
		dc.SetStrokeStyle(paint)
		dc.SetFillStyle(paint)
		switch kind {
		case "headphones":
			dc.SetLineWidth(p.u(6))
			dc.SetLineCap(gg.LineCapRound)
			dc.DrawArc(p.u(50), p.u(54), p.u(25), gg.Radians(180), gg.Radians(360))
			dc.Stroke()
			dc.DrawRoundedRectangle(p.u(20), p.u(50), p.u(14), p.u(24), p.u(6))
			dc.Fill()
			dc.DrawRoundedRectangle(p.u(66), p.u(50), p.u(14), p.u(24), p.u(6))
			dc.Fill()
		case "earbuds":
			for _, x := range []float64{36, 64} {
				dc.DrawCircle(p.u(x), p.u(40), p.u(10))
				dc.Fill()
				dc.DrawRoundedRectangle(p.u(x-3.5), p.u(42), p.u(7), p.u(28), p.u(3.5))
				dc.Fill()
			}
		case "speaker":
			dc.DrawRoundedRectangle(p.u(30), p.u(22), p.u(40), p.u(56), p.u(9))
			dc.Fill()
			dc.SetFillStyle(p.solid(hex(c[1])))
			dc.DrawCircle(p.u(50), p.u(58), p.u(11))
			dc.Fill()
			dc.DrawCircle(p.u(50), p.u(35), p.u(5))
			dc.Fill()
		case "gamepad":
			dc.DrawRoundedRectangle(p.u(18), p.u(36), p.u(64), p.u(32), p.u(16))
			dc.Fill()
			dc.SetFillStyle(p.solid(hex(c[1])))
			dc.DrawRectangle(p.u(29), p.u(49.5), p.u(16), p.u(5))
			dc.Fill()
			dc.DrawRectangle(p.u(34.5), p.u(44), p.u(5), p.u(16))
			dc.Fill()
			dc.DrawCircle(p.u(62), p.u(48), p.u(3.5))
			dc.Fill()
			dc.DrawCircle(p.u(70), p.u(56), p.u(3.5))
			dc.Fill()
		default: // the Bluetooth rune
			dc.SetLineWidth(p.u(6))
			dc.SetLineCap(gg.LineCapRound)
			dc.SetLineJoin(gg.LineJoinRound)
			dc.MoveTo(p.u(36), p.u(36))
			dc.LineTo(p.u(62), p.u(60))
			dc.LineTo(p.u(50), p.u(72))
			dc.LineTo(p.u(50), p.u(28))
			dc.LineTo(p.u(62), p.u(40))
			dc.LineTo(p.u(36), p.u(64))
			dc.Stroke()
		}
		dc.Pop()
	}
	draw(1.5, shade)
	draw(0, white)
	return img
}
