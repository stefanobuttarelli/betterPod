package main

// ij_libinfo.go — album grouping, scan statistics and the Settings → Library
// screen (music folder, counts, sizes, last scan, Rescan now).

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"sync"
	"syscall"
	"time"
)

// ---- album grouping ----

var discDirRe = regexp.MustCompile(`(?i)^(cd|disc|disk|disco)[ _.\-]*\d+\b`)

// albumKeyFor groups tracks by folder + album name, so an album stays whole
// even when its tracks carry different artists (features, compilations,
// missing "album artist" tags). Per-disc subfolders (CD1, Disc 2…) count as
// their parent folder.
func albumKeyFor(t *Track) string {
	dir := filepath.Dir(t.Path)
	if discDirRe.MatchString(filepath.Base(dir)) {
		dir = filepath.Dir(dir)
	}
	return dir + "|" + strings.ToLower(strings.TrimSpace(t.Album))
}

// finishAlbums picks each album's artist and rebuilds the artist index.
func finishAlbums(lib *Library) {
	for _, al := range lib.Albums {
		counts := map[string]int{}
		aa := ""
		for _, t := range al.Tracks {
			if t.AlbumArtist != "" && aa == "" {
				aa = t.AlbumArtist
			}
			counts[t.Artist]++
		}
		if aa != "" {
			al.Artist = aa
			continue
		}
		best, bestN := "", 0
		for a, n := range counts {
			if n > bestN || (n == bestN && a < best) {
				best, bestN = a, n
			}
		}
		if len(counts) > 1 && bestN*2 < len(al.Tracks) {
			best = "Various Artists"
		}
		if best != "" {
			al.Artist = best
		}
	}
	lib.Artists = nil
	lib.ArtistsByName = make(map[string]*Artist)
	for _, al := range lib.Albums {
		ar, ok := lib.ArtistsByName[al.Artist]
		if !ok {
			ar = &Artist{Name: al.Artist}
			lib.ArtistsByName[al.Artist] = ar
			lib.Artists = append(lib.Artists, ar)
		}
		ar.Albums = append(ar.Albums, al)
	}
}

// ---- scan statistics ----

type ScanStats struct {
	Root       string         `json:"root"`
	When       time.Time      `json:"when"`
	Took       float64        `json:"took"` // seconds
	Files      int            `json:"files"`
	Folders    int            `json:"folders"`
	Audio      int            `json:"audio"`
	AudioBytes int64          `json:"audioBytes"`
	ByFormat   map[string]int `json:"byFormat"`
	Skipped    map[string]int `json:"skipped"` // audio we can't play (AAC, Opus…)
	Tracks     int            `json:"tracks"`
	Albums     int            `json:"albums"`
	Artists    int            `json:"artists"`
	Playlists  int            `json:"playlists"`
	TagErrors  int            `json:"tagErrors"`
	// every folder the scan walked into or skipped, with what it holds
	FolderList []ScanFolder `json:"folderList,omitempty"`

	folderIdx map[string]int // path → index in FolderList (during the scan)
}

// ScanFolder is one folder of the last scan (Settings → Library → Scanned
// Folders): its path under the music folder, the songs and other files
// directly in it, or why it was skipped.
type ScanFolder struct {
	Path  string `json:"path"`
	Audio int    `json:"audio,omitempty"`
	Other int    `json:"other,omitempty"`
	Note  string `json:"note,omitempty"`
}

// maxScanFolders caps the list (a huge library still gets its counts).
const maxScanFolders = 5000

// scanFolder returns the record of dir, adding it (with note) if new.
func (s *ScanStats) scanFolder(dir, note string) *ScanFolder {
	if s.folderIdx == nil {
		s.folderIdx = map[string]int{}
	}
	if i, ok := s.folderIdx[dir]; ok {
		f := &s.FolderList[i]
		if note != "" {
			f.Note = note
		}
		return f
	}
	if len(s.FolderList) >= maxScanFolders {
		return nil
	}
	s.FolderList = append(s.FolderList, ScanFolder{Path: dir, Note: note})
	s.folderIdx[dir] = len(s.FolderList) - 1
	return &s.FolderList[len(s.FolderList)-1]
}

