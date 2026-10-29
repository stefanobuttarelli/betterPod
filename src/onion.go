package main

/*
#cgo LDFLAGS: -ldl
void onion_set_mode(int on);
int onion_ao_get_raw(void);
int onion_ao_set_raw(int value);
int onion_ao_stats(int *failed);
*/
import "C"

// onion.go — betterPod's platform layer for the Miyoo: Miyoo Mini, Mini Plus, Mini v4 and
// Mini Flip running Onion OS (32-bit ARM, 128 MB of RAM).
//
// KnulliPod's interface runs as it is; what changes is the hardware around
// it (Knulli's layer is knulli.go):
//
//   - launch.sh stops keymon (it would suspend the app on POWER, so the
//     music would stop) and Onion's audioserver; the app owns the keys:
//     POWER turns the screen off and on (hold 5 s: quit), the volume keys
//     of the Mini Plus move the volume, MENU + volume keys or L2 / R2 the
//     brightness, and MENU is the iPod's MENU button (back);
//   - POWER and the volume keys are read from /dev/input/event0 by
//     onion.c, the volume is MI_AO's hardware volume (Onion's curve, up to
//     0 dB), and onion.c makes the SDL2 audio driver wait for MI_AO instead
//     of dropping buffers;
//   - frames are presented on a thread (main.c) and drawn with the C pixel
//     loops of cdraw.c; Settings → Effects is Light by default
//     (ij_effects.go);
//   - the backlight is the PWM duty cycle (0-100, Onion's levels);
//   - battery level from Onion's batmon (/tmp/percBat), charging from
//     gpio59 (Mini) or the AXP223 (Mini Plus);
//   - memory: Go keeps to one core and a soft memory limit, the artwork
//     cache is small, smaller tracks are played from RAM.
//
// Every MIYOOPOD_* switch of the Knulli build still works (see launch.sh).

import (
	"context"
	"encoding/json"
	"fmt"
	"image"
	"math"
	"os"
	"os/exec"
	"os/signal"
	"regexp"
	"runtime/debug"
	"strconv"
	"strings"
	"sync/atomic"
	"syscall"
	"time"
)

// isOnion: the Miyoo build (MIYOOPOD_PLATFORM, see knulli.go).
var isOnion = !isKnulli

func init() {
	if !isOnion {
		return
	}
	C.onion_set_mode(1)
	// Onion keeps about half of the 128 MB: the GC works harder past this
	debug.SetMemoryLimit(int64(envInt("MIYOOPOD_MEMLIMIT_MB", 48, 16, 512)) << 20)
	// a decoded cover is scaled for the lists and Cover Flow only; the
	// screensaver's sizes are made from Cover Flow's when it needs them
	prefetchSizes = prefetchSizes[:2]
	go onionWatchSignals()
}

// envInt reads an integer switch, with a default and a valid range.
func envInt(name string, def, min, max int) int {
	if v, err := strconv.Atoi(os.Getenv(name)); err == nil && v >= min && v <= max {
		return v
	}
	return def
}

// onionGoProcs: the cores Go may use (MIYOOPOD_GOMAXPROCS). One by default:
// the other Cortex-A7 is left to SDL_mixer's decoding thread, so a busy
// frame (Cover Flow, a scan) can't make the music stutter.
func onionGoProcs() int { return envInt("MIYOOPOD_GOMAXPROCS", 1, 1, 4) }

// onionMaxRAMTrack: tracks up to this size are played from RAM, bigger ones
// are streamed from the card (MIYOOPOD_RAM_TRACK_MB).
func onionMaxRAMTrack() int64 { return int64(envInt("MIYOOPOD_RAM_TRACK_MB", 24, 0, 256)) << 20 }

// onionArtBudget: bytes of scaled artwork kept in memory.
func onionArtBudget() int64 { return int64(envInt("MIYOOPOD_ART_CACHE_MB", 16, 4, 256)) << 20 }

// onionWatchSignals: a shutdown (SIGTERM from Onion, e.g. on a low
// battery) saves the queue and position and quits cleanly.
func onionWatchSignals() {
	ch := make(chan os.Signal, 1)
	signal.Notify(ch, syscall.SIGTERM, syscall.SIGINT, syscall.SIGHUP)
	s := <-ch
	logMsg("Signal " + s.String() + ": saving and quitting")
	onionLogAudio()
	app := globalApp
	if app == nil {
		os.Exit(0)
	}
	app.Running = false
	// if the main loop can't get there in time
	time.AfterFunc(3*time.Second, func() {
		app.savePlaybackState()
		restoreBrightness()
		os.Exit(0)
	})
}

// --- volume (MI_AO) --------------------------------------------------------

