package main

import (
	"fmt"
	"image"
	"math/rand"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

// queueMu: one change of the current song (and its load) at a time. Skips
// run in a goroutine per press: without it a second skip could start its
// song while the first one still checks its own, and the first would then
// call it a failure (no track, nothing to pause). It is never held while
// waiting for the UI goroutine (ui.post only, never postSure).
var queueMu sync.Mutex

// pausePresses: play/pause pressed while a song was loading (an odd count:
// the song that comes pauses).
var pausePresses atomic.Int32

// unlockQueue frees the queue; a play/pause pressed meanwhile takes effect
// now (not after a stop: the queue's end isn't played again by it).
func (app *MiyooPod) unlockQueue() {
	for n := pausePresses.Swap(0); n != 0; n = pausePresses.Swap(0) {
		if n%2 == 1 && app.Playing != nil && app.Playing.State != StateStopped {
			app.playPause()
		}
	}
	queueMu.Unlock()
	if pausePresses.Load() != 0 && queueMu.TryLock() {
		app.unlockQueue() // (one came in between)
	}
}

// maxPlayFails: songs that won't play skipped in a row before it stops.
const maxPlayFails = 5

// playTrackFromList starts playing a track from a list, building a queue from the context
func (app *MiyooPod) playTrackFromList(tracks []*Track, startIdx int) {
	if len(tracks) == 0 || startIdx < 0 || startIdx >= len(tracks) {
		return
	}

	queueMu.Lock()
	app.Queue.Tracks = make([]*Track, len(tracks))
	copy(app.Queue.Tracks, tracks)
	app.Queue.CurrentIndex = startIdx

	if app.Queue.Shuffle {
		app.buildShuffleOrder(startIdx)
		// buildShuffleOrder puts startIdx at index 0 of the shuffle order
		app.Queue.CurrentIndex = 0
	} else {
		// Clear old shuffle order when playing without shuffle
		app.Queue.ShuffleOrder = nil
	}

	app.playCurrentQueueTrack()
	app.unlockQueue()

	if !useIpodUI { // (the classic UI's screens; the iPod UI is on its own goroutine)
		app.setScreen(ScreenNowPlaying)
		app.refreshRootMenu()
		app.drawCurrentScreen()
	}
}

// shuffleAllAndPlay queues all library tracks, shuffles, and starts playing
func (app *MiyooPod) shuffleAllAndPlay() {
	if len(app.Library.Tracks) == 0 {
		return
	}

	queueMu.Lock()
	app.Queue.Tracks = make([]*Track, len(app.Library.Tracks))
	copy(app.Queue.Tracks, app.Library.Tracks)
	app.Queue.Shuffle = true
	app.Queue.CurrentIndex = 0
	app.buildShuffleOrder(0)

	app.playCurrentQueueTrack()
	app.unlockQueue()

	if !useIpodUI { // (the classic UI's screens; the iPod UI is on its own goroutine)
		app.setScreen(ScreenNowPlaying)
		app.refreshRootMenu()
		app.drawCurrentScreen()
	}
}

// playCurrentQueueTrack loads and plays the current track in the queue
// (false: it couldn't). The caller holds queueMu (but the classic UI,
// which does everything on its own loop).
func (app *MiyooPod) playCurrentQueueTrack() bool {
	track := app.getCurrentTrack()
	if track == nil {
		return false
	}

	app.Playing.Track = track
	app.Playing.State = StatePlaying
	app.Playing.Position = 0
	// Use track duration if available, otherwise will be updated by poller
	if track.Duration > 0 {
		app.Playing.Duration = track.Duration
	} else {
		app.Playing.Duration = 0
	}

	err := app.mpvLoadFile(track.Path)
	if err != nil {
		logMsg(fmt.Sprintf("ERROR: Failed to load: %v", err))
		app.Playing.State = StateStopped
		app.Playing.Track = nil // Clear track on failure
		app.showError(fmt.Sprintf("Failed to load audio\n%s", err.Error()))
		playFailed(track)
		return false
	}

	// Verify playback actually started
	time.Sleep(100 * time.Millisecond)
	state := audioGetState()
	if !state.IsPlaying && !state.IsPaused {
		logMsg("ERROR: Audio failed to start playing")
		app.Playing.State = StateStopped
		app.Playing.Track = nil
		app.showError("Playback failed to start")
		playFailed(track)
		return false
	}

	app.updateCoverflowForCurrentTrack()
	app.NPCacheDirty = true

	// Reset lyrics state for new track
	app.LyricsCachedTrack = ""
	app.LyricsLastActiveLRC = -1
	app.LyricsScrollOffset = 0
	app.LyricsManualScroll = false

	// Persist queue state when track changes
	app.savePlaybackState()
	return true
}

// playFailed: a song couldn't play (the iPod UI says so; showError is the
// classic UI's).
func playFailed(t *Track) {
	if ui := ipodUI; ui != nil {
		title := t.Title
		ui.post(func() { ui.showToast("Couldn't play " + title) })
	}
}

// getCurrentTrack returns the current track based on queue and shuffle state
func (app *MiyooPod) getCurrentTrack() *Track {
	if app.Queue == nil || len(app.Queue.Tracks) == 0 {
		return nil
	}

	idx := app.Queue.CurrentIndex
	if app.Queue.Shuffle && len(app.Queue.ShuffleOrder) > 0 {
		if idx >= 0 && idx < len(app.Queue.ShuffleOrder) {
			idx = app.Queue.ShuffleOrder[idx]
		}
	}

	if idx >= 0 && idx < len(app.Queue.Tracks) {
		return app.Queue.Tracks[idx]
	}
	return nil
}

func (app *MiyooPod) togglePlayPause() {
	if previewActive() {
		return // (a Listen plays: A on its row pauses it)
	}
	if !queueMu.TryLock() {
		// a song is loading (a skip): it pauses once it plays
		pausePresses.Add(1)
		return
	}
	app.playPause()
	app.unlockQueue()
}

// playPause plays or pauses the current song. Stopped with a song (the end
// of the queue), it plays it again. The caller holds queueMu.
func (app *MiyooPod) playPause() {
	if app.Playing == nil || app.Playing.Track == nil {
		return
	}
	app.mpvTogglePause()
	app.NPCacheDirty = true
	app.drawCurrentScreen()
}

// queueMaxIdx: the last play position.
func (app *MiyooPod) queueMaxIdx() int {
	if app.Queue.Shuffle && len(app.Queue.ShuffleOrder) > 0 {
		return len(app.Queue.ShuffleOrder) - 1
	}
	return len(app.Queue.Tracks) - 1
}

// queueStep moves to the next play position (Repeat All: from the last to
// the first). false: nothing comes after this song.
func (app *MiyooPod) queueStep() bool {
	maxIdx := app.queueMaxIdx()
	if app.Queue.CurrentIndex < maxIdx {
		app.Queue.CurrentIndex++
		return true
	}
	if app.Queue.Repeat == RepeatAll {
		app.Queue.CurrentIndex = 0
		return true
	}
	app.Queue.CurrentIndex = maxIdx
	return false
}

// playForward plays the song at the current position; one that won't play
// is skipped for the next (maxPlayFails in a row at most, then it stops).
func (app *MiyooPod) playForward() {
	for fails := 0; !app.playCurrentQueueTrack(); {
		fails++
		if fails >= maxPlayFails || !app.queueStep() {
			return
		}
	}
}

func (app *MiyooPod) nextTrack() {
	if previewActive() {
		return // (a Listen has the mixer: its own rows pick its songs)
	}
	queueMu.Lock()
	defer app.unlockQueue()
	if app.Queue == nil || len(app.Queue.Tracks) == 0 {
		return
	}

	if !app.queueStep() {
		if queueEnded != nil {
			// the iPod UI says so, and the last song plays on
			// (ij_endqueue.go)
			queueEnded(true)
			return
		}
		app.mpvStop()
		app.Playing.State = StateStopped
		return
	}

	app.playForward()
}

func (app *MiyooPod) prevTrack() {
	if previewActive() {
		return // (a Listen has the mixer: its own rows pick its songs)
	}
	queueMu.Lock()
	defer app.unlockQueue()
	if app.Queue == nil || len(app.Queue.Tracks) == 0 {
		return
	}

	if app.Playing != nil && app.Playing.Position > 3.0 && app.Playing.State != StateStopped {
		app.mpvSeek(-app.Playing.Position)
		app.NPCacheDirty = true // Force UI update when restarting track
		return
	}

	// Determine the max index based on shuffle state
	maxIdx := app.queueMaxIdx()

	app.Queue.CurrentIndex--
	if app.Queue.CurrentIndex < 0 {
		if app.Queue.Repeat == RepeatAll {
			app.Queue.CurrentIndex = maxIdx
		} else {
			app.Queue.CurrentIndex = 0
		}
	}

	app.playCurrentQueueTrack()
}

func (app *MiyooPod) handleTrackEnd() {
	queueMu.Lock()
	defer app.unlockQueue()
	if previewActive() || trackEndGen.Load() != loadGen.Load() {
		return // (a song was loaded since that end: a skip, a Listen)
	}
	if app.Queue == nil || len(app.Queue.Tracks) == 0 {
		app.Playing.State = StateStopped
		return
	}

	if app.Queue.Repeat == RepeatOne {
		app.playCurrentQueueTrack()
		return
	}

	if !app.queueStep() {
		// the end: stopped at the start of the last song (play plays it
		// again, NEXT shows the card again)
		app.Playing.State = StateStopped
		app.Playing.Position = 0
		app.NPCacheDirty = true
		app.refreshRootMenu()
		app.requestRedraw()
		if queueEnded != nil {
			queueEnded(false) // End of the Queue (ij_endqueue.go)
		}
		return
	}

	app.playForward()
	app.requestRedraw()
}

func (app *MiyooPod) toggleShuffle() {
	if app.Queue == nil || len(app.Queue.Tracks) == 0 {
		return
	}

	// Remember which track is currently playing
	currentTrack := app.getCurrentTrack()
	var currentPhysicalIdx int = -1
	if currentTrack != nil {
		for i, track := range app.Queue.Tracks {
			if track == currentTrack {
				currentPhysicalIdx = i
				break
			}
		}
	}

	app.Queue.Shuffle = !app.Queue.Shuffle
	if app.Queue.Shuffle {
		// Build shuffle order with current track's physical index
		if currentPhysicalIdx >= 0 {
			app.buildShuffleOrder(currentPhysicalIdx)
		} else {
			app.buildShuffleOrder(0)
		}
		// Reset playback position to start of new shuffle order
		app.Queue.CurrentIndex = 0
		logMsg("INFO: Shuffle enabled")
	} else {
		// When turning off shuffle, set CurrentIndex to physical position
		if currentPhysicalIdx >= 0 {
			app.Queue.CurrentIndex = currentPhysicalIdx
		}
		app.Queue.ShuffleOrder = nil
		logMsg("INFO: Shuffle disabled")
	}
	app.NPCacheDirty = true
}

func (app *MiyooPod) cycleRepeat() {
	if app.Queue == nil {
		return
	}
	switch app.Queue.Repeat {
	case RepeatOff:
		app.Queue.Repeat = RepeatAll
	case RepeatAll:
		app.Queue.Repeat = RepeatOne
	case RepeatOne:
		app.Queue.Repeat = RepeatOff
	}
	app.NPCacheDirty = true
}

func (app *MiyooPod) buildShuffleOrder(startIdx int) {
	app.Queue.ShuffleOrder = shuffleOrder(len(app.Queue.Tracks), startIdx)
}

// shuffleOrder: the n tracks in a random order, startIdx first.
func shuffleOrder(n, startIdx int) []int {
	order := make([]int, n)
	for i := range order {
		order[i] = i
	}
	for i := n - 1; i > 0; i-- {
		j := rand.Intn(i + 1)
		order[i], order[j] = order[j], order[i]
	}
	for i, v := range order {
		if v == startIdx {
			order[0], order[i] = order[i], order[0]
			break
		}
	}
	return order
}

func (app *MiyooPod) updateCoverflowForCurrentTrack() {
	if app.Playing == nil || app.Playing.Track == nil {
		return
	}

	track := app.Playing.Track

	if len(app.Coverflow.Albums) == 0 {
		app.Coverflow.Albums = app.Library.Albums
	}

	for i, album := range app.Coverflow.Albums {
		if album.Name == track.Album && album.Artist == track.Artist {
			app.Coverflow.CenterIndex = i
			return
		}
		albumArtist := track.AlbumArtist
		if albumArtist == "" {
			albumArtist = track.Artist
		}
		if album.Name == track.Album && album.Artist == albumArtist {
			app.Coverflow.CenterIndex = i
			return
		}
	}
}

// drawNowPlayingScreen renders the Now Playing screen with background caching.
// Static elements are cached as a pixel buffer. Only progress bar redraws each second.
func (app *MiyooPod) drawNowPlayingScreen() {
	dc := app.DC

	if app.Playing == nil || app.Playing.Track == nil {
		dc.SetHexColor(app.CurrentTheme.BG)
		dc.Clear()
		app.drawHeader("Now Playing")
		dc.SetFontFace(app.FontMenu)
		dc.SetHexColor(app.CurrentTheme.Dim)
		dc.DrawStringAnchored("No track playing", SCREEN_WIDTH/2, SCREEN_HEIGHT/2, 0.5, 0.5)
		return
	}

	if app.NowPlayingBG == nil || app.NPCacheDirty {
		app.renderNowPlayingFull()
		app.drawStatusBar()

		if app.NowPlayingBG == nil {
			app.NowPlayingBG = image.NewRGBA(image.Rect(0, 0, SCREEN_WIDTH, SCREEN_HEIGHT))
		}
		copy(app.NowPlayingBG.Pix, app.FB.Pix)
		app.NPCacheDirty = false
	} else {
		copy(app.FB.Pix, app.NowPlayingBG.Pix)
	}

	// Draw progress bar using direct pixel operations (bypass gg)
	app.fastDrawProgressBar(40, PROGRESS_BAR_Y, SCREEN_WIDTH-80, app.Playing.Position, app.Playing.Duration)
}

// updateProgressBarOnly is the fast path called by the playback poller.
// Only updates progress bar region, bypasses gg entirely.
func (app *MiyooPod) updateProgressBarOnly() {
	if app.Playing == nil || app.NowPlayingBG == nil {
		return
	}

	// Don't draw progress bar over lock, volume/brightness, or power-exit overlay
	if app.Locked || app.OverlayVisible || app.PowerExitWarning {
		return
	}

	// Double-check we're still on Now Playing screen
	if app.CurrentScreen != ScreenNowPlaying {
		return
	}

	// Restore progress bar region from cache
	fastCopyRegion(app.FB, app.NowPlayingBG, PROGRESS_REGION_Y0, PROGRESS_REGION_Y1)

	// Draw progress bar with direct pixel ops
	app.fastDrawProgressBar(40, PROGRESS_BAR_Y, SCREEN_WIDTH-80, app.Playing.Position, app.Playing.Duration)

	// Full refresh (partial was buggy on Miyoo Mini SDL2)
	app.triggerRefresh()
}

// renderNowPlayingFull draws all static now-playing elements via gg
func (app *MiyooPod) renderNowPlayingFull() {
	dc := app.DC
	track := app.Playing.Track

	dc.SetHexColor(app.CurrentTheme.BG)
	dc.Clear()

	app.drawHeader("Now Playing")
	app.DrawCoverflow()

	// Track info on the right side of the album art
	infoX := 250
	infoStartY := 80

	// Title
	dc.SetFontFace(app.FontTitle)
	dc.SetHexColor(app.CurrentTheme.ItemTxt)
	maxWidth := float64(SCREEN_WIDTH - infoX - 20)
	titleText := app.truncateText(track.Title, maxWidth, app.FontTitle)
	dc.DrawString(titleText, float64(infoX), float64(infoStartY))

	// Artist
	dc.SetFontFace(app.FontArtist)
	dc.SetHexColor(app.CurrentTheme.Dim)
	artistText := app.truncateText(track.Artist, maxWidth, app.FontArtist)
	dc.DrawString(artistText, float64(infoX), float64(infoStartY+35))

	// Album
	if track.Album != "" && track.Album != "Unknown Album" {
		albumText := app.truncateText(track.Album, maxWidth, app.FontArtist)
		dc.DrawString(albumText, float64(infoX), float64(infoStartY+65))
	}

	// Track/Disc numbers if available
	if track.TrackNum > 0 {
		dc.SetFontFace(app.FontSmall)
		trackInfo := fmt.Sprintf("Track %d", track.TrackNum)
		if track.TrackTotal > 0 {
			trackInfo = fmt.Sprintf("Track %d/%d", track.TrackNum, track.TrackTotal)
		}
		dc.DrawString(trackInfo, float64(infoX), float64(infoStartY+95))
	}

	// Control hints
	dc.SetFontFace(app.FontSmall)
	dc.SetHexColor(app.CurrentTheme.Dim)
	dc.DrawString("Hold L/R to seek", float64(infoX), float64(infoStartY+125))
	dc.DrawString("X Repeat · SELECT Shuffle", float64(infoX), float64(infoStartY+150))

	// Format tag (FLAC / MP3 / OGG + bitrate)
	app.drawFormatTag(track, float64(infoX), float64(infoStartY+175))

	app.drawStatusIndicators(infoStartY + 250)
}

// drawFormatTag draws a small pill badge showing the audio format (e.g. "FLAC") and
// average bitrate (e.g. "1234 kbps") to the right of it, matching the button hint style.
func (app *MiyooPod) drawFormatTag(track *Track, x, y float64) {
	dc := app.DC
	dc.SetFontFace(app.FontSmall)

	ext := strings.ToLower(filepath.Ext(track.Path))
	var label string
	switch ext {
	case ".flac":
		label = "FLAC"
	case ".ogg":
		label = "OGG"
	case ".mp3":
		label = "MP3"
	default:
		label = strings.ToUpper(strings.TrimPrefix(ext, "."))
	}

	// Badge — same geometry as drawButtonLegend
	labelW, _ := dc.MeasureString(label)
	badgePad := 6.0
	badgeW := labelW + badgePad*2
	badgeH := 20.0
	badgeY := y - badgeH/2

	dc.SetHexColor(app.CurrentTheme.ProgBG)
	dc.DrawRoundedRectangle(x, badgeY, badgeW, badgeH, 4)
	dc.Fill()

	dc.SetHexColor(app.CurrentTheme.HeaderTxt)
	dc.DrawStringAnchored(label, x+badgeW/2, y-1, 0.5, 0.5)

	// Bitrate and sample rate to the right of the badge
	dc.SetHexColor(app.CurrentTheme.Dim)
	if track.Bitrate > 0 && track.SampleRate > 0 {
		dc.DrawStringAnchored(fmt.Sprintf("%d kbps · %g kHz", track.Bitrate, float64(track.SampleRate)/1000), x+badgeW+6, y, 0, 0.5)
	} else if track.Bitrate > 0 {
		dc.DrawStringAnchored(fmt.Sprintf("%d kbps", track.Bitrate), x+badgeW+6, y, 0, 0.5)
	} else if track.SampleRate > 0 {
		dc.DrawStringAnchored(fmt.Sprintf("%g kHz", float64(track.SampleRate)/1000), x+badgeW+6, y, 0, 0.5)
	}
}
