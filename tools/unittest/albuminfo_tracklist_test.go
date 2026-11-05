//go:build ignore

package main

import (
	"fmt"
	"path/filepath"
	"reflect"
	"testing"
)

// A wrong first chapter title can shift every subsequent title. The full
// selected tracklist must replace all 16 songs, even with different cuts
// and after an earlier title-based correction saved wrong numbers too.
func TestAlbumInfoReplacesCompleteTracklist(t *testing.T) {
	root := t.TempDir()
	t.Setenv("BETTERPOD_MUSIC_DIR", root)
	for _, mode := range []string{"numbered tags", "YouTube permuted numbers", "YouTube duplicate numbers"} {
		t.Run(mode, func(t *testing.T) {
			al := &Album{Name: "Odio pieno", Artist: "Old artist"}
			a := &albumInfo{Title: "ODIO PIENO (Remastered)", Artist: "Colle der Fomento", Year: "1996", Genre: "Hip-Hop", Count: 16}
			want := map[string]songTags{}
			for i := 0; i < 16; i++ {
				oldTitle := fmt.Sprintf("song %02d", (i+1)%16+1)
				dir, ext, num := filepath.Join(root, "Rips", "Old artist", al.Name), ".mp3", i+1
				if mode != "numbered tags" {
					dir, ext, num = filepath.Join(root, "YouTube", "Old artist", al.Name), ".m4a", (i+1)%16+1
				}
				if mode == "YouTube duplicate numbers" {
					num = 1
				}
				p := filepath.Join(dir, fmt.Sprintf("%02d %s%s", i+1, oldTitle, ext))
				al.Tracks = append(al.Tracks, &Track{Path: p, Title: oldTitle, Album: al.Name, Artist: "Old artist",
					AlbumArtist: "Old album artist", Genre: "Rock", Year: 2025, TrackNum: num, TrackTotal: 99, DiscNum: 2, Duration: float64(900 - i)})
				x := infoTrack{Num: i + 1, Title: fmt.Sprintf("Song %02d (Remastered)", i+1), Secs: float64(100 + i)}
				artist := a.Artist // MusicBrainz may omit a track credit inherited from the album.
				if i%3 == 0 {
					x.Artist, artist = "Colle der Fomento feat. Guest", "Colle der Fomento feat. Guest"
				}
				a.Tracks = append(a.Tracks, x)
				want[p] = songTags{Title: x.Title, Album: a.Title, AlbumArtist: a.Artist, Artist: artist,
					Genre: a.Genre, Year: 1996, Track: i + 1, TrackTotal: 16, Disc: 1}
			}
			// Source response order is not necessarily track order.
			for i, j := 0, len(a.Tracks)-1; i < j; i, j = i+1, j-1 {
				a.Tracks[i], a.Tracks[j] = a.Tracks[j], a.Tracks[i]
			}
			e := newAlbumEdit(al)
			if n := e.apply(a); n != 16 {
				t.Fatalf("updated %d of 16 songs", n)
			}
			for _, s := range e.songs {
				if s.tags != want[s.t.Path] {
					t.Errorf("%s: got %+v, want %+v", filepath.Base(s.t.Path), s.tags, want[s.t.Path])
				}
				if tagsOf(s.t) != s.orig {
					t.Errorf("%s: source selection changed the library before saving", s.t.Path)
				}
			}
			if changed, writable := e.changed(); changed != 16 || writable != 16 {
				t.Errorf("pending changes: %d changed, %d writable", changed, writable)
			}
		})
	}
}

func TestAlbumInfoReplacesEquivalentSpellings(t *testing.T) {
	track := &Track{Path: "/music/menu.mp3", Title: "menu", Album: "game", Artist: "Wrong artist", AlbumArtist: "Wrong album artist",
		Year: 2025, Genre: "Wrong genre", TrackNum: 1, TrackTotal: 99, DiscNum: 3}
	e := newAlbumEdit(&Album{Tracks: []*Track{track}})
	a := &albumInfo{Title: "Game (Deluxe Edition)", Artist: "Album artist", Year: "2000", Genre: "Soundtrack",
		Tracks: []infoTrack{{Num: 1, Title: "Menu (Remastered)", Artist: "Track artist"}}}
	if n := e.apply(a); n != 1 {
		t.Fatalf("updated %d songs", n)
	}
	want := songTags{Title: "Menu (Remastered)", Album: a.Title, Artist: "Track artist", AlbumArtist: "Album artist",
		Genre: "Soundtrack", Year: 2000, Track: 1, TrackTotal: 1, Disc: 1}
	if got := e.songs[0].tags; got != want {
		t.Errorf("got %+v, want %+v", got, want)
	}
}

