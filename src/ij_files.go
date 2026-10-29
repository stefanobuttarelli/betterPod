package main

// ij_files.go — Settings → Library → Browse Files: a little Finder for the
// music folder, to tidy it up (songs downloaded and no longer wanted,
// leftovers, empty folders).
//
//	Music                                   ← the folder's name
//	  12.4 GB free
//	  Animal Crossing            1.2 GB  ›   folders first
//	    Folder · 174 items
//	  cover.jpg                  240 KB
//	    JPEG image
//
// A opens a folder (on a file: its options). Y on a row: Delete, or Select
// to pick several (A then ticks them; the top row deletes them all).
// Deleting always asks first, and the library is scanned again after.

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
)

type fileEntry struct {
	name  string
	path  string
	dir   bool
	size  int64 // files: their size; folders: filled in the background (-1 until then)
	items int   // folders: entries directly inside
}

// folder sizes and item counts, computed in the background and kept for
// the session
type dirInfo struct {
	size  int64
	items int
}

var (
	dirSizeMu sync.Mutex
	dirSizes  = map[string]dirInfo{}
)

// listFolder reads a folder: one directory read, and a stat only for links
// and files (their size). Folder sizes and counts come later (dirInfoOf).
func listFolder(dir string) ([]fileEntry, error) {
	des, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}
	out := make([]fileEntry, 0, len(des))
	for _, de := range des {
		e := fileEntry{name: de.Name(), path: filepath.Join(dir, de.Name()), size: -1, items: -1}
		isDir := de.IsDir()
		if de.Type()&os.ModeSymlink != 0 {
			if info, err := os.Stat(e.path); err == nil {
				isDir = info.IsDir()
				if !isDir {
					e.size = info.Size()
				}
			}
		} else if !isDir {
			if info, err := de.Info(); err == nil {
				e.size = info.Size()
			}
		}
		if isDir {
			e.dir = true
			dirSizeMu.Lock()
			if di, ok := dirSizes[e.path]; ok {
				e.size, e.items = di.size, di.items
			}
			dirSizeMu.Unlock()
		}
		out = append(out, e)
	}
	sort.SliceStable(out, func(i, j int) bool {
		if out[i].dir != out[j].dir {
			return out[i].dir
		}
		return strings.ToLower(out[i].name) < strings.ToLower(out[j].name)
	})
	return out, nil
}

// dirInfoOf: a folder's total size and its number of entries.
func dirInfoOf(p string) dirInfo {
	di := dirInfo{size: dirSize(p)}
	if des, err := os.ReadDir(p); err == nil {
		di.items = len(des)
	}
	return di
}

// forgetDirInfo drops what is known of p and the folders around it (after
// a delete).
func forgetDirInfo(p string) {
	dirSizeMu.Lock()
	for k := range dirSizes {
		if k == p || strings.HasPrefix(p, k+string(filepath.Separator)) || strings.HasPrefix(k, p+string(filepath.Separator)) {
			delete(dirSizes, k)
		}
	}
	dirSizeMu.Unlock()
}

func fileKind(name string) string {
	switch strings.ToLower(filepath.Ext(name)) {
	case ".mp3":
		return "MP3 audio"
	case ".flac":
		return "FLAC audio"
	case ".m4a", ".mp4":
		return "AAC / ALAC audio"
	case ".wav":
		return "WAV audio"
	case ".aif", ".aiff":
		return "AIFF audio"
	case ".ogg", ".opus":
		return "Ogg audio"
	case ".jpg", ".jpeg":
		return "JPEG image"
	case ".png":
		return "PNG image"
	case ".m3u", ".m3u8":
		return "Playlist"
	case ".part":
		return "Unfinished download"
	case ".lrc":
		return "Lyrics"
	}
	if strings.HasPrefix(name, "._") {
		return "macOS leftover"
	}
	if e := strings.TrimPrefix(filepath.Ext(name), "."); e != "" {
		return strings.ToUpper(e) + " file"
	}
	return "File"
}

// insideMusic: p is in the music folder (and is not the folder itself),
// also once links are resolved: the folder holding p must really be inside
// the music folder, so a link to elsewhere can't lead outside it (deleting
// a link itself only removes the link).
func insideMusic(p string) bool {
	root := filepath.Clean(MUSIC_ROOT)
	p = filepath.Clean(p)
	if p == root || !strings.HasPrefix(p, root+string(filepath.Separator)) {
		return false
	}
	realRoot, err1 := filepath.EvalSymlinks(root)
	realDir, err2 := filepath.EvalSymlinks(filepath.Dir(p))
	if err1 != nil || err2 != nil {
		return false
	}
	return realDir == realRoot || strings.HasPrefix(realDir, realRoot+string(filepath.Separator))
}

