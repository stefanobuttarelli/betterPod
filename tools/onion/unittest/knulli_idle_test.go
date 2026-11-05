//go:build ignore

package main

import (
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"
)

func TestKnulliIdlePlaybackLifecycle(t *testing.T) {
	path := filepath.Join(t.TempDir(), "battery-saver", "betterpod-test.pause")
	g := &knulliIdleGuard{path: path}
	defer g.close()
	at := time.Unix(100, 0)
	check := func(offset time.Duration, active, paused, held bool) {
		t.Helper()
		if err := g.update(at.Add(offset), active, paused); err != nil {
			t.Fatal(err)
		}
		_, err := os.Stat(path)
		if (err == nil) != held {
			t.Fatalf("at %s: pause marker held=%v, want %v (stat: %v)", offset, err == nil, held, err)
		}
	}
	check(0, false, false, false) // browsing and a restored paused queue
	check(time.Second, true, false, true)
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	check(10*time.Second, true, false, true)
	after, _ := os.Stat(path)
	if !os.SameFile(info, after) || !info.ModTime().Equal(after.ModTime()) {
		t.Fatal("steady playback rewrote its marker")
	}
	check(11*time.Second, false, false, true) // natural track handoff
	check(12*time.Second, true, false, true)  // the next track/loading gap
	check(13*time.Second, false, true, false) // pause releases without grace
	check(14*time.Second, true, false, true)
	check(16*time.Second, false, false, false) // stopped/end of the queue
	check(17*time.Second, true, false, true)
	g.close()
	check(18*time.Second, true, false, false) // late poll after shutdown
}

func TestKnulliIdleDoesNotRemoveOtherMarkers(t *testing.T) {
	dir := t.TempDir()
	other := filepath.Join(dir, "other-app.pause")
	if err := os.WriteFile(other, []byte("other owner"), 0600); err != nil {
		t.Fatal(err)
	}
	g := &knulliIdleGuard{path: filepath.Join(dir, "betterpod-test.pause")}
	if err := g.update(time.Now(), true, false); err != nil {
		t.Fatal(err)
	}
	g.close()
	if data, err := os.ReadFile(other); err != nil || string(data) != "other owner" {
		t.Fatalf("another app's marker was changed: %q, %v", data, err)
	}
	conflict := &knulliIdleGuard{path: other}
	if err := conflict.update(time.Now(), true, false); !os.IsExist(err) {
		t.Fatalf("claimed an existing marker: %v", err)
	}
	conflict.close()
	if _, err := os.Stat(other); err != nil {
		t.Fatal("removed a marker it did not create:", err)
	}
}

func TestKnulliIdleCloseWinsConcurrentPlayback(t *testing.T) {
	path := filepath.Join(t.TempDir(), "betterpod-test.pause")
	g := &knulliIdleGuard{path: path}
	if err := g.update(time.Now(), true, false); err != nil {
		t.Fatal(err)
	}
	var wg sync.WaitGroup
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for j := 0; j < 20; j++ {
				g.update(time.Now(), true, false)
			}
		}()
	}
	g.close()
	wg.Wait()
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatal("shutdown left/recreated the marker:", err)
	}
}

func TestKnulliVideoIdleIntent(t *testing.T) {
	for _, state := range []vState{vLoading, vPlaying, vBuffering} {
		vv := &VideoView{pl: &videoPlayer{state: state}}
		if !knulliVideoActive(vv) {
			t.Fatalf("state %d should protect playback, including buffering", state)
		}
		vv.pl.paused = true
		if knulliVideoActive(vv) {
			t.Fatalf("paused state %d should permit idle actions", state)
		}
	}
	for _, state := range []vState{vEnded, vFailed} {
		if knulliVideoActive(&VideoView{pl: &videoPlayer{state: state}}) {
			t.Fatalf("state %d should permit idle actions", state)
		}
	}
	if !knulliVideoActive(&VideoView{cancel: func() {}}) {
		t.Fatal("opening streams should protect playback")
	}
	if knulliVideoActive(&VideoView{cancel: func() {}, err: "failed"}) {
		t.Fatal("failed stream opening should release playback protection")
	}
}

func TestKnulliIdleDoesNothingOnOnion(t *testing.T) {
	oldKnulli, oldGuard := isKnulli, knulliIdle.guard
	defer func() { isKnulli, knulliIdle.guard = oldKnulli, oldGuard }()
	isKnulli = false
	path := filepath.Join(t.TempDir(), "should-not-exist", "betterpod-test.pause")
	t.Setenv("MIYOOPOD_KNULLI_IDLE_FILE", path)
	knulliIdle.guard = nil
	initKnulliIdle()
	knulliIdleTick(nil, time.Now())
	closeKnulliIdle()
	if knulliIdle.guard != nil {
		t.Fatal("Onion initialized a Knulli idle guard")
	}
	if _, err := os.Stat(filepath.Dir(path)); !os.IsNotExist(err) {
		t.Fatal("Onion touched the Knulli pause directory:", err)
	}
}
