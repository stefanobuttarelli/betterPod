package main

import (
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"
)

// parsePlaylists reads the library's playlists (lower: see m3uTrack, made
// once for all of them, and only if needed).
func (app *MiyooPod) parsePlaylists(lib *Library) {
	var lower map[string]*Track
	for _, pl := range lib.Playlists {
		app.parsePlaylist(pl, lib, &lower)
	}
}

// parsePlaylist reads an M3U file and resolves track references
func (app *MiyooPod) parsePlaylist(pl *Playlist, lib *Library, lower *map[string]*Track) {
	data, err := os.ReadFile(pl.Path)
	if err != nil {
		logMsg("ERROR: Failed to read playlist " + pl.Path + ": " + err.Error())
		return
	}

	baseDir := filepath.Dir(pl.Path)

	for _, line := range strings.Split(string(data), "\n") {
		line = m3uLine(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}

		// (the library the playlist belongs to: a scan parses them before
		// its library is installed, and on a first scan there is no other)
		if lib == nil {
			break
		}
		if track := m3uTrack(lib, baseDir, line, lower); track != nil {
			pl.Tracks = append(pl.Tracks, track)
		}
	}

	logMsg(fmt.Sprintf("Parsed playlist %s: %d tracks", pl.Name, len(pl.Tracks)))
}

// m3uLine: a line of an M3U file without its spaces, \r and UTF-8 BOM
// (Windows' Notepad starts the file with one).
func m3uLine(line string) string {
	line = strings.TrimSpace(line)
	line = strings.TrimPrefix(line, "\uFEFF")
	return strings.TrimSpace(line)
}

// m3uPath: where a song's line points, as the library keys songs. A
// relative path is relative to the .m3u file's folder; file:// URLs are
// read as paths. ok is false for other URLs. slashes reads Windows'
// backslashes as folders.
func m3uPath(baseDir, line string, slashes bool) (string, bool) {
	p := line
	if strings.HasPrefix(p, "file://") {
		if u, err := url.Parse(p); err == nil {
			p = u.Path
		}
	} else if strings.Contains(p, "://") {
		return "", false
	}
	if slashes {
		p = strings.ReplaceAll(p, `\`, "/")
	}
	if !filepath.IsAbs(p) {
		p = filepath.Join(baseDir, p)
	}
	return filepath.Clean(p), true
}

// m3uTrack: the library's song for a line, or nil: the path as written,
// then with Windows' backslashes, then ignoring upper/lower case (a FAT card
// doesn't tell them apart). lower, the paths in lower case, is made on the
// first need (nil: not looked at).
func m3uTrack(lib *Library, baseDir, line string, lower *map[string]*Track) *Track {
	p, ok := m3uPath(baseDir, line, false)
	if !ok {
		return nil
	}
	if t := lib.TracksByPath[p]; t != nil {
		return t
	}
	if strings.Contains(line, `\`) {
		p, _ = m3uPath(baseDir, line, true)
		if t := lib.TracksByPath[p]; t != nil {
			return t
		}
	}
	if lower == nil {
		return nil
	}
	if *lower == nil {
		*lower = make(map[string]*Track, len(lib.TracksByPath))
		for k, t := range lib.TracksByPath {
			(*lower)[strings.ToLower(k)] = t
		}
	}
	return (*lower)[strings.ToLower(p)]
}
