package main

import (
	"encoding/json"
	"fmt"
	"os"
	"syscall"
	"time"
)

// clearAppData deletes library cache, settings, and artwork, preserving the installation UUID
func (app *BetterPod) clearAppData() {
	dc := app.DC

	// Full-screen layout matching app design language
	dc.SetHexColor(app.CurrentTheme.BG)
	dc.Clear()

	// Header bar
	dc.SetHexColor(app.CurrentTheme.HeaderBG)
	dc.DrawRectangle(0, 0, SCREEN_WIDTH, HEADER_HEIGHT)
	dc.Fill()

	dc.SetFontFace(app.FontHeader)
	dc.SetHexColor(app.CurrentTheme.HeaderTxt)
	dc.DrawStringAnchored("Clear App Data", SCREEN_WIDTH/2, HEADER_HEIGHT/2, 0.5, 0.5)

	// Warning message centered in content area
	dc.SetFontFace(app.FontMenu)
	dc.SetHexColor(app.CurrentTheme.ItemTxt)
	dc.DrawStringAnchored("Clear all app data?", SCREEN_WIDTH/2, SCREEN_HEIGHT/2-20, 0.5, 0.5)

	dc.SetFontFace(app.FontSmall)
	dc.SetHexColor(app.CurrentTheme.Dim)
	dc.DrawStringAnchored("This will reset settings and rescan your library", SCREEN_WIDTH/2, SCREEN_HEIGHT/2+16, 0.5, 0.5)

	// Status bar at bottom
	barY := float64(SCREEN_HEIGHT - STATUS_BAR_HEIGHT)

	dc.SetHexColor(app.CurrentTheme.HeaderBG)
	dc.DrawRectangle(0, barY, SCREEN_WIDTH, STATUS_BAR_HEIGHT)
	dc.Fill()

	dc.SetHexColor(app.CurrentTheme.ProgBG)
	dc.SetLineWidth(1)
	dc.DrawLine(0, barY, SCREEN_WIDTH, barY)
	dc.Stroke()

	centerY := barY + float64(STATUS_BAR_HEIGHT)/2
	dc.SetFontFace(app.FontSmall)
	app.drawButtonLegend(12, centerY, "A", "Confirm")
	app.drawButtonLegend(150, centerY, "B", "Cancel")

	app.triggerRefresh()

	// Wait for user input
	for app.Running {
		key := Key(C_GetKeyPress())
		if key == NONE {
			time.Sleep(33 * time.Millisecond)
			continue
		}

		switch key {
		case A:
			app.performClearAppData()
			return
		case B, MENU:
			// Clear armed key-repeat: this loop consumed the release of the
			// key that opened the modal, so the main loop must not repeat it.
			app.LastKey = NONE
			app.drawCurrentScreen()
			return
		}
	}
}

// performClearAppData does the actual deletion and restarts the app
func (app *BetterPod) performClearAppData() {
	// Preserve installation UUID
	installID := app.InstallationID

	// Show clearing message
	dc := app.DC
	dc.SetHexColor(app.CurrentTheme.BG)
	dc.Clear()
	dc.SetFontFace(app.FontMenu)
	dc.SetHexColor(app.CurrentTheme.HeaderTxt)
	dc.DrawStringAnchored("Clearing app data...", SCREEN_WIDTH/2, SCREEN_HEIGHT/2, 0.5, 0.5)
	app.triggerRefresh()

	// Delete library cache
	if err := os.Remove(LIBRARY_JSON_PATH()); err != nil && !os.IsNotExist(err) {
		logMsg(fmt.Sprintf("WARNING: Failed to remove library cache: %v", err))
	}

	// Delete settings
	if err := os.Remove(SETTINGS_PATH()); err != nil && !os.IsNotExist(err) {
		logMsg(fmt.Sprintf("WARNING: Failed to remove settings: %v", err))
	}

	// Delete artwork directory
	if err := os.RemoveAll(ARTWORK_DIR()); err != nil && !os.IsNotExist(err) {
		logMsg(fmt.Sprintf("WARNING: Failed to remove artwork: %v", err))
	}

	// Write back a minimal settings file with just the UUID
	minimalSettings := Settings{
		InstallationID: installID,
	}
	data, err := json.MarshalIndent(minimalSettings, "", "  ")
	if err == nil {
		os.WriteFile(SETTINGS_PATH(), data, 0644)
	}

	logMsg("INFO: App data cleared, restarting...")

	// Stop playback
	if app.Playing != nil && app.Playing.State == StatePlaying {
		audioStop()
	}

	// Cleanup
	close(app.RefreshChan)
	sdlCleanup()

	// Restart the current binary.
	closeKnulliIdle()
	if exe, err := os.Executable(); err == nil {
		if err := syscall.Exec(exe, os.Args, os.Environ()); err != nil {
			logMsg(fmt.Sprintf("ERROR: Failed to restart: %v", err))
		}
	}
}

// toggleUpdateNotifications toggles the update notifications setting
func (app *BetterPod) toggleUpdateNotifications() {
	app.UpdateNotifications = !app.UpdateNotifications

	// Rebuild the settings menu to update the label
	app.RootMenu = app.buildRootMenu()
	app.MenuStack = []*MenuScreen{app.RootMenu}

	// Navigate to settings menu
	for _, item := range app.RootMenu.Items {
		if item.Label == "Settings" {
			app.MenuStack = append(app.MenuStack, item.Submenu)
			break
		}
	}

	app.drawCurrentScreen()

	if err := app.saveSettings(); err != nil {
		logMsg(fmt.Sprintf("ERROR: Failed to save update notifications preference: %v", err))
	}
}
