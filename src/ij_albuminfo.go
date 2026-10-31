package main

// ij_albuminfo.go — Album Info: an album's tags, filled in from the
// internet or by hand, and written into its files.
//
//	Album Info                   Find Info Online
//	  Find Info Online             ⌕ Edit search
//	  Album       Colin McRae…     ▣ Colin McRae Rally 2.0 — Codemasters (developer) · 2000 · Wikipedia
//	  Album Artist  —              ▣ Colin McRae Rally — Jonathan Colling · 12 songs · iTunes
//	  Artist      Missing
//	  Year, Genre, Songs
//	  Save Changes
//
// It opens from an album's options (hold A, or Y) and from Settings →
// Library → Missing Info, the albums without an artist or track numbers.
// Find Info Online asks MusicBrainz, iTunes, Deezer and Wikipedia
// (ij_infosources.go) at once, with the checks of the cover search first;
// choosing a result fills in the album and pairs its songs with ours
// (matchTracks). Nothing is written until Save Changes: then the tags go
// into the MP3, FLAC and Ogg files (tagwrite.go) and the library is
// scanned again. Every field can also be typed with the keyboard (START
// keeps it, holding B leaves it as it was).

import (
	"errors"
	"fmt"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"syscall"
)

type songEdit struct {
	t          *Track
	tags, orig songTags
}

type albumEdit struct {
	al     *Album
	songs  []*songEdit
	status string         // what the last search or save did
	saving string         // progress while saving
	views  map[*View]bool // its screens (Album Info, Songs, Song Info): closed after a save
}

// albumSaving: the albums whose tags are being written (UI goroutine). A
// second Album Info on one of them would start from the tags before the
// save, and write them back.
var albumSaving = map[*Album]bool{}

// infoText: a value from a source, made fit for a tag: no control
// characters, at most 200 characters.
func infoText(s string) string {
	s = strings.Map(func(r rune) rune {
		if r < 0x20 || r == 0x7f {
			return ' '
		}
		return r
	}, s)
	s = strings.Join(strings.Fields(s), " ")
	if r := []rune(s); len(r) > 200 {
		s = string(r[:200])
	}
	return s
}

func tagsOf(t *Track) songTags {
	clean := func(s, placeholder string) string {
		if s == placeholder {
			return ""
		}
		return s
	}
	year := t.Year
	if year < 1000 || year > 9999 {
		year = 0 // a date the reader didn't understand ("2000/05/12" reads as 1): not shown, not written
	}
	return songTags{Title: t.Title, Artist: clean(t.Artist, "Unknown Artist"), Album: clean(t.Album, "Unknown Album"),
		AlbumArtist: t.AlbumArtist, Genre: t.Genre, Track: max(t.TrackNum, 0), TrackTotal: max(t.TrackTotal, 0), Disc: max(t.DiscNum, 0), Year: year}
}

func newAlbumEdit(al *Album) *albumEdit {
	e := &albumEdit{al: al, views: map[*View]bool{}}
	for _, t := range albumTracks(al) {
		g := tagsOf(t)
		e.songs = append(e.songs, &songEdit{t: t, tags: g, orig: g})
	}
	return e
}

// common: the value the songs share ("" and false when they differ).
func (e *albumEdit) common(get func(*songTags) string) (string, bool) {
	v, same := "", true
	for i, s := range e.songs {
		x := get(&s.tags)
		if i == 0 {
			v = x
		} else if x != v {
			same = false
		}
	}
	return v, same
}

func (e *albumEdit) setAll(set func(*songTags)) {
	for _, s := range e.songs {
		set(&s.tags)
	}
}

func (e *albumEdit) changed() (n, writable int) {
	for _, s := range e.songs {
		if s.tags != s.orig {
			n++
			if canWriteTags(s.t.Path) {
				writable++
			}
		}
	}
	return
}

// query: what the album is searched for, with its edited title.
func (e *albumEdit) query() albumQuery {
	q := newAlbumQuery(e.al)
	if t, _ := e.common(func(g *songTags) string { return g.Album }); t != "" {
		q.title = t
	}
	if a, _ := e.common(func(g *songTags) string { return g.AlbumArtist }); usableArtist(a) {
		q.artists = append([]string{a}, q.artists...)
	}
	return q
}

