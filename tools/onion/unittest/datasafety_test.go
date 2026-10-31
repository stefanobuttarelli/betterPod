//go:build ignore

// Tests for what keeps the user's data: playlists edited in place
// (ij_playlists.go, playlist.go), ID3 tags that aren't understood left
// alone (tagwrite.go), and the database folder's list (datadir.go). Run
// with tools/onion/unittest.sh.

package main

import (
	"bytes"
	"encoding/binary"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"testing"
)

// A playlist made elsewhere: a BOM, Windows' line ends and backslashes, a
// URL, a song the library doesn't have, a name in another case. Removing a
// song and adding one keep all the rest.
func TestPlaylistEditKeepsLines(t *testing.T) {
	root := t.TempDir()
	plDir := filepath.Join(root, "Playlists")
	one := &Track{Path: filepath.Join(root, "Music", "One.mp3"), Title: "One", Artist: "A"}
	two := &Track{Path: filepath.Join(plDir, "two.mp3"), Title: "Two", Artist: "B"}
	three := &Track{Path: filepath.Join(root, "Music", "three.mp3"), Title: "Three", Artist: "C", Duration: 61}
	lib := &Library{TracksByPath: map[string]*Track{one.Path: one, two.Path: two, three.Path: three}}

	src := "\uFEFF#EXTM3U\r\n" +
		"#EXTINF:10,A - One\r\n..\\Music\\One.mp3\r\n" +
		"#EXTINF:-1,Radio\r\nhttp://radio.example/stream\r\n" +
		"#EXTINF:3,D - On another card\r\n/media/other/song.m4a\r\n" +
		"#EXTINF:5,B - Two\r\nTWO.MP3\r\n" +
		"# a comment of the user's\r\n"
	pl := &Playlist{Name: "Mix", Path: filepath.Join(plDir, "Mix.m3u")}
	os.MkdirAll(plDir, 0o755)
	if err := os.WriteFile(pl.Path, []byte(src), 0o644); err != nil {
		t.Fatal(err)
	}
	(&MiyooPod{}).parsePlaylists(&Library{TracksByPath: lib.TracksByPath, Playlists: []*Playlist{pl}})
	if len(pl.Tracks) != 2 || pl.Tracks[0] != one || pl.Tracks[1] != two {
		t.Fatalf("parsed %d songs, want One and Two (backslashes, BOM, case)", len(pl.Tracks))
	}

	if err := editPlaylist(pl, lib, nil, two.Path, 1); err != nil {
		t.Fatal(err)
	}
	if err := editPlaylist(pl, lib, []*Track{three}, "", 0); err != nil {
		t.Fatal(err)
	}
	b, _ := os.ReadFile(pl.Path)
	got := string(b)
	want := "\uFEFF#EXTM3U\r\n" +
		"#EXTINF:10,A - One\r\n..\\Music\\One.mp3\r\n" +
		"#EXTINF:-1,Radio\r\nhttp://radio.example/stream\r\n" +
		"#EXTINF:3,D - On another card\r\n/media/other/song.m4a\r\n" +
		"# a comment of the user's\r\n" +
		"#EXTINF:61,C - Three\r\n../Music/three.mp3\r\n"
	if got != want {
		t.Fatalf("playlist after edits:\n%q\nwant:\n%q", got, want)
	}
	// a song that isn't in the file: an error, the file as it was
	if err := editPlaylist(pl, lib, nil, two.Path, 0); err == nil {
		t.Fatal("removing a song not in the file: no error")
	}
	if b2, _ := os.ReadFile(pl.Path); string(b2) != want {
		t.Fatal("the file changed after a failed edit")
	}
	if _, err := os.Stat(pl.Path + ".tmp"); err == nil {
		t.Fatal("a .tmp was left")
	}
}

// id3Tag: an ID3v2 tag of the version, flags and body, then some audio.
func id3Tag(ver, flags byte, body []byte) []byte {
	h := []byte{'I', 'D', '3', ver, 0, flags, 0, 0, 0, 0}
	putSyncsafe(h[6:10], len(body))
	return append(append(h, body...), 0xff, 0xfb, 0x90, 0x00, 1, 2, 3, 4)
}

