//go:build ignore

package main

// M4A edits: real AAC and ALAC still decode to the same audio, while only
// the selected tags change. Run with tools/onion/unittest.sh.

import (
	"bytes"
	"encoding/binary"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/dhowden/tag"
)

func m4aMetadata(t *testing.T, path string) tag.Metadata {
	t.Helper()
	f, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	m, err := tag.ReadFrom(f)
	if err != nil {
		t.Fatal(err)
	}
	return m
}

func TestWriteM4ATagsRealAudio(t *testing.T) {
	if _, err := exec.LookPath("ffmpeg"); err != nil {
		t.Skip("ffmpeg not installed")
	}
	dir := t.TempDir()
	wav, cover := filepath.Join(dir, "source.wav"), filepath.Join(dir, "cover.jpg")
	sineWAV(t, wav)
	art := jpegBytes()
	if err := os.WriteFile(cover, art, 0644); err != nil {
		t.Fatal(err)
	}
	for _, codec := range []string{"aac", "alac"} {
		for _, layout := range []string{"front", "end"} {
			t.Run(codec+"/"+layout, func(t *testing.T) {
				path := filepath.Join(t.TempDir(), "song.m4a")
				args := []string{"-v", "error", "-i", wav, "-i", cover, "-map", "0:a", "-map", "1:v",
					"-c:a", codec, "-c:v", "copy", "-disposition:v", "attached_pic",
					"-metadata", "title=Old title", "-metadata", "artist=Old artist", "-metadata", "album=Old album",
					"-metadata", "album_artist=Old album artist", "-metadata", "genre=Rock", "-metadata", "date=2000-05-12",
					"-metadata", "track=3/12", "-metadata", "disc=1/2", "-metadata", "comment=Keep this comment",
					"-metadata", "lyrics=Keep these lyrics"}
				if layout == "front" {
					args = append(args, "-movflags", "+faststart")
				}
				runTool(t, "ffmpeg", append(args, path)...)
				decode := func() []byte {
					t.Helper()
					b, err := exec.Command("ffmpeg", "-v", "error", "-i", path, "-map", "0:a:0", "-f", "s16le", "-").Output()
					if err != nil {
						t.Fatalf("decode: %v", err)
					}
					return b
				}
				before := decode()
				if len(before) == 0 {
					t.Fatal("no decoded audio")
				}
				if err := writeTagsMasked(path, ".m4a", songTags{Album: "Città — 東京"}, tagAlbum); err != nil {
					t.Fatal(err)
				}
				m := m4aMetadata(t, path)
				if m.Album() != "Città — 東京" || m.Title() != "Old title" || m.Artist() != "Old artist" ||
					m.AlbumArtist() != "Old album artist" || m.Genre() != "Rock" || m.Raw()["\xa9day"] != "2000-05-12" ||
					m.Comment() != "Keep this comment" || m.Lyrics() != "Keep these lyrics" {
					t.Errorf("tags after the rename: %+v", m.Raw())
				}
				if m.Picture() == nil || !bytes.Equal(m.Picture().Data, art) {
					t.Error("the embedded cover changed")
				}
				if !bytes.Equal(before, decode()) {
					t.Error("the decoded audio changed after renaming the album")
				}
				for _, change := range []struct {
					mask tagMask
					tags songTags
				}{{tagTrack, songTags{Track: 4}}, {tagTrackTotal, songTags{TrackTotal: 15}}, {tagDisc, songTags{Disc: 2}}} {
					if err := writeTagsMasked(path, ".m4a", change.tags, change.mask); err != nil {
						t.Fatal(err)
					}
				}
				m = m4aMetadata(t, path)
				if n, total := m.Track(); n != 4 || total != 15 {
					t.Errorf("track = %d/%d, want 4/15", n, total)
				}
				if n, total := m.Disc(); n != 2 || total != 2 {
					t.Errorf("disc = %d/%d, want 2/2", n, total)
				}
				want := songTags{Title: "New title", Artist: "New artist", Album: "New album", AlbumArtist: "New album artist",
					Genre: "Jazz", Year: 2026, Track: 7, TrackTotal: 20, Disc: 3}
				if err := writeTags(path, want); err != nil {
					t.Fatal(err)
				}
				m = m4aMetadata(t, path)
				if m.Title() != want.Title || m.Artist() != want.Artist || m.Album() != want.Album || m.AlbumArtist() != want.AlbumArtist ||
					m.Genre() != want.Genre || m.Year() != want.Year || m.Comment() != "Keep this comment" || m.Lyrics() != "Keep these lyrics" {
					t.Errorf("full tag edit: %+v", m.Raw())
				}
				if !bytes.Equal(before, decode()) {
					t.Error("the decoded audio changed after editing all fields")
				}
				st, _ := os.Stat(path)
				if err := writeTags(path, want); err != nil {
					t.Fatal(err)
				}
				if after, _ := os.Stat(path); after.Size() != st.Size() {
					t.Error("saving the same tags again grew the file")
				}
				if err := writeTagsMasked(path, ".m4a", songTags{}, tagTitle|tagAlbumArtist|tagGenre|tagYear); err != nil {
					t.Fatal(err)
				}
				m = m4aMetadata(t, path)
				if m.Title() != "" || m.AlbumArtist() != "" || m.Genre() != "" || m.Year() != 0 || m.Album() != want.Album {
					t.Errorf("clearing fields: %+v", m.Raw())
				}
				if _, err := os.Stat(path + ".tagtmp"); !os.IsNotExist(err) {
					t.Error("a temporary file was left behind")
				}
			})
		}
	}
}

