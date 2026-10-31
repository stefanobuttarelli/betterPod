package main

import (
	"fmt"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"
)

// restoreBrightness sets brightness back to a visible level before app exits
func restoreBrightness() {
	if isKnulli || isOnion {
		restoreBacklight()
		return
	}
	// Set to mid-level brightness (duty_cycle range is typically 0-100)
	brightnessPath := "/sys/class/pwm/pwmchip0/pwm0/duty_cycle"
	defaultBrightness := "50"

	if err := os.WriteFile(brightnessPath, []byte(defaultBrightness), 0644); err != nil {
		logMsg(fmt.Sprintf("WARNING: Could not restore brightness: %v", err))
	} else {
		logMsg("Brightness restored to default level")
	}
}

// startInactivityMonitor runs in background and auto-locks screen after period of inactivity
func (app *MiyooPod) startInactivityMonitor() {
	ticker := time.NewTicker(30 * time.Second) // Check every 30 seconds
	defer ticker.Stop()

	for {
		select {
		case <-ticker.C:
			// Skip if auto-lock is disabled or already locked
			if app.AutoLockMinutes <= 0 || app.Locked {
				continue
			}

			// Check if inactive for specified duration
			inactiveDuration := time.Since(app.LastActivityTime)
			autoLockDuration := time.Duration(app.AutoLockMinutes) * time.Minute

			if inactiveDuration >= autoLockDuration {
				logMsg(fmt.Sprintf("INFO: Auto-lock triggered after %v of inactivity", inactiveDuration))
				app.toggleLock()
			}
		}
	}
}

// exitAppGracefully quits after the power button was held 5 s. It runs on
// a timer's goroutine, so it only stops the main loop: the loop's way out
// (main.go) saves playback and puts the backlight and the CPU back, without
// racing the interface.
func (app *MiyooPod) exitAppGracefully() {
	logMsg("INFO: Power button held for 5s — exiting gracefully")
	app.Running = false
}

// watchQuitSignals: a shutdown (SIGTERM from the system, e.g. Onion on a
// low battery, or Knulli closing the port) saves the queue and position
// and quits cleanly, through the main loop's way out.
func watchQuitSignals() {
	ch := make(chan os.Signal, 1)
	signal.Notify(ch, syscall.SIGTERM, syscall.SIGINT, syscall.SIGHUP)
	s := <-ch
	logMsg("Signal " + s.String() + ": saving and quitting")
	app := globalApp
	if app == nil {
		os.Exit(0)
	}
	app.Running = false
	// if the main loop can't get there in time
	time.AfterFunc(3*time.Second, func() {
		logMsg("Signal: the main loop didn't quit in 3 s, quitting here")
		onionWake()
		app.savePlaybackState()
		restoreBrightness()
		app.restoreCPUGovernor()
		os.Exit(0)
	})
}

// showPowerExitWarning brightens the screen if dimmed and shows the exit warning overlay.
func (app *MiyooPod) showPowerExitWarning() {
	if app.Locked {
		restoreBrightness()
	}
	app.PowerExitWarning = true
	app.requestRedraw()
}

// hidePowerExitWarning removes the exit warning overlay and re-dims the screen if locked.
func (app *MiyooPod) hidePowerExitWarning() {
	if !app.PowerExitWarning {
		return
	}
	app.PowerExitWarning = false
	if app.Locked {
		app.dimScreen()
	}
	app.requestRedraw()
}

// resetInactivityTimer resets the inactivity timer (called on user interaction)
func (app *MiyooPod) resetInactivityTimer() {
	app.LastActivityTime = time.Now()
}

// dimScreen reduces brightness to minimum (for locked state)
func (app *MiyooPod) dimScreen() {
	if isKnulli || isOnion {
		setBacklightRaw(0)
		logMsg("Screen dimmed (locked)")
		return
	}
	brightnessPath := "/sys/class/pwm/pwmchip0/pwm0/duty_cycle"
	dimBrightness := "0"

	if err := os.WriteFile(brightnessPath, []byte(dimBrightness), 0644); err != nil {
		logMsg(fmt.Sprintf("WARNING: Could not dim screen: %v", err))
	} else {
		logMsg("Screen dimmed (locked)")
	}
}

