package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"sync"
	"time"
)

func PLAYBACK_STATE_PATH() string { return dataPath(".miyoopod_playback.json") }

// PlaybackState stores queue and playback position for session persistence
type PlaybackState struct {
	// Queue tracks stored as paths (resolved against library on load)
	TrackPaths []string `json:"track_paths"`
	// CurrentIndex is a play position: with shuffle on, a place in
	// ShuffleOrder. CurrentPath is the song there (files before 1.0
	// don't have it).
	CurrentIndex int        `json:"current_index"`
	CurrentPath  string     `json:"current_path,omitempty"`
	Position     float64    `json:"position"`
	Shuffle      bool       `json:"shuffle"`
	ShuffleOrder []int      `json:"shuffle_order,omitempty"`
	Repeat       RepeatMode `json:"repeat"`
	Volume       float64    `json:"volume,omitempty"` // (no longer written)
}

// The queue's part of the file is kept while the queue doesn't change:
// it is saved every 10 s for the position, and a big shuffled queue would
// otherwise be made again each time (MBs of garbage on the Miyoo).
var (
	saveMu        sync.Mutex
	saveQueueKey  uint64
	saveQueueJSON []byte // `"track_paths":[…],"shuffle_order":[…]`
	saveBuf       bytes.Buffer
)

// queueKey: a hash of the queue's paths and order (nothing allocated).
func queueKey(tracks []*Track, order []int) uint64 {
	const prime = 1099511628211
	h := uint64(14695981039346656037)
	for _, t := range tracks {
		for i := 0; i < len(t.Path); i++ {
			h = (h ^ uint64(t.Path[i])) * prime
		}
		h = (h ^ 0x100) * prime // (the end of a path)
	}
	h = (h ^ uint64(len(tracks))) * prime
	for _, v := range order {
		h = (h ^ uint64(v)) * prime
	}
	return (h ^ uint64(len(order))) * prime
}

// savePlaybackState persists the current queue and playback position to disk
func (app *MiyooPod) savePlaybackState() {
	saveMu.Lock()
	defer saveMu.Unlock()
	if app.Queue == nil || len(app.Queue.Tracks) == 0 {
		// Remove stale state file if queue is empty
		os.Remove(PLAYBACK_STATE_PATH())
		saveQueueJSON = nil
		return
	}
	tracks, order := app.Queue.Tracks, app.Queue.ShuffleOrder
	if !app.Queue.Shuffle {
		order = nil
	}

	if k := queueKey(tracks, order); saveQueueJSON == nil || k != saveQueueKey {
		paths := make([]string, len(tracks))
		for i, t := range tracks {
			paths[i] = t.Path
		}
		q, err := json.Marshal(struct {
			TrackPaths   []string `json:"track_paths"`
			ShuffleOrder []int    `json:"shuffle_order,omitempty"`
		}{paths, order})
		if err != nil {
			logMsg(fmt.Sprintf("ERROR: Failed to marshal playback state: %v", err))
			return
		}
		saveQueueJSON, saveQueueKey = q[1:len(q)-1], k // (without the braces)
	}

	// Get current playback position — try audio backend first, fall back to poller value
	position := 0.0
	if app.Playing != nil {
		if app.Playing.State != StateStopped {
			state := audioGetState()
			if state.Position > 0 {
				position = state.Position
			} else if app.Playing.Position > 0 {
				// Audio backend may be unavailable (shutting down), use last known position
				position = app.Playing.Position
			}
		} else if app.Playing.Position > 0 {
			// Stopped but have last known position (e.g., force quit)
			position = app.Playing.Position
		}
	}
	cur := ""
	if t := app.getCurrentTrack(); t != nil {
		cur = t.Path
	}

	head, err := json.Marshal(struct {
		CurrentIndex int        `json:"current_index"`
		CurrentPath  string     `json:"current_path,omitempty"`
		Position     float64    `json:"position"`
		Shuffle      bool       `json:"shuffle"`
		Repeat       RepeatMode `json:"repeat"`
	}{app.Queue.CurrentIndex, cur, position, app.Queue.Shuffle, app.Queue.Repeat})
	if err != nil {
		logMsg(fmt.Sprintf("ERROR: Failed to marshal playback state: %v", err))
		return
	}
	saveBuf.Reset()
	saveBuf.Write(head[:len(head)-1])
	saveBuf.WriteByte(',')
	saveBuf.Write(saveQueueJSON)
	saveBuf.WriteString("}\n")

	if err := writeFileAtomic(PLAYBACK_STATE_PATH(), saveBuf.Bytes()); err != nil {
		logMsg(fmt.Sprintf("ERROR: Failed to save playback state: %v", err))
	}
}