// The fixture includes a freeform tag, a chapter box, a numeric genre and
// dates/totals the library does not fully expose. Their bytes must survive.
func TestM4ATagsKeepUnknownData(t *testing.T) {
	text := func(key, value string) []byte { return mp4Make(key, mp4Make("data", u32(1), u32(0), []byte(value))) }
	unknown := mp4Make("----", mp4Make("mean", u32(0), []byte("com.apple.iTunes")),
		mp4Make("name", u32(0), []byte("REPLAYGAIN_TRACK_GAIN")), mp4Make("data", u32(1), u32(0), []byte("-4.25 dB")))
	chapters := mp4Make("chpl", []byte{0, 0, 0, 0, 0, 0, 0, 0, 0})
	date := text("\xa9day", "2000-05-12")
	meta := mp4Make("meta", u32(0), mp4Make("ilst", unknown, date, text("\xa9art", "Old artist"),
		text("\xa9alb", "Old album"), mp4Make("gnre", mp4Make("data", u32(0), u32(0), u16(18)))))
	ftyp := mp4Make("ftyp", []byte("M4A "), u32(0), []byte("isom"))
	moov := mp4Make("moov", mp4Make("udta", chapters, meta))
	mdat := mp4Make("mdat", []byte("audio bytes stay at the same offset"))
	for _, layout := range []string{"front", "end", "size-zero-mdat", "extended-mdat", "no-tags"} {
		t.Run(layout, func(t *testing.T) {
			file := append(append(append([]byte{}, ftyp...), moov...), mdat...)
			switch layout {
			case "end":
				file = bytes.Join([][]byte{ftyp, mdat, moov}, nil)
			case "size-zero-mdat":
				binary.BigEndian.PutUint32(file[len(ftyp)+len(moov):], 0)
			case "extended-mdat":
				wide := append(append(u32(1), []byte("mdat")...), make([]byte, 8)...)
				binary.BigEndian.PutUint64(wide[8:], uint64(len(mdat)+8))
				file = bytes.Join([][]byte{ftyp, moov, wide, mdat[8:]}, nil)
			case "no-tags":
				file = bytes.Join([][]byte{ftyp, mdat, mp4Make("moov")}, nil)
			}
			path := filepath.Join(t.TempDir(), "song.M4A")
			if err := os.WriteFile(path, file, 0644); err != nil {
				t.Fatal(err)
			}
			if err := writeTagsMasked(path, ".M4A", songTags{Album: "New album", Artist: "New artist", Genre: "Jazz"}, tagAlbum|tagArtist|tagGenre); err != nil {
				t.Fatal(err)
			}
			got, _ := os.ReadFile(path)
			oldAudio := bytes.Index(file, mdat[8:])
			if !bytes.Equal(got[oldAudio:oldAudio+len(mdat)-8], mdat[8:]) {
				t.Error("the audio moved or changed")
			}
			bs, err := mp4Boxes(bytes.NewReader(got), 0, int64(len(got)))
			if err != nil {
				t.Fatal(err)
			}
			var edited []byte
			n := 0
			for _, b := range bs {
				if b.typ == "moov" {
					n++
					edited = got[b.body:b.end]
				}
			}
			if n != 1 {
				t.Fatalf("%d moov boxes, want one", n)
			}
			if layout != "no-tags" {
				for _, kept := range [][]byte{unknown, chapters, date} {
					if !bytes.Contains(edited, kept) {
						t.Error("untouched metadata changed")
					}
				}
			}
			if bytes.Contains(edited, []byte("gnre")) || bytes.Contains(edited, text("\xa9art", "Old artist")) {
				t.Error("an old spelling of an edited field was left behind")
			}
			m := m4aMetadata(t, path)
			if m.Album() != "New album" || m.Artist() != "New artist" || m.Genre() != "Jazz" {
				t.Errorf("edited tags: %+v", m.Raw())
			}
		})
	}
}

