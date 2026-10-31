package main

// ij_playlists.go — playlists made on the betterPod: Music → Playlists →
// New Playlist… (a name with the keyboard), and Add to Playlist… in the
// options of a song or an album (Y, or holding A): a playlist there, or a
// new one. In a playlist, a song's options remove it; a playlist's own
// options delete it.
//
// They are plain M3U files in <music>/Playlists (paths relative to the
// file), so a computer's player reads them too, and the library scan
// finds them as it finds any other .m3u in the music folder.

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

func playlistsDir() string { return filepath.Join(musicRoot(), "Playlists") }

// writePlaylist: the playlist's file, from its songs.
func writePlaylist(pl *Playlist) error {
	var b strings.Builder
	b.WriteString("#EXTM3U\n")
	dir := filepath.Dir(pl.Path)
	for _, t := range pl.Tracks {
		p := t.Path
		if rel, err := filepath.Rel(dir, p); err == nil {
			p = rel
		}
		fmt.Fprintf(&b, "#EXTINF:%d,%s - %s\n%s\n", int(t.Duration), t.Artist, t.Title, filepath.ToSlash(p))
	}
	os.MkdirAll(dir, 0o755)
	return writeFileAtomic(pl.Path, []byte(b.String()))
}

// createPlaylist: a new, empty playlist called name (a number added when
// the name is taken).
func (ui *IpodUI) createPlaylist(name string) (*Playlist, error) {
	name = strings.TrimSpace(name)
	if name == "" {
		return nil, fmt.Errorf("no name")
	}
	lib := ui.library()
	base := safeName(name)
	path := filepath.Join(playlistsDir(), base+".m3u")
	for i := 2; fileExists(path); i++ {
		path = filepath.Join(playlistsDir(), fmt.Sprintf("%s (%d).m3u", base, i))
	}
	pl := &Playlist{Name: strings.TrimSuffix(filepath.Base(path), ".m3u"), Path: path}
	if err := writePlaylist(pl); err != nil {
		return nil, err
	}
	lib.Playlists = append(lib.Playlists, pl)
	sort.SliceStable(lib.Playlists, func(i, j int) bool {
		return strings.ToLower(lib.Playlists[i].Name) < strings.ToLower(lib.Playlists[j].Name)
	})
	logMsg("Playlists: new " + path)
	go ui.app.saveLibraryJSON()
	return pl, nil
}

// addToPlaylist: tracks at the end of pl (those already in it are left
// out); it returns the toast.
func (ui *IpodUI) addToPlaylist(pl *Playlist, tracks []*Track) string {
	have := map[string]bool{}
	for _, t := range pl.Tracks {
		have[t.Path] = true
	}
	n := 0
	for _, t := range tracks {
		if !have[t.Path] {
			pl.Tracks = append(pl.Tracks, t)
			have[t.Path] = true
			n++
		}
	}
	if n == 0 {
		return "Already in " + pl.Name
	}
	if err := writePlaylist(pl); err != nil {
		logMsg("Playlists: " + err.Error())
		return "Couldn't save " + pl.Name
	}
	if n == 1 {
		return "Added to " + pl.Name
	}
	return fmt.Sprintf("%d songs added to %s", n, pl.Name)
}

// openNewPlaylist: the name of a new playlist, with the keyboard; done gets
// the playlist made (B on the empty keyboard leaves).
func (ui *IpodUI) openNewPlaylist(done func(pl *Playlist)) {
	name := ""
	v := &View{ID: "newPlaylist", Title: "New Playlist", Kind: KindFull}
	finish := func() {
		if strings.TrimSpace(name) == "" {
			return
		}
		pl, err := ui.createPlaylist(name)
		ui.hideKeyboard()
		ui.hideView()
		if err != nil {
			ui.showToast("Couldn't make it: " + shortErr(err))
			return
		}
		done(pl)
	}
	open := func() {
		ui.showKeyboard(&name, func() { ui.refreshView(v) }, finish)
		ui.refreshView(v)
	}
	v.Build = func() []Item {
		label := name
		if ui.keyboardEditing(&name) {
			label = ui.withCaret(&name)
		} else if label == "" {
			label = "Type a name"
		}
		items := []Item{{Label: label, Sublabel: "The playlist's name", NoArrow: true, OnSelect: open}}
		if strings.TrimSpace(name) != "" {
			items = append(items, Item{Label: "Make Playlist", Sublabel: "Or [START] on the keyboard", OnSelect: finish})
		}
		return items
	}
	v.Items = v.Build()
	ui.showView(v, "")
	open()
}