// apply fills the album in from a result and returns how many of its
// songs were paired with the result's.
func (e *albumEdit) apply(a *albumInfo) int {
	ours := make([]songTags, len(e.songs))
	secs := make([]float64, len(e.songs))
	for i, s := range e.songs {
		ours[i], secs[i] = s.tags, s.t.Duration
	}
	m := matchTracks(ours, secs, a.Tracks)
	a.Title, a.Artist, a.Genre = infoText(a.Title), infoText(a.Artist), infoText(a.Genre)
	cur, _ := e.common(func(g *songTags) string { return g.Album })
	keepTitle := cur != "" && sameTitle(cur, a.Title) // "Rumours" stays "Rumours", not "Rumours (Deluxe)"
	discs, perDisc := 0, map[int]int{}
	for _, t := range a.Tracks {
		discs = max(discs, t.Disc)
		perDisc[max(t.Disc, 1)]++
	}
	year, _ := strconv.Atoi(a.Year)
	n := 0
	for i, s := range e.songs {
		g := &s.tags
		if !keepTitle && a.Title != "" {
			g.Album = a.Title
		}
		if a.Artist != "" {
			g.AlbumArtist = a.Artist
		}
		if year > 0 {
			g.Year = year
		}
		if a.Genre != "" {
			g.Genre = a.Genre
		}
		if j := m[i]; j >= 0 {
			x := a.Tracks[j]
			x.Title, x.Artist, x.Num, x.Disc = infoText(x.Title), infoText(x.Artist), max(x.Num, 0), max(x.Disc, 0)
			n++
			if x.Title != "" && !sameTitle(g.Title, x.Title) { // ours stays when only an edition note differs
				g.Title = x.Title
			}
			if x.Artist != "" {
				g.Artist = x.Artist
			}
			if x.Num > 0 {
				g.Track, g.TrackTotal = x.Num, perDisc[max(x.Disc, 1)]
			}
			if discs > 1 {
				g.Disc = x.Disc
			}
		}
		if g.Artist == "" {
			g.Artist = g.AlbumArtist
		}
	}
	return n
}

// ---- entry points ----

// albumOptions: an album's long-press sheet, with Edit Album Info.
func (ui *IpodUI) albumOptions(al *Album) []SheetOption {
	return append(ui.mediaOptions(albumTracks(al)),
		SheetOption{Label: "Edit Album Info", OnSelect: func() { ui.openAlbumInfo(al) }},
		SheetOption{Label: "Delete Album", Danger: true, OnSelect: func() { ui.askDeleteAlbum(al) }})
}

// openAlbumInfo opens Album Info (not while the album is being saved).
func (ui *IpodUI) openAlbumInfo(al *Album) {
	if albumSaving[al] {
		ui.showToast("Still saving this album")
		return
	}
	ui.showView(ui.albumInfoView(al), "")
}

// variousArtists: a compilation's album artist (not missing).
func variousArtists(a string) bool {
	switch strings.ToLower(strings.TrimSpace(a)) {
	case "various artists", "various", "va":
		return true
	}
	return false
}

// missingInfo: albums without an artist, or with songs without one or
// without a track number.
func (ui *IpodUI) missingInfo() []*Album {
	var out []*Album
	for _, a := range ui.library().Albums {
		bad := !usableArtist(a.Artist) && !variousArtists(a.Artist)
		for _, t := range a.Tracks {
			if t != nil && (!usableArtist(t.Artist) || t.TrackNum == 0) {
				bad = true
			}
		}
		if bad {
			out = append(out, a)
		}
	}
	sort.SliceStable(out, func(i, j int) bool { return strings.ToLower(out[i].Name) < strings.ToLower(out[j].Name) })
	return out
}

// missingInfoItem: the row of Settings → Library.
func (ui *IpodUI) missingInfoItem() Item {
	sub := "Every album has its artist and track numbers"
	if n := len(ui.missingInfo()); n > 0 {
		sub = fmt.Sprintf("%d %s without an artist or track numbers", n, pluralize(n, "album", "albums"))
	}
	return Item{Label: "Missing Info", Sublabel: sub, OnSelect: func() { ui.showView(ui.missingInfoView(), "") }}
}

