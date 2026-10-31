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

// writePlaylist: the file of a new playlist, from its songs. A playlist
// already on disk is changed with editPlaylist instead, which keeps its
// lines.
func writePlaylist(pl *Playlist) error {
	b, _ := editM3U(nil, filepath.Dir(pl.Path), nil, pl.Tracks, "", 0)
	os.MkdirAll(filepath.Dir(pl.Path), 0o755)
	return writeFileAtomic(pl.Path, b)
}

// editPlaylist changes a playlist's file in place (see editM3U): the songs
// of add go at the end, the song at removePath is taken out.
func editPlaylist(pl *Playlist, lib *Library, add []*Track, removePath string, nth int) error {
	data, err := os.ReadFile(pl.Path)
	if os.IsNotExist(err) {
		// gone (renamed or deleted elsewhere): written again from the songs
		// the library knows, with the change
		tracks := append([]*Track(nil), pl.Tracks...)
		if removePath != "" {
			at := -1
			for i, t := range tracks {
				if t.Path == removePath && (i == nth || at < 0) {
					at = i
				}
			}
			if at >= 0 {
				tracks = append(tracks[:at], tracks[at+1:]...)
			}
		}
		data, _ = editM3U(nil, filepath.Dir(pl.Path), nil, tracks, "", 0)
		removePath = ""
	} else if err != nil {
		return err // (a file that can't be read is not replaced)
	}
	b, err := editM3U(data, filepath.Dir(pl.Path), lib, add, removePath, nth)
	if err != nil {
		return err
	}
	os.MkdirAll(filepath.Dir(pl.Path), 0o755)
	return writeFileAtomic(pl.Path, b)
}

// editM3U: an M3U file with the songs of add at its end, and without the
// song at removePath (the nth song the library found in the file, or else
// the first line that points at it) and the #EXT lines just before it
// (#EXTINF…). Every other line stays as it was: songs the library doesn't
// have (on another card, a format this system skips, a URL), comments, a
// BOM, Windows' line ends. Rewriting the file from the library's songs would
// drop them all.
func editM3U(data []byte, baseDir string, lib *Library, add []*Track, removePath string, nth int) ([]byte, error) {
	text := string(data)
	cr := ""
	if strings.Contains(text, "\r\n") {
		cr = "\r"
	}
	var lines []string
	if strings.TrimSpace(text) != "" {
		lines = strings.Split(strings.TrimSuffix(text, "\n"), "\n")
	}
	if removePath != "" {
		var lower map[string]*Track
		at, k := -1, 0
		for i, l := range lines {
			l = m3uLine(l)
			if l == "" || strings.HasPrefix(l, "#") || lib == nil {
				continue
			}
			t := m3uTrack(lib, baseDir, l, &lower)
			if t == nil {
				continue
			}
			if t.Path == removePath && (k == nth || at < 0) {
				at = i
				if k == nth {
					break
				}
			}
			k++
		}
		if at < 0 {
			return nil, fmt.Errorf("%s is not in the playlist's file", filepath.Base(removePath))
		}
		from := at
		for from > 0 {
			l := m3uLine(lines[from-1])
			if !strings.HasPrefix(l, "#EXT") || strings.HasPrefix(l, "#EXTM3U") {
				break
			}
			from--
		}
		lines = append(lines[:from], lines[at+1:]...)
	}
	if len(lines) == 0 {
		lines = append(lines, "#EXTM3U"+cr)
	}
	for _, t := range add {
		p := t.Path
		if rel, err := filepath.Rel(baseDir, p); err == nil {
			p = rel
		}
		lines = append(lines, fmt.Sprintf("#EXTINF:%d,%s - %s%s", int(t.Duration), t.Artist, t.Title, cr), filepath.ToSlash(p)+cr)
	}
	return []byte(strings.Join(lines, "\n") + "\n"), nil
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
	var add []*Track
	for _, t := range tracks {
		if !have[t.Path] {
			add = append(add, t)
			have[t.Path] = true
		}
	}
	n := len(add)
	if n == 0 {
		return "Already in " + pl.Name
	}
	if err := editPlaylist(pl, ui.library(), add, "", 0); err != nil {
		logMsg("Playlists: " + err.Error())
		return "Couldn't save " + pl.Name
	}
	pl.Tracks = append(pl.Tracks, add...)
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
						if err := editPlaylist(pl, ui.library(), nil, t.Path, i); err != nil {
							logMsg("Playlists: " + err.Error())
							ui.showToast("Couldn't save " + pl.Name)
							return
						}
						pl.Tracks = append(append([]*Track{}, pl.Tracks[:i]...), pl.Tracks[i+1:]...)
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
