package main

import (
	"os"
	"strings"
)

// envGet accepts the prefix used by existing Knulli launchers so an in-app
// update keeps its music folder, database and playback guard on first restart.
// The new name takes precedence, including an explicitly empty value.
func envGet(name string) string {
	if value, ok := os.LookupEnv(name); ok {
		return value
	}
	if suffix, ok := strings.CutPrefix(name, "BETTERPOD_"); ok {
		return os.Getenv("MIYOOPOD_" + suffix)
	}
	return ""
}

// Existing Knulli database names stay unchanged so the stable release can
// load libraries, settings, artwork and playback saved by earlier versions.
const (
	libraryFile   = ".miyoopod_library.json"
	settingsFile  = ".miyoopod_settings.json"
	playbackFile  = ".miyoopod_playback.json"
	artworkFolder = ".miyoopod_artwork"
)
