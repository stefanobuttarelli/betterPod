package main

// onion_sleep.go — the Miyoo's sleep, as Onion's keymon does it (launch.sh
// stops keymon, so without this the Miyoo would never sleep under betterPod:
// left alone with the screen on, or off with POWER, it ran flat in a few
// hours).
//
//   - With nothing playing, Onion's sleep timer (Settings → System → Sleep
//     timer, "hibernate" in system.json, 5 minutes by default, 0: never)
//     without a key turns the screen off, and one minute later the Miyoo
//     powers off, as Onion's deep sleep does.
//   - The screen turned off with POWER and nothing playing powers off after
//     the sleep timer plus one minute (Onion's suspend timeout).
//   - Never while music plays, while charging, or while a download, a scan,
//     a cover search, an album's tags or a software update are under way.
//   - Powering off: playback is saved as on any exit, /tmp/.offOrder makes
//     Onion's runtime.sh power off once betterPod has quit, and Onion's
//     cmd_to_run.sh is removed so the next start opens Onion's menu (what
//     Onion's deep sleep does to an app).
//   - The screen off is the panel off too (Onion's display_off: gpio4),
//     not only the backlight at 0, and 10 s after the music stops (with
//     nothing under way) the CPU goes to its lowest clock (Onion's powersave: scaling_min_freq)
//     and MI_AO is muted, as in Onion's suspend.
//
// MIYOOPOD_SLEEP_MINUTES overrides the sleep timer (0: never) and
// MIYOOPOD_SLEEP_SCALE the seconds in a minute (tests).