func (ui *IpodUI) missingInfoView() *View {
	v := &View{ID: "missingInfo", Title: "Missing Info", Kind: KindFull}
	v.Build = func() []Item {
		albums := ui.missingInfo()
		if len(albums) == 0 {
			return []Item{{Label: "Every album has its artist and track numbers", NoArrow: true}}
		}
		var items []Item
		for _, a := range albums {
			var miss []string
			noArtist, noNum := !usableArtist(a.Artist) && !variousArtists(a.Artist), 0
			for _, t := range a.Tracks {
				if t != nil && t.TrackNum == 0 {
					noNum++
				}
				if t != nil && !usableArtist(t.Artist) {
					noArtist = true
				}
			}
			if noArtist {
				miss = append(miss, "no artist")
			}
			if noNum > 0 {
				miss = append(miss, fmt.Sprintf("%d without a number", noNum))
			}
			items = append(items, Item{Label: a.Name, Sublabel: strings.Join(miss, " · "), Album: a,
				OnSelect: func() { ui.openAlbumInfo(a) }})
		}
		return items
	}
	v.Items = v.Build()
	return v
}

// ---- Album Info ----

func (ui *IpodUI) albumInfoView(al *Album) *View {
	e := newAlbumEdit(al)
	v := &View{ID: "albumInfo", Title: "Album Info", Kind: KindFull}
	e.views[v] = true
	st := &struct{ text, field string }{}
	// edit: the keyboard on a field; START keeps it
	edit := func(field, cur string, set func(string)) {
		st.field, st.text = field, cur
		ui.showKeyboard(&st.text, func() { ui.refreshView(v) }, func() {
			set(strings.TrimSpace(st.text))
			st.field = ""
			ui.refreshView(v)
		})
		ui.refreshView(v)
	}
	shown := func(field, val string) string {
		if st.field == field && ui.keyboardEditing(&st.text) {
			return ui.withCaret(&st.text)
		}
		if val == "" {
			return "—"
		}
		return val
	}
	row := func(label, field string, get func(*songTags) string, set func(*songTags, string), number bool) Item {
		val, same := e.common(get)
		sub := shown(field, val)
		if !same && st.field != field {
			sub = "Different on each song"
		}
		changed := false
		for _, s := range e.songs {
			if get(&s.tags) != get(&s.orig) {
				changed = true
			}
		}
		it := Item{Label: label, Sublabel: sub, OnSelect: func() {
			start := val
			if !same {
				start = "" // different on each song: nothing to keep by mistake
			}
			edit(field, start, func(x string) {
				if (same && x == val) || (!same && x == "") {
					return // not changed: each song keeps its own
				}
				if number {
					if _, err := strconv.Atoi(x); err != nil && x != "" {
						e.status = label + ": type a number"
						return
					}
				}
				e.setAll(func(g *songTags) { set(g, x) })
			})
		}}
		if changed {
			it.Value = "changed"
		}
		return it
	}
	num := func(n int) string { return itoaPos(n) }
	atoi := func(s string) int { n, _ := strconv.Atoi(s); return n }
	v.Build = func() []Item {
		var items []Item
		if e.saving != "" {
			return []Item{{Label: "Saving…", Sublabel: e.saving, NoArrow: true}}
		}
		if e.status != "" {
			items = append(items, Item{Label: e.status, NoArrow: true})
		}
		items = append(items,
			Item{Label: "Find Info Online", Sublabel: "MusicBrainz, iTunes, Deezer, Wikipedia", Icon: "search_icon",
				OnSelect: func() { ui.showView(ui.infoSearchView(e, v), "") }},
			row("Album", "album", func(g *songTags) string { return g.Album }, func(g *songTags, x string) { g.Album = x }, false),
			row("Album Artist", "albumArtist", func(g *songTags) string { return g.AlbumArtist }, func(g *songTags, x string) { g.AlbumArtist = x }, false),
			row("Artist", "artist", func(g *songTags) string { return g.Artist }, func(g *songTags, x string) { g.Artist = x }, false),
			row("Year", "year", func(g *songTags) string { return num(g.Year) }, func(g *songTags, x string) { g.Year = atoi(x) }, true),
			row("Genre", "genre", func(g *songTags) string { return g.Genre }, func(g *songTags, x string) { g.Genre = x }, false),
			Item{Label: "Songs", Sublabel: fmt.Sprintf("%d %s", len(e.songs), pluralize(len(e.songs), "song", "songs")),
				OnSelect: func() { ui.showView(ui.songsInfoView(e, v), "") }})
		// the cover: the same search as Missing Covers, for any album
		cover := Item{Label: "Cover", Sublabel: "Search for another one", Album: e.al,
			OnSelect: func() { ui.showView(ui.coverPickView(e.al, func() { e.status = "" }), "") }}
		if e.al.ArtPath == "" && e.al.ArtData == nil {
			cover.Sublabel = "No cover · search for one"
		}
		if hasChosenCover(e.al) {
			cover.Sublabel = "Chosen by you · search for another one"
			cover.LongPress = []SheetOption{{Label: "Use the Album's Own Cover", OnSelect: func() {
				ui.forgetChosenCover(e.al)
				ui.refreshView(v)
			}}}
		}
		items = append(items, cover)
		items = append(items, ui.mergeItems(e, nil, func() { ui.closeAlbumInfo(e) })...) // (ij_albummerge.go)
		if n, w := e.changed(); n > 0 {
			sub := e.summary()
			if n > w {
				sub += fmt.Sprintf(" · %d %s can't take tags (M4A, WAV, AIFF…)", n-w, pluralize(n-w, "song", "songs"))
				if e.albumChanged() {
					sub += ": they'd stay in the old album"
				}
			}
			items = append(items,
				Item{Label: "Save Changes", Sublabel: sub, OnSelect: func() {
					if w == 0 {
						e.status = "None of these songs can take tags (M4A, WAV, AIFF…)"
						ui.refreshView(v)
						return
					}
					ui.showSheetTitled(fmt.Sprintf("Write into %d %s? %s. It can't be undone.", w, pluralize(w, "file", "files"), e.summary()),
						[]SheetOption{{Label: "Save Changes", Danger: true, OnSelect: func() { ui.saveAlbumEdit(e, v) }}})
				}},
				Item{Label: "Undo Changes", OnSelect: func() {
					for _, s := range e.songs {
						s.tags = s.orig
					}
					e.status = ""
					ui.refreshView(v)
				}})
		}
		return items
	}
	v.Items = v.Build()
	return v
}