func TestAlbumInfoDiscTracklists(t *testing.T) {
	for _, mode := range []string{"global numbering", "disc numbering"} {
		t.Run(mode, func(t *testing.T) {
			al := &Album{}
			a := &albumInfo{Tracks: []infoTrack{
				{Disc: 2, Num: 2, Title: "Finale"}, {Disc: 1, Num: 2, Title: "Second"},
				{Disc: 2, Num: 1, Title: "Third"}, {Disc: 1, Num: 1, Title: "First"}}}
			wants := []songTags{
				{Title: "First", Disc: 1, Track: 1, TrackTotal: 2}, {Title: "Second", Disc: 1, Track: 2, TrackTotal: 2},
				{Title: "Third", Disc: 2, Track: 1, TrackTotal: 2}, {Title: "Finale", Disc: 2, Track: 2, TrackTotal: 2}}
			for i := 3; i >= 0; i-- {
				num, disc := i+1, 0
				if mode == "disc numbering" {
					num, disc = i%2+1, i/2+1
				}
				al.Tracks = append(al.Tracks, &Track{Path: fmt.Sprintf("/music/%d.mp3", i), Title: "Wrong title", TrackNum: num, DiscNum: disc})
			}
			e := newAlbumEdit(al)
			if n := e.apply(a); n != 4 {
				t.Fatalf("updated %d of 4", n)
			}
			for i, s := range e.songs {
				if s.tags != wants[i] {
					t.Errorf("song %d: got %+v, want %+v", i, s.tags, wants[i])
				}
			}
		})
	}
}

func TestAlbumInfoDoesNotGuessIncompleteOrder(t *testing.T) {
	for _, mode := range []string{"unnumbered files", "fewer local songs", "truncated source"} {
		t.Run(mode, func(t *testing.T) {
			al := &Album{Tracks: []*Track{
				{Path: "/music/b.mp3", Title: "Unknown B", TrackTotal: 99, Duration: 900},
				{Path: "/music/a.mp3", Title: "Unknown A", TrackTotal: 99, Duration: 800}}}
			a := &albumInfo{Count: 2, Tracks: []infoTrack{{Num: 1, Title: "First", Secs: 60}, {Num: 2, Title: "Second", Secs: 120}}}
			if mode != "unnumbered files" {
				al.Tracks[0].TrackNum, al.Tracks[1].TrackNum = 1, 2
			}
			if mode == "fewer local songs" {
				a.Count = 3
				a.Tracks = append(a.Tracks, infoTrack{Num: 3, Title: "Third", Secs: 180})
			}
			if mode == "truncated source" {
				a.Count = 3 // The source says 3 songs but only returned 2.
			}
			e := newAlbumEdit(al)
			if n := e.apply(a); n != 0 {
				t.Fatalf("guessed %d matches without a complete order", n)
			}
			for _, s := range e.songs {
				if s.tags != s.orig {
					t.Errorf("changed an unmatched song: %+v", s.tags)
				}
			}
		})
	}
}

func TestAlbumInfoKeepsFieldsAbsentFromSource(t *testing.T) {
	track := &Track{Path: "/music/a.mp3", Title: "Song", Album: "Old album", Artist: "Known artist", AlbumArtist: "Known album artist",
		Genre: "Known genre", Year: 1996, TrackNum: 1, TrackTotal: 16, DiscNum: 2}
	e := newAlbumEdit(&Album{Tracks: []*Track{track}})
	want := e.songs[0].orig
	want.Album = "New album"
	if n := e.apply(&albumInfo{Title: want.Album, Year: "invalid"}); n != 0 {
		t.Errorf("a source without songs paired %d", n)
	}
	if got := e.songs[0].tags; got != want {
		t.Errorf("got %+v, want %+v", got, want)
	}
}

func TestMatchNumberedTracksRejectsAmbiguousNumbers(t *testing.T) {
	theirs := []infoTrack{{Num: 1}, {Num: 2}, {Num: 3}}
	for _, ours := range [][]songTags{
		{{}, {}, {}}, {{Track: 1}, {Track: 1}, {Track: 3}}, {{Track: 1}, {Track: 3}, {Track: 4}},
	} {
		if m := matchNumberedTracks(ours, theirs); m != nil {
			t.Errorf("ambiguous numbers %+v gave %v", ours, m)
		}
	}
	ours := []songTags{{Track: 1}, {Track: 2}, {Track: 3}}
	for _, tracks := range [][]infoTrack{
		{{Num: 1}, {Num: 1}, {Num: 3}}, {{Num: 1}, {Num: 3}, {Num: 4}},
		{{Disc: 2, Num: 1}, {Disc: 2, Num: 2}, {Disc: 2, Num: 3}},
		{{Disc: 1, Num: 1}, {Disc: 3, Num: 1}, {Disc: 3, Num: 2}},
	} {
		if m := matchNumberedTracks(ours, tracks); m != nil {
			t.Errorf("incomplete source %+v gave %v", tracks, m)
		}
	}
	if m := matchNumberedTracks([]songTags{{Track: 3}, {Track: 1}, {Track: 2}}, theirs); !reflect.DeepEqual(m, []int{2, 0, 1}) {
		t.Errorf("numbered order: %v", m)
	}
}