// onionVolumeRaw maps the player volume (0..1) to MI_AO's dB with the shape
// of Onion's curve (volume.h: level 0-20, raw = 48·log10(1+level) − 60), so
// 5% is one step of the console's volume keys. 0 mutes. Onion's curve ends
// at +3 dB, which clips loud recordings (a crackle on every peak): here it
// is scaled to end at 0 dB, unless MIYOOPOD_VOLUME_BOOST=1.
func onionVolumeRaw(v float64) int {
	if v <= 0 {
		return -60
	}
	if v > 1 {
		v = 1
	}
	k := 60 / math.Log10(21)
	if os.Getenv("MIYOOPOD_VOLUME_BOOST") == "1" {
		k = 48
	}
	return int(math.Round(k*math.Log10(1+20*v))) - 60
}

// onionLogAudio logs how MI_AO took the sound so far: refused buffers mean
// clicks (with MI_AO_SendFrame waiting for room there should be none).
func onionLogAudio() {
	var failed C.int
	sent := int(C.onion_ao_stats(&failed))
	if sent > 0 || failed > 0 {
		logMsg(fmt.Sprintf("Audio: %d buffers played, %d refused by MI_AO", sent, int(failed)))
	}
}

var warmGen int32

// onionWarmFile reads a track that is too big for RAM ahead of the player,
// at a gentle pace, so the kernel's cache holds it and the audio thread
// doesn't wait for the SD card mid-song. A new track stops the last one.
func onionWarmFile(path string) {
	gen := atomic.AddInt32(&warmGen, 1)
	f, err := os.Open(path)
	if err != nil {
		return
	}
	defer f.Close()
	buf := make([]byte, 256<<10)
	for atomic.LoadInt32(&warmGen) == gen {
		n, err := f.Read(buf)
		if n == 0 || err != nil {
			return
		}
		time.Sleep(20 * time.Millisecond) // about 12 MB/s
	}
}

var onionAOFailed bool

// onionSetVolume sets the hardware volume (0..1).
func onionSetVolume(v float64) {
	raw := onionVolumeRaw(v)
	if r := int(C.onion_ao_set_raw(C.int(raw))); r == -1000 {
		if !onionAOFailed {
			onionAOFailed = true
			logMsg("WARNING: can't open /dev/mi_ao, volume not set")
		}
	}
}

// onionSystemVolume returns Onion's own volume setting (0-20 in
// /appconfigs/system.json) as 0..1, or -1.
func onionSystemVolume() float64 {
	data, err := os.ReadFile("/appconfigs/system.json")
	if err != nil {
		return -1
	}
	var cfg struct {
		Vol  *float64 `json:"vol"`
		Mute *float64 `json:"mute"`
	}
	if json.Unmarshal(data, &cfg) != nil || cfg.Vol == nil {
		return -1
	}
	if cfg.Mute != nil && *cfg.Mute != 0 {
		return 0
	}
	return math.Min(math.Max(*cfg.Vol/20, 0), 1)
}

// onionInitVolume: the player volume is the hardware volume (SDL_mixer
// stays at 100%). The first start takes Onion's volume setting, later
// starts the one saved in the iPod settings.
func (ui *IpodUI) onionInitVolume() {
	if _, err := os.Stat(ipodPrefsPath()); err != nil {
		if v := onionSystemVolume(); v >= 0 {
			ui.volume = v
		}
	}
	audioSetVolume(100)
	ui.applyVolume()
	logMsg(fmt.Sprintf("Volume %d%% (MI_AO %d dB)", int(ui.volume*100+0.5), onionVolumeRaw(ui.volume)))
}

// onionApplyVolume sets the volume; app.SystemVolume follows it, since
// mpvLoadFile applies it again on every track (MI_AO may reset).
func onionApplyVolume(app *MiyooPod, v float64) {
	app.SystemVolume = int(v*100 + 0.5)
	onionSetVolume(v)
}

// onionStepVolume: a volume key, one Onion step (5%).
func (ui *IpodUI) onionStepVolume(up bool) {
	v := ui.volume - 0.05
	if up {
		v = ui.volume + 0.05
	}
	ui.volume = math.Min(math.Max(math.Round(v*20)/20, 0), 1)
	ui.applyVolume()
	ui.savePrefs()
	if ui.app.Locked {
		return // screen off: just the volume
	}
	ui.dirty = true
	for _, npv := range ui.nowPlayingViews() {
		npv.flashVolume(ui)
	}
	if ui.saver != nil || !ui.nowPlayingOnScreen() { // (the screensaver shows it in its bar)
		ui.showVolumeOSD()
	}
}

// --- backlight (PWM) -------------------------------------------------------

const onionPWMDuty = "/sys/class/pwm/pwmchip0/pwm0/duty_cycle"

// onionBacklightGet returns the duty cycle (0-100), or -1.
func onionBacklightGet() int {
	v := readInt(onionPWMDuty)
	if v > 100 {
		v = 100
	}
	return v
}

