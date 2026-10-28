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
)

type fileEntry struct {
	name  string
	path  string
	dir   bool
	size  int64 // files: their size; folders: filled in the background (-1 until then)
	items int   // folders: entries directly inside
}

// folder sizes, computed in the background and kept for the session
var (
	dirSizeMu sync.Mutex
	dirSizes  = map[string]int64{}
)

func listFolder(dir string) ([]fileEntry, error) {
	des, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}
	var out []fileEntry
	for _, de := range des {
		e := fileEntry{name: de.Name(), path: filepath.Join(dir, de.Name()), size: -1}
		info, err := os.Stat(e.path) // follows links
		if err != nil {
			info, _ = de.Info()
		}
		if info != nil && info.IsDir() {
			e.dir = true
			if sub, err := os.ReadDir(e.path); err == nil {
				e.items = len(sub)
			}
			dirSizeMu.Lock()
			if s, ok := dirSizes[e.path]; ok {
				e.size = s
			}
			dirSizeMu.Unlock()
		} else if info != nil {
			e.size = info.Size()
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

// insideMusic: p is in the music folder (and is not the folder itself).
func insideMusic(p string) bool {
	root := filepath.Clean(MUSIC_ROOT)
	p = filepath.Clean(p)
	return p != root && strings.HasPrefix(p, root+string(filepath.Separator))
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
	}{picked: map[string]bool{}}
	v := &View{ID: "files", Title: title, Kind: KindFull, Empty: "Empty folder"}
	var load func()
	var del func(paths []string, what string)

	load = func() {
		es, err := listFolder(dir)
		st.entries, st.err = es, ""
		if err != nil {
			st.err = "Couldn't open: " + shortErr(err)
		}
		// folder sizes in the background
		var todo []string
		for _, e := range es {
			if e.dir && e.size < 0 {
				todo = append(todo, e.path)
			}
		}
		if len(todo) > 0 {
			go func() {
				for _, p := range todo {
					n := dirSize(p)
					dirSizeMu.Lock()
					dirSizes[p] = n
					dirSizeMu.Unlock()
				}
				ui.post(func() {
					for i := range st.entries {
						dirSizeMu.Lock()
						if s, ok := dirSizes[st.entries[i].path]; ok {
							st.entries[i].size = s
						}
						dirSizeMu.Unlock()
					}
					ui.refreshView(v)
				})
			}()
		}
	}

	del = func(paths []string, what string) {
		ui.showSheetTitled("Delete "+what+"? This can't be undone.", []SheetOption{{Label: "Delete", Danger: true, OnSelect: func() {
			st.busy = "Deleting…"
			ui.refreshView(v)
			go func() {
				var failed error
				for _, p := range paths {
					if !insideMusic(p) {
						continue
					}
					if err := os.RemoveAll(p); err != nil && failed == nil {
						failed = err
					}
					// the sizes of the folders above it change
					dirSizeMu.Lock()
					for k := range dirSizes {
						if strings.HasPrefix(p, k+string(filepath.Separator)) || k == p || strings.HasPrefix(k, p+string(filepath.Separator)) {
							delete(dirSizes, k)
						}
					}
					dirSizeMu.Unlock()
				}
				ui.post(func() {
					st.busy = ""
					st.selecting = false
					st.picked = map[string]bool{}
					load()
					ui.refreshView(v)
					if len(v.Items) > 1 {
						v.Index = 1
					}
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
			sub := "Press A on the items to select them"
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
				ui.refreshView(v)
			}})
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
				it.Sublabel = fmt.Sprintf("Folder · %d %s", e.items, pluralize(e.items, "item", "items"))
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
	load()
	v.Items = v.Build()
	if len(v.Items) > 1 {
		v.Index = 1
	}
	return v
}
