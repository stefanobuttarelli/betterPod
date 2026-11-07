package main

import (
	"bytes"
	"encoding/base64"
	"encoding/binary"
	"image"
	"image/color"
	"image/png"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func testArtwork(t *testing.T) []byte {
	t.Helper()
	var b bytes.Buffer
	img := image.NewRGBA(image.Rect(0, 0, 2, 2))
	img.Set(0, 0, color.RGBA{R: 200, A: 255})
	if err := png.Encode(&b, img); err != nil {
		t.Fatal(err)
	}
	return b.Bytes()
}

// Small container fixtures isolate tag rewriting from codecs. Their audio bytes
// must survive each edit byte for byte; metadata parsers read back each result.
func libraryFixture(t *testing.T, ext string) (string, []byte) {
	t.Helper()
	audio := []byte("AUDIO PAYLOAD THAT MUST STAY IDENTICAL\x00\xff\x01")
	var file []byte
	switch ext {
	case ".mp3":
		file = append([]byte{0xff, 0xfb, 0x90, 0x64}, audio...)
		audio = append([]byte{0xff, 0xfb, 0x90, 0x64}, audio...)
	case ".flac":
		// STREAMINFO followed by a vendor field and a custom comment.
		vc := vorbisComment{vendor: "Original vendor", fields: []string{"COMMENT=keep this", "DATE=1999-03-04"}}
		block := vc.bytes()
		file = append([]byte("fLaC\x00\x00\x00\x22"), make([]byte, 34)...)
		file = append(file, 0x84, byte(len(block)>>16), byte(len(block)>>8), byte(len(block)))
		file = append(file, block...)
		file = append(file, audio...)
	case ".m4a":
		ftyp := mp4Make("ftyp", []byte("M4A "), u32(0), []byte("isom"))
		originalMeta := mp4Make("udta", mp4Make("meta", u32(0), mp4Make("hdlr", u32(0), u32(0), []byte("mdir"), []byte("appl"), make([]byte, 8), []byte{0}), mp4Make("ilst", mp4Make("\xa9cmt", mp4Make("data", u32(1), u32(0), []byte("keep this"))))))
		file = append(ftyp, mp4Make("moov", originalMeta)...)
		file = append(file, mp4Make("mdat", audio)...)
	case ".ogg":
		first := &oggPage{flags: 2, serial: 12, seq: 0, lacing: []byte{30}, body: append([]byte("\x01vorbis"), make([]byte, 23)...)}
		first.body[11] = 2
		vc := vorbisComment{vendor: "Original vendor", fields: []string{"COMMENT=keep this"}}
		comment := append([]byte("\x03vorbis"), vc.bytes()...)
		comment = append(comment, 1)
		file = append(file, first.bytes()...)
		for _, p := range oggPaginate([][]byte{comment, []byte("\x05vorbis setup")}, 12, 1) {
			file = append(file, p.bytes()...)
		}
		audioPage := &oggPage{flags: 4, serial: 12, seq: 2, lacing: []byte{byte(len(audio))}, body: audio}
		file = append(file, audioPage.bytes()...)
	}
	path := filepath.Join(t.TempDir(), "01 Test"+ext)
	if err := os.WriteFile(path, file, 0640); err != nil {
		t.Fatal(err)
	}
	return path, audio
}

func changeFor(t *testing.T, path string) LibraryChange {
	t.Helper()
	st, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	return LibraryChange{Path: path, Size: st.Size(), Modified: st.ModTime().UnixNano()}
}

func TestLibraryMetadataAndArtworkRoundTrip(t *testing.T) {
	for _, ext := range []string{".mp3", ".flac", ".m4a", ".ogg"} {
		t.Run(ext, func(t *testing.T) {
			path, audio := libraryFixture(t, ext)
			title, album, artist, genre := "A new title ✓", "Correct album", "Correct artist", "Rock"
			track, total, disc, year := 2, 12, 1, 2025
			c := changeFor(t, path)
			c.Fields = LibraryFields{Title: &title, Album: &album, Artist: &artist, AlbumArtist: &artist, Genre: &genre, Track: &track, TrackTotal: &total, Disc: &disc, Year: &year}
			c.Cover = testArtwork(t)
			r := saveLibrary(LibrarySaveRequest{Tracks: []LibraryChange{c}})
			if len(r.Failures) > 0 || len(r.Saved) != 1 {
				t.Fatalf("save: %+v", r)
			}
			got, err := readLibraryTrack(path)
			if err != nil {
				t.Fatal(err)
			}
			if got.Title != title || got.Album != album || got.Artist != artist || got.Track != track || got.TrackTotal != total || got.Disc != disc || got.Year != year || got.Genre != genre {
				t.Fatalf("wrong metadata: %+v", got)
			}
			if !bytes.Equal(got.Cover, c.Cover) {
				t.Fatalf("cover did not round-trip (%d bytes)", len(got.Cover))
			}
			data, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Contains(data, audio) {
				t.Fatal("audio payload changed")
			}
			if ext != ".mp3" && !bytes.Contains(data, []byte("keep this")) {
				t.Fatal("unrelated comment changed")
			}
			st, _ := os.Stat(path)
			if st.Mode().Perm() != 0640 {
				t.Fatalf("permissions changed: %o", st.Mode().Perm())
			}
			// Changing only album keeps title, cover, numbering and other tags.
			album = "Another album"
			c = changeFor(t, path)
			c.Fields.Album = &album
			if err := saveLibraryTrack(c); err != nil {
				t.Fatal(err)
			}
			got, err = readLibraryTrack(path)
			if err != nil {
				t.Fatal(err)
			}
			if got.Title != title || got.Track != track || !bytes.Equal(got.Cover, testArtwork(t)) {
				t.Fatal("a sparse edit replaced unrelated fields")
			}
			// A cover can be removed without removing text or audio.
			c = changeFor(t, path)
			c.RemoveCover = true
			if err := saveLibraryTrack(c); err != nil {
				t.Fatal(err)
			}
			got, err = readLibraryTrack(path)
			if err != nil {
				t.Fatal(err)
			}
			if got.Cover != nil || got.Title != title {
				t.Fatal("remove cover damaged metadata")
			}
			data, _ = os.ReadFile(path)
			if !bytes.Contains(data, audio) {
				t.Fatal("audio changed after removal")
			}
			matches, _ := filepath.Glob(filepath.Join(filepath.Dir(path), ".betterpod-edit-*"))
			if len(matches) > 0 {
				t.Fatal("staged files leaked")
			}
		})
	}
}

func TestLibraryRejectsConflictAndLeavesOriginal(t *testing.T) {
	path, _ := libraryFixture(t, ".mp3")
	c := changeFor(t, path)
	title := "Changed"
	c.Fields.Title = &title
	before, _ := os.ReadFile(path)
	if err := os.WriteFile(path, append(before, 'x'), 0640); err != nil {
		t.Fatal(err)
	}
	changed, _ := os.ReadFile(path)
	if err := saveLibraryTrack(c); err == nil || !strings.Contains(err.Error(), "changed") {
		t.Fatalf("expected conflict, got %v", err)
	}
	after, _ := os.ReadFile(path)
	if !bytes.Equal(after, changed) {
		t.Fatal("original modified on conflict")
	}
}

func TestLibraryInvalidEditDoesNotModifyOriginal(t *testing.T) {
	path, _ := libraryFixture(t, ".flac")
	before, _ := os.ReadFile(path)
	c := changeFor(t, path)
	c.Cover = []byte("not artwork")
	if err := saveLibraryTrack(c); err == nil {
		t.Fatal("invalid artwork accepted")
	}
	after, _ := os.ReadFile(path)
	if !bytes.Equal(before, after) {
		t.Fatal("original modified on invalid artwork")
	}
	c = changeFor(t, path)
	year := -1
	c.Fields.Year = &year
	if err := saveLibraryTrack(c); err == nil {
		t.Fatal("negative year accepted")
	}
	after, _ = os.ReadFile(path)
	if !bytes.Equal(before, after) {
		t.Fatal("original modified on invalid number")
	}
}

func TestLibrarySymbolicLinkIsReadOnly(t *testing.T) {
	path, _ := libraryFixture(t, ".mp3")
	link := filepath.Join(filepath.Dir(path), "link.mp3")
	if err := os.Symlink(path, link); err != nil {
		t.Fatal(err)
	}
	got, err := readLibraryTrack(link)
	if err != nil {
		t.Fatal(err)
	}
	if got.Writable {
		t.Fatal("symlink is writable")
	}
	c := changeFor(t, link)
	title := "Changed"
	c.Fields.Title = &title
	if err := saveLibraryTrack(c); err == nil {
		t.Fatal("symlink write allowed")
	}
	original, err := os.Lstat(link)
	if err != nil || original.Mode()&os.ModeSymlink == 0 {
		t.Fatal("link was replaced")
	}
}

func TestLibraryReportsPartialSave(t *testing.T) {
	good, _ := libraryFixture(t, ".mp3")
	bad, _ := libraryFixture(t, ".mp3")
	a, b := changeFor(t, good), changeFor(t, bad)
	b.Modified--
	album := "Album"
	a.Fields.Album = &album
	b.Fields.Album = &album
	r := saveLibrary(LibrarySaveRequest{Tracks: []LibraryChange{a, b}})
	if len(r.Saved) != 1 || r.Saved[0] != good || len(r.Failures) != 1 || r.Failures[0].Path != bad {
		t.Fatalf("wrong result: %+v", r)
	}
}

func TestLibraryMalformedMetadataIsNotReplaced(t *testing.T) {
	path, _ := libraryFixture(t, ".mp3")
	data := append([]byte("ID3\x04\x00\x00\x00\x00\x00\x70"), []byte("truncated")...)
	if err := os.WriteFile(path, data, 0644); err != nil {
		t.Fatal(err)
	}
	c := changeFor(t, path)
	album := "Album"
	c.Fields.Album = &album
	if err := saveLibraryTrack(c); err == nil {
		t.Fatal("truncated metadata accepted")
	}
	got, _ := os.ReadFile(path)
	if !bytes.Equal(got, data) {
		t.Fatal("corrupt source changed")
	}
}

func TestLibraryPreservesOtherArtwork(t *testing.T) {
	vc := vorbisComment{vendor: "vendor", fields: []string{"TITLE=old"}}
	other := flacPicture(testArtwork(t))
	binary.BigEndian.PutUint32(other[:4], 4)
	vc.fields = append(vc.fields, "METADATA_BLOCK_PICTURE="+base64.StdEncoding.EncodeToString(other))
	vc.set(songTags{Cover: testArtwork(t)}, tagCover)
	count := 0
	for _, f := range vc.fields {
		if strings.HasPrefix(f, "METADATA_BLOCK_PICTURE=") {
			count++
		}
	}
	if count != 2 {
		t.Fatal("unrelated artwork removed")
	}
}

// On macOS, exercise real codecs as well as the compact container fixtures.
// afconvert is provided by the OS, so this adds no app or test dependency.
func TestLibraryM4ARealAudio(t *testing.T) {
	converter, err := exec.LookPath("afconvert")
	if err != nil {
		t.Skip("macOS afconvert is not available")
	}
	formats, _ := exec.Command(converter, "-hf").CombinedOutput()
	if len(formats) == 0 {
		t.Skip("afconvert codec discovery is unavailable")
	}
	dir := t.TempDir()
	source := filepath.Join(dir, "source.wav")
	var wave bytes.Buffer
	samples := 22050
	wave.WriteString("RIFF")
	binary.Write(&wave, binary.LittleEndian, uint32(36+samples*2))
	wave.WriteString("WAVEfmt ")
	binary.Write(&wave, binary.LittleEndian, uint32(16))
	for _, v := range []uint16{1, 1} {
		binary.Write(&wave, binary.LittleEndian, v)
	}
	binary.Write(&wave, binary.LittleEndian, uint32(44100))
	binary.Write(&wave, binary.LittleEndian, uint32(88200))
	binary.Write(&wave, binary.LittleEndian, uint16(2))
	binary.Write(&wave, binary.LittleEndian, uint16(16))
	wave.WriteString("data")
	binary.Write(&wave, binary.LittleEndian, uint32(samples*2))
	for i := 0; i < samples; i++ {
		binary.Write(&wave, binary.LittleEndian, int16(math.Sin(float64(i)*2*math.Pi*440/44100)*12000))
	}
	if err := os.WriteFile(source, wave.Bytes(), 0644); err != nil {
		t.Fatal(err)
	}
	run := func(t *testing.T, args ...string) {
		t.Helper()
		if data, err := exec.Command(converter, args...).CombinedOutput(); err != nil {
			t.Fatalf("afconvert: %v: %s", err, data)
		}
	}
	for _, codec := range []string{"aac ", "alac"} {
		t.Run(codec, func(t *testing.T) {
			if !strings.Contains(string(formats), "'"+codec+"'") {
				t.Skip("macOS codec is unavailable in this environment")
			}
			path := filepath.Join(t.TempDir(), "song.m4a")
			run(t, "-f", "m4af", "-d", codec, source, path)
			decode := func(suffix string) []byte {
				t.Helper()
				dest := filepath.Join(filepath.Dir(path), suffix+".wav")
				run(t, "-f", "WAVE", "-d", "LEI16", path, dest)
				data, err := os.ReadFile(dest)
				if err != nil {
					t.Fatal(err)
				}
				return data
			}
			before := decode("before")
			title, album, artist := "Real audio ✓", "Album", "Artist"
			c := changeFor(t, path)
			c.Fields = LibraryFields{Title: &title, Album: &album, Artist: &artist}
			c.Cover = testArtwork(t)
			if err := saveLibraryTrack(c); err != nil {
				t.Fatal(err)
			}
			got, err := readLibraryTrack(path)
			if err != nil {
				t.Fatal(err)
			}
			if got.Title != title || got.Album != album || !bytes.Equal(got.Cover, c.Cover) {
				t.Fatalf("metadata roundtrip failed: %+v", got)
			}
			after := decode("after")
			if !bytes.Equal(before, after) {
				t.Fatal("decoded PCM changed after metadata and cover edit")
			}
			c = changeFor(t, path)
			c.RemoveCover = true
			if err := saveLibraryTrack(c); err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(before, decode("cleared")) {
				t.Fatal("decoded PCM changed after cover removal")
			}
		})
	}
}

func TestLibraryRejectsMalformedCoverLengthsBeforeParsing(t *testing.T) {
	path, _ := libraryFixture(t, ".flac")
	bad := append(u32(3), u32(0xffffffff)...)
	data := append([]byte("fLaC\x00\x00\x00\x22"), make([]byte, 34)...)
	data = append(data, 0x86, 0, 0, byte(len(bad)))
	data = append(data, bad...)
	if err := os.WriteFile(path, data, 0644); err != nil {
		t.Fatal(err)
	}
	got, err := readLibraryTrack(path)
	if err != nil {
		t.Fatal(err)
	}
	if got.Writable || !strings.Contains(got.Warning, "cover lengths") {
		t.Fatalf("unsafe metadata was not read-only: %+v", got)
	}
}

func TestLibraryRemoveAllArtworkTypes(t *testing.T) {
	for _, ext := range []string{".mp3", ".flac", ".ogg"} {
		t.Run(ext, func(t *testing.T) {
			path, _ := libraryFixture(t, ext)
			c := changeFor(t, path)
			c.Cover = testArtwork(t)
			if err := saveLibraryTrack(c); err != nil {
				t.Fatal(err)
			}
			data, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			// Change the stored picture type from front to unspecified/back.
			switch ext {
			case ".mp3":
				marker := []byte("image/png\x00\x03\x00")
				index := bytes.Index(data, marker)
				if index < 0 {
					t.Fatal("no APIC fixture")
				}
				data[index+len("image/png")+1] = 4
			case ".flac":
				offset := 4
				for offset+4 < len(data) {
					n := int(data[offset+1])<<16 | int(data[offset+2])<<8 | int(data[offset+3])
					if data[offset]&0x7f == 6 {
						binary.BigEndian.PutUint32(data[offset+4:offset+8], 0)
						break
					}
					offset += 4 + n
				}
			case ".ogg":
				front := base64.StdEncoding.EncodeToString(flacPicture(testArtwork(t)))
				picture := flacPicture(testArtwork(t))
				binary.BigEndian.PutUint32(picture[:4], 4)
				back := base64.StdEncoding.EncodeToString(picture)
				data = bytes.Replace(data, []byte(front), []byte(back), 1)
				// The packet payload length stays the same. Rebuild page CRCs.
				reader := bytes.NewReader(data)
				var out bytes.Buffer
				for {
					p, e := readOggPage(reader)
					if e != nil {
						break
					}
					out.Write(p.bytes())
				}
				data = out.Bytes()
			}
			if err := os.WriteFile(path, data, 0640); err != nil {
				t.Fatal(err)
			}
			got, err := readLibraryTrack(path)
			if err != nil {
				t.Fatal(err)
			}
			if len(got.Cover) == 0 {
				t.Fatal("non-front fixture not displayed")
			}
			replacement := append(testArtwork(t), byte(0))
			c = changeFor(t, path)
			c.Cover = replacement
			if err := saveLibraryTrack(c); err != nil {
				t.Fatal(err)
			}
			got, err = readLibraryTrack(path)
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(got.Cover, replacement) {
				t.Fatal("replacement is hidden behind retained non-front artwork")
			}
			c = changeFor(t, path)
			c.RemoveCover = true
			if err := saveLibraryTrack(c); err != nil {
				t.Fatal(err)
			}
			got, err = readLibraryTrack(path)
			if err != nil {
				t.Fatal(err)
			}
			if len(got.Cover) > 0 {
				t.Fatal("non-front cover survived removal")
			}
		})
	}
}

func TestLibraryOggChainedStreamSequencePreserved(t *testing.T) {
	path, _ := libraryFixture(t, ".ogg")
	next := (&oggPage{flags: 2, serial: 99, seq: 0, lacing: []byte{7}, body: []byte("\x01vorbis")}).bytes()
	f, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0)
	if err != nil {
		t.Fatal(err)
	}
	f.Write(next)
	f.Close()
	// An otherwise valid picture with trailing PNG bytes makes the comment
	// span multiple Ogg pages, requiring sequence adjustment in the first stream.
	art := append(testArtwork(t), make([]byte, 90000)...)
	c := changeFor(t, path)
	c.Cover = art
	if err := saveLibraryTrack(c); err != nil {
		t.Fatal(err)
	}
	data, _ := os.ReadFile(path)
	reader := bytes.NewReader(data)
	found := false
	for {
		p, e := readOggPage(reader)
		if e != nil {
			break
		}
		if p.serial == 99 {
			found = true
			if p.seq != 0 {
				t.Fatalf("chained stream starts at sequence %d", p.seq)
			}
		}
	}
	if !found {
		t.Fatal("chained stream was removed")
	}
}