// filesView: one folder.
func (ui *IpodUI) filesView(dir string) *View {
	title := filepath.Base(dir)
	if filepath.Clean(dir) == filepath.Clean(MUSIC_ROOT) {
		title = "Music Folder"
	}
	st := &struct {
		entries   []fileEntry
		err       string
		selecting bool
		picked    map[string]bool
		busy      string
		loading   bool
		gen       int // the current load; older background work stops
	}{picked: map[string]bool{}}
	v := &View{ID: "files", Title: title, Kind: KindFull, Empty: "Empty folder"}
	var load func()
	var measure func(gen int, todo []string)
	var del func(paths []string, what string)

	// load reads the folder in the background ("Loading…" meanwhile), then
	// measures its subfolders one by one, updating the rows as it goes and
	// stopping when the view is left or loaded again
	load = func() {
		st.gen++
		gen := st.gen
		st.loading = true
		go func() {
			es, err := listFolder(dir)
			ui.postSure(func() {
				if gen != st.gen {
					return
				}
				st.loading = false
				st.entries, st.err = es, ""
				if err != nil {
					st.err = "Couldn't open: " + shortErr(err)
				}
				keep := v.Index
				ui.refreshView(v)
				if keep == 0 && len(v.Items) > 1 {
					v.Index = 1
				}
				var todo []string
				for _, e := range es {
					if e.dir && e.size < 0 {
						todo = append(todo, e.path)
					}
				}
				if len(todo) > 0 {
					go measure(gen, todo)
				}
			})
		}()
	}
	measure = func(gen int, todo []string) {
		last := time.Now()
		var done []string
		flush := func() {
			got := done
			done = nil
			ui.postSure(func() {
				if gen != st.gen {
					return
				}
				if !ui.viewOpen(v) {
					st.gen++ // left: stop measuring
					return
				}
				for _, p := range got {
					dirSizeMu.Lock()
					di := dirSizes[p]
					dirSizeMu.Unlock()
					for i := range st.entries {
						if st.entries[i].path == p {
							st.entries[i].size, st.entries[i].items = di.size, di.items
						}
					}
				}
				ui.refreshView(v)
			})
		}
		for _, p := range todo {
			if !ui.app.Running {
				return
			}
			di := dirInfoOf(p)
			dirSizeMu.Lock()
			dirSizes[p] = di
			dirSizeMu.Unlock()
			done = append(done, p)
			if time.Since(last) > 250*time.Millisecond {
				flush()
				last = time.Now()
				// the view may have been left: checked on the UI side
				stop := make(chan bool, 1)
				ui.postSure(func() { stop <- gen != st.gen })
				if <-stop {
					return
				}
			}
		}
		if len(done) > 0 {
			flush()
		}
	}

	del = func(paths []string, what string) {
		ui.showSheetTitled("Delete "+what+"? This can't be undone.", []SheetOption{{Label: "Delete", Danger: true, OnSelect: func() {
			st.busy = "Deleting…"
			ui.refreshView(v)
			go func() {
				failed := removeMusicPaths(paths)
				ui.postSure(func() {
					ui.dropDeletedFromQueue(paths)
					st.busy = ""
					st.selecting = false
					st.picked = map[string]bool{}
					// this folder and the ones above it read again (the
					// cursor stays where it was, clamped)
					ui.refreshAllFiles()
					if failed != nil {
						ui.showToast("Couldn't delete: " + shortErr(failed))
					} else {
						ui.showToast("Deleted")
					}
					logMsg(fmt.Sprintf("Files: deleted %d item(s) in %s", len(paths), dir))
					ui.rescanLibrary() // the library forgets what's gone
				})
			}()
		}}})
	}

	v.Build = func() []Item {
		var items []Item
		switch {
		case st.busy != "":
			items = append(items, Item{Label: st.busy, NoArrow: true})
		case st.selecting:
			var paths []string
			for _, e := range st.entries {
				if st.picked[e.path] {
					paths = append(paths, e.path)
				}
			}
			n := len(paths)
			lab := "Delete Selected"
			sub := "Press [A] on the items to select them"
			if n > 0 {
				sub = fmt.Sprintf("%d %s selected", n, pluralize(n, "item", "items"))
			}
			items = append(items, Item{Label: lab, Sublabel: sub, NoArrow: true, OnSelect: func() {
				if n > 0 {
					del(paths, fmt.Sprintf("%d %s", n, pluralize(n, "item", "items")))
				}
			}})
			items = append(items, Item{Label: "Done Selecting", NoArrow: true, OnSelect: func() {
				st.selecting = false
				st.picked = map[string]bool{}
				v.Index = 1
				ui.refreshView(v)
			}})
		case st.loading && st.entries == nil:
			items = append(items, Item{Label: "Loading…", NoArrow: true})
		default:
			free := ""
			if fb := freeBytes(dir); fb >= 0 {
				free = formatBytes(fb) + " free on the card"
			}
			n := len(st.entries)
			items = append(items, Item{Label: fmt.Sprintf("%d %s", n, pluralize(n, "item", "items")), Sublabel: free, NoArrow: true})
		}
		if st.err != "" {
			items = append(items, Item{Label: st.err, NoArrow: true})
		}
		for _, e := range st.entries {
			e := e
			it := Item{Label: e.name}
			if e.size >= 0 {
				it.Value = formatBytes(e.size)
			} else if e.dir {
				it.Value = "…"
			}
			if e.dir {
				it.Sublabel = "Folder"
				if e.items >= 0 {
					it.Sublabel = fmt.Sprintf("Folder · %d %s", e.items, pluralize(e.items, "item", "items"))
				}
			} else {
				it.Sublabel = fileKind(e.name)
				it.NoArrow = true
			}
			if st.selecting {
				it.Checked = st.picked[e.path]
				it.NoArrow = true
				it.OnSelect = func() {
					st.picked[e.path] = !st.picked[e.path]
					ui.refreshView(v)
				}
			} else if e.dir {
				it.OnSelect = func() { ui.showView(ui.filesView(e.path), "") }
			} else {
				it.OnSelect = func() {
					ui.showSheetTitled(e.name, []SheetOption{{Label: "Delete", Danger: true, OnSelect: func() { del([]string{e.path}, "“"+e.name+"”") }}})
				}
			}
			kind := "file"
			if e.dir {
				kind = "folder and everything in it"
			}
			it.LongPress = []SheetOption{
				{Label: "Delete " + kind, Danger: true, OnSelect: func() { del([]string{e.path}, "“"+e.name+"”") }},
				{Label: "Select Several", OnSelect: func() {
					st.selecting = true
					st.picked = map[string]bool{e.path: true}
					v.Index++ // two rows on top now instead of one
					ui.refreshView(v)
				}},
			}
			if st.selecting {
				it.LongPress = nil
			}
			items = append(items, it)
		}
		return items
	}
	v.refresh = load
	load()
	v.Items = v.Build()
	return v
}

