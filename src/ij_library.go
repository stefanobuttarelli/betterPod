package main

// ij_library.go — the menus: home, Music, Artists, Albums, Playlists, Songs, Games, Settings (Shuffle, Repeat).

import (
	"math/rand"
	"sort"
	"strings"
)

func (ui *IpodUI) homeView() *View {
	v := &View{ID: "home", Title: "iPod", Kind: KindSplit}
	v.Build = func() []Item {
		items := []Item{
			{Label: "Cover Flow", Preview: PrevMusic, OnSelect: ui.openCoverFlow},
			{Label: "Music", Preview: PrevMusic, OnSelect: func() { ui.showView(ui.musicView(), "") }},
			{Label: "Games", Preview: PrevGames, OnSelect: func() { ui.showView(ui.gamesView(), "") }},
			{Label: "Settings", Preview: PrevSettings, OnSelect: func() { ui.showView(ui.settingsView(), "") }},
		}
		if podsInMenu { // under Music (Settings → Podcasts turns it off)
			items = append(items[:2], append([]Item{{Label: "Podcasts", Preview: PrevMusic,
				OnSelect: func() { ui.showView(ui.podcastsView(), "") }}}, items[2:]...)...)
		}
		if p := ui.app.Playing; p != nil && p.Track != nil {
			items = append(items, Item{Label: "Now Playing", Preview: PrevNowPlaying, OnSelect: ui.showNowPlaying,
				LongPress: ui.clearNowPlayingOptions()})
		}
		// a new version found (by hand or by the daily check)
		if updateAvailable() {
			items = append(items, Item{Label: "Update Available", Preview: PrevSettings, OnSelect: func() { ui.showView(ui.updateView(), "") }})
		}
		return items
	}
	v.Items = v.Build()
	return v
}

func (ui *IpodUI) musicView() *View {
	v := &View{ID: "music", Title: "Music", Kind: KindSplit}
	v.Build = func() []Item {
		items := []Item{
			{Label: "Cover Flow", Preview: PrevMusic, OnSelect: ui.openCoverFlow},
			{Label: "Playlists", Preview: PrevMusic, OnSelect: func() { ui.showView(ui.playlistsView(), "") }},
			{Label: "Artists", Preview: PrevMusic, OnSelect: func() { ui.showView(ui.artistsView(), "") }},
			{Label: "Albums", Preview: PrevMusic, OnSelect: func() { ui.showView(ui.albumsView(nil), "") }},
			{Label: "Search", Preview: PrevMusic, OnSelect: func() { ui.showView(ui.searchHubView(), "") }},
		}
		if p := ui.app.Playing; p != nil && p.Track != nil {
			items = append(items, Item{Label: "Now playing", Preview: PrevNowPlaying, OnSelect: ui.showNowPlaying,
				LongPress: ui.clearNowPlayingOptions()})
			items = append(items, Item{Label: "Up Next", Preview: PrevNowPlaying, OnSelect: func() { ui.showView(ui.upNextView(), "") }})
		}
		return items
	}
	v.Items = v.Build()
	return v
}

func (ui *IpodUI) library() *Library {
	if ui.app.Library == nil {
		return &Library{}
	}
	return ui.app.Library
}

func (ui *IpodUI) artistsView() *View {
	v := &View{ID: "artists", Title: "Artists", Kind: KindFull, Empty: "No artists"}
	for _, ar := range sortedArtists(ui.library().Artists) {
		//
		v.Items = append(v.Items, Item{
			Label: ar.Name,
			OnSelect: func() {
				ui.showView(ui.artistView(ar), ar.Name)
			},
		})
	}
	return v
}

func (ui *IpodUI) artistView(ar *Artist) *View {
	v := &View{ID: "artist", Title: "Artist", Kind: KindFull, Empty: "No albums by this artist"}
	for _, al := range ar.Albums {
		//
		v.Items = append(v.Items, Item{
			Label:     al.Name,
			Sublabel:  al.Artist,
			Album:     al,
			OnSelect:  func() { ui.showView(ui.albumView(al), al.Name) },
			LongPress: ui.albumOptions(al), // with Edit Album Info (ij_albuminfo.go)
		})
	}
	return v
}