// summary: what Save Changes would change ("Titles of 9 · Year · Genre").
func (e *albumEdit) summary() string {
	count := map[tagMask]int{}
	for _, s := range e.songs {
		if !canWriteTags(s.t.Path) {
			continue
		}
		m := changedTags(s.orig, s.tags)
		for b := tagMask(1); b <= tagYear; b <<= 1 {
			if m&b != 0 {
				count[b]++
			}
		}
	}
	var parts []string
	for _, f := range []struct {
		bit     tagMask
		name    string
		perSong bool
	}{{tagTitle, "Titles", true}, {tagArtist, "Artists", true}, {tagTrack, "Numbers", true}, {tagDisc, "Discs", true},
		{tagAlbum, "Album", false}, {tagAlbumArtist, "Album Artist", false}, {tagYear, "Year", false}, {tagGenre, "Genre", false}} {
		n := count[f.bit]
		if f.bit == tagTrack {
			n = max(n, count[tagTrackTotal])
		}
		switch {
		case n == 0:
		case !f.perSong || n == len(e.songs):
			parts = append(parts, f.name)
		default:
			parts = append(parts, fmt.Sprintf("%s of %d", f.name, n))
		}
	}
	if len(parts) == 0 {
		return "No changes"
	}
	return strings.Join(parts, " · ")
}

// albumChanged: an album-wide field changes (the album would split if
// some songs can't take it).
func (e *albumEdit) albumChanged() bool {
	for _, s := range e.songs {
		if changedTags(s.orig, s.tags)&(tagAlbum|tagAlbumArtist) != 0 {
			return true
		}
	}
	return false
}

