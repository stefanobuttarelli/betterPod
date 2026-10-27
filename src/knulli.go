package main

/*
int disp_brightness(int value);
*/
import "C"

// Knulli / PortMaster platform layer.
//
// Upstream MiyooPod talks directly to Miyoo Mini hardware (MI_AO audio ioctl,
// PWM backlight, raw power-button events, OTA updates of armhf builds).
// On Knulli (Anbernic H700 and similar) the OS owns the power and volume keys,
// the backlight lives under /sys/class/backlight, and updates come from
// PortMaster, so those paths are replaced here.

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
)

// isKnulli is true unless MIYOOPOD_PLATFORM=miyoo is set (keeps upstream behaviour).
var isKnulli = os.Getenv("MIYOOPOD_PLATFORM") != "miyoo"

// otaDisabled: upstream OTA would install a Miyoo (armhf) build over this port.
var otaDisabled = isKnulli

// musicRoot returns the folder scanned for music, always with a trailing slash.
// Default on Knulli is /userdata/music; override with MIYOOPOD_MUSIC_DIR.
func musicRoot() string {
	dir := os.Getenv("MIYOOPOD_MUSIC_DIR")
	if dir == "" {
		if isKnulli {
			dir = "/userdata/music"
		} else {
			dir = "/mnt/SDCARD/Media/Music"
		}
	}
	return strings.TrimRight(dir, "/") + "/"
}

// dataPath returns where library cache, settings and artwork are stored:
// the database folder (datadir.go), by default the port's conf folder
// (MIYOOPOD_DATA_DIR); without it, upstream behaviour (inside music root).
func dataPath(name string) string {
	dir := dataDir()
	_ = os.MkdirAll(dir, 0755)
	return filepath.Join(dir, name) + map[bool]string{true: "/", false: ""}[strings.HasSuffix(name, "/")]
}

// setSystemVolume is intentionally a no-op on Knulli: the hardware volume keys
// are handled by the OS, and the app keeps SDL_mixer at 100%.
func setSystemVolume(percent int) {}

// --- Backlight (/sys/class/backlight/*) ---

var (
	backlightDir     string
	backlightMax     = -1
	backlightAtStart = -1 // percent
)

// On Knulli the backlight goes through batocera-brightness: on the H700
// consoles it drives the display engine (/dev/disp ioctl via "brightness"),
// /sys/class/backlight is not used there. sysfs is the fallback elsewhere.
var (
	brightTool     = ""
	brightToolInit bool
	brightMu       sync.Mutex
	brightWant     = -1
	brightCur      = -1
	brightWake     = make(chan struct{}, 1)
)

func brightnessTool() string {
	if !brightToolInit {
		brightToolInit = true
		if os.Getenv("MIYOOPOD_SYSFS_BACKLIGHT") != "1" {
			brightTool = findBin("batocera-brightness")
		}
		if brightTool != "" {
			go brightWriter()
		}
	}
	return brightTool
}

func brightWriter() {
	for range brightWake {
		for {
			brightMu.Lock()
			v := brightWant
			brightWant = -1
			brightMu.Unlock()
			if v < 0 {
				break
			}
			brightSet(v)
		}
	}
}

// toolEnv makes sure helper scripts find their own tools (bc, brightness)
// even when the launcher trimmed PATH.
func toolEnv() []string {
	env := os.Environ()
	path := os.Getenv("PATH")
	for _, d := range []string{"/usr/bin", "/bin", "/usr/sbin", "/sbin"} {
		if !strings.Contains(":"+path+":", ":"+d+":") {
			path += ":" + d
		}
	}
	return append(env, "PATH="+strings.TrimPrefix(path, ":"))
}

var brightLogged int

// dispRaw maps a percentage to the display engine's 0..255 range (0 = off).
func dispRaw(pct int) int {
	if pct <= 0 {
		return 0
	}
	r := pct * 255 / 100
	if r < 3 {
		r = 3
	}
	return r
}

// brightSet applies a level synchronously: /dev/disp ioctl first (H700),
// then batocera-brightness, the "brightness" tool, and sysfs.
func brightSet(pct int) {
	if _, err := os.Stat("/dev/disp"); err == nil {
		r := int(C.disp_brightness(C.int(dispRaw(pct))))
		if brightLogged < 6 {
			brightLogged++
			logMsg(fmt.Sprintf("Brightness %d%%: /dev/disp ioctl -> %d", pct, r))
		}
		if r == 0 {
			// applied
			return
		}
	}
	c := exec.Command(brightTool, strconv.Itoa(pct))
	c.Env = toolEnv()
	out, err := c.CombinedOutput()
	if brightLogged < 6 {
		brightLogged++
		logMsg(fmt.Sprintf("Brightness %d%%: %s %v %s", pct, brightTool, err, strings.TrimSpace(string(out))))
	}
	if err == nil {
		return
	}
	if b := findBin("brightness"); b != "" {
		raw := pct * 255 / 100
		if pct > 0 && raw < 3 {
			raw = 3
		}
		c := exec.Command(b, "set", strconv.Itoa(raw))
		c.Env = toolEnv()
		out, err := c.CombinedOutput()
		logMsg(fmt.Sprintf("Brightness fallback: %s set %d: %v %s", b, raw, err, strings.TrimSpace(string(out))))
		if err == nil {
			return
		}
	}
	sysfsBacklight(pct)
}