import (
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

var onionSleep struct {
	locked   bool      // the lock as last seen
	since    time.Time // when it changed
	auto     bool      // the screen was turned off by the sleep timer, not POWER
	lastPlay time.Time // music last heard
	checked  time.Time // the last check (once a second)
	deep     bool      // sleeping: screen off, nothing playing (CPU lowest, muted)
	minFreq  string    // scaling_min_freq before sleeping
	mins     int       // the sleep timer, minutes (0: never)
	minsRead time.Time
	charging bool
	chgRead  time.Time
	busy     string // what kept it awake, as last logged
}

// onionSleepAction: what the sleep timer does now: "" nothing, "screen off"
// or "power off". idle is the time since the last key, the last music or
// the screen turning off, whichever came last.
func onionSleepAction(locked, auto, playing, busy, charging bool, idle time.Duration, mins int, minute time.Duration) string {
	if mins <= 0 || playing || busy {
		return ""
	}
	if !locked {
		if idle >= time.Duration(mins)*minute {
			return "screen off"
		}
		return ""
	}
	limit := time.Duration(mins+1) * minute // POWER: the timer and one minute
	if auto {
		limit = minute // the timer turned it off: one more minute
	}
	if idle >= limit && !charging {
		return "power off"
	}
	return ""
}

// onionSleepTick runs the sleep timer (every frame; it works once a second).
func (ui *IpodUI) onionSleepTick(now time.Time) {
	if !isOnion {
		return
	}
	s := &onionSleep
	app := ui.app
	if app.Locked != s.locked {
		s.locked, s.since = app.Locked, now
		if !app.Locked {
			s.auto = false
		} else {
			// for miyoopod.log: whether the clock really goes down
			const dir = "/sys/devices/system/cpu/cpu0/cpufreq/"
			logMsg(fmt.Sprintf("Screen off: CPU %s kHz (%s, min %s, max %s)", readTrim(dir+"scaling_cur_freq"),
				readTrim(dir+"scaling_governor"), readTrim(dir+"scaling_min_freq"), readTrim(dir+"scaling_max_freq")))
		}
	}
	playing := app.Playing != nil && app.Playing.State == StatePlaying
	if playing || s.lastPlay.IsZero() {
		s.lastPlay = now
	}
	if s.deep && (playing || !app.Locked) {
		onionDeepSleep(ui, false) // awake at once: the music or the screen is back
	}
	if now.Sub(s.checked) < time.Second {
		return
	}
	s.checked = now
	busy := ui.onionBusy()
	// asleep 10 s after the music stops (not in the gap between two songs),
	// and not while something is under way (it keeps its usual clock)
	onionDeepSleep(ui, app.Locked && now.Sub(s.lastPlay) >= 10*time.Second && busy == "")

	minute := time.Duration(envInt("MIYOOPOD_SLEEP_SCALE", 60, 1, 60)) * time.Second
	if now.Sub(s.minsRead) > time.Minute || s.minsRead.IsZero() {
		s.mins, s.minsRead = onionSleepMinutes(), now
	}
	from := s.since
	for _, t := range []time.Time{app.LastActivityTime, s.lastPlay} {
		if t.After(from) {
			from = t
		}
	}
	idle := now.Sub(from)
	if busy != s.busy {
		s.busy = busy
		if busy != "" && s.mins > 0 && !playing {
			logMsg("Sleep timer waits: " + busy)
		}
	}
	// charging (axp_test on the Mini Plus) is only read near power off
	if app.Locked && s.mins > 0 && idle >= minute && now.Sub(s.chgRead) > time.Minute {
		s.charging, s.chgRead = onionChargingRead(), now
	}
	switch onionSleepAction(app.Locked, s.auto, playing, busy != "", s.charging, idle, s.mins, minute) {
	case "screen off":
		logMsg(fmt.Sprintf("Sleep timer: %d min without a key or music, screen off", s.mins))
		s.auto = true
		app.toggleLock()
	case "power off":
		// the reading above may be a minute old: read it again
		if s.charging, s.chgRead = onionChargingRead(), now; s.charging {
			logMsg("Sleep timer: charging, not powering off")
			break
		}
		onionPowerOff(ui, idle)
	}
}

// onionBusy: the work that keeps the Miyoo awake ("" when none).
func (ui *IpodUI) onionBusy() string {
	switch {
	case ui.app.LibScanRunning:
		return "the library scan"
	case iaCur != nil && !iaCur.finished:
		return "a download"
	case coverCur != nil && !coverCur.finished:
		return "the cover search"
	case moveCur != nil && moveRunning(moveCur.snapPhase()):
		return "the music being moved"
	case upd.state == updDownloading || upd.state == updInstalling:
		return "the software update"
	case len(albumSaving) > 0:
		return "an album's tags being written"
	}
	return ""
}

// onionSleepMinutes: Onion's sleep timer (system.json's "hibernate", in
// minutes; 5 when it isn't there, as in Onion).
func onionSleepMinutes() int {
	if v, err := strconv.Atoi(os.Getenv("MIYOOPOD_SLEEP_MINUTES")); err == nil && v >= 0 {
		return v
	}
	for _, p := range []string{"/mnt/SDCARD/system.json", "/appconfigs/system.json"} {
		data, err := os.ReadFile(p)
		if err != nil {
			continue
		}
		var cfg struct {
			Hibernate *int `json:"hibernate"`
		}
		if json.Unmarshal(data, &cfg) == nil && cfg.Hibernate != nil {
			return max(*cfg.Hibernate, 0)
		}
	}
	return 5
}

// onionDeepSleep: with the screen off and nothing playing, the CPU at its
// lowest clock and the sound muted; back as they were otherwise.
func onionDeepSleep(ui *IpodUI, on bool) {
	s := &onionSleep
	if on == s.deep {
		return
	}
	s.deep = on
	const dir = "/sys/devices/system/cpu/cpu0/cpufreq/"
	if on {
		s.minFreq = readTrim(dir + "scaling_min_freq")
		if lo := readTrim(dir + "cpuinfo_min_freq"); lo != "" {
			_ = os.WriteFile(dir+"scaling_min_freq", []byte(lo), 0644)
		}
		_ = os.WriteFile(dir+"scaling_governor", []byte("powersave"), 0644)
		onionSetVolume(0)
		logMsg(fmt.Sprintf("Sleeping: CPU %s kHz (%s, min %s, was %s), sound muted",
			readTrim(dir+"scaling_cur_freq"), readTrim(dir+"scaling_governor"), readTrim(dir+"scaling_min_freq"), s.minFreq))
		return
	}
	if s.minFreq != "" {
		_ = os.WriteFile(dir+"scaling_min_freq", []byte(s.minFreq), 0644)
	}
	ui.applyVolume()
	logMsg("Awake: CPU " + readTrim(dir+"scaling_cur_freq") + " kHz, sound back")
}

// onionWake: on the way out, the clock and the sound as they were before
// sleeping (a quit while asleep left MI_AO muted at -60 dB).
func onionWake() {
	if isOnion && onionSleep.deep && ipodUI != nil {
		onionDeepSleep(ipodUI, false)
	}
}

// onionPowerOff powers the Miyoo off as Onion's deep sleep does for an app:
// betterPod quits (saving playback), and runtime.sh powers off.
func onionPowerOff(ui *IpodUI, idle time.Duration) {
	logMsg(fmt.Sprintf("Sleep timer: screen off for %s with nothing playing, powering off", idle.Round(time.Second)))
	_ = os.WriteFile("/tmp/.offOrder", nil, 0644)
	_ = os.Remove("/mnt/SDCARD/.tmp_update/cmd_to_run.sh")
	// the clock, for the Miyoos without a battery-backed one (Onion's
	// system_clock_save)
	const clockFile = "/mnt/SDCARD/Saves/CurrentProfile/saves/currentTime.txt"
	if _, err := os.Stat("/mnt/SDCARD/Saves/CurrentProfile/saves"); err == nil {
		_ = os.WriteFile(clockFile, []byte(strconv.FormatInt(time.Now().Unix(), 10)), 0644)
	}
	ui.app.Running = false
}

// --- the panel ---------------------------------------------------------------

var onionPanelOff bool

// onionPanel turns the LCD panel on or off (Onion's display_setScreen:
// gpio4), so the screen off draws next to nothing, not only a dark backlight.
func onionPanel(on bool) {
	if on != onionPanelOff {
		return
	}
	put := func(p, v string) { _ = os.WriteFile(p, []byte(v), 0644) }
	put("/sys/class/gpio/export", "4")
	put("/sys/devices/gpiochip0/gpio/gpio4/direction", "out")
	if on {
		put("/sys/devices/gpiochip0/gpio/gpio4/value", "1")
	} else {
		put("/sys/devices/gpiochip0/gpio/gpio4/value", "0")
	}
	put("/sys/class/gpio/unexport", "4")
	if on {
		// the backlight's control, enabled again as Onion does
		put("/sys/class/pwm/pwmchip0/export", "0")
		put("/sys/class/pwm/pwmchip0/pwm0/enable", "0")
		put("/sys/class/pwm/pwmchip0/pwm0/enable", "1")
	}
	onionPanelOff = !on
	if on {
		logMsg("Screen panel on")
	} else {
		logMsg("Screen panel off")
	}
}

// onionLockedFrame: how often the loop wakes with the screen off. POWER
// and the volume keys answer within 0.1 s while music plays; asleep, a
// quarter of a second is enough and wakes the CPU less.
func onionLockedFrame() time.Duration {
	if onionSleep.deep {
		return 250 * time.Millisecond
	}
	return 100 * time.Millisecond
}

func readTrim(p string) string {
	b, err := os.ReadFile(p)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(b))
}