var (
	scanStatsMu  sync.Mutex
	scanStats    *ScanStats // last completed scan
	scanProgress *ScanStats // scan in progress
)

// other audio formats worth reporting as "not supported"
var unsupportedAudio = map[string]string{
	".aac": "AAC", ".opus": "Opus", ".wma": "WMA", ".ape": "APE", ".wv": "WavPack",
	".dsf": "DSD", ".dff": "DSD", ".mpc": "Musepack", ".ac3": "AC3", ".mka": "MKA",
	".m4b": "Audiobook", ".oga": "OGA", ".amr": "AMR",
}

func formatName(ext string) string {
	switch ext {
	case ".m4a", ".mp4":
		return "ALAC"
	case ".aif", ".aiff":
		return "AIFF"
	}
	return strings.ToUpper(strings.TrimPrefix(ext, "."))
}

func scanStatsPath() string { return dataPath("scan_stats.json") }

func loadScanStats() {
	scanStatsMu.Lock()
	defer scanStatsMu.Unlock()
	if scanStats != nil {
		return
	}
	data, err := os.ReadFile(scanStatsPath())
	if err != nil {
		return
	}
	var s ScanStats
	if json.Unmarshal(data, &s) == nil {
		scanStats = &s
	}
}

func scanBegin() *ScanStats {
	s := &ScanStats{Root: MUSIC_ROOT, When: time.Now(), ByFormat: map[string]int{}, Skipped: map[string]int{}}
	scanStatsMu.Lock()
	scanProgress = s
	scanStatsMu.Unlock()
	return s
}

// scanNote records one walked entry (called from the scan goroutine).
func scanNote(fn func(s *ScanStats)) {
	scanStatsMu.Lock()
	if scanProgress != nil {
		fn(scanProgress)
	}
	scanStatsMu.Unlock()
}

func scanEnd(lib *Library, start time.Time) {
	scanStatsMu.Lock()
	s := scanProgress
	scanProgress = nil
	if s != nil {
		s.Took = time.Since(start).Seconds()
		s.Tracks, s.Albums, s.Artists, s.Playlists = len(lib.Tracks), len(lib.Albums), len(lib.Artists), len(lib.Playlists)
		scanStats = s
	}
	scanStatsMu.Unlock()
	if s != nil {
		if data, err := json.Marshal(s); err == nil {
			_ = os.WriteFile(scanStatsPath(), data, 0644)
		}
		logMsg(fmt.Sprintf("Scan: %d files, %d audio (%s), skipped %v", s.Files, s.Audio, humanBytes(s.AudioBytes), s.Skipped))
	}
}

func humanBytes(n int64) string {
	switch {
	case n >= 1<<30:
		return fmt.Sprintf("%.1f GB", float64(n)/(1<<30))
	case n >= 1<<20:
		return fmt.Sprintf("%.0f MB", float64(n)/(1<<20))
	case n >= 1<<10:
		return fmt.Sprintf("%.0f KB", float64(n)/(1<<10))
	}
	return fmt.Sprintf("%d B", n)
}

func diskSpace(path string) (free, total int64, ok bool) {
	var st syscall.Statfs_t
	if syscall.Statfs(path, &st) != nil {
		return 0, 0, false
	}
	return int64(st.Bavail) * int64(st.Bsize), int64(st.Blocks) * int64(st.Bsize), true
}

func dirSize(dir string) int64 {
	var n int64
	filepath.Walk(dir, func(_ string, info os.FileInfo, err error) error {
		if err == nil && !info.IsDir() {
			n += info.Size()
		}
		return nil
	})
	return n
}

