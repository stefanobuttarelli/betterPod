package main

// ij_hdrticker.go — the song in the header: when a song starts playing (a
// new song, or play after a pause) on a screen without the Now Playing
// block, the header's title slides up and the song comes in from below,
// with its little cover, for a few seconds; then it slides up in turn and
// the title comes back from below.

import (
	"image"
	"time"
)

const (
	tickIn   = 380 * time.Millisecond
	tickHold = 3500 * time.Millisecond
	tickOut  = 380 * time.Millisecond
)

var ticker struct {
	track     *Track    // the song shown
	start     time.Time // zero: not showing
	lastTrack *Track
	lastState PlayState
}

// tickerAllowed: the screen on top has no Now Playing block (and isn't Now
// Playing or Cover Flow).
func (ui *IpodUI) tickerAllowed() bool {
	if ui.cover != nil || ui.nowPlayingOnScreen() {
		return false
	}
	top := ui.topScreen()
	return top != nil && !ui.miniReserve(top)
}

// followTicker starts the ticker when a song starts playing (called every
// loop).
func (ui *IpodUI) followTicker(now time.Time) {
	var tr *Track
	st := StateStopped
	if p := ui.app.Playing; p != nil {
		tr, st = p.Track, p.State
	}
	if tr != nil && st == StatePlaying && (tr != ticker.lastTrack || ticker.lastState != StatePlaying) && ui.tickerAllowed() {
		if e := now.Sub(ticker.start); !ticker.start.IsZero() && e > tickIn/2 && e < tickIn+tickHold {
			// already showing: the new song takes its place, the time restarts
			ticker.start = now.Add(-tickIn)
		} else if ticker.start.IsZero() || e >= tickIn+tickHold {
			ticker.start = now
		}
		ticker.track = tr
		ui.dirty = true
	}
	if tr != ticker.lastTrack && tr != nil && queuedMarks[tr] != "" {
		// a queued song is playing: no longer "queued"
		delete(queuedMarks, tr)
		ui.dirtyAllViews()
	}
	ticker.lastTrack, ticker.lastState = tr, st
}

// tickerOffsets: how far (CSS px, + is down) the title and the song are
// moved; ok false when the ticker isn't showing.
func tickerOffsets(now time.Time) (title, song float64, ok bool) {
	if ticker.start.IsZero() || ticker.track == nil {
		return 0, 0, false
	}
	const h = 20.0
	e := now.Sub(ticker.start)
	switch {
	case e < tickIn:
		p := easeInOut(float64(e) / float64(tickIn))
		return -h * p, h * (1 - p), true
	case e < tickIn+tickHold:
		return -h, 0, true
	case e < tickIn+tickHold+tickOut:
		q := easeInOut(float64(e-tickIn-tickHold) / float64(tickOut))
		return h * (1 - q), -h * q, true
	}
	ticker.start = time.Time{}
	return 0, 0, false
}

func tickerAnimating(now time.Time) bool {
	if ticker.start.IsZero() {
		return false
	}
	e := now.Sub(ticker.start)
	// moving, or about to end (one more frame puts the title back)
	return e < tickIn || e >= tickIn+tickHold-50*time.Millisecond
}

// drawHeaderTitle draws the header's title (tx: its left edge, maxW: the
// room up to the icons, CSS px), with the ticker when it is showing.
func (ui *IpodUI) drawHeaderTitle(sub *image.RGBA, title string, tx, maxW float64, active bool, now time.Time) {
	f := face(W700, 13)
	_, asc := lineBox(13)
	base := 1.5 + asc
	toff, soff, ok := tickerOffsets(now)
	if !ok || !active || !ui.tickerAllowed() {
		drawText(sub, f, ellipsize(f, title, maxW*DPR), tx*DPR, base*DPR, th.headerText)
		return
	}
	if toff > -20 && toff < 20 {
		drawText(sub, f, ellipsize(f, title, maxW*DPR), tx*DPR, (base+toff)*DPR, th.headerText)
	}
	if soff > -20 && soff < 20 {
		t := ticker.track
		const art = 14.0
		if a := ui.trackAlbum(t); a != nil {
			blit(sub, ui.thumb(a, art), px(tx), px(3+soff), 1)
		}
		name := t.Title
		if name == "" {
			name = "Unknown Song"
		}
		// clear of the icons on the right
		room := maxW - art - 6 - 8
		drawText(sub, f, ellipsize(f, name, room*DPR), (tx+art+6)*DPR, (base+soff)*DPR, th.headerText)
	}
}
