//go:build ignore

package main

import (
	"testing"
	"time"
)

func TestFramePacerIncludesEventWork(t *testing.T) {
	var p framePacer
	start := time.Now()
	period := animatedFrameInterval
	// Input/callback work has already taken 20 ms before rendering ends.
	if got := p.delay(start, start.Add(20*time.Millisecond), period, time.Time{}); got != period-20*time.Millisecond {
		t.Fatalf("event work added to frame period: wait %v", got)
	}
}

func TestFramePacerAbsorbsSleepJitter(t *testing.T) {
	var p framePacer
	start := time.Now()
	period := animatedFrameInterval
	p.delay(start, start.Add(10*time.Millisecond), period, time.Time{})
	// Sleep woke a millisecond late. The next frame keeps the original
	// phase instead of adding another millisecond to every future frame.
	nextStart := start.Add(period + time.Millisecond)
	got := p.delay(nextStart, nextStart.Add(10*time.Millisecond), period, time.Time{})
	want := period - 11*time.Millisecond
	if got != want {
		t.Fatalf("sleep jitter accumulated: got %v, want %v", got, want)
	}
}

func TestFramePacerSkipsMissedSlots(t *testing.T) {
	var p framePacer
	start := time.Now()
	period := animatedFrameInterval
	// A frame takes longer than its budget. Skip the missed deadline,
	// rather than following it with immediate catch-up frames.
	late := start.Add(period + 7*time.Millisecond)
	if got := p.delay(start, late, period, time.Time{}); got != period-7*time.Millisecond {
		t.Fatalf("overrun caused catch-up: wait %v", got)
	}
	// A long stall must skip all missed slots in one step.
	late = start.Add(100*period + 5*time.Millisecond)
	if got := p.delay(start.Add(2*period), late, period, time.Time{}); got != period-5*time.Millisecond {
		t.Fatalf("long stall left stale deadlines: wait %v", got)
	}
}

func TestFramePacerChangesModeWithoutOldDeadline(t *testing.T) {
	var p framePacer
	start := time.Now()
	p.delay(start, start.Add(time.Millisecond), 200*time.Millisecond, time.Time{})
	awake := start.Add(200 * time.Millisecond)
	if got := p.delay(awake, awake.Add(5*time.Millisecond), animatedFrameInterval, time.Time{}); got != animatedFrameInterval-5*time.Millisecond {
		t.Fatalf("screen-off deadline delayed controls: wait %v", got)
	}
}

func TestFramePacerHonorsAudioVideoDeadline(t *testing.T) {
	var p framePacer
	start := time.Now()
	due := start.Add(18 * time.Millisecond)
	// The audio position was sampled before a 12 ms render. Only 6 ms
	// remain until the source video's next frame, even at a 30 fps UI cap.
	if got := p.delay(start, start.Add(12*time.Millisecond), animatedFrameInterval, due); got != 6*time.Millisecond {
		t.Fatalf("video wait counted rendering twice: %v", got)
	}
	// An overdue video frame takes priority over the UI's missed-slot rule.
	if got := p.delay(start, start.Add(40*time.Millisecond), animatedFrameInterval, due); got != 0 {
		t.Fatalf("audio-clock video frame was postponed: %v", got)
	}
}

func TestCoverFlowOcclusionKeepsTransitionsVisible(t *testing.T) {
	now := time.Now()
	view := func() *View { return &View{Kind: KindFull, enter: newAnim(0), hide: newAnim(0)} }
	under := view()
	ui := &IpodUI{fullX: newAnim(0), full: []*View{under}, cover: &CoverFlow{fullBase: 1}}
	if ui.coverFlowCovered(now) {
		t.Fatal("view below Cover Flow hid the flow")
	}
	over := view()
	ui.full = append(ui.full, over)
	if !ui.coverFlowCovered(now) {
		t.Fatal("settled full-screen overlay did not cover the flow")
	}
	over.enter.Set(0.5)
	if ui.coverFlowCovered(now) {
		t.Fatal("entering overlay hid exposed flow")
	}
	over.enter.Set(0)
	over.exiting = true
	if ui.coverFlowCovered(now) {
		t.Fatal("leaving overlay hid exposed flow")
	}
	over.exiting = false
	ui.fullX.Set(0.5)
	if ui.coverFlowCovered(now) {
		t.Fatal("sliding full-screen panel hid exposed flow")
	}
}