func onionBacklightSet(pct int) {
	if pct > 0 {
		onionPanel(true) // (the panel follows the backlight: onion_sleep.go)
	}
	if err := os.WriteFile(onionPWMDuty, []byte(strconv.Itoa(pct)), 0644); err != nil && brightLogged < 6 {
		brightLogged++
		logMsg(fmt.Sprintf("Brightness %d%%: %v", pct, err))
	}
	if pct <= 0 && globalApp != nil && globalApp.Locked {
		onionPanel(false) // the screen off: the panel off too, as in Onion
	}
}

// Onion's eleven brightness levels (display.h): raw = 3·e^(0.350656·level).
func onionLevelRaw(level int) int {
	return int(math.Round(3 * math.Exp(0.350656*float64(level))))
}

func onionLevelFromRaw(raw int) int {
	if raw <= 3 {
		return 0
	}
	l := int(math.Round(math.Log(float64(raw)/3) / 0.350656))
	return clamp(l, 0, 10)
}

// --- battery -----------------------------------------------------------------

// onionModel: 283 = Miyoo Mini (and v4), 354 = Mini Plus (written by Onion
// at boot).
var onionModel = readInt("/tmp/deviceModel")

var axpCharging = regexp.MustCompile(`"charging"\s*:\s*(\d+)`)

// onionCharging tells whether the charger is plugged in, as Onion's
// battery.h does: gpio59 on the Mini, the AXP223 (axp_test) on the Mini Plus.
func onionCharging() bool {
	switch onionModel {
	case 283:
		const gpio = "/sys/class/gpio/gpio59/value"
		if _, err := os.Stat(gpio); err != nil {
			_ = os.WriteFile("/sys/class/gpio/export", []byte("59"), 0644)
			_ = os.WriteFile("/sys/class/gpio/gpio59/direction", []byte("in"), 0644)
		}
		b, err := os.ReadFile(gpio)
		return err == nil && len(b) > 0 && b[0] == '1'
	case 354:
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		c := exec.CommandContext(ctx, "./axp_test")
		c.Dir = "/customer/app"
		out, err := c.Output()
		if err != nil {
			return false
		}
		m := axpCharging.FindSubmatch(out)
		return m != nil && string(m[1]) == "3"
	}
	return false
}

// --- keys --------------------------------------------------------------------

var onionMenu struct{ down, combo bool }

// onionKey handles the Onion keys before the usual ones (Run). It returns
// the event to go on with (MENU becomes B) and whether it was used up.
//
//   - volume keys: the volume (also with the screen off or in Hold);
//     MENU + volume keys: the brightness, like in Onion (L2 / R2 are
//     KnulliPod's, in Onion's levels: stepBrightness);
//   - MENU: the iPod's MENU button, i.e. B (back; hold: main menu).
func (ui *IpodUI) onionKey(ev int) (int, bool) {
	if !isOnion {
		return ev, false
	}
	press := ev >= 0
	k := Key(ev)
	if !press {
		k = Key(-ev - 1)
	}
	switch k {
	case VOL_UP, VOL_DOWN:
		if press {
			if onionMenu.down && !hold.on && !ui.app.Locked {
				onionMenu.combo = true
				delete(ui.pressStart, B) // MENU no longer means back
				ui.onionStepBrightness(k == VOL_UP)
			} else {
				ui.onionStepVolume(k == VOL_UP)
			}
		}
		return ev, true
	case MENU:
		if press {
			onionMenu.down, onionMenu.combo = true, false
		} else {
			onionMenu.down = false
		}
		if hold.on || ui.app.Locked {
			return ev, false // swallowed by Hold / the lock
		}
		if press {
			return int(B), false
		}
		if onionMenu.combo {
			return ev, true
		}
		return -int(B) - 1, false
	}
	return ev, false
}

// onionStepBrightness moves the backlight one of Onion's levels (MENU +
// volume keys, and L2 / R2 through KnulliPod's stepBrightness), shown in
// the header like KnulliPod's.
func (ui *IpodUI) onionStepBrightness(up bool) {
	lvl := onionLevelFromRaw(getBacklightPercent())
	if up && lvl < 10 {
		lvl++
	} else if !up && lvl > 0 {
		lvl--
	}
	setBacklightPercent(onionLevelRaw(lvl))
	logMsg(fmt.Sprintf("Brightness level %d (%d%%)", lvl, onionLevelRaw(lvl)))
	now := animNow()
	volOSD.until = now      // one at a time in the header
	briOSD.level = lvl * 10 // (the bar in even steps)
	briOSD.until = now.Add(1500 * time.Millisecond)
	ui.dirty = true
}

var osdBoxGrad = []GradStop{{0, Col{30, 30, 30, 0.82}}, {1, Col{0, 0, 0, 0.82}}}

