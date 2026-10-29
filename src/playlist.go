package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

// parsePlaylist reads an M3U file and resolves track references
func (app *MiyooPod) parsePlaylist(pl *Playlist, lib *Library) {
	data, err := os.ReadFile(pl.Path)
	if err != nil {
		logMsg("ERROR: Failed to read playlist " + pl.Path + ": " + err.Error())
		return
	}

	lines := strings.Split(string(data), "\n")
	baseDir := filepath.Dir(pl.Path)

	for _, line := range lines {
		line = strings.TrimSpace(line)
		// Handle Windows line endings
		line = strings.TrimRight(line, "\r")

		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}

		// Resolve relative paths against the .m3u file's directory
		trackPath := line
		if !filepath.IsAbs(trackPath) {
			trackPath = filepath.Join(baseDir, trackPath)
		}
		trackPath = filepath.Clean(trackPath)

		// (the library the playlist belongs to: a scan parses them before
		// its library is installed, and on a first scan there is no other)
		if lib == nil {
			break
		}
		if track, ok := lib.TracksByPath[trackPath]; ok {
			pl.Tracks = append(pl.Tracks, track)
		}
	}

	logMsg(fmt.Sprintf("Parsed playlist %s: %d tracks", pl.Name, len(pl.Tracks)))
}