func (ui *IpodUI) playlistsView() *View {
	v := &View{ID: "playlists", Title: "Playlists", Kind: KindFull}
	v.Build = func() []Item {
		items := []Item{{Label: "New Playlist…", OnSelect: func() {
			ui.openNewPlaylist(func(pl *Playlist) {
				ui.refreshView(v)
				ui.showToast("Made " + pl.Name)
			})
		}}}
		for _, pl := range ui.library().Playlists {
			var art *Album
			if len(pl.Tracks) > 0 {
				art = ui.trackAlbum(pl.Tracks[0])
			}
			n := len(pl.Tracks)
			items = append(items, Item{
				Label:    pl.Name,
				Sublabel: fmt.Sprintf("%d %s", n, pluralize(n, "song", "songs")),
				Album:    art,
				OnSelect: func() { ui.showView(ui.playlistView(pl), pl.Name) },
				LongPress: append(ui.mediaOptions(pl.Tracks), SheetOption{Label: "Delete Playlist", Danger: true, OnSelect: func() {
					ui.showSheetTitled("Delete "+pl.Name+"? Its songs stay in your library.", []SheetOption{{Label: "Delete Playlist", Danger: true, OnSelect: func() {
						os.Remove(pl.Path)
						lib := ui.library()
						for i, p := range lib.Playlists {
							if p == pl {
								lib.Playlists = append(lib.Playlists[:i], lib.Playlists[i+1:]...)
								break
							}
						}
						go ui.app.saveLibraryJSON()
						ui.refreshView(v)
						ui.showToast("Deleted " + pl.Name)
					}}})
				}}),
			})
		}
		return items
	}
	v.Items = v.Build()
	if len(v.Items) > 1 {
		v.Index = 1
	}
	return v
}

func (ui *IpodUI) playlistView(pl *Playlist) *View {
	v := &View{ID: "playlist", Title: "Playlist", Kind: KindFull, Empty: "No songs yet · Y on a song → Add to Playlist…"}
	v.Build = func() []Item {
		var items []Item
		for i, t := range pl.Tracks {
			artist := t.Artist
			if artist == "" {
				artist = "Unknown artist"
			}
			i := i
			items = append(items, Item{
				Label:          t.Title,
				Sublabel:       artist,
				Album:          ui.trackAlbum(t),
				Tracks:         pl.Tracks,
				TrackIdx:       i,
				ShowNowPlaying: true,
				LongPress: append(ui.mediaOptions([]*Track{t}), SheetOption{Label: "Remove from Playlist", Danger: true, OnSelect: func() {
					if i < len(pl.Tracks) && pl.Tracks[i] == t {
						pl.Tracks = append(append([]*Track{}, pl.Tracks[:i]...), pl.Tracks[i+1:]...)
						if err := writePlaylist(pl); err != nil {
							logMsg("Playlists: " + err.Error())
						}
						ui.refreshView(v)
					}
				}}),
			})
		}
		return items
	}
	v.Items = v.Build()
	return v
}

// addToPlaylistView: Add to Playlist… (a song's or an album's options):
// a new playlist, or one there is.
func (ui *IpodUI) addToPlaylistView(tracks []*Track) *View {
	v := &View{ID: "addToPlaylist", Title: "Add to Playlist", Kind: KindFull}
	v.Build = func() []Item {
		items := []Item{{Label: "New Playlist…", Sublabel: "Name it, and the songs go in it", OnSelect: func() {
			ui.openNewPlaylist(func(pl *Playlist) {
				msg := ui.addToPlaylist(pl, tracks)
				if ui.topScreen() == v {
					ui.hideView()
				}
				ui.showToast(msg)
			})
		}}}
		for _, pl := range ui.library().Playlists {
			n := len(pl.Tracks)
			var art *Album
			if n > 0 {
				art = ui.trackAlbum(pl.Tracks[0])
			}
			items = append(items, Item{Label: pl.Name, Sublabel: fmt.Sprintf("%d %s", n, pluralize(n, "song", "songs")), Album: art, NoArrow: true,
				OnSelect: func() {
					msg := ui.addToPlaylist(pl, tracks)
					ui.hideView()
					ui.showToast(msg)
				}})
		}
		return items
	}
	v.Items = v.Build()
	if len(v.Items) > 1 {
		v.Index = 1
	}
	return v
}