// songsInfoView: the album's songs; each opens its own fields.
func (ui *IpodUI) songsInfoView(e *albumEdit, parent *View) *View {
	v := &View{ID: "songsInfo", Title: "Songs", Kind: KindFull}
	e.views[v] = true
	v.Build = func() []Item {
		var items []Item
		for _, s := range e.songs {
			label := s.tags.Title
			if s.tags.Track > 0 {
				label = fmt.Sprintf("%d. %s", s.tags.Track, label)
			}
			it := Item{Label: label, Sublabel: s.tags.Artist, OnSelect: func() { ui.showView(ui.songInfoView(e, s, v, parent), "") }}
			if s.tags != s.orig {
				it.Value = "changed"
			}
			items = append(items, it)
		}
		return items
	}
	v.Items = v.Build()
	return v
}

func (ui *IpodUI) songInfoView(e *albumEdit, s *songEdit, list, album *View) *View {
	v := &View{ID: "songInfo", Title: "Song Info", Kind: KindFull}
	e.views[v] = true
	st := &struct{ text, field string }{}
	v.Build = func() []Item {
		field := func(label, key, val, was string, number bool, set func(string)) Item {
			sub := val
			if st.field == key && ui.keyboardEditing(&st.text) {
				sub = ui.withCaret(&st.text)
			} else if sub == "" {
				sub = "—"
			}
			it := Item{Label: label, Sublabel: sub, OnSelect: func() {
				st.field, st.text = key, val
				ui.showKeyboard(&st.text, func() { ui.refreshView(v) }, func() {
					x := strings.TrimSpace(st.text)
					if _, err := strconv.Atoi(x); number && err != nil && x != "" {
						return
					}
					set(x)
					st.field = ""
					ui.refreshView(v)
					ui.refreshView(list)
					ui.refreshView(album)
				})
				ui.refreshView(v)
			}}
			if val != was {
				it.Value = "changed"
			}
			return it
		}
		g, o := &s.tags, s.orig
		return []Item{
			field("Title", "title", g.Title, o.Title, false, func(x string) { g.Title = x }),
			field("Artist", "artist", g.Artist, o.Artist, false, func(x string) { g.Artist = x }),
			field("Track Number", "track", itoaPos(g.Track), itoaPos(o.Track), true, func(x string) { g.Track, _ = strconv.Atoi(x) }),
			field("Disc Number", "disc", itoaPos(g.Disc), itoaPos(o.Disc), true, func(x string) { g.Disc, _ = strconv.Atoi(x) }),
			{Label: "File", Sublabel: s.t.Path, NoArrow: true},
		}
	}
	v.Items = v.Build()
	return v
}

