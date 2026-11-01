package main

// ij_lrseek.go — L and R: a tap goes to the previous / next song (as
// before), held they rewind / fast forward the song playing, anywhere, as
// an iPod's wheel buttons do. Seeking starts after 400 ms, a step every
// 200 ms: 2 s steps, then 5 s after 2 s, 10 s after 4 s (seekAmount's
// curve). Fast forward stops just before the end, so the song ends and the
// next one comes as usual.

import "time"

const (
	lrHold = 400 * time.Millisecond
	lrStep = 200 * time.Millisecond
)

// lrSeek: L or R held (dir -1 / 1, 0: none).
var lrSeek struct {
	dir         int
	start, last time.Time
	active      bool // it has seeked: the release doesn't skip
}

func (ui *IpodUI) lrPress(k Key, now time.Time) {
	lrSeek.dir, lrSeek.start, lrSeek.active = 1, now, false
	if k == L {
		lrSeek.dir = -1
	}
}

// lrRelease: L or R let go; a tap skips. It reports whether it was L / R's
// own press (false: pressed elsewhere, a custom screen took it).
func (ui *IpodUI) lrRelease(k Key) bool {
	dir := lrSeek.dir
	if dir == 0 || (k == L) != (dir < 0) {
		return false
	}
	active := lrSeek.active
	lrSeek.dir, lrSeek.active = 0, false
	if active {
		ui.dirty = true
		return true
	}
	skipDir = float64(dir)
	if dir > 0 {
		go func() { ui.app.nextTrack(); ui.post(ui.onTrackChanged) }()
	} else {
		go func() { ui.app.prevTrack(); ui.post(ui.onTrackChanged) }()
	}
	ui.dirty = true
	return true
}

// lrSeekPoll: a step of the seek while L / R is held (each loop).
func (ui *IpodUI) lrSeekPoll(now time.Time) {
	if lrSeek.dir == 0 || now.Sub(lrSeek.start) < lrHold {
		return
	}
	if lrSeek.active && now.Sub(lrSeek.last) < lrStep {
		return
	}
	p := ui.app.Playing
	if p == nil || p.Track == nil || p.State == StateStopped || hold.on {
		return
	}
	lrSeek.active, lrSeek.last = true, now
	held := now.Sub(lrSeek.start) - lrHold
	step := 2.0
	switch {
	case held >= 4*time.Second:
		step = 10
	case held >= 2*time.Second:
		step = 5
	}
	st := audioGetState()
	pos, dur := st.Position, st.Duration
	if dur <= 0 {
		dur = p.Duration
	}
	pos += step * float64(lrSeek.dir)
	if pos < 0 {
		pos = 0
	}
	if dur > 0 && pos > dur-1 {
		pos = dur - 1
	}
	audioSeek(pos)
	p.Position = pos
	ui.dirty = true
}
