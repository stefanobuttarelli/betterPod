package main

import "time"

func (app *BetterPod) handlePowerButtonPress() {
	if !app.PowerButtonPressed {
		app.PowerButtonPressed = true
		app.PowerButtonPressTime = time.Now()

		app.PowerHoldTimer = time.AfterFunc(2*time.Second, func() {
			if !app.PowerButtonPressed {
				return
			}
			app.showPowerExitWarning()

			app.PowerExitTimer = time.AfterFunc(3*time.Second, func() {
				if !app.PowerButtonPressed {
					return
				}
				app.exitAppGracefully()
			})
		})
	}
}

func (app *BetterPod) handlePowerButtonRelease() {
	if app.PowerButtonPressed {
		holdDuration := time.Since(app.PowerButtonPressTime)
		app.PowerButtonPressed = false

		app.stopPowerTimers()
		app.hidePowerExitWarning()

		if holdDuration < 5*time.Second {
			app.toggleLock()
		}
	}
}

func (app *BetterPod) stopPowerTimers() {
	if app.PowerHoldTimer != nil {
		app.PowerHoldTimer.Stop()
		app.PowerHoldTimer = nil
	}
	if app.PowerExitTimer != nil {
		app.PowerExitTimer.Stop()
		app.PowerExitTimer = nil
	}
}