// saveAlbumEdit writes the changed fields of the changed songs, and only
// those (each file keeps its own values of the others), then scans the
// library again and goes back. The old values are logged first, so they
// can be put back by hand. A read-only card stops it at the first file.
func (ui *IpodUI) saveAlbumEdit(e *albumEdit, v *View) {
	if albumSaving[e.al] {
		return
	}
	type job struct {
		s    *songEdit
		path string
		tags songTags
		mask tagMask
	}
	var todo []job
	for _, s := range e.songs {
		if m := changedTags(s.orig, s.tags); m != 0 && canWriteTags(s.t.Path) && insideMusic(s.t.Path) {
			todo = append(todo, job{s, s.t.Path, s.tags, m})
		}
	}
	if len(todo) == 0 {
		return
	}
	albumSaving[e.al] = true
	e.saving = fmt.Sprintf("0 of %d", len(todo))
	ui.refreshView(v)
	go func() {
		var failed []string
		var written []job
		var stop error
		for i, j := range todo {
			logMsg(fmt.Sprintf("Album Info: %s: was %+v, writing %+v", j.path, j.s.orig, j.tags))
			if err := writeTagsMasked(j.path, filepath.Ext(j.path), j.tags, j.mask); err != nil {
				logMsg(fmt.Sprintf("Album Info: %s: %v", j.path, err))
				failed = append(failed, j.tags.Title)
				if errors.Is(err, syscall.EROFS) || errors.Is(err, syscall.EACCES) || errors.Is(err, syscall.EPERM) || isDiskErr(err) {
					stop = err
					break
				}
			} else {
				written = append(written, j)
			}
			ui.post(func() {
				e.saving = fmt.Sprintf("%d of %d", i+1, len(todo))
				ui.refreshView(v)
			})
		}
		logMsg(fmt.Sprintf("Album Info: %q: %d songs written, %d failed", e.al.Name, len(written), len(todo)-len(written)))
		ui.postSure(func() {
			delete(albumSaving, e.al)
			e.saving = ""
			for _, j := range written {
				j.s.orig = j.tags
				// the library shows the new tags at once (the rescan takes a
				// while): a new Album Info starts from them
				t, g := j.s.t, j.tags
				t.Title, t.Artist, t.Album, t.AlbumArtist, t.Genre = g.Title, g.Artist, g.Album, g.AlbumArtist, g.Genre
				t.TrackNum, t.TrackTotal, t.DiscNum, t.Year = g.Track, g.TrackTotal, g.Disc, g.Year
			}
			saved := len(written)
			if saved > 0 {
				ui.dirtyAllViews()
				ui.rescanLibrary()
			}
			if stop != nil || len(failed) > 0 {
				if stop != nil {
					e.status = fmt.Sprintf("Stopped: %s · %d of %d saved", shortErr(stop), saved, len(todo))
				} else {
					e.status = fmt.Sprintf("%d of %d not saved (see the log): %s", len(failed), len(todo), strings.Join(failed, ", "))
				}
				ui.refreshView(v)
				return
			}
			// back to where Album Info was opened from: only its own screens
			for e.views[ui.topScreen()] && ui.keyboard == nil && ui.activeSheet() == nil {
				top := ui.topScreen()
				if ui.hideView(); ui.topScreen() == top {
					break
				}
			}
			ui.showToast(fmt.Sprintf("Saved %d %s", saved, pluralize(saved, "song", "songs")))
		})
	}()
}

// closeAlbumInfo: back to where Album Info was opened from (only its own
// screens close).
func (ui *IpodUI) closeAlbumInfo(e *albumEdit) {
	for e.views[ui.topScreen()] && ui.keyboard == nil && ui.activeSheet() == nil {
		top := ui.topScreen()
		if ui.hideView(); ui.topScreen() == top {
			break
		}
	}
}

// ---- Find Info Online ----

type infoHit struct {
	a    *albumInfo
	best bool
}

func (h *infoHit) sublabel(ours int) string {
	var parts []string
	if h.a.Artist != "" {
		a := h.a.Artist
		if h.a.ArtistNote != "" {
			a += " (" + h.a.ArtistNote + ")"
		}
		parts = append(parts, a)
	}
	if h.a.Year != "" {
		parts = append(parts, h.a.Year)
	}
	if h.a.Count > 0 {
		s := fmt.Sprintf("%d %s", h.a.Count, pluralize(h.a.Count, "song", "songs"))
		if h.a.Count == ours {
			s += " ✓"
		}
		parts = append(parts, s)
	}
	if h.a.Kind != "" {
		parts = append(parts, h.a.Kind)
	}
	parts = append(parts, h.a.Source)
	if h.best {
		parts = append(parts, "Best match")
	}
	return strings.Join(parts, " · ")
}