func (ui *IpodUI) albumsView(albums []*Album) *View {
	v := &View{ID: "albums", Title: "Albums", Kind: KindFull, Empty: "No albums"}
	if albums == nil {
		albums = ui.library().Albums
	}
	for _, al := range albums {
		// album rows: cover, title and artist; long press offers
		// Play Next / Play Later
		v.Items = append(v.Items, Item{
			Label:     al.Name,
			Sublabel:  al.Artist,
			Album:     al,
			OnSelect:  func() { ui.showView(ui.albumView(al), al.Name) },
			LongPress: ui.albumOptions(al), // with Edit Album Info (ij_albuminfo.go)
		})
	}
	return v
}

func (ui *IpodUI) albumView(al *Album) *View {
	v := &View{ID: "album", Title: "Album", Kind: KindFull, Empty: "No saved songs"}
	tracks := albumTracks(al)
	for i, t := range tracks {
		v.Items = append(v.Items, Item{
			Label:          t.Title,
			Tracks:         tracks,
			TrackIdx:       i,
			ShowNowPlaying: true,
			LongPress:      ui.mediaOptions([]*Track{t}),
		})
	}
	return v
}

func (ui *IpodUI) playlistsView() *View {
	v := &View{ID: "playlists", Title: "Playlists", Kind: KindFull, Empty: "No playlists"}
	for _, pl := range ui.library().Playlists {
		//
		var art *Album
		if len(pl.Tracks) > 0 {
			art = ui.trackAlbum(pl.Tracks[0])
		}
		v.Items = append(v.Items, Item{
			Label:    pl.Name,
			Sublabel: "By you",
			Album:    art,
			OnSelect: func() { ui.showView(ui.playlistView(pl), pl.Name) },
		})
	}
	return v
}

func (ui *IpodUI) playlistView(pl *Playlist) *View {
	v := &View{ID: "playlist", Title: "Playlist", Kind: KindFull, Empty: "No songs in this playlist"}
	for i, t := range pl.Tracks {
		artist := t.Artist
		if artist == "" {
			artist = "Unknown artist"
		}
		v.Items = append(v.Items, Item{
			Label:          t.Title,
			Sublabel:       artist,
			Album:          ui.trackAlbum(t),
			Tracks:         pl.Tracks,
			TrackIdx:       i,
			ShowNowPlaying: true,
			LongPress:      ui.mediaOptions([]*Track{t}),
		})
	}
	return v
}

func (ui *IpodUI) songsView(tracks []*Track) *View {
	v := &View{ID: "songs", Title: "Songs", Kind: KindFull, Empty: "No songs to show"}
	for _, t := range tracks {
		//
		v.Items = append(v.Items, Item{
			Label:          t.Title,
			Sublabel:       t.Artist + " • " + t.Album,
			Album:          ui.trackAlbum(t),
			Tracks:         []*Track{t},
			TrackIdx:       0,
			ShowNowPlaying: true,
			LongPress:      ui.mediaOptions([]*Track{t}),
		})
	}
	return v
}

// albumTracks returns the album's tracks by disc, track number and title.
func albumTracks(al *Album) []*Track {
	out := append([]*Track{}, al.Tracks...)
	sort.SliceStable(out, func(i, j int) bool {
		a, b := out[i], out[j]
		if a.DiscNum != b.DiscNum {
			return a.DiscNum < b.DiscNum
		}
		if a.TrackNum != b.TrackNum {
			return a.TrackNum < b.TrackNum
		}
		return strings.ToLower(a.Title) < strings.ToLower(b.Title)
	})
	return out
}

func sortedArtists(in []*Artist) []*Artist {
	out := append([]*Artist{}, in...)
	sort.SliceStable(out, func(i, j int) bool {
		return strings.ToLower(out[i].Name) < strings.ToLower(out[j].Name)
	})
	return out
}

// mediaOptions: the long-press sheet of songs and albums.
func (ui *IpodUI) mediaOptions(tracks []*Track) []SheetOption {
	return []SheetOption{
		{Label: "Play Next", Icon: "queue_next", OnSelect: func() { ui.showToast(ui.enqueue(tracks, true)) }},
		{Label: "Add to End of Queue", Icon: "queue_last", OnSelect: func() { ui.showToast(ui.enqueue(tracks, false)) }},
	}
}