func formatList(m map[string]int) string {
	type kv struct {
		k string
		v int
	}
	var l []kv
	for k, v := range m {
		l = append(l, kv{k, v})
	}
	sort.Slice(l, func(i, j int) bool { return l[i].v > l[j].v || (l[i].v == l[j].v && l[i].k < l[j].k) })
	var parts []string
	for _, e := range l {
		parts = append(parts, fmt.Sprintf("%s %d", e.k, e.v))
	}
	return strings.Join(parts, " · ")
}

// ---- Settings → Library ----

var dataSizeCache struct {
	at   time.Time
	size int64
}

func (ui *IpodUI) libraryInfoView() *View {
	loadScanStats()
	v := &View{ID: "libraryInfo", Title: "Library", Kind: KindFull}
	v.Build = func() []Item {
		app := ui.app
		info := func(label, value string) Item { return Item{Label: label, Value: value, NoArrow: true} }
		var items []Item
		if app.LibScanRunning {
			scanStatsMu.Lock()
			n := 0
			if scanProgress != nil {
				n = scanProgress.Audio
			}
			scanStatsMu.Unlock()
			items = append(items, Item{Label: "Scanning…", Value: fmt.Sprintf("%d songs", n), NoArrow: true})
			folder := strings.TrimPrefix(app.LibScanFolder, MUSIC_ROOT)
			if folder == "" {
				folder = "/"
			}
			items = append(items, Item{Label: "Folder", Sublabel: folder, NoArrow: true})
		} else {
			items = append(items, Item{Label: "Rescan now", OnSelect: func() { ui.rescanWithProgress(v) }})
			n := len(ui.missingCovers())
			sub := "Every album has a cover"
			if n > 0 {
				sub = fmt.Sprintf("%d %s without a cover", n, pluralize(n, "album", "albums"))
			}
			items = append(items, Item{Label: "Missing Covers", Sublabel: sub, OnSelect: func() { ui.showView(ui.missingCoversView(), "") }})
			items = append(items, ui.missingInfoItem())
			items = append(items, Item{Label: "Browse Files", Sublabel: "See and delete files and folders", OnSelect: func() { ui.showView(ui.filesView(MUSIC_ROOT), "") }})
			scanStatsMu.Lock()
			st := scanStats
			scanStatsMu.Unlock()
			if st != nil && len(st.FolderList) > 0 {
				withMusic := 0
				for _, f := range st.FolderList {
					if f.Audio > 0 {
						withMusic++
					}
				}
				items = append(items, Item{Label: "Scanned Folders",
					Sublabel: fmt.Sprintf("%d %s · %d with music", len(st.FolderList), pluralize(len(st.FolderList), "folder", "folders"), withMusic),
					OnSelect: func() { ui.showView(ui.scannedFoldersView(), "") }})
			}
		}
		items = append(items, Item{Label: "Music Folder", Sublabel: strings.TrimRight(MUSIC_ROOT, "/") + " · move it to another card",
			OnSelect: func() { ui.showView(ui.musicFolderView(), "") }})
		lib := ui.library()
		items = append(items,
			info("Songs", fmt.Sprint(len(lib.Tracks))),
			info("Albums", fmt.Sprint(len(lib.Albums))),
			info("Artists", fmt.Sprint(len(lib.Artists))),
			info("Playlists", fmt.Sprint(len(lib.Playlists))),
		)
		scanStatsMu.Lock()
		s := scanStats
		scanStatsMu.Unlock()
		if s != nil {
			items = append(items,
				info("Last scan", s.When.Format("02 Jan 2006, 15:04")),
				info("Scan time", fmt.Sprintf("%.1f s", s.Took)),
				info("Files in folder", fmt.Sprint(s.Files)),
				info("Audio files", fmt.Sprint(s.Audio)),
			)
			if len(s.ByFormat) > 0 {
				items = append(items, Item{Label: "Formats", Sublabel: formatList(s.ByFormat), NoArrow: true})
			}
			skipped := 0
			for _, n := range s.Skipped {
				skipped += n
			}
			items = append(items, info("Not supported", fmt.Sprint(skipped)))
			if skipped > 0 {
				items = append(items, Item{Label: "Skipped formats", Sublabel: formatList(s.Skipped), NoArrow: true})
			}
			items = append(items, info("Music size", humanBytes(s.AudioBytes)))
		}
		if free, total, ok := diskSpace(MUSIC_ROOT); ok {
			items = append(items, info("Free space", humanBytes(free)+" of "+humanBytes(total)))
		}
		if time.Since(dataSizeCache.at) > 10*time.Second {
			dataSizeCache.size = databaseSize(dataDir())
			dataSizeCache.at = time.Now()
		}
		items = append(items, info("Library data", humanBytes(dataSizeCache.size)))
		return items
	}
	v.Items = v.Build()
	return v
}