func TestM4ATagsRefuseUnsafeFiles(t *testing.T) {
	ftyp := mp4Make("ftyp", []byte("M4A "), u32(0), []byte("isom"))
	mdat := mp4Make("mdat", []byte("unchanged audio"))
	meta := func(body []byte) []byte { return mp4Make("moov", mp4Make("udta", mp4Make("meta", body))) }
	good := m4aUdta(m4aTags{Album: "Old album"})
	cases := map[string][]byte{
		"missing movie":     nil,
		"bad child":         mp4Make("moov", []byte{1, 2, 3}),
		"duplicate movies":  bytes.Join([][]byte{mp4Make("moov"), mp4Make("moov")}, nil),
		"fragmented":        bytes.Join([][]byte{mp4Make("moov"), mp4Make("moof")}, nil),
		"fragment defaults": mp4Make("moov", mp4Make("mvex")),
		"compressed movie":  mp4Make("moov", mp4Make("cmov")),
		"direct metadata":   mp4Make("moov", mp4Make("meta", u32(0))),
		"new meta version":  meta(u32(0x1000000)),
		"keyed metadata":    meta(bytes.Join([][]byte{u32(0), mp4Make("keys")}, nil)),
		"other handler":     meta(bytes.Join([][]byte{u32(0), mp4Make("hdlr", u32(0), u32(0), []byte("mdta"))}, nil)),
		"broken items":      meta(bytes.Join([][]byte{u32(0), mp4Make("ilst", []byte{1, 2, 3})}, nil)),
		"zero-size child":   mp4Make("moov", u32(0), []byte("udta")),
		"huge box":          bytes.Join([][]byte{u32(1), []byte("moov"), bytes.Repeat([]byte{255}, 8)}, nil),
		"trailing junk":     append(mp4Make("moov", good), 1),
	}
	for name, middle := range cases {
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "song.m4a")
			original := bytes.Join([][]byte{ftyp, middle, mdat}, nil)
			if err := os.WriteFile(path, original, 0644); err != nil {
				t.Fatal(err)
			}
			if err := safeWriteTags(path, songTags{Album: "New album"}, tagAlbum); err == nil {
				t.Fatal("unsafe M4A accepted")
			}
			if after, _ := os.ReadFile(path); !bytes.Equal(after, original) {
				t.Error("the original changed after a failed edit")
			}
			if _, err := os.Stat(path + ".tagtmp"); !os.IsNotExist(err) {
				t.Error("a temporary file was left after a failed edit")
			}
		})
	}
	path := filepath.Join(t.TempDir(), "song.m4a")
	original := bytes.Join([][]byte{ftyp, mp4Make("moov", good), mdat}, nil)
	os.WriteFile(path, original, 0644)
	if err := writeTagsMasked(path, ".m4a", songTags{Track: 65536}, tagTrack); err == nil || !strings.Contains(err.Error(), "65535") {
		t.Fatalf("overflowing track number: %v", err)
	}
	if after, _ := os.ReadFile(path); !bytes.Equal(after, original) {
		t.Error("invalid track number changed the original")
	}
	os.WriteFile(path+".tagtmp", []byte("an existing file"), 0644)
	if err := writeTagsMasked(path, ".m4a", songTags{Album: "New album"}, tagAlbum); err == nil {
		t.Fatal("an existing temporary file was overwritten")
	}
	if b, _ := os.ReadFile(path + ".tagtmp"); string(b) != "an existing file" {
		t.Error("the existing temporary file changed")
	}
}