// enqueue adds tracks after the current song (next) or at the end of the
// queue; with nothing playing they start at once. It returns what happened,
// for the toast.
func (ui *IpodUI) enqueue(tracks []*Track, next bool) string {
	app := ui.app
	q := app.Queue
	if q == nil || len(q.Tracks) == 0 || app.Playing.Track == nil || app.Playing.State == StateStopped {
		// nothing playing (or the queue ran out): play them now
		ui.playSong(tracks, 0, false)
		return "Playing"
	}
	msg := "Playing Next"
	if !next {
		msg = "Added to Queue"
	}
	ui.markQueued(tracks, next)
	if q.Shuffle && len(q.ShuffleOrder) > 0 {
		// shuffled: new tracks go in the shuffle order after the current one
		base := len(q.Tracks)
		q.Tracks = append(q.Tracks, tracks...)
		pos := len(q.ShuffleOrder)
		if next {
			pos = q.CurrentIndex + 1
		}
		var ins []int
		for i := range tracks {
			ins = append(ins, base+i)
		}
		q.ShuffleOrder = append(q.ShuffleOrder[:pos], append(ins, q.ShuffleOrder[pos:]...)...)
		return msg
	}
	pos := len(q.Tracks)
	if next {
		pos = q.CurrentIndex + 1
	}
	q.Tracks = append(q.Tracks[:pos], append(append([]*Track{}, tracks...), q.Tracks[pos:]...)...)
	return msg
}

// gamesView: Brick, Solitaire and Weather.
// The preview on the right is the Games icon.
func (ui *IpodUI) gamesView() *View {
	v := &View{ID: "games", Title: "Games", Kind: KindSplit}
	v.Items = []Item{
		{Label: "Brick", Preview: PrevBrick, OnSelect: func() { ui.openBrick() }},
		{Label: "Solitaire", Preview: PrevSolitaire, OnSelect: func() { ui.openSolitaire() }},
		{Label: "Weather", Preview: PrevWeather, OnSelect: func() { ui.openWeather() }},
	}
	return v
}

// settingsView: About, Shuffle, Repeat, Theme, Library and Database, with
// the current values.
func (ui *IpodUI) settingsView() *View {
	v := &View{ID: "settings", Title: "Settings", Kind: KindSplit}
	v.Build = func() []Item {
		return []Item{
			{Label: "About", Preview: PrevSettings, OnSelect: func() { ui.showView(ui.aboutView(), "") }},
			{Label: "Shuffle", Preview: PrevSettings, Value: shuffleLabel(ui.shuffleMode), OnSelect: func() { ui.showView(ui.shuffleView(), "") }},
			{Label: "Repeat", Preview: PrevSettings, Value: repeatLabel(ui.repeatMode()), OnSelect: func() { ui.showView(ui.repeatView(), "") }},
			{Label: "Theme", Preview: PrevSettings, Value: themeLabel(themeMode), OnSelect: func() { ui.showView(ui.themeView(), "") }},
			{Label: "Library", Preview: PrevSettings, OnSelect: func() { ui.showView(ui.libraryInfoView(), "") }},
			{Label: "Database", Preview: PrevSettings, Value: dbLabel(dataDir()), OnSelect: func() { ui.showView(ui.databaseView(), "") }},
			{Label: "YouTube", Preview: PrevSettings, Value: onOff(ytEnabled), OnSelect: func() { ui.showView(ui.youtubeSettingsView(), "") }},
			{Label: "Bluetooth", Preview: PrevSettings, Value: btSettingsValue(), OnSelect: func() { ui.showView(ui.bluetoothView(), "") }},
			{Label: "Podcasts", Preview: PrevSettings, Value: onOff(podsInMenu), OnSelect: func() {
				podsInMenu = !podsInMenu // in the main menu or not (downloaded episodes stay in Music)
				ui.savePrefs()
				// the main menu's cursor stays on the same entry (Settings)
				for _, h := range ui.split {
					if h.ID == "home" && h.Index >= 2 {
						if podsInMenu {
							h.Index++
						} else {
							h.Index--
						}
					}
				}
				ui.refreshBuilt()
			}},
			{Label: "Software Update", Preview: PrevSettings, OnSelect: func() { ui.showView(ui.updateView(), "") }},
		}
	}
	v.Items = v.Build()
	return v
}

func shuffleLabel(m string) string {
	switch m {
	case "songs":
		return "Songs"
	case "albums":
		return "Albums"
	}
	return "Off"
}

func repeatLabel(r RepeatMode) string {
	switch r {
	case RepeatOne:
		return "One"
	case RepeatAll:
		return "All"
	}
	return "Off"
}

func (ui *IpodUI) repeatMode() RepeatMode {
	if q := ui.app.Queue; q != nil {
		return q.Repeat
	}
	return RepeatOff
}

