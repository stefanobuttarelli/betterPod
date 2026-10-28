package main

import "time"

// ij_clearnp.go — Clear Now Playing: stops the music and empties the queue,
// so Now Playing leaves the menus (and the session saved for the next
// start) until something is played again.
//
// Y (or holding A) on the Now Playing row of the main menu and of Music
// asks inside the Now Playing block at the bottom: the song goes up and
// out, "Clear Now Playing?" comes up with Keep (chosen first) and Clear;
// left / right choose, A does it, B keeps. Keep (or 5 s without a key)
// brings the song back down; Clear stops the music and the block goes down
// and away. Elsewhere (no block on screen) a sheet asks. It is also the
// last row of Up Next, where it asks like Clear Up Next.

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

// miniAsk: the Now Playing block asking "Clear Now Playing?" (UI goroutine).
var miniAsk = struct {
	on  bool
	a   *Anim // 0: the song, 1: the question
	sel int   // 0 Keep, 1 Clear
	gen int
}{a: newAnim(0)}

// askClearNowPlaying: the Now Playing rows' Y.
func (ui *IpodUI) askClearNowPlaying() {
	if p := ui.app.Playing; p == nil || p.Track == nil {
		return
	}
	top := ui.topScreen()
	if top == nil || !ui.miniReserve(top) || len(ui.full) > 0 {
		ui.showSheetTitled("Now Playing", ui.clearNowPlayingOptions())
		return
	}
	miniAsk.on, miniAsk.sel = true, 0
	miniAsk.a.To(1, 320*time.Millisecond, easeLinear, 0)
	ui.miniAskTimeout()
	ui.dirty = true
}

// miniAskTimeout: 5 s without a key bring the song back.
func (ui *IpodUI) miniAskTimeout() {
	miniAsk.gen++
	gen := miniAsk.gen
	time.AfterFunc(5*time.Second, func() {
		ui.post(func() {
			if miniAsk.on && gen == miniAsk.gen {
				ui.miniConfirmEnd(false)
			}
		})
	})
}

// miniConfirmEnd: Keep (the song comes back down) or Clear (the block goes
// down and away with the question on it).
func (ui *IpodUI) miniConfirmEnd(clear bool) {
	miniAsk.on = false
	miniAsk.gen++
	if clear {
		ui.clearNowPlaying()
		// the block leaves showing the question; then it is reset
		time.AfterFunc(600*time.Millisecond, func() { ui.post(func() { miniAsk.a.Set(0); ui.dirty = true }) })
	} else {
		miniAsk.a.To(0, 320*time.Millisecond, easeLinear, 0)
	}
	ui.dirty = true
}

// miniConfirmKey takes the keys while the block asks: left / right (and
// the wheel) choose, A does it, B keeps; the others are ignored.
func (ui *IpodUI) miniConfirmKey(ev int) bool {
	if !miniAsk.on {
		return false
	}
	if ev < 0 {
		// releases: nothing (A and B act on their press here); the key's
		// hold is forgotten (the A held to ask comes up now)
		k := Key(-ev - 1)
		delete(ui.pressStart, k)
		delete(ui.longFired, k)
		if ui.repeatKey == k {
			ui.repeatKey = NONE
		}
		return true
	}
	switch Key(ev) {
	case LEFT, RIGHT, WHEEL_CW, WHEEL_CCW:
		miniAsk.sel = 1 - miniAsk.sel
		ui.miniAskTimeout()
	case A:
		ui.miniConfirmEnd(miniAsk.sel == 1)
	case B:
		ui.miniConfirmEnd(false)
	}
	ui.dirty = true
	return true
}