func mkFrame(ver byte, id string, data []byte) []byte {
	h := make([]byte, 10)
	copy(h, id)
	if ver == 4 {
		putSyncsafe(h[4:8], len(data))
	} else {
		binary.BigEndian.PutUint32(h[4:8], uint32(len(data)))
	}
	return append(h, data...)
}

// A tag that isn't understood whole is not rewritten: its cover and lyrics
// would be lost.
func TestID3RefusesWhatItDoesntUnderstand(t *testing.T) {
	dir := t.TempDir()
	cover := append([]byte("\x00image/jpeg\x00\x03\x00"), bytes.Repeat([]byte{0xab}, 300)...)
	frames3 := append(mkFrame(3, "TIT2", []byte("\x00Old")), mkFrame(3, "APIC", cover)...)
	frames4 := append(mkFrame(4, "TIT2", []byte("\x03Old")), mkFrame(4, "APIC", cover)...)
	cases := map[string][]byte{
		// v2.3, an extended header far past the tag (or below 0 on 32 bits)
		"ext3-huge": id3Tag(3, 0x40, append([]byte{0xff, 0xff, 0xff, 0xf0, 0, 0}, frames3...)),
		// v2.4, an extended header longer than the tag
		"ext4-long": id3Tag(4, 0x40, append([]byte{0x7f, 0x7f, 0x7f, 0x7f, 1, 0}, frames4...)),
		// v2.4, an extended header too short to be one
		"ext4-short": id3Tag(4, 0x40, append([]byte{0, 0, 0, 1, 1, 0}, frames4...)),
		// a frame ID that isn't one (junk after the frames)
		"junk-id": id3Tag(3, 0, append(append([]byte{}, frames3...), []byte("ab\x01\x02\x00\x00\x00\x04\x00\x00wxyz")...)),
		// a frame running past the tag
		"long-frame": id3Tag(3, 0, append(append([]byte{}, frames3[:len(frames3)-10]...))),
	}
	for name, data := range cases {
		p := filepath.Join(dir, name+".mp3")
		if err := os.WriteFile(p, data, 0o644); err != nil {
			t.Fatal(err)
		}
		err := writeTagsMasked(p, ".mp3", songTags{Title: "New"}, tagTitle)
		if err == nil {
			t.Errorf("%s: written, want an error", name)
		}
		if b, _ := os.ReadFile(p); !bytes.Equal(b, data) {
			t.Errorf("%s: the file changed", name)
		}
	}

	// a good tag is rewritten, the cover kept; v2.4 sizes written plain
	// (older iTunes) too
	plain := append(mkFrame(3, "TIT2", []byte("\x03Old")), mkFrame(3, "APIC", cover)...)
	for name, data := range map[string][]byte{
		"good3":  id3Tag(3, 0, append(append([]byte{}, frames3...), make([]byte, 64)...)),
		"good4":  id3Tag(4, 0, frames4),
		"plain4": id3Tag(4, 0, plain),
		"ext3":   id3Tag(3, 0x40, append([]byte{0, 0, 0, 6, 0, 0, 0, 0, 0, 0}, frames3...)),
	} {
		p := filepath.Join(dir, name+".mp3")
		os.WriteFile(p, data, 0o644)
		if err := writeTagsMasked(p, ".mp3", songTags{Title: "New"}, tagTitle); err != nil {
			t.Errorf("%s: %v", name, err)
			continue
		}
		f, _ := os.Open(p)
		_, frames, _, err := readID3(f)
		f.Close()
		if err != nil {
			t.Errorf("%s: read back: %v", name, err)
			continue
		}
		var ids []string
		for _, fr := range frames {
			ids = append(ids, fr.id)
			if fr.id == "APIC" && !bytes.Equal(fr.data, cover) {
				t.Errorf("%s: the cover changed", name)
			}
		}
		if !slices.Contains(ids, "APIC") || !slices.Contains(ids, "TIT2") {
			t.Errorf("%s: frames after the write: %v", name, ids)
		}
	}
}

