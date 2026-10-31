package main

// ij_genius.go — Genius, in the main menu while nothing is playing: 50
// songs picked at random from the library (no podcast episodes), the same
// artist never twice in a row when it can be helped, and they play at once.
// Despite the name, it's chance, not taste.

import (
	"math/rand"
	"path/filepath"
	"strings"
)

// geniusSize: how many songs a Genius list has.
const geniusSize = 50

// geniusTracks: up to n songs of lib at random, artists spread out.
func geniusTracks(lib *Library, n int) []*Track {
	pods := strings.ToLower(filepath.Join(musicRoot(), "Podcasts")) + string(filepath.Separator)
	var pool []*Track
	for _, t := range lib.Tracks {
		if strings.EqualFold(t.Genre, "Podcast") || strings.HasPrefix(strings.ToLower(t.Path), pods) {
			continue
		}
		pool = append(pool, t)
	}
	rand.Shuffle(len(pool), func(i, j int) { pool[i], pool[j] = pool[j], pool[i] })
	if len(pool) > n {
		pool = pool[:n]
	}
	// the same artist twice in a row: swap the second with a later song by
	// someone else
	artist := func(t *Track) string { return strings.ToLower(strings.TrimSpace(t.Artist)) }
	for i := 1; i < len(pool); i++ {
		if artist(pool[i]) != artist(pool[i-1]) {
			continue
		}
		for j := i + 1; j < len(pool); j++ {
			if artist(pool[j]) != artist(pool[i-1]) {
				pool[i], pool[j] = pool[j], pool[i]
				break
			}
		}
	}
	return pool
}

// playGenius starts a Genius list and opens Now Playing.
func (ui *IpodUI) playGenius() {
	tracks := geniusTracks(ui.library(), geniusSize)
	if len(tracks) == 0 {
		ui.showToast("No songs in your library yet")
		return
	}
	// playSong keeps the current song playing when it is the first: the
	// list wouldn't start, so it goes last
	if p := ui.app.Playing; len(tracks) > 1 && tracks[0] == p.Track {
		tracks = append(tracks[1:], tracks[0])
	}
	ui.playSong(tracks, 0, true)
}