// drawPowerExitOSD: POWER held for 2 s, betterPod quits at 5 s.
func (ui *IpodUI) drawPowerExitOSD(dst *image.RGBA) {
	if !isOnion || !ui.app.PowerExitWarning { // (Knulli's key is MENU: as before, no overlay)
		return
	}
	fillRoundedGradient(dst, 70*DPR, 84*DPR, 250*DPR, 156*DPR, 14*DPR, osdBoxGrad, 1)
	f := face(W700, 16)
	t := "Keep holding to quit"
	drawText(dst, f, t, 320-textWidth(f, t)/2, 236, Col{255, 255, 255, 1})
	f2 := face(W500, 11)
	t2 := "Release POWER to cancel"
	drawText(dst, f2, t2, 320-textWidth(f2, t2)/2, 272, Col{200, 200, 200, 1})
}

// lockChanged runs when POWER locks or unlocks the screen. Unlocking puts
// the backlight and the CPU back (toggleLock leaves that to the classic
// UI's redraw, which the iPod UI never runs) and redraws the whole frame.
func (ui *IpodUI) lockChanged() {
	if !isOnion {
		return // (Knulli as before: see the PR's note on unlocking there)
	}
	app := ui.app
	ui.dirty = true
	ui.prevFB = nil
	if !app.Locked {
		onionPanel(true) // (also when the backlight can't be read to put it back)
	}
	if app.Locked || !app.BrightnessNeedsRestore {
		return
	}
	app.BrightnessNeedsRestore = false
	lvl := app.BrightnessBeforeLock
	if lvl <= 0 {
		lvl = backlightAtStart
	}
	if lvl > 0 {
		setBacklightPercent(lvl)
	}
	app.setCPUGovernorUnlocked()
}

// onionProgressEvery: the Internet Archive download page (ij_archive.go)
// is redrawn at most this often on the Miyoo. Elsewhere its bar is redrawn
// at every frame while it moves; here 5 frames a second leave the CPU to
// the music, and the bar jumps instead of gliding.
const onionProgressEvery = 200 * time.Millisecond

// npTrackFormat is trackFormat (ij_lossless.go) for the Now Playing badge.
// On the Miyoo there is no badge: its moving bars would keep Now Playing
// redrawing all the time a song plays, and the output is 16-bit / 44.1 kHz
// whatever the file. Without a format nothing is read or drawn.
func (ui *IpodUI) npTrackFormat(t *Track) (audioFormat, bool) {
	if isOnion {
		return audioFormat{}, false
	}
	return ui.trackFormat(t)
}

// onionUnsupported: formats the Miyoo's SDL_mixer can't play (no AAC / ALAC
// decoder in it), with the name shown in the library statistics.
func onionUnsupported(ext string) (string, bool) {
	if !isOnion {
		return "", false
	}
	switch strings.ToLower(ext) {
	case ".m4a", ".mp4", ".alac":
		return "AAC / ALAC (.m4a)", true
	}
	return "", false
}

// onionSettings: Settings without YouTube (KnulliPod 0.38) and Bluetooth
// (KnulliPod 0.49: the Miyoo has none) on the Miyoo.
// yt-dlp needs Python (about 90 MB) and a JavaScript runtime to solve
// YouTube's player challenge, and that alone takes more than 96 MB of
// QuickJS-ng heap (Node.js: more than 48 MB of heap on top of its own
// 40), against the Miyoo's 128 MB with Onion and betterPod running.
// Measured with the armv7 builds, emulated, for the Miyoo version 0.38.2. The
// setting saved by a console sharing the database is left as it is.
func onionSettings(items []Item) []Item {
	if !isOnion {
		return items
	}
	out := items[:0]
	for _, it := range items {
		if it.Label != "YouTube" && it.Label != "Bluetooth" {
			out = append(out, it)
		}
	}
	return out
}

// No Visualizer on the Miyoo (KnulliPod 0.59, Settings → Screensaver →
// Style): it draws the whole screen 30 times a second, with an FFT each
// frame, for as long as the screensaver shows, which would keep the A7's
// one Go core busy (and the music's room smaller) for hours. The Cover Flip
// draws only while covers turn. A style saved by a console sharing the
// database is left as it is; the Miyoo shows the covers.
func onionSaverSettings(items []Item) []Item {
	if !isOnion {
		return items
	}
	out := items[:0]
	for _, it := range items {
		if it.Label != "Style" {
			out = append(out, it)
		}
	}
	return out
}

// onionSaverWarm: the albums whose screensaver covers are kept ready on the
// Miyoo (KnulliPod keeps 240, 11 MB at 48 KB each; 48 are 2.3 MB, twice the
// mosaic's cells).
const onionSaverWarm = 48