func sysfsBacklight(pct int) {
	entries, _ := filepath.Glob("/sys/class/backlight/*")
	for _, dir := range entries {
		max := readInt(filepath.Join(dir, "max_brightness"))
		if max > 0 {
			_ = os.WriteFile(filepath.Join(dir, "brightness"), []byte(strconv.Itoa((pct*max+50)/100)), 0644)
			return
		}
	}
}

func findBacklight() bool {
	if brightnessTool() != "" {
		return true
	}
	if backlightMax > 0 {
		return true
	}
	entries, err := filepath.Glob("/sys/class/backlight/*")
	if err != nil || len(entries) == 0 {
		return false
	}
	for _, dir := range entries {
		max := readInt(filepath.Join(dir, "max_brightness"))
		if max > 0 {
			backlightDir = dir
			backlightMax = max
			return true
		}
	}
	return false
}

func readInt(path string) int {
	data, err := os.ReadFile(path)
	if err != nil {
		return -1
	}
	v, err := strconv.Atoi(strings.TrimSpace(string(data)))
	if err != nil {
		return -1
	}
	return v
}

func getBacklightPercent() int {
	if _, err := os.Stat("/dev/disp"); err == nil {
		brightMu.Lock()
		cur, pending := brightCur, brightWant >= 0
		brightMu.Unlock()
		if pending && cur >= 0 {
			return cur
		}
		if raw := int(C.disp_brightness(-1)); raw >= 0 {
			v := (raw*100 + 127) / 255
			brightMu.Lock()
			brightCur = v
			brightMu.Unlock()
			return v
		}
	}
	if t := brightnessTool(); t != "" {
		brightMu.Lock()
		cur, pending := brightCur, brightWant >= 0
		brightMu.Unlock()
		if pending && cur >= 0 {
			return cur
		}
		c := exec.Command(t)
		c.Env = toolEnv()
		out, err := c.Output()
		if err != nil {
			logMsg(fmt.Sprintf("Brightness read failed: %s: %v", t, err))
			return cur
		}
		v, err := strconv.Atoi(strings.TrimSpace(string(out)))
		if err != nil {
			return cur
		}
		brightMu.Lock()
		brightCur = v
		brightMu.Unlock()
		return v
	}
	if !findBacklight() {
		return -1
	}
	raw := readInt(filepath.Join(backlightDir, "brightness"))
	if raw < 0 {
		return -1
	}
	return (raw*100 + backlightMax/2) / backlightMax
}

// setBacklightPercent sets the backlight (0 = off). Asynchronous with the
// Knulli tool: rapid changes are coalesced.
func setBacklightPercent(pct int) {
	if pct < 0 {
		pct = 0
	}
	if pct > 100 {
		pct = 100
	}
	if _, err := os.Stat("/dev/disp"); err == nil {
		if r := int(C.disp_brightness(C.int(dispRaw(pct)))); r == 0 {
			brightMu.Lock()
			brightCur = pct
			brightMu.Unlock()
			if brightLogged < 6 {
				brightLogged++
				logMsg(fmt.Sprintf("Brightness %d%% via /dev/disp", pct))
			}
			return
		} else if brightLogged < 6 {
			brightLogged++
			logMsg(fmt.Sprintf("Brightness: /dev/disp ioctl failed (%d), trying the Knulli tools", r))
		}
	}
	if brightnessTool() != "" {
		brightMu.Lock()
		brightWant, brightCur = pct, pct
		brightMu.Unlock()
		select {
		case brightWake <- struct{}{}:
		default:
		}
		return
	}
	if !findBacklight() {
		return
	}
	raw := (pct*backlightMax + 50) / 100
	if pct > 0 && raw == 0 {
		raw = 1
	}
	_ = os.WriteFile(filepath.Join(backlightDir, "brightness"), []byte(strconv.Itoa(raw)), 0644)
}

// setBacklightRaw is kept for callers that switch the screen off (0).
func setBacklightRaw(v int) {
	if v <= 0 {
		setBacklightPercent(0)
		return
	}
	setBacklightPercent(v)
}

// restoreBacklight puts the backlight back to where it was when the app
// started (synchronously: used on exit).
func restoreBacklight() {
	if backlightAtStart <= 0 {
		return
	}
	if t := brightnessTool(); t != "" {
		brightMu.Lock()
		brightWant = -1
		brightMu.Unlock()
		brightSet(backlightAtStart)
		return
	}
	setBacklightPercent(backlightAtStart)
}

func init() {
	if isKnulli {
		backlightAtStart = getBacklightPercent() // remember the starting level before anything dims it
	}
}