// shuffleView: Off / Songs / Albums.
func (ui *IpodUI) shuffleView() *View {
	v := &View{ID: "shuffle", Title: "Shuffle", Kind: KindSplit}
	v.Build = func() []Item {
		var items []Item
		for _, m := range []string{"off", "songs", "albums"} {
			//
			items = append(items, Item{Label: shuffleLabel(m), Checked: ui.shuffleMode == m,
				Preview: PrevSettings, NoArrow: true, OnSelect: func() { ui.setShuffle(m) }})
		}
		return items
	}
	v.Items = v.Build()
	for i, it := range v.Items {
		if it.Checked {
			v.Index = i
		}
	}
	return v
}

func (ui *IpodUI) repeatView() *View {
	v := &View{ID: "repeat", Title: "Repeat", Kind: KindSplit}
	v.Build = func() []Item {
		var items []Item
		for _, r := range []RepeatMode{RepeatOff, RepeatOne, RepeatAll} {
			//
			items = append(items, Item{Label: repeatLabel(r), Checked: ui.repeatMode() == r,
				Preview: PrevSettings, NoArrow: true, OnSelect: func() { ui.setRepeat(r) }})
		}
		return items
	}
	v.Items = v.Build()
	for i, it := range v.Items {
		if it.Checked {
			v.Index = i
		}
	}
	return v
}

// setShuffle changes the shuffle mode and reorders the queue.
func (ui *IpodUI) setShuffle(mode string) {
	ui.shuffleMode = mode
	ui.applyShuffle()
	ui.savePrefs()
	ui.refreshBuilt()
}

// applyShuffle rebuilds the play order for the shuffle mode, keeping the
// current track playing.
func (ui *IpodUI) applyShuffle() {
	app := ui.app
	q := app.Queue
	if q == nil {
		return
	}
	cur := app.getCurrentTrack()
	phys := 0
	for i, t := range q.Tracks {
		if t == cur {
			phys = i
			break
		}
	}
	switch ui.shuffleMode {
	case "songs":
		q.Shuffle = true
		if len(q.Tracks) > 0 {
			app.buildShuffleOrder(phys)
		}
		q.CurrentIndex = 0
	case "albums":
		q.Shuffle = true
		q.ShuffleOrder = albumShuffleOrder(q.Tracks, phys)
		q.CurrentIndex = 0
	default:
		q.Shuffle = false
		q.ShuffleOrder = nil
		q.CurrentIndex = phys
	}
}

// albumShuffleOrder shuffles whole albums (tracks keep their order inside
// an album); the album of startIdx comes first, starting at startIdx.
func albumShuffleOrder(tracks []*Track, startIdx int) []int {
	type group struct {
		key string
		idx []int
	}
	var groups []*group
	byKey := map[string]*group{}
	for i, t := range tracks {
		k := albumKeyFor(t)
		g, ok := byKey[k]
		if !ok {
			g = &group{key: k}
			byKey[k] = g
			groups = append(groups, g)
		}
		g.idx = append(g.idx, i)
	}
	rand.Shuffle(len(groups), func(i, j int) { groups[i], groups[j] = groups[j], groups[i] })
	var order []int
	var first []int
	for _, g := range groups {
		for p, i := range g.idx {
			if i == startIdx {
				first = append(first, g.idx[p:]...)
				first = append(first, g.idx[:p]...)
			}
		}
	}
	order = append(order, first...)
	for _, g := range groups {
		if len(first) > 0 && containsInt(g.idx, startIdx) {
			continue
		}
		order = append(order, g.idx...)
	}
	return order
}

func containsInt(s []int, v int) bool {
	for _, x := range s {
		if x == v {
			return true
		}
	}
	return false
}

func (ui *IpodUI) setRepeat(r RepeatMode) {
	if q := ui.app.Queue; q != nil {
		q.Repeat = r
	}
	ui.savePrefs()
	ui.refreshBuilt()
}

func (ui *IpodUI) refreshBuilt() {
	for _, st := range [][]*View{ui.split, ui.full} {
		for _, v := range st {
			if v.Build != nil {
				idx := v.Index
				v.Items = v.Build()
				v.Index = max(0, min(idx, len(v.Items)-1)) // the list may be shorter
			}
			v.dirty = true
		}
	}
	// a row that went (Now Playing after Clear Now Playing, Podcasts…) can
	// move the cursor onto another one: its preview follows
	if len(ui.full) == 0 && len(ui.split) > 0 {
		if v := ui.split[len(ui.split)-1]; v.Index >= 0 && v.Index < len(v.Items) && v.Items[v.Index].Preview != ui.preview {
			ui.previewForSplit()
		}
	}
	ui.dirty = true
}