// rescanWithProgress rescans and keeps the Library screen updated.
func (ui *IpodUI) rescanWithProgress(v *View) {
	if ui.app.LibScanRunning {
		return
	}
	ui.rescanLibrary()
	go func() {
		time.Sleep(50 * time.Millisecond)
		for ui.app.LibScanRunning {
			ui.post(func() { ui.refreshView(v) })
			time.Sleep(300 * time.Millisecond)
		}
		time.Sleep(100 * time.Millisecond)
		ui.postSure(func() {
			dataSizeCache.at = time.Time{}
			ui.refreshView(v)
		})
	}()
}

func (ui *IpodUI) refreshView(v *View) {
	if v.Build == nil {
		return
	}
	idx := v.Index
	v.Items = v.Build()
	if idx >= len(v.Items) {
		idx = len(v.Items) - 1
	}
	if idx < 0 {
		idx = 0
	}
	v.Index = idx
	v.dirty = true
	ui.dirty = true
}

// scannedFoldersView lists the folders of the last scan: songs and other
// files in each, and the folders skipped (hidden, system, links, unreadable)
// with the reason. For finding out why music doesn't show up.
func (ui *IpodUI) scannedFoldersView() *View {
	v := &View{ID: "scannedFolders", Title: "Scanned Folders", Kind: KindFull, Empty: "No scan yet"}
	scanStatsMu.Lock()
	st := scanStats
	var list []ScanFolder
	root := MUSIC_ROOT
	if st != nil {
		list = append(list, st.FolderList...)
		if st.Root != "" {
			root = st.Root
		}
	}
	scanStatsMu.Unlock()
	sort.SliceStable(list, func(i, j int) bool { return strings.ToLower(list[i].Path) < strings.ToLower(list[j].Path) })
	hasSub := map[string]bool{}
	for _, f := range list {
		hasSub[filepath.Dir(f.Path)] = true
	}
	for _, f := range list {
		label := strings.TrimPrefix(strings.TrimPrefix(f.Path, strings.TrimRight(root, "/")), "/")
		if label == "" {
			label = "(music folder)"
		}
		it := Item{Label: label, NoArrow: true}
		if f.Audio > 0 {
			it.Value = fmt.Sprintf("%d %s", f.Audio, pluralize(f.Audio, "song", "songs"))
		}
		switch {
		case f.Note != "":
			it.Sublabel = f.Note
		case f.Other > 0:
			it.Sublabel = fmt.Sprintf("%d other %s", f.Other, pluralize(f.Other, "file", "files"))
		case f.Audio == 0 && hasSub[f.Path]:
			it.Sublabel = "Only subfolders"
		case f.Audio == 0:
			it.Sublabel = "Empty"
		}
		v.Items = append(v.Items, it)
	}
	if st != nil && len(st.FolderList) >= maxScanFolders {
		v.Items = append(v.Items, Item{Label: fmt.Sprintf("(only the first %d folders are listed)", maxScanFolders), NoArrow: true})
	}
	return v
}
