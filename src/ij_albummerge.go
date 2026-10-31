package main

// ij_albummerge.go — Album Info → Add to Another Album: songs (one, or a
// whole album) put into another album, when the library sees two albums
// where there is one (another folder, a different spelling, a bonus disc
// tagged apart…).
//
//	Album Info           Add to Album
//	  …                    ⌕ Search
//	  Songs                ▣ Rumours  Fleetwood Mac · 11 songs   (same artist first)
//	  Add to Another… →    ▣ …
//	  Separate Added Songs (when the album has some)
//
// Nothing is written into the files (M4A and WAV take no tags, and the
// songs needn't move): the database keeps, for each song put elsewhere, the
// album it now belongs to (album_merges.json: the song's path → that
// album's key, name and artist, paths relative to the music folder). The
// scan reads the song as usual, then gives it that album (scanTrack →
// applyAlbumMerge), and albumKeyFor groups it there. When the numbers clash
// with the album's own (both have a track 1), the songs come after it as
// the next disc. Separate Added Songs (or Back to Its Own Album, on a song)
// forgets them: they go back to the album of their tags.
//
// When Album Info renames an album, the songs put into it follow
// (retargetAlbumMerges).

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
)

type albumMerge struct {
	Key    string `json:"key"` // the album's key, its folder relative to the music folder
	Album  string `json:"album"`
	Artist string `json:"artist,omitempty"`
	Disc   int    `json:"disc,omitempty"` // the disc it comes as (0: its own)
}

var (
	mergeMu    sync.Mutex
	merges     map[string]albumMerge // song path (relative) → its album
	mergesFrom string                // the database folder they were read from
)

const mergesFile = "album_merges.json"

// mergesLocked: the merges of the database folder in use (read again when
// the folder changes).
func mergesLocked() map[string]albumMerge {
	if d := dataDir(); d != mergesFrom {
		merges = map[string]albumMerge{}
		p := filepath.Join(d, mergesFile)
		if b, err := os.ReadFile(p); err == nil {
			if err := json.Unmarshal(b, &merges); err != nil {
				// (the next save would replace it with what's left: the
				// file is kept aside as it was)
				merges = map[string]albumMerge{}
				logMsg("Album merges: " + p + " can't be read (" + err.Error() + "): kept as " + mergesFile + ".bad")
				_ = writeFileAtomic(p+".bad", b)
			}
		}
		mergesFrom = d
	}
	return merges
}

func saveMergesLocked() {
	m := mergesLocked()
	for rel := range m { // (songs deleted meanwhile)
		if _, err := os.Stat(musicAbs(rel)); os.IsNotExist(err) {
			delete(m, rel)
		}
	}
	p := filepath.Join(mergesFrom, mergesFile)
	if len(m) == 0 {
		os.Remove(p)
		return
	}
	b, _ := json.MarshalIndent(m, "", " ")
	if err := writeFileAtomic(p, b); err != nil {
		logMsg("Album merges: " + err.Error())
	}
}

// musicRel and musicAbs: a path relative to the music folder (as kept),
// and back (paths outside it stay absolute).
func musicRel(p string) string {
	root := filepath.Clean(musicRoot()) + string(filepath.Separator)
	if rel, ok := strings.CutPrefix(filepath.Clean(p), root); ok {
		return rel
	}
	return p
}

func musicAbs(rel string) string {
	if filepath.IsAbs(rel) {
		return rel
	}
	return filepath.Join(musicRoot(), rel)
}

// keyRel and keyAbs: an album key ("<folder>|<album>") with its folder
// relative to the music folder, and back (not cleaned as paths: the album
// part is a name).
func keyRel(key string) string {
	if rel, ok := strings.CutPrefix(key, filepath.Clean(musicRoot())+string(filepath.Separator)); ok {
		return rel
	}
	return key
}

func keyAbs(rel string) string {
	if strings.HasPrefix(rel, string(filepath.Separator)) {
		return rel
	}
	return filepath.Clean(musicRoot()) + string(filepath.Separator) + rel
}

// albumMergeFor: the album a song was put into, if it was.
func albumMergeFor(path string) (albumMerge, bool) {
	mergeMu.Lock()
	defer mergeMu.Unlock()
	m := mergesLocked()
	if len(m) == 0 {
		return albumMerge{}, false
	}
	am, ok := m[musicRel(path)]
	return am, ok
}