// viewOpen: v is still in one of the stacks.
func (ui *IpodUI) viewOpen(v *View) bool {
	for _, st := range [][]*View{ui.split, ui.full} {
		for _, x := range st {
			if x == v {
				return true
			}
		}
	}
	return false
}

// refreshAllFiles reloads the open Browse Files folders (after a delete the
// folders above have other sizes and counts).
func (ui *IpodUI) refreshAllFiles() {
	for _, v := range ui.full {
		if v.ID == "files" && v.refresh != nil {
			v.refresh()
		}
	}
}

// removeMusicPaths deletes paths (files or folders), only inside the music
// folder; off the UI goroutine. The first error is returned.
func removeMusicPaths(paths []string) error {
	var failed error
	for _, p := range paths {
		if !insideMusic(p) {
			if failed == nil {
				failed = fmt.Errorf("outside the music folder")
			}
			continue
		}
		if err := os.RemoveAll(p); err != nil && failed == nil {
			failed = err
		}
		forgetDirInfo(p) // the folders above it change size
	}
	return failed
}

// dropDeletedFromQueue: songs that were deleted leave the queue at once
// (the one playing goes on: the player has it open). On the UI goroutine.
func (ui *IpodUI) dropDeletedFromQueue(paths []string) {
	ui.app.remapQueueWith(func(t *Track) *Track {
		for _, p := range paths {
			if t.Path == p || strings.HasPrefix(t.Path, p+string(filepath.Separator)) {
				return nil
			}
		}
		return t
	})
}
