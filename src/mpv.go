package main

import (
	"fmt"
	"math"
	"os"
	"sync"
	"sync/atomic"
	"time"
)

// startPlaybackPoller checks audio state and updates progress display.
// Runs in its own goroutine. Minimal work per tick to avoid starving audio.
func (app *MiyooPod) startPlaybackPoller() {
	lastDrawnSecond := -1
	saveTickCount := 0

	for app.Running {
		if app.Playing != nil && app.Playing.State != StateStopped {
			state := audioGetState()

			if state.Position >= 0 {
				app.Playing.Position = state.Position
			}
			if state.Duration > 0 {
				// The decoder's duration is exact; the scan only estimated it
				// (or left it at 0 on first play).
				if app.Playing.Track != nil && math.Abs(app.Playing.Track.Duration-state.Duration) > 0.5 {
					app.Playing.Track.Duration = state.Duration
				}
				if app.Playing.Duration > 0 && math.Abs(app.Playing.Duration-state.Duration) > 0.5 {
					app.Playing.Duration = state.Duration
					app.NPCacheDirty = true
					app.requestRedraw()
				}
				// Also update the live playing duration used by the progress bar.
				// On first play track.Duration is unknown (0) until the backend
				// reports it, so without this the bar shows a 0 duration.
				if app.Playing.Duration == 0 {
					app.Playing.Duration = state.Duration
					app.NPCacheDirty = true
					app.requestRedraw()
				}
				// Lazy-fill sample rate and (average) bitrate for the format badge
				// now that the duration is known.
				if app.Playing.Track != nil {
					app.fillTrackTechInfo(app.Playing.Track, state.Duration)
				}
			}

			if atomic.LoadInt32(&trackLoading) == 0 {
				// While a track is loading the mixer briefly reports
				// stopped/paused: don't flip the UI state then.
				if state.IsPaused && app.Playing.State != StatePaused {
					app.Playing.State = StatePaused
					app.NPCacheDirty = true
					app.requestRedraw()
				} else if state.IsPlaying && app.Playing.State != StatePlaying {
					app.Playing.State = StatePlaying
					app.NPCacheDirty = true
					app.requestRedraw()
				}
			}
			if state.Finished && atomic.LoadInt32(&trackLoading) == 0 {
				// Defer to main loop — avoids racing on Queue/Playing state.
				// (A track being loaded stops the previous one, which also
				// says "finished": that's no track end.)
				app.TrackEndPending = true
				app.requestRedraw()
			}

			// Update progress bar when on Now Playing screen and second changes
			if app.CurrentScreen == ScreenNowPlaying {
				currentSecond := int(app.Playing.Position)
				if currentSecond != lastDrawnSecond {
					lastDrawnSecond = currentSecond
					app.updateProgressBarOnly()
				}
			}

			// Redraw lyrics screen only when the highlighted LRC line changes,
			// and only when the user is not holding a scroll key (avoids flash during scroll).
			if app.CurrentScreen == ScreenLyrics && app.LyricsCachedLRC != nil && app.LastKey == NONE {
				activeLRC := activeLRCIndex(app.LyricsCachedLRC, app.Playing.Position)
				if activeLRC != app.LyricsLastActiveLRC {
					app.LyricsLastActiveLRC = activeLRC
					app.requestRedraw()
				}
			}

			// Save playback state every 10 seconds during active playback
			saveTickCount++
			if saveTickCount >= 10 {
				app.savePlaybackState()
				saveTickCount = 0
			}
		}
		// Increased sleep to reduce CPU usage and SD card contention
		time.Sleep(1000 * time.Millisecond)
	}
}

// trackLoading is 1 while mpvLoadFile runs (read by the playback poller).
var trackLoading int32

// audioLoadMu: mpvLoadFile, one at a time.
var audioLoadMu sync.Mutex

func (app *MiyooPod) mpvLoadFile(path string) error {
	// one load at a time: a song picked (loaded in the background) and a
	// track end (on the UI's loop) must never reopen the audio device
	// together (it could hang the mixer)
	audioLoadMu.Lock()
	defer audioLoadMu.Unlock()
	atomic.StoreInt32(&trackLoading, 1)
	defer atomic.StoreInt32(&trackLoading, 0)
	const maxRAMSize = 48 * 1024 * 1024 // 48MB (hi-res FLAC fits; the H700 has 1GB)

	// Reopen the audio device at the track's sample rate (no resampling).
	audioPrepareFor(path)

	// Ensure hardware volume is set before playback starts.
	// loadSettings may have set it too early (during Init) when MI_AO isn't ready yet.
	setMiAOVolume(app.SystemVolume)

	if info, err := os.Stat(path); err == nil && info.Size() > 0 && info.Size() < maxRAMSize {
		loadErr := audioLoadFileToMemory(path)
		if loadErr == nil {
			playErr := audioPlay()
			if playErr == nil {
				// Re-apply volume now that the audio hardware is active. On a
				// restored session the track is paused immediately after load,
				// so the pre-play set above doesn't stick until this point.
				setMiAOVolume(app.SystemVolume)
				return nil
			}
			logMsg(fmt.Sprintf("WARNING: RAM play failed (%v), falling back to streaming: %s", playErr, path))
		} else {
			logMsg(fmt.Sprintf("WARNING: RAM load failed (%v), falling back to streaming: %s", loadErr, path))
		}
		audioStop()
	}

	err := audioLoadFile(path)
	if err != nil {
		return err
	}
	if err := audioPlay(); err != nil {
		return err
	}
	setMiAOVolume(app.SystemVolume)
	return nil
}

func (app *MiyooPod) mpvTogglePause() {
	// After a sample-rate switch (or a restored session) the mixer may have
	// no track loaded: "play" then reloads the current track and seeks back
	// to the saved position.
	if !audioIsPlaying() && !audioIsPaused() && app.Playing != nil && app.Playing.Track != nil {
		pos := app.Playing.Position
		logMsg(fmt.Sprintf("Play: track not loaded in the mixer, reloading %s at %.1fs", app.Playing.Track.Path, pos))
		if err := app.mpvLoadFile(app.Playing.Track.Path); err != nil {
			logMsg(fmt.Sprintf("Play: reload failed: %v", err))
			return
		}
		if pos > 1 {
			audioSeek(pos)
		}
		app.Playing.State = StatePlaying
		return
	}
	audioTogglePause()
	// keep the UI state in sync with the mixer
	if audioIsPaused() {
		app.Playing.State = StatePaused
	} else if audioIsPlaying() {
		app.Playing.State = StatePlaying
	}
}

func (app *MiyooPod) mpvStop() {
	audioStop()
}

func (app *MiyooPod) mpvSeek(seconds float64) {
	if app.Playing == nil {
		return
	}
	newPos := app.Playing.Position + seconds
	if newPos < 0 {
		newPos = 0
	}
	if newPos > app.Playing.Duration && app.Playing.Duration > 0 {
		newPos = app.Playing.Duration
	}
	audioSeek(newPos)
}