// mergedKey: the key of the album a song was put into ("" if none).
func mergedKey(path string) string {
	if am, ok := albumMergeFor(path); ok {
		return keyAbs(am.Key)
	}
	return ""
}

// applyAlbumMerge: a song just read gets the album it was put into (the
// scan, before grouping it).
func applyAlbumMerge(t *Track) {
	am, ok := albumMergeFor(t.Path)
	if !ok {
		return
	}
	t.Album = am.Album
	if am.Artist != "" {
		t.AlbumArtist = am.Artist
	}
	if am.Disc > 0 {
		t.DiscNum = am.Disc
	}
}

// ownAlbumDir: the folder part of a song's own album key (albumKeyFor).
func ownAlbumDir(t *Track) string {
	dir := filepath.Dir(t.Path)
	if discDirRe.MatchString(filepath.Base(dir)) {
		dir = filepath.Dir(dir)
	}
	return dir
}

// ---- putting songs into an album, and taking them out ----

// mergeIntoAlbum puts tracks into the album target, then reads them again
// into the library.
func (ui *IpodUI) mergeIntoAlbum(tracks []*Track, target *Album) {
	own := map[string]bool{}
	var moving []*Track
	for _, t := range albumTracks(target) {
		own[t.Path] = true
	}
	for _, t := range tracks {
		if !own[t.Path] {
			moving = append(moving, t)
		}
	}
	if len(moving) == 0 {
		return
	}
	disc := mergeDisc(albumTracks(target), moving)
	key := keyRel(target.Key)
	mergeMu.Lock()
	m := mergesLocked()
	var paths []string
	for _, t := range moving {
		d := 0
		if disc > 0 {
			d = disc + max(t.DiscNum, 1) - 1
		}
		m[musicRel(t.Path)] = albumMerge{Key: key, Album: target.Name, Artist: target.Artist, Disc: d}
		paths = append(paths, t.Path)
	}
	saveMergesLocked()
	mergeMu.Unlock()
	logMsg(fmt.Sprintf("Album merges: %d songs into %q (%s), disc %d", len(moving), target.Name, key, disc))
	go ui.rereadTracks(paths)
}

// mergeDisc: the disc the songs come as in the album, when their numbers
// clash with the album's own (0: they keep theirs).
func mergeDisc(album, moving []*Track) int {
	type pos struct{ disc, num int }
	have := map[pos]bool{}
	last := 1
	for _, t := range album {
		have[pos{max(t.DiscNum, 1), t.TrackNum}] = true
		last = max(last, t.DiscNum)
	}
	for _, t := range moving {
		if t.TrackNum > 0 && have[pos{max(t.DiscNum, 1), t.TrackNum}] {
			return last + 1
		}
	}
	return 0
}

// separateMerged: the songs go back to the album of their tags.
func (ui *IpodUI) separateMerged(tracks []*Track) {
	mergeMu.Lock()
	m := mergesLocked()
	var paths []string
	for _, t := range tracks {
		if _, ok := m[musicRel(t.Path)]; ok {
			delete(m, musicRel(t.Path))
			paths = append(paths, t.Path)
		}
	}
	saveMergesLocked()
	mergeMu.Unlock()
	if len(paths) > 0 {
		logMsg(fmt.Sprintf("Album merges: %d songs back to their own albums", len(paths)))
		go ui.rereadTracks(paths)
	}
}

// mergedIn: the songs of an album that were put into it.
func mergedIn(al *Album) []*Track {
	var out []*Track
	for _, t := range albumTracks(al) {
		if _, ok := albumMergeFor(t.Path); ok {
			out = append(out, t)
		}
	}
	return out
}

// retargetAlbumMerges: an album renamed by Album Info (its key was oldKey,
// one of its own songs is t, with its new tags): the songs put into it
// follow it.
func retargetAlbumMerges(oldKey string, t *Track, name, artist string) {
	newKey := ownAlbumDir(t) + "|" + strings.ToLower(strings.TrimSpace(name))
	mergeMu.Lock()
	defer mergeMu.Unlock()
	m := mergesLocked()
	oldRel, changed := keyRel(oldKey), false
	for rel, am := range m {
		if am.Key == oldRel {
			am.Key, am.Album = keyRel(newKey), name
			if artist != "" {
				am.Artist = artist
			}
			m[rel] = am
			changed = true
		}
	}
	if changed {
		saveMergesLocked()
	}
}