// --- CPU governor management ---

// cpuGovernorPaths returns the sysfs paths for all CPU cores' scaling_governor
func cpuGovernorPaths() []string {
	var paths []string
	for i := 0; i < 2; i++ { // Dual-core Cortex-A7
		paths = append(paths, fmt.Sprintf("/sys/devices/system/cpu/cpu%d/cpufreq/scaling_governor", i))
	}
	return paths
}

// readCPUGovernor reads the current governor from cpu0
func readCPUGovernor() string {
	data, err := os.ReadFile("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(data))
}

// writeCPUGovernor writes the governor to all CPU cores
func writeCPUGovernor(governor string) error {
	for _, path := range cpuGovernorPaths() {
		if err := os.WriteFile(path, []byte(governor), 0644); err != nil {
			return err
		}
	}
	return nil
}

// initCPUGovernor saves the original CPU governor for later restoration on exit
func (app *MiyooPod) initCPUGovernor() {
	app.OriginalCPUGovernor = readCPUGovernor()
	if app.OriginalCPUGovernor != "" {
		logMsg(fmt.Sprintf("INFO: Original CPU governor: %s", app.OriginalCPUGovernor))
	}
}

// setCPUGovernorLocked switches to powersave (screen off, audio-only — max battery savings)
func (app *MiyooPod) setCPUGovernorLocked() {
	if err := writeCPUGovernor("powersave"); err != nil {
		logMsg(fmt.Sprintf("WARNING: Could not set CPU governor to powersave: %v", err))
	}
}

// setCPUGovernorUnlocked switches to performance (full speed for responsive UI)
func (app *MiyooPod) setCPUGovernorUnlocked() {
	if err := writeCPUGovernor("performance"); err != nil {
		logMsg(fmt.Sprintf("WARNING: Could not set CPU governor to performance: %v", err))
	}
}

// restoreCPUGovernor restores the original CPU governor on app exit
func (app *MiyooPod) restoreCPUGovernor() {
	if app.OriginalCPUGovernor == "" {
		return
	}
	if err := writeCPUGovernor(app.OriginalCPUGovernor); err != nil {
		logMsg(fmt.Sprintf("WARNING: Could not restore CPU governor to %s: %v", app.OriginalCPUGovernor, err))
	} else {
		logMsg(fmt.Sprintf("INFO: CPU governor restored to %s", app.OriginalCPUGovernor))
	}
}

// cpuOnLockLabel returns the settings menu label for the CPU-on-lock option.
func (app *MiyooPod) cpuOnLockLabel() string {
	if app.KeepPerformanceOnLock {
		return "CPU on Lock: Performance"
	}
	return "CPU on Lock: Power Save"
}

// toggleKeepPerformanceOnLock flips whether the CPU stays in performance while
// the screen is locked (instead of dropping to powersave). It updates the menu
// label in place and, if currently locked, applies the change immediately.
func (app *MiyooPod) toggleKeepPerformanceOnLock() {
	app.KeepPerformanceOnLock = !app.KeepPerformanceOnLock

	// Apply immediately if the screen is already locked.
	if app.Locked {
		if app.KeepPerformanceOnLock {
			app.setCPUGovernorUnlocked()
		} else {
			app.setCPUGovernorLocked()
		}
	}

	// Update the current menu item's label in place (avoids resetting the
	// menu selection, which could otherwise trigger an accidental submenu entry).
	if len(app.MenuStack) > 0 {
		current := app.MenuStack[len(app.MenuStack)-1]
		if current.SelIndex >= 0 && current.SelIndex < len(current.Items) {
			current.Items[current.SelIndex].Label = app.cpuOnLockLabel()
		}
	}

	app.MenuBG = nil // force redraw with updated label
	app.drawCurrentScreen()

	if err := app.saveSettings(); err != nil {
		logMsg(fmt.Sprintf("ERROR: Failed to save CPU-on-lock preference: %v", err))
	}
}
