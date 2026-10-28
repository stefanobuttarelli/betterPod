package main

// ij_clearnp.go — Clear Now Playing: stops the music and empties the queue,
// so Now Playing leaves the menus (and the session saved for the next
// start) until something is played again.
//
// It is offered by holding A (or Y) on the Now Playing row of the main menu
// and of Music, and at the end of Up Next (Now Playing → Y), where it asks
// first like Clear Up Next.

// clearNowPlayingOptions: the options of the Now Playing rows (nil when
// nothing is loaded).
func (ui *IpodUI) clearNowPlayingOptions() []SheetOption {
	if p := ui.app.Playing; p == nil || p.Track == nil {
		return nil
	}
	return []SheetOption{{Label: "Clear Now Playing", Danger: true, OnSelect: ui.clearNowPlaying}}
}

// clearNowPlayingItem: the last row of Up Next.
func (ui *IpodUI) clearNowPlayingItem() []Item {
	if p := ui.app.Playing; p == nil || p.Track == nil {
		return nil
	}
	return []Item{{Label: "Clear Now Playing", Sublabel: "Stop the music and empty the queue", NoArrow: true,
		OnSelect: func() {
			ui.showSheetTitled("Stop the music and clear Now Playing?", []SheetOption{
				{Label: "Clear Now Playing", Danger: true, OnSelect: ui.clearNowPlaying}})
		}}}
}

func (ui *IpodUI) clearNowPlaying() {
	app := ui.app
	audioStop()
	if q := app.Queue; q != nil {
		q.Tracks, q.ShuffleOrder, q.CurrentIndex = nil, nil, 0
	}
	if p := app.Playing; p != nil {
		p.Track, p.State, p.Position, p.Duration = nil, StateStopped, 0, 0
	}
	for t := range queuedMarks {
		delete(queuedMarks, t)
	}
	app.NPCacheDirty = true
	app.savePlaybackState() // an empty queue removes the saved session
	logMsg("Now Playing cleared")
	// Now Playing and Up Next have nothing left to show
	for range 2 {
		top := ui.topScreen()
		if top == nil || top.ID != "nowPlaying" && top.ID != "upNext" {
			break
		}
		if ui.hideView(); ui.topScreen() == top {
			break // something else on top (a keyboard, Cover Flow): leave it
		}
	}
	ui.refreshBuilt()
}