// albumRenamed: Album Info gave all of al's own songs (t is one of them)
// the album name and artist. The songs put into it follow it, and so does
// its chosen cover (both are kept by the album's key, which has its name).
// It returns the songs put into it, to be read again with the others.
func albumRenamed(al *Album, t *Track, name, artist string) []string {
	var paths []string
	for _, m := range mergedIn(al) {
		paths = append(paths, m.Path)
	}
	retargetAlbumMerges(al.Key, t, name, artist)
	newKey := ownAlbumDir(t) + "|" + strings.ToLower(strings.TrimSpace(name))
	if newKey != al.Key {
		moveChosenCover(al.Key, newKey)
		logMsg(fmt.Sprintf("Album Info: %q renamed %q: %d songs put into it follow", al.Name, name, len(paths)))
	}
	return paths
}

// rereadTracks reads songs again into the library (their album may have
// changed), without scanning the whole music folder: as removeFromLibrary
// without them, then the scan's scanTrack on each. Background goroutine.
func (ui *IpodUI) rereadTracks(paths []string) {
	app := ui.app
	scanStartMu.Lock()
	if app.LibScanRunning || dbBusy {
		scanStartMu.Unlock()
		ui.post(ui.rescanLibrary)
		return
	}
	app.LibScanRunning = true
	scanStartMu.Unlock()
	defer func() { app.LibScanRunning = false }()
	old := app.Library
	if old == nil {
		return
	}
	again := map[string]bool{}
	for _, p := range paths {
		again[p] = true
	}
	lib := &Library{
		Playlists:     old.Playlists,
		TracksByPath:  make(map[string]*Track, len(old.Tracks)),
		AlbumsByKey:   make(map[string]*Album, len(old.Albums)+2),
		ArtistsByName: make(map[string]*Artist),
	}
	for _, t := range old.Tracks {
		if !again[t.Path] {
			lib.Tracks = append(lib.Tracks, t)
			lib.TracksByPath[t.Path] = t
		}
	}
	for _, al := range old.Albums {
		var keep []*Track
		for _, t := range al.Tracks {
			if !again[t.Path] {
				keep = append(keep, t)
			}
		}
		if len(keep) == 0 {
			continue
		}
		c := *al // (a copy: the old library keeps its own until it's replaced)
		c.Tracks = keep
		lib.Albums = append(lib.Albums, &c)
		lib.AlbumsByKey[c.Key] = &c
	}
	scanLib = lib
	for _, p := range paths {
		app.scanTrack(p)
	}
	finishAlbums(lib)
	touched := map[string]bool{}
	for _, p := range paths {
		if t := lib.TracksByPath[p]; t != nil {
			touched[albumKeyFor(t)] = true
		}
	}
	for _, al := range lib.Albums {
		if !touched[al.Key] {
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
	sort.Slice(lib.Tracks, func(i, j int) bool {
		return strings.ToLower(lib.Tracks[i].Title) < strings.ToLower(lib.Tracks[j].Title)
	})
	sort.Slice(lib.Albums, func(i, j int) bool {
		return strings.ToLower(lib.Albums[i].Name) < strings.ToLower(lib.Albums[j].Name)
	})
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
	ui.postSure(func() {
		// the playlists show the songs read again, not the old ones
		for _, pl := range lib.Playlists {
			for i, t := range pl.Tracks {
				if nt := lib.TracksByPath[t.Path]; nt != nil && again[t.Path] {
					pl.Tracks[i] = nt
				}
			}
		}
		ui.kb.initialized = false
		ui.refreshBuilt()
	})
	// a song back in its own album may bring that album back: its cover
	go app.decodeAlbumArt()
	go app.deferredArtExtraction()
}

// ---- the screens ----

// addToAlbumView: the albums tracks can go into (not their own), the
// likeliest first: the same artist, or a name that starts the same way.
// done runs once they're in (Album Info closes its screens).
func (ui *IpodUI) addToAlbumView(tracks []*Track, from *Album, what string, done func()) *View {
	v := &View{ID: "addToAlbum", Title: "Add to Album", Kind: KindFull, Empty: "No other albums"}
	st := &struct{ term string }{}
	artist := ""
	if from != nil {
		artist = strings.ToLower(from.Artist)
	}
	stem := func(s string) string { // "Album (Disc 2)", "Album CD2" → "album"
		s = strings.ToLower(s)
		for _, sep := range []string{" (", " [", " - ", " cd", " disc", " disk", ":"} {
			if i := strings.Index(s, sep); i > 0 {
				s = s[:i]
			}
		}
		return strings.TrimSpace(s)
	}
	fromStem := ""
	if from != nil {
		fromStem = stem(from.Name)
	}
	v.Build = func() []Item {
		edit := func() {
			ui.showKeyboard(&st.term, func() { ui.refreshView(v) }, func() { ui.refreshView(v) })
			ui.refreshView(v)
		}
		var near, rest []*Album
		term := strings.ToLower(strings.TrimSpace(st.term))
		for _, al := range ui.library().Albums {
			if from != nil && al.Key == from.Key {
				continue
			}
			if term != "" && !strings.Contains(strings.ToLower(al.Name+" "+al.Artist), term) {
				continue
			}
			if (artist != "" && usableArtist(from.Artist) && strings.ToLower(al.Artist) == artist) || (fromStem != "" && stem(al.Name) == fromStem) {
				near = append(near, al)
			} else {
				rest = append(rest, al)
			}
		}
		note := ""
		if term != "" {
			n := len(near) + len(rest)
			note = fmt.Sprintf("%d %s", n, pluralize(n, "album", "albums"))
		}
		items := []Item{ui.searchField(&st.term, "Search albums", note, edit, []SheetOption{
			{Label: "Clear search", OnSelect: func() { st.term = ""; ui.refreshView(v) }},
		})}
		for _, al := range append(near, rest...) {
			n := len(albumTracks(al))
			sub := fmt.Sprintf("%d %s", n, pluralize(n, "song", "songs"))
			if al.Artist != "" {
				sub = al.Artist + " · " + sub
			}
			items = append(items, Item{Label: al.Name, Sublabel: sub, Album: al, OnSelect: func() {
				// (the sheet's title is one line: the disc goes in the button)
				label := "Add to Album"
				if d := mergeDisc(albumTracks(al), tracks); d > 0 {
					label = fmt.Sprintf("Add as Disc %d", d) // (their numbers are taken)
				}
				ui.showSheetTitled(fmt.Sprintf("Add %s to %s?", what, al.Name), []SheetOption{{Label: label, OnSelect: func() {
					ui.mergeIntoAlbum(tracks, al)
					if done != nil {
						done()
					}
					ui.showToast("Added to " + al.Name)
				}}})
			}})
		}
		return items
	}
	v.Items = v.Build()
	return v
}

// mergeItems: Album Info's rows (album: the songs of the album; one song:
// Song Info's), to put them into another album or take them out again.
func (ui *IpodUI) mergeItems(e *albumEdit, song *songEdit, closeAll func()) []Item {
	var tracks []*Track
	what := ""
	if song != nil {
		tracks = []*Track{song.t}
		what = "“" + song.t.Title + "”"
	} else {
		for _, s := range e.songs {
			tracks = append(tracks, s.t)
		}
		what = fmt.Sprintf("%d %s", len(tracks), pluralize(len(tracks), "song", "songs"))
		if len(tracks) == 1 {
			what = "“" + tracks[0].Title + "”"
		}
	}
	sub := "Merge it with an album seen as another one"
	if song != nil {
		sub = "Put this song in another album"
	}
	items := []Item{{Label: "Add to Another Album", Sublabel: sub, OnSelect: func() {
		var pv *View
		pv = ui.addToAlbumView(tracks, e.al, what, func() {
			e.views[pv] = true
			closeAll()
		})
		ui.showView(pv, "")
	}}}
	if song != nil {
		if _, ok := albumMergeFor(song.t.Path); ok {
			items = append(items, Item{Label: "Back to Its Own Album", Sublabel: "It was added to this album", NoArrow: true, OnSelect: func() {
				ui.separateMerged(tracks)
				closeAll()
				ui.showToast("Back in its own album")
			}})
		}
		return items
	}
	if in := mergedIn(e.al); len(in) > 0 {
		n := len(in)
		items = append(items, Item{Label: "Separate Added Songs",
			Sublabel: fmt.Sprintf("%d %s added from other albums", n, pluralize(n, "song", "songs")), OnSelect: func() {
				ui.showSheetTitled(fmt.Sprintf("Send %d %s back to %s own %s?", n, pluralize(n, "song", "songs"),
					pluralize(n, "its", "their"), pluralize(n, "album", "albums")),
					[]SheetOption{{Label: "Separate", OnSelect: func() {
						ui.separateMerged(in)
						closeAll()
						ui.showToast("Back in their own albums")
					}}})
			}})
	}
	return items
}
