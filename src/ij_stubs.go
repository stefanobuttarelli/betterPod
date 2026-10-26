package main

// ij_stubs.go — glue between the ipod.js UI and the MiyooPod back end.

import "time"

// rescanLibrary rescans the music folder in the background and rebuilds
// the menus when it's done.
func (ui *IpodUI) rescanLibrary() {
	go func() {
		done := make(chan struct{})
		ui.app.startLibraryScan(func() { close(done) })
		<-done
		time.Sleep(10 * time.Millisecond)
		ui.post(func() {
			ui.kb.initialized = false
			ui.refreshBuilt()
		})
	}()
}