// Every file the app keeps in the database folder is in dataItems, so a
// move takes it along (and a database moved deletes only what it copied).
func TestDataItemsComplete(t *testing.T) {
	files, _ := filepath.Glob("*.go")
	res := []*regexp.Regexp{
		regexp.MustCompile(`dataPath\("([^"]+)"\)`),
		regexp.MustCompile(`dataPath\(filepath\.Join\("([^"]+)"`),
		regexp.MustCompile(`filepath\.Join\(dataDir\(\), "([^"]+)"`),
		regexp.MustCompile(`filepath\.Join\(d, (mergesFile)\)`),
	}
	seen := 0
	for _, f := range files {
		if strings.HasSuffix(f, "_test.go") {
			continue
		}
		b, _ := os.ReadFile(f)
		for _, re := range res {
			for _, m := range re.FindAllStringSubmatch(string(b), -1) {
				name := strings.Split(strings.TrimSuffix(m[1], "/"), "/")[0]
				if name == "mergesFile" {
					name = mergesFile
				}
				seen++
				if name != "tmp" && !slices.Contains(dataItems, name) {
					t.Errorf("%s: %q is kept in the data folder but not in dataItems", f, name)
				}
			}
		}
	}
	if seen < 20 {
		t.Fatalf("only %d data files found in the code: is the test in src?", seen)
	}
}

// A move copies, then deletes only what was copied.
func TestDataFolderCopy(t *testing.T) {
	src, dst := t.TempDir(), t.TempDir()
	put := func(rel, s string) {
		p := filepath.Join(src, rel)
		os.MkdirAll(filepath.Dir(p), 0o755)
		os.WriteFile(p, []byte(s), 0o644)
	}
	put("album_merges.json", "{}")
	put("chosen_covers/abc.jpg", "jpg")
	put("puzzles/net.sav", "save")
	put("tools/qjs", "#!/bin/sh")
	os.Chmod(filepath.Join(src, "tools/qjs"), 0o755)
	put("tmp/listen/x.part", "part")
	put("unknown.txt", "the user's") // a shared folder's own file
	put(dataDirPointer, "/x")

	if err := copyDatabase(src, dst, false, nil); err != nil {
		t.Fatal(err)
	}
	for _, rel := range []string{"album_merges.json", "chosen_covers/abc.jpg", "puzzles/net.sav", "tools/qjs"} {
		if _, err := os.Stat(filepath.Join(dst, rel)); err != nil {
			t.Errorf("%s not copied", rel)
		}
	}
	if st, err := os.Stat(filepath.Join(dst, "tools/qjs")); err != nil || st.Mode().Perm()&0o100 == 0 {
		t.Error("tools/qjs lost its mode")
	}
	for _, rel := range []string{"tmp", "unknown.txt", dataDirPointer} {
		if _, err := os.Stat(filepath.Join(dst, rel)); err == nil {
			t.Errorf("%s copied", rel)
		}
	}
	// written after the copy: kept in the old folder
	put("puzzles/net.sav", "saved again, longer")
	if n := removeCopied(src, dst, dataItems); n != 1 {
		t.Errorf("kept %d files, want 1", n)
	}
	for rel, want := range map[string]bool{"album_merges.json": false, "chosen_covers": false, "tools": false,
		"puzzles/net.sav": true, "unknown.txt": true, dataDirPointer: true, "tmp/listen/x.part": true} {
		if _, err := os.Stat(filepath.Join(src, rel)); (err == nil) != want {
			t.Errorf("%s: there %v, want %v", rel, err == nil, want)
		}
	}

	// a folder of its own (Move Music): all of it but tmp/ and the pointers
	items := folderItems(src)
	slices.Sort(items)
	if want := []string{"puzzles", "unknown.txt"}; !slices.Equal(items, want) {
		t.Errorf("folderItems = %v, want %v", items, want)
	}
}
