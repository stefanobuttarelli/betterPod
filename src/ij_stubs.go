package main

// ij_stubs.go — glue between the ipod.js UI and the MiyooPod back end.

import (
	"sync/atomic"
	"time"
)

// rescanPending: a rescan was asked for while one was running.
var rescanPending atomic.Bool

// rescanLibrary rescans the music folder in the background and rebuilds
// the menus when it's done.
func (ui *IpodUI) rescanLibrary() {
	if dbBusy { // the database or the music is being moved
		return
	}
	go func() {
		done := make(chan struct{})
		if !ui.app.startLibraryScan(func() { close(done) }) {
			// one is running: scan again when it ends (it may have
			// passed the folder that just changed)
			rescanPending.Store(true)
			return
		}
		<-done
		time.Sleep(10 * time.Millisecond)
		ui.postSure(func() {
			ui.kb.initialized = false
			ui.refreshBuilt()
			if rescanPending.Swap(false) {
				ui.rescanLibrary()
			}
		})
		// covers from the files and from cover.jpg / folder.jpg of the new
		// albums (at start-up main runs this pass; after a rescan, here)
		ui.app.deferredArtExtraction()
	}()
}