func (ui *IpodUI) infoSearchView(e *albumEdit, album *View) *View {
	q := e.query()
	st := &struct {
		term, ran, status string
		manual, busy      bool
		hits              []*infoHit
		gen               int
		getting           *infoHit
		closed            bool
	}{}
	st.term = strings.TrimSpace(cleanTitle(q.title))
	if len(q.artists) > 0 {
		st.term += " " + cleanArtist(q.artists[0])
	}
	v := &View{ID: "infoSearch", Title: "Find Info Online", Kind: KindFull}
	v.onClose = func() { st.closed = true; st.gen++ }
	var run func(manual bool)
	v.Build = func() []Item {
		note := ""
		if st.ran != "" && !st.busy && !ui.keyboardEditing(&st.term) {
			note = fmt.Sprintf("%d %s", len(st.hits), pluralize(len(st.hits), "result", "results"))
		}
		edit := func() {
			ui.showKeyboard(&st.term, func() { ui.refreshView(v) }, func() { run(true) })
			ui.refreshView(v)
		}
		items := []Item{ui.searchField(&st.term, "Search MusicBrainz, iTunes, Deezer…", note, edit, []SheetOption{
			{Label: "Clear search", OnSelect: func() { st.term = ""; edit() }},
			{Label: "Search automatically again", OnSelect: func() { run(false) }},
		})}
		switch {
		case st.busy:
			items = append(items, Item{Label: "Searching…", Sublabel: "MusicBrainz, iTunes, Deezer, Wikipedia", NoArrow: true})
		case st.status != "":
			l, s, _ := strings.Cut(st.status, " · ")
			items = append(items, Item{Label: l, Sublabel: s, NoArrow: true})
		}
		for _, h := range st.hits {
			sub := h.sublabel(len(e.songs))
			if st.getting == h {
				sub = "Getting the songs…"
			}
			items = append(items, Item{Label: h.a.Title, Sublabel: sub, OnSelect: func() {
				if st.getting != nil {
					return
				}
				st.getting = h
				ui.refreshView(v)
				c := *h.a // loaded into a copy: the list reads h.a meanwhile
				go func() {
					err := c.detail()
					ui.postSure(func() {
						st.getting = nil
						if st.closed {
							return // left before it came: nothing is changed
						}
						*h.a = c
						if err != nil {
							st.status = "Not loaded · " + iaErrText(err)
							ui.refreshView(v)
							return
						}
						n := e.apply(h.a)
						e.status = fmt.Sprintf("From %s: %d of %d songs paired · check, then Save Changes", h.a.Source, n, len(e.songs))
						if len(h.a.Tracks) == 0 {
							e.status = fmt.Sprintf("From %s: album artist, year and genre · check, then Save Changes", h.a.Source)
						}
						logMsg(fmt.Sprintf("Album Info: %q: %s %q by %q (%s), %d songs paired", e.al.Name, h.a.Source, h.a.Title, h.a.Artist, h.a.ID, n))
						if ui.topScreen() == v && ui.keyboard == nil && ui.activeSheet() == nil {
							ui.hideView()
						}
						ui.refreshView(album)
					})
				}()
			}})
		}
		return items
	}
	run = func(manual bool) {
		term := strings.TrimSpace(st.term)
		if manual && term == "" {
			return
		}
		st.gen++
		gen := st.gen
		st.manual, st.busy, st.status, st.hits = manual, true, "", nil
		st.ran = term
		if !manual {
			st.ran = "Automatic search"
		}
		v.Index = 0
		ui.refreshView(v)
		go func() {
			found := make([][]albumInfo, len(infoSources))
			errs := make([]error, len(infoSources))
			var wg sync.WaitGroup
			for i, s := range infoSources {
				wg.Add(1)
				go func() {
					defer wg.Done()
					if manual {
						found[i], errs[i] = s.search(term)
						return
					}
					found[i], errs[i] = s.find(q)
				}()
			}
			wg.Wait()
			var err error
			var all []*infoHit
			for i := range found {
				if errs[i] != nil {
					logMsg("Album Info: " + infoSources[i].name + ": " + errs[i].Error())
					err = errs[i]
				}
				for j := range found[i] {
					a := &found[i][j]
					all = append(all, &infoHit{a: a, best: q.matches(a.cand())})
				}
			}
			ui.postSure(func() {
				if gen != st.gen {
					return
				}
				st.busy = false
				// the checked ones first: the very same title, then the same
				// number of songs
				rank := func(h *infoHit) int {
					r := 0
					if h.best {
						r += 4
						if sameTitle(q.title, h.a.Title) {
							r += 2
						}
					}
					if h.a.Count == len(e.songs) {
						r++
					}
					return r
				}
				sort.SliceStable(all, func(i, j int) bool { return rank(all[i]) > rank(all[j]) })
				st.hits = all
				switch {
				case err != nil && len(all) == 0:
					st.status = "No answer · " + iaErrText(err)
				case len(all) == 0:
					st.status = "No results · edit the search above and try other words"
				}
				ui.refreshView(v)
				if len(all) > 0 {
					v.Index = 1
					if st.status != "" {
						v.Index = 2
					}
				}
			})
		}()
	}
	st.busy = true
	v.Items = v.Build()
	run(false)
	return v
}
