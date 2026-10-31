package main

// ij_search.go — Search: the results list (the keyboard is in
// ij_keyboard.go).

import (
	"fmt"
	"strings"
)

// searchState is the text typed in Search.
type searchState struct {
	term string
}

// pluralize returns one or many depending on n.
// Only English plurals are needed ("1 song", "2 songs"): the
// interface is in English like the iPod.
func pluralize(n int, one, many string) string {
	if n == 1 {
		return one
	}
	return many
}

func (ui *IpodUI) searchView() *View {
	st := &searchState{}
	v := &View{ID: "search", Title: "Search", Kind: KindFull, Empty: "No results"}
	v.Build = func() []Item {
		items := []Item{ui.searchField(&st.term, "Search your music", "", func() {
			ui.showKeyboard(&st.term, func() { ui.searchChanged(v) }, nil)
		}, nil)}
		q := strings.ToLower(strings.TrimSpace(st.term))
		if q == "" {
			return items
		}
		lib := ui.library()
		var artists []*Artist
		for _, a := range sortedArtists(lib.Artists) {
			if strings.Contains(strings.ToLower(a.Name), q) {
				artists = append(artists, a)
			}
		}
		var albums []*Album
		for _, a := range lib.Albums {
			if strings.Contains(strings.ToLower(a.Name), q) {
				albums = append(albums, a)
			}
		}
		var songs []*Track
		for _, t := range lib.Tracks {
			if strings.Contains(strings.ToLower(t.Title), q) {
				songs = append(songs, t)
			}
		}
		var playlists []*Playlist
		for _, p := range lib.Playlists {
			if strings.Contains(strings.ToLower(p.Name), q) {
				playlists = append(playlists, p)
			}
		}
		if len(artists) > 0 {
			items = append(items, Item{Label: "Artists", Icon: "artists_icon",
				Sublabel: fmt.Sprintf("%d %s", len(artists), pluralize(len(artists), "artist", "artists")),
				OnSelect: func() { ui.showView(ui.artistsListView(artists, true), "") }})
		}
		if len(albums) > 0 {
			items = append(items, Item{Label: "Albums", Icon: "albums_icon",
				Sublabel: fmt.Sprintf("%d %s", len(albums), pluralize(len(albums), "album", "albums")),
				OnSelect: func() { ui.showView(ui.albumsView(albums), "") }})
		}
		if len(songs) > 0 {
			items = append(items, Item{Label: "Songs", Icon: "song_icon",
				Sublabel: fmt.Sprintf("%d %s", len(songs), pluralize(len(songs), "song", "songs")),
				OnSelect: func() { ui.showView(ui.songsView(songs), "") }})
		}
		if len(playlists) > 0 {
			items = append(items, Item{Label: "Playlists", Icon: "playlist_icon",
				Sublabel: fmt.Sprintf("%d %s", len(playlists), pluralize(len(playlists), "playlist", "playlists")),
				OnSelect: func() { ui.showView(ui.playlistsListView(playlists), "") }})
		}
		return items
	}
	v.Items = v.Build()
	// the keyboard opens right away; results follow every key
	ui.showKeyboard(&st.term, func() { ui.searchChanged(v) }, nil)
	return v
}

// searchChanged rebuilds the results after an edit.
func (ui *IpodUI) searchChanged(v *View) {
	v.Items = v.Build()
	if v.Index >= len(v.Items) {
		v.Index = 0
	}
	v.dirty = true
	ui.dirty = true
}

// artistsListView lists artists (with the first album's cover if thumbs).
func (ui *IpodUI) artistsListView(artists []*Artist, thumbs bool) *View {
	v := &View{ID: "artists", Title: "Artists", Kind: KindFull, Empty: "No artists"}
	for _, ar := range artists {
		//
		it := Item{Label: ar.Name, OnSelect: func() { ui.showView(ui.artistView(ar), ar.Name) }}
		if thumbs {
			if len(ar.Albums) > 0 {
				it.Album = ar.Albums[0]
			} else {
				it.Icon = "artists_icon"
			}
		}
		v.Items = append(v.Items, it)
	}
	return v
}

func (ui *IpodUI) playlistsListView(pls []*Playlist) *View {
	v := ui.playlistsView()
	all := v.Build
	v.Build = func() []Item {
		var items []Item
		for _, it := range all() {
			for _, p := range pls {
				if it.Label == p.Name && it.OnSelect != nil && !strings.HasPrefix(it.Label, "New Playlist") {
					items = append(items, it)
					break
				}
			}
		}
		return items
	}
	v.Items = v.Build()
	return v
}
