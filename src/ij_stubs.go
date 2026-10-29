package main

// ij_stubs.go — glue between the ipod.js UI and the MiyooPod back end.

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync/atomic"
	"time"
)

// rescanPending: a rescan was asked for while one was running.
var rescanPending atomic.Bool

// rescanLibrary rescans the music folder in the background and rebuilds
// the menus when it's done.
func (ui *IpodUI) rescanLibrary() {
	if dbBusy { // the database or the music is being moved
		return
	}
	go func() {
		done := make(chan struct{})
		if !ui.app.startLibraryScan(func() { close(done) }) {
			// one is running: scan again when it ends (it may have
			// passed the folder that just changed)
			rescanPending.Store(true)
			return
		}
		<-done
		time.Sleep(10 * time.Millisecond)
		ui.postSure(func() {
			ui.kb.initialized = false
			ui.refreshBuilt()
			if rescanPending.Swap(false) {
				ui.rescanLibrary()
			}
		})
		// covers from the files and from cover.jpg / folder.jpg of the new
		// albums (at start-up main runs this pass; after a rescan, here)
		ui.app.deferredArtExtraction()
	}()
}

// addFolderToLibrary puts the songs of one new folder (a received share)
// into the library without scanning the whole music folder: the library is
// copied, the folder's files are read with the scan's own scanTrack, and the
// copy replaces the library and is saved. While a full scan runs it just
// asks for another one (that one will find the folder). Background goroutine.
func (ui *IpodUI) addFolderToLibrary(dir string) bool {
	app := ui.app
	scanStartMu.Lock()
	if app.LibScanRunning || dbBusy {
		scanStartMu.Unlock()
		return false
	}
	app.LibScanRunning = true // (no full scan starts meanwhile)
	scanStartMu.Unlock()
	defer func() { app.LibScanRunning = false }()

	old := app.Library
	if old == nil {
		old = &Library{}
	}
	lib := &Library{
		Tracks:        append([]*Track{}, old.Tracks...),
		Albums:        append([]*Album{}, old.Albums...),
		Playlists:     old.Playlists,
		TracksByPath:  make(map[string]*Track, len(old.Tracks)+32),
		AlbumsByKey:   make(map[string]*Album, len(old.Albums)+4),
		ArtistsByName: make(map[string]*Artist),
	}
	for k, v := range old.TracksByPath {
		lib.TracksByPath[k] = v
	}
	for k, v := range old.AlbumsByKey {
		lib.AlbumsByKey[k] = v
	}
	scanLib = lib
	n := 0
	filepath.Walk(dir, func(path string, info os.FileInfo, err error) error {
		if err != nil || info.IsDir() || lib.TracksByPath[path] != nil {
			return nil
		}
		switch strings.ToLower(filepath.Ext(path)) {
		case ".mp3", ".flac", ".ogg", ".wav", ".m4a", ".mp4", ".aif", ".aiff":
			app.scanTrack(path)
			n++
		}
		return nil
	})
	finishAlbums(lib)
	sort.Slice(lib.Tracks, func(i, j int) bool {
		return strings.ToLower(lib.Tracks[i].Title) < strings.ToLower(lib.Tracks[j].Title)
	})
	sort.Slice(lib.Albums, func(i, j int) bool {
		return strings.ToLower(lib.Albums[i].Name) < strings.ToLower(lib.Albums[j].Name)
	})
	sort.Slice(lib.Artists, func(i, j int) bool {
		return strings.ToLower(lib.Artists[i].Name) < strings.ToLower(lib.Artists[j].Name)
	})
	for _, al := range lib.Albums {
		if !strings.HasPrefix(albumDir(al), dir) {
			continue
		}
		al := al
		sort.SliceStable(al.Tracks, func(i, j int) bool {
			if al.Tracks[i].DiscNum != al.Tracks[j].DiscNum {
				return al.Tracks[i].DiscNum < al.Tracks[j].DiscNum
			}
			return al.Tracks[i].TrackNum < al.Tracks[j].TrackNum
		})
	}
	for _, ar := range lib.Artists {
		ar := ar
		sort.Slice(ar.Albums, func(i, j int) bool {
			return strings.ToLower(ar.Albums[i].Name) < strings.ToLower(ar.Albums[j].Name)
		})
	}
	app.installLibrary(lib)
	if err := app.saveLibraryJSON(); err != nil {
		logMsg("WARNING: Failed to save library: " + err.Error())
	}
	logMsg(fmt.Sprintf("Library: %d songs added from %s", n, dir))
	ui.postSure(func() {
		ui.kb.initialized = false
		ui.refreshBuilt()
	})
	// its cover (cover.jpg next to the songs, or in the files)
	go app.decodeAlbumArt()
	go app.deferredArtExtraction()
	return true
}

func albumDir(al *Album) string {
	if len(al.Tracks) == 0 || al.Tracks[0] == nil {
		return ""
	}
	return filepath.Dir(al.Tracks[0].Path)
}

