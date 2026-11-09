//go:build ignore

// Copied into an isolated source tree by check-console-followup.sh.
package main

import (
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestShareFollowupReference(t *testing.T) {
	model := qaReadManifest(t)
	lib := qaLibrary(t, model)
	epoch := time.Date(2026, 11, 9, 12, 0, 0, 0, time.UTC)
	now := epoch
	oldNow := animNow
	animNow = func() time.Time { return now }
	defer func() { animNow = oldNow }()
	themeMode, effectsMode, shareName = "light", "full", ""
	out := os.Getenv("BETTERPOD_VISUAL_OUTPUT")
	if err := os.MkdirAll(out, 0755); err != nil {
		t.Fatal(err)
	}
	for _, key := range []string{"share-name", "share-receive", "share-preparing", "share-waiting", "share-sent"} {
		app := &BetterPod{Library: lib, Queue: &PlaybackQueue{}, Playing: &NowPlaying{}, RefreshChan: make(chan struct{}, 16), RedrawChan: make(chan struct{}, 16), LastActivityTime: epoch}
		globalApp = app
		ui := newIpodUI(app)
		ipodUI = ui
		now = epoch
		if key == "share-name" {
			shareName = ""
			ui.needShareName(func() {})
			ui.keyboard.anim.Set(1)
		} else {
			phase, title := shCode, "Receive"
			send := key != "share-receive"
			if send {
				title = "Share"
				phase = shPreparing
			}
			if key == "share-waiting" {
				phase = shWaiting
			}
			if key == "share-sent" {
				phase = shDone
			}
			s := &ShareView{send: send, phase: phase, phaseAt: epoch, code: [8]int{1, 2, 3, 4, 5, 6, 7, 8}}
			if send {
				s.info = shareInfo{Album: "A Very Long Album Title That Must Stay Inside Its Column", Artist: "A Very Long Artist Name and Collaborators", Songs: 12}
				s.cover = ui.defaultArtSized(shareCoverPx)
				s.coverSent = shareSentLook(s.cover)
			} else {
				s.code = [8]int{}
			}
			ui.showView(&View{ID: "share", Title: title, Kind: KindFull, Custom: s}, "")
		}
		if ui.previewTimer != nil {
			ui.previewTimer.Stop()
		}
		ui.finishSlides()
		qaDrain(ui)
		now = epoch.Add(time.Second)
		ui.renderFrame(now)
		file, err := os.Create(filepath.Join(out, "followup-"+key+".png"))
		if err != nil {
			t.Fatal(err)
		}
		if err := png.Encode(file, ui.fb); err != nil {
			t.Fatal(err)
		}
		file.Close()
		ui.artc.close()
	}
}
