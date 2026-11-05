package main

// Knulli's idle hooks mute and later suspend after keyboard/gamepad inactivity.
// Audio does not reset that timer. Its battery-saver pause files are the
// supported way to leave those automatic actions off while media plays.

import (
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"time"
)

type knulliIdleGuard struct {
	mu         sync.Mutex
	path       string
	held       bool
	closed     bool
	lastActive time.Time
}

var knulliIdle struct {
	guard   *knulliIdleGuard
	checked time.Time // UI goroutine
	lastErr string
}

func initKnulliIdle() {
	path := envGet("BETTERPOD_KNULLI_IDLE_FILE")
	if path == "" {
		if _, err := os.Stat("/etc/idlewatcher"); err != nil {
			return // older Knulli and desktop builds have no idle hooks
		}
		path = fmt.Sprintf("/var/run/battery-saver/betterpod-%d.pause", os.Getpid())
	}
	knulliIdle.guard = &knulliIdleGuard{path: path}
}

// update does filesystem work only when playback starts or ends. A short
// handoff grace covers the mixer becoming empty before the next queue item
// starts loading. An explicit pause releases it on the next tick.
func (g *knulliIdleGuard) update(now time.Time, active, paused bool) error {
	g.mu.Lock()
	defer g.mu.Unlock()
	if g.closed {
		return nil
	}
	if active {
		g.lastActive = now
	}
	want := active || g.held && !paused && now.Sub(g.lastActive) < 2*time.Second
	if want == g.held {
		return nil
	}
	if !want {
		if err := os.Remove(g.path); err != nil && !os.IsNotExist(err) {
			return err
		}
		g.held = false
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(g.path), 0755); err != nil {
		return err
	}
	f, err := os.OpenFile(g.path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err // never claim or remove somebody else's pause file
	}
	g.held = true
	return f.Close()
}

func (g *knulliIdleGuard) close() {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.closed = true // a late update must not recreate it during shutdown
	if g.held {
		os.Remove(g.path)
		g.held = false
	}
}

func closeKnulliIdle() {
	if knulliIdle.guard != nil {
		knulliIdle.guard.close()
	}
}

func knulliVideoActive(vv *VideoView) bool {
	if vv.err != "" || vv.ended {
		return false
	}
	if p := vv.pl; p != nil {
		return !p.paused && (p.state == vLoading || p.state == vPlaying || p.state == vBuffering)
	}
	return vv.load != nil || vv.cancel != nil // finding/opening the streams
}

// knulliIdleTick runs on each UI's own goroutine, at most once a second.
// Video views are read here, never by the background playback poller.
func knulliIdleTick(ui *IpodUI, now time.Time) {
	if knulliIdle.guard == nil || now.Sub(knulliIdle.checked) < time.Second {
		return
	}
	knulliIdle.checked = now
	active, paused := atomic.LoadInt32(&trackLoading) != 0, false
	if !active {
		state := audioGetState()
		active, paused = state.IsPlaying, state.IsPaused
	}
	previewMu.Lock()
	p := previewCur
	previewMu.Unlock()
	if p != nil {
		p.mu.Lock()
		active, paused = !p.paused && p.err == "", p.paused
		p.mu.Unlock()
	}
	if ui != nil {
		for _, v := range ui.full {
			if vv, ok := v.Custom.(*VideoView); ok && !v.exiting && knulliVideoActive(vv) {
				active = true
				break
			}
		}
	}
	if err := knulliIdle.guard.update(now, active, paused); err != nil {
		if msg := err.Error(); msg != knulliIdle.lastErr {
			logMsg("Knulli playback idle guard: " + msg)
			knulliIdle.lastErr = msg
		}
	} else {
		knulliIdle.lastErr = ""
	}
}