// restoreQueue maps a saved queue onto the library (lookup: a path's
// track, or nil; songs no longer there are left out). It returns the
// tracks, the current play position and, with shuffle on, the order. The
// current song is found by its path, and stays current; if it is gone, the
// next song that is still there is.
func restoreQueue(ps *PlaybackState, lookup func(string) *Track) (tracks []*Track, cur int, order []int) {
	newIdx := make([]int, len(ps.TrackPaths)) // old index -> new, or -1
	for i, path := range ps.TrackPaths {
		newIdx[i] = -1
		if t := lookup(path); t != nil {
			newIdx[i] = len(tracks)
			tracks = append(tracks, t)
		}
	}
	if len(tracks) == 0 {
		return nil, 0, nil
	}
	inOld := func(i int) bool { return i >= 0 && i < len(ps.TrackPaths) }

	// the current song's old index
	shuffled := ps.Shuffle && len(ps.ShuffleOrder) > 0
	curOld := ps.CurrentIndex
	if shuffled {
		curOld = -1
		if p := ps.CurrentIndex; p >= 0 && p < len(ps.ShuffleOrder) {
			curOld = ps.ShuffleOrder[p]
		}
	}
	if ps.CurrentPath != "" && (!inOld(curOld) || ps.TrackPaths[curOld] != ps.CurrentPath) {
		for i, p := range ps.TrackPaths {
			if p == ps.CurrentPath {
				curOld = i
				break
			}
		}
	}

	if !ps.Shuffle {
		for i := max(curOld, 0); i < len(newIdx); i++ {
			if newIdx[i] >= 0 {
				return tracks, newIdx[i], nil
			}
		}
		return tracks, 0, nil
	}

	// the old order, without the songs gone: the current one (or the
	// first still there after it) keeps its place
	cur = -1
	seen := make([]bool, len(tracks))
	passed := false
	for _, o := range ps.ShuffleOrder {
		if !inOld(o) {
			continue
		}
		if o == curOld {
			passed = true
		}
		n := newIdx[o]
		if n < 0 || seen[n] {
			continue
		}
		seen[n] = true
		if cur < 0 && passed {
			cur = len(order)
		}
		order = append(order, n)
	}
	if len(order) != len(tracks) {
		// no order saved, or not one of these songs: a new one, from the
		// current song
		start := 0
		if inOld(curOld) && newIdx[curOld] >= 0 {
			start = newIdx[curOld]
		}
		return tracks, 0, shuffleOrder(len(tracks), start)
	}
	if cur < 0 {
		cur = 0
	}
	return tracks, cur, order
}

// restorePlaybackState loads a previously saved queue and resumes paused at the saved position
func (app *MiyooPod) restorePlaybackState() {
	data, err := os.ReadFile(PLAYBACK_STATE_PATH())
	if err != nil {
		// No saved state, nothing to restore
		return
	}

	var ps PlaybackState
	if err := json.Unmarshal(data, &ps); err != nil {
		logMsg(fmt.Sprintf("WARNING: Could not parse playback state: %v", err))
		return
	}

	if len(ps.TrackPaths) == 0 {
		return
	}

	// Resolve track paths against the loaded library
	if app.Library == nil || app.Library.TracksByPath == nil {
		logMsg("WARNING: Cannot restore playback state - library not loaded")
		return
	}

	tracks, cur, order := restoreQueue(&ps, func(p string) *Track { return app.Library.TracksByPath[p] })
	if len(tracks) == 0 {
		logMsg("WARNING: No saved queue tracks found in library")
		return
	}

	// Rebuild queue
	app.Queue.Tracks = tracks
	app.Queue.CurrentIndex = cur
	app.Queue.Shuffle = ps.Shuffle
	app.Queue.ShuffleOrder = order
	app.Queue.Repeat = ps.Repeat

	// Volume is now restored from settings, not playback state

	// Load the current track and pause at the saved position
	track := app.getCurrentTrack()
	if track == nil {
		return
	}

	app.Playing.Track = track
	if track.Duration > 0 {
		app.Playing.Duration = track.Duration
	}

	err = app.mpvLoadFile(track.Path)
	if err != nil {
		logMsg(fmt.Sprintf("WARNING: Failed to load saved track: %v", err))
		app.Playing.State = StateStopped
		app.Playing.Track = nil
		return
	}

	// Pause immediately - user expects to resume manually
	audioPause()
	app.Playing.State = StatePaused

	// Set position immediately so UI shows saved position
	if ps.Position > 0 {
		app.Playing.Position = ps.Position
	}

	app.updateCoverflowForCurrentTrack()
	app.NPCacheDirty = true

	logMsg(fmt.Sprintf("INFO: Restored playback state - %s at %.1fs (paused)", track.Title, ps.Position))

	// Seek to saved position in background — audio backend needs time to be ready
	if ps.Position > 0 {
		seekTarget := ps.Position
		go func() {
			for attempt := 0; attempt < 10; attempt++ {
				time.Sleep(100 * time.Millisecond)
				audioSeek(seekTarget)

				time.Sleep(50 * time.Millisecond)
				state := audioGetState()
				if state.Position > 0 && state.Duration > 0 {
					app.Playing.Position = state.Position
					app.Playing.Duration = state.Duration
					app.NPCacheDirty = true
					app.requestRedraw()
					return
				}
			}
			logMsg(fmt.Sprintf("WARNING: Could not verify seek to %.1fs for %s", seekTarget, track.Title))
		}()
	}
}