// removeFromLibrary drops songs (deleted files) from the library without
// scanning the whole music folder: the library is copied without them,
// albums and artists are rebuilt, and the copy replaces it and is saved.
// Background goroutine (installLibrary waits for the UI goroutine).
func (ui *IpodUI) removeFromLibrary(paths []string) {
	app := ui.app
	scanStartMu.Lock()
	if app.LibScanRunning || dbBusy {
		scanStartMu.Unlock()
		ui.post(ui.rescanLibrary) // (the scan running will see they're gone)
		return
	}
	app.LibScanRunning = true
	scanStartMu.Unlock()
	defer func() { app.LibScanRunning = false }()

	gone := map[string]bool{}
	for _, p := range paths {
		gone[p] = true
	}
	old := app.Library
	if old == nil {
		return
	}
	lib := &Library{
		Playlists:     old.Playlists,
		TracksByPath:  make(map[string]*Track, len(old.Tracks)),
		AlbumsByKey:   make(map[string]*Album, len(old.Albums)),
		ArtistsByName: make(map[string]*Artist),
	}
	for _, t := range old.Tracks {
		if !gone[t.Path] {
			lib.Tracks = append(lib.Tracks, t)
			lib.TracksByPath[t.Path] = t
		}
	}
	for _, al := range old.Albums {
		var keep []*Track
		for _, t := range al.Tracks {
			if !gone[t.Path] {
				keep = append(keep, t)
			}
		}
		if len(keep) == 0 {
			continue
		}
		if len(keep) != len(al.Tracks) {
			// (a copy: the old library keeps its own until it's replaced)
			c := *al
			c.Tracks = keep
			al = &c
		}
		lib.Albums = append(lib.Albums, al)
		lib.AlbumsByKey[al.Key] = al
	}
	finishAlbums(lib)
	sort.Slice(lib.Artists, func(i, j int) bool {
		return strings.ToLower(lib.Artists[i].Name) < strings.ToLower(lib.Artists[j].Name)
	})
	for _, ar := range lib.Artists {
		ar := ar
		sort.Slice(ar.Albums, func(i, j int) bool {
			return strings.ToLower(ar.Albums[i].Name) < strings.ToLower(ar.Albums[j].Name)
		})
	}
	app.installLibrary(lib)
	if err := app.saveLibraryJSON(); err != nil {
		logMsg("WARNING: Failed to save library: " + err.Error())
	}
	logMsg(fmt.Sprintf("Library: %d songs removed", len(old.Tracks)-len(lib.Tracks)))
	ui.postSure(func() {
		ui.kb.initialized = false
		ui.refreshBuilt()
	})
}

// askDeleteAlbum: Delete Album (an album's options), after a confirmation:
// its songs are deleted from the card and leave the library; its folder
// goes too when nothing but pictures and lists is left in it.
func (ui *IpodUI) askDeleteAlbum(al *Album) {
	tracks := albumTracks(al)
	if len(tracks) == 0 {
		return
	}
	n := len(tracks)
	title := fmt.Sprintf("Delete %s? Its %d %s will be deleted from the card. This can't be undone.", al.Name, n, pluralize(n, "song", "songs"))
	ui.showSheetTitled(title, []SheetOption{{Label: "Delete Album", Danger: true, OnSelect: func() {
		var paths []string
		dirs := map[string]bool{}
		for _, t := range tracks {
			paths = append(paths, t.Path)
			dirs[filepath.Dir(t.Path)] = true
		}
		// a song of it playing: the music stops (its file goes)
		if p := ui.app.Playing; p != nil && p.Track != nil {
			for _, t := range tracks {
				if t == p.Track || t.Path == p.Track.Path {
					ui.clearNowPlaying()
					break
				}
			}
		}
		go func() {
			failed := removeMusicPaths(paths)
			for d := range dirs {
				removeAlbumDirIfDone(d)
			}
			ui.postSure(func() {
				ui.dropDeletedFromQueue(paths)
				if failed != nil {
					ui.showToast("Couldn't delete: " + shortErr(failed))
				} else {
					ui.showToast("Deleted " + al.Name)
				}
				logMsg(fmt.Sprintf("Delete Album: %q, %d songs", al.Name, n))
			})
			ui.removeFromLibrary(paths)
		}()
	}}})
}

// removeAlbumDirIfDone: an album's folder, once its songs are deleted, goes
// too if only pictures, playlists, lyrics or cue sheets are left (never
// the music folder itself, never a folder with other files or folders).
func removeAlbumDirIfDone(dir string) {
	if !insideMusic(dir) || filepath.Clean(dir) == filepath.Clean(musicRoot()) {
		return
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return
	}
	for _, e := range entries {
		if e.IsDir() {
			return
		}
		switch strings.ToLower(filepath.Ext(e.Name())) {
		case ".jpg", ".jpeg", ".png", ".webp", ".gif", ".bmp", ".m3u", ".m3u8", ".lrc", ".cue", ".txt", ".nfo", ".log", ".json":
		default:
			if !strings.HasPrefix(e.Name(), ".") {
				return // something else: the folder stays
			}
		}
	}
	os.RemoveAll(dir)
	forgetDirInfo(dir)
	// its artist's folder too, when it is left empty
	if up := filepath.Dir(dir); insideMusic(up) && filepath.Clean(up) != filepath.Clean(musicRoot()) {
		if left, err := os.ReadDir(up); err == nil && len(left) == 0 {
			os.Remove(up)
			forgetDirInfo(up)
		}
	}
}
