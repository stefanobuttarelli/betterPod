package main

// This check starts the real Rust server on copied temporary music. No console,
// relay or user library is contacted. The pairing code stays in subprocess pipes.

import (
	"bufio"
	"bytes"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"io"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type lanInteropTrack struct {
	LibraryTrack
	DisplayAlbum       *string `json:"displayAlbum"`
	DisplayAlbumArtist *string `json:"displayAlbumArtist"`
	AlbumGroup         string  `json:"albumGroup"`
	DisplayAlbumID     string  `json:"displayAlbumID"`
}

func TestLanRustInterop(t *testing.T) {
	fixture := os.Getenv("BETTERPOD_LAN_FIXTURE")
	if fixture == "" {
		t.Skip("set BETTERPOD_LAN_FIXTURE to the compiled wifi_library_fixture example")
	}
	converter, err := exec.LookPath("afconvert")
	if err != nil {
		t.Fatal("The Rust/Mac interop check requires macOS afconvert")
	}
	directory := t.TempDir()
	directory, err = filepath.EvalSymlinks(directory)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, ".betterpod-wifi-test-fixture"), []byte("betterpod-wifi-library-test-v1"), 0600); err != nil {
		t.Fatal(err)
	}
	music, data := filepath.Join(directory, "music"), filepath.Join(directory, "data")
	for _, path := range []string{music, data} {
		if err := os.Mkdir(path, 0700); err != nil {
			t.Fatal(err)
		}
	}
	source := filepath.Join(directory, "source.wav")
	lanInteropWave(t, source)
	run := func(args ...string) {
		t.Helper()
		if output, err := exec.Command(converter, args...).CombinedOutput(); err != nil {
			t.Fatalf("Local audio fixture conversion failed: %v: %s", err, output)
		}
	}
	fixturePaths := []string{filepath.Join(music, "First folder", "01 Fixture.m4a"), filepath.Join(music, "Second folder", "02 Fixture.m4a")}
	for index, path := range fixturePaths {
		i := index + 1
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		run("-f", "m4af", "-d", "aac ", source, path)
		if err := writeTagsMasked(path, ".m4a", songTags{Title: fmt.Sprintf("Fixture %d", i), Artist: "Fixture artist", Album: "Fixture album", Genre: "Jazz", Year: 2026, Track: i, TrackTotal: 2, Disc: 1}, tagTitle|tagArtist|tagAlbum|tagGenre|tagYear|tagTrack|tagTrackTotal|tagDisc); err != nil {
			t.Fatal(err)
		}
	}
	// Start with two private logical albums over equal embedded tags. A remote
	// normalization must explicitly clear these overrides, even for equal values.
	privateCover := filepath.Join(data, "chosen-covers", "fixture.png")
	if err := os.MkdirAll(filepath.Dir(privateCover), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(privateCover, testArtwork(t), 0600); err != nil {
		t.Fatal(err)
	}
	overrides := make(map[string]any)
	for index, path := range fixturePaths {
		i := index + 1
		overrides[path] = map[string]any{
			"album": fmt.Sprintf("Private album %d", i), "artist": "Private album artist", "group": fmt.Sprintf("private:fixture-%d", i),
			"cover": privateCover, "disc": 7, "added": true,
			"prior": map[string]any{"album": "Older private album", "group": "private:older"},
		}
	}
	manifest, _ := json.Marshal(map[string]any{"tracks": overrides})
	if err := os.WriteFile(filepath.Join(data, "curation.json"), manifest, 0600); err != nil {
		t.Fatal(err)
	}
	outside := filepath.Join(directory, "outside.m4a")
	outData, err := os.ReadFile(fixturePaths[0])
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(outside, outData, 0600); err != nil {
		t.Fatal(err)
	}
	command := exec.Command(fixture, music, data)
	stdin, err := command.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	stdout, err := command.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	var diagnostics bytes.Buffer
	command.Stderr = &diagnostics
	if err := command.Start(); err != nil {
		t.Fatal("Could not start the Rust LAN fixture: ", err)
	}
	finished := make(chan error, 1)
	go func() { finished <- command.Wait() }()
	t.Cleanup(func() {
		_, _ = io.WriteString(stdin, "stop\n")
		_ = stdin.Close()
		select {
		case <-finished:
		case <-time.After(5 * time.Second):
			_ = command.Process.Kill()
			<-finished
		}
	})
	ready := make(chan []byte, 1)
	go func() {
		scanner := bufio.NewScanner(stdout)
		if scanner.Scan() {
			ready <- append([]byte(nil), scanner.Bytes()...)
		} else {
			ready <- nil
		}
	}()
	var details struct {
		Version int    `json:"v"`
		Host    string `json:"host"`
		Port    int    `json:"port"`
		ID      string `json:"id"`
		Code    string `json:"code"`
		Total   int    `json:"total"`
	}
	select {
	case line := <-ready:
		if len(line) == 0 || json.Unmarshal(line, &details) != nil || details.Version != 1 || details.Total != 2 {
			t.Fatal("The Rust fixture did not announce a valid two-track session")
		}
	case <-time.After(15 * time.Second):
		t.Fatal("The Rust fixture did not become ready")
	}
	discovery := lanDiscover()
	found := false
	for _, device := range discovery.Devices {
		if device.ID == details.ID && device.Host == details.Host && device.Port == details.Port {
			found = true
		}
	}
	if discovery.Error != "" || !found {
		t.Fatal("The actual Rust server was not found through local UDP discovery")
	}
	request := LanConnectRequest{Host: details.Host, Port: details.Port, Code: details.Code, Name: "Fixture Mac", DeviceID: details.ID}
	wrong := request
	if request.Code == "00000000" {
		wrong.Code = "11111111"
	} else {
		wrong.Code = "00000000"
	}
	if result := lanConnectRegistered(wrong); result.ID != 0 || result.Error == "" {
		t.Fatal("The actual Rust server accepted a wrong pairing code")
	}
	connection := lanConnectRegistered(request)
	if connection.Error != "" || connection.ID == 0 {
		t.Fatalf("Rust/Go authenticated pairing failed: %s", connection.Error)
	}
	defer lanDisconnect(connection.ID)
	if connection.Name != "Library interop fixture" {
		t.Fatal("Refusing to edit a server that is not the disposable interop fixture")
	}
	call := func(value any, result any) {
		t.Helper()
		encoded, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		response, err := lanRequest(connection.ID, encoded)
		if err != nil {
			t.Fatal(err)
		}
		var failure struct {
			Error string `json:"error"`
		}
		if json.Unmarshal(response, &failure) != nil || failure.Error != "" {
			t.Fatalf("Rust operation failed: %s", failure.Error)
		}
		if err := json.Unmarshal(response, result); err != nil {
			t.Fatalf("Cannot decode the Rust operation result: %v", err)
		}
	}
	var page struct {
		Tracks       []lanInteropTrack `json:"tracks"`
		Total        int               `json:"total"`
		Revision     string            `json:"revision"`
		Capabilities []string          `json:"capabilities"`
	}
	call(map[string]any{"op": "list", "offset": 0, "limit": 256}, &page)
	if page.Total != 2 || len(page.Tracks) != 2 || page.Revision == "" || !page.Tracks[0].Writable {
		t.Fatal("The Rust server did not expose the editable fixture library")
	}
	hasJoin := false
	for _, capability := range page.Capabilities {
		if capability == "album-join-v1" {
			hasJoin = true
		}
	}
	if !hasJoin {
		t.Fatal("The Rust fixture did not advertise confirmed remote album joining")
	}
	if page.Tracks[0].AlbumGroup == "" || page.Tracks[1].AlbumGroup == "" || page.Tracks[0].DisplayAlbumID == page.Tracks[1].DisplayAlbumID ||
		page.Tracks[0].DisplayAlbum == nil || *page.Tracks[0].DisplayAlbum == page.Tracks[0].Album {
		t.Fatal("The Rust server did not expose the fixture's distinct private album identities")
	}
	track := page.Tracks[0]
	decode := func(path, suffix string) []byte {
		t.Helper()
		destination := filepath.Join(directory, suffix+".wav")
		// Float PCM avoids a converter's optional 16-bit quantization/dither.
		run("-f", "WAVE", "-d", "LEF32", path, destination)
		pcm, err := os.ReadFile(destination)
		if err != nil {
			t.Fatal(err)
		}
		return lanInteropPCM(t, pcm)
	}
	before := decode(track.Path, "before")
	mediaBefore := lanInteropMedia(t, track.Path)
	secondBefore := decode(page.Tracks[1].Path, "before-second")
	secondMediaBefore := lanInteropMedia(t, page.Tracks[1].Path)
	title, album := "Remote Unicode title ✓", "Correct remote album"
	cover := testArtwork(t)
	change := LibraryChange{Path: track.Path, Fields: LibraryFields{Title: &title, Album: &album}, Cover: cover, Size: track.Size, Modified: track.Modified}
	escape := change
	escape.Path = outside
	var saved LibrarySaveResult
	call(map[string]any{"op": "save", "tracks": []LibraryChange{change, escape}}, &saved)
	if len(saved.Saved) != 1 || saved.Saved[0] != track.Path || len(saved.Failures) != 1 || saved.Failures[0].Path != outside {
		t.Fatal("The Rust server did not report the successful edit and rejected path separately")
	}
	outsideAfter, err := os.ReadFile(outside)
	if err != nil || !bytes.Equal(outsideAfter, outData) {
		t.Fatal("The out-of-library file was modified")
	}
	var reloaded struct {
		Tracks []lanInteropTrack `json:"tracks"`
	}
	call(map[string]any{"op": "read", "paths": []string{track.Path}}, &reloaded)
	if len(reloaded.Tracks) != 1 || reloaded.Tracks[0].Title != title || reloaded.Tracks[0].Album != album ||
		reloaded.Tracks[0].Artist != track.Artist || reloaded.Tracks[0].Genre != track.Genre {
		t.Fatal("Actual Rust metadata writes did not preserve untouched fields")
	}
	var artwork struct {
		Cover []byte `json:"cover"`
	}
	call(map[string]any{"op": "cover", "path": track.Path}, &artwork)
	if !bytes.Equal(artwork.Cover, cover) {
		t.Fatal("The newly embedded cover was not fetched over the encrypted session")
	}
	var stale LibrarySaveResult
	call(map[string]any{"op": "save", "tracks": []LibraryChange{change}}, &stale)
	if len(stale.Saved) != 0 || len(stale.Failures) != 1 || !strings.Contains(stale.Failures[0].Error, "changed") {
		t.Fatal("The Rust server accepted a stale file revision")
	}
	track = reloaded.Tracks[0]
	remove := LibraryChange{Path: track.Path, RemoveCover: true, Size: track.Size, Modified: track.Modified}
	var removed LibrarySaveResult
	call(map[string]any{"op": "save", "tracks": []LibraryChange{remove}}, &removed)
	if len(removed.Saved) != 1 || len(removed.Failures) != 0 {
		t.Fatal("The Rust server could not remove embedded cover art")
	}
	call(map[string]any{"op": "cover", "path": track.Path}, &artwork)
	if len(artwork.Cover) != 0 {
		t.Fatal("Removed artwork was still returned")
	}
	if !bytes.Equal(mediaBefore, lanInteropMedia(t, track.Path)) {
		t.Fatal("The encoded AAC media changed after remote metadata editing")
	}
	if after := decode(track.Path, "after"); !bytes.Equal(before, after) {
		t.Fatal("Decoded AAC audio changed after the actual remote tag and cover edits")
	}

	// Equal-valued album fields still carry an explicit regrouping intent. The
	// original embedded album/album artist must win over stale private choices.
	second := page.Tracks[1]
	embeddedAlbum, embeddedAlbumArtist := second.Album, second.AlbumArtist
	normalize := LibraryChange{Path: second.Path, Fields: LibraryFields{Album: &embeddedAlbum, AlbumArtist: &embeddedAlbumArtist}, Size: second.Size, Modified: second.Modified}
	var normalized LibrarySaveResult
	call(map[string]any{"op": "save", "tracks": []LibraryChange{normalize}}, &normalized)
	if len(normalized.Saved) != 1 || len(normalized.Failures) != 0 {
		t.Fatal("Equal-valued explicit album metadata could not clear a private grouping")
	}
	var normalizedRead struct {
		Tracks []lanInteropTrack `json:"tracks"`
	}
	paths := []string{track.Path, second.Path}
	call(map[string]any{"op": "read", "paths": paths}, &normalizedRead)
	if len(normalizedRead.Tracks) != 2 || normalizedRead.Tracks[1].AlbumGroup != "" || strings.HasPrefix(normalizedRead.Tracks[1].DisplayAlbumID, "private:") {
		t.Fatal("The old private grouping still hid explicit remote album normalization")
	}
	checkOverrides := func(group string) {
		t.Helper()
		bytes, err := os.ReadFile(filepath.Join(data, "curation.json"))
		if err != nil {
			t.Fatal(err)
		}
		var manifest struct {
			Tracks map[string]struct {
				Album  *string `json:"album"`
				Artist *string `json:"artist"`
				Group  string  `json:"group"`
				Cover  *string `json:"cover"`
				Prior  any     `json:"prior"`
				Disc   *uint32 `json:"disc"`
				Added  bool    `json:"added"`
			} `json:"tracks"`
		}
		if json.Unmarshal(bytes, &manifest) != nil {
			t.Fatal("Cannot inspect disposable private album choices")
		}
		for _, path := range paths {
			entry := manifest.Tracks[path]
			if entry.Album != nil || entry.Artist != nil || entry.Group != group || entry.Cover != nil || entry.Prior != nil || entry.Disc != nil || entry.Added {
				t.Fatal("A private album or chosen-cover override survived the confirmed remote Magic batch")
			}
		}
	}

	// This is the same metadata+cover payload prepared by Magic: titles and
	// numbering vary per track; explicit joining reunites different source folders.
	magicAlbum, magicArtist, magicGenre := "Confirmed Magic album", "Confirmed album artist", "Neo Jazz"
	magicYear, magicDisc, magicTotal := 2025, 1, 2
	var magicChanges []map[string]any
	for index, row := range normalizedRead.Tracks {
		magicTitle, magicTrack := fmt.Sprintf("Magic track %d", index+1), index+1
		change := LibraryChange{Path: row.Path,
			Fields: LibraryFields{Title: &magicTitle, Album: &magicAlbum, AlbumArtist: &magicArtist, Genre: &magicGenre, Year: &magicYear, Disc: &magicDisc, Track: &magicTrack, TrackTotal: &magicTotal},
			Cover:  cover, Size: row.Size, Modified: row.Modified}
		encoded, _ := json.Marshal(change)
		var object map[string]any
		// Keep exact int64 timestamps; decoding into interface{} would round them.
		decoder := json.NewDecoder(bytes.NewReader(encoded))
		decoder.UseNumber()
		if err := decoder.Decode(&object); err != nil {
			t.Fatal(err)
		}
		object["joinAlbum"] = true
		magicChanges = append(magicChanges, object)
	}
	var magicSaved struct {
		LibrarySaveResult
		Joined []string `json:"joined"`
	}
	call(map[string]any{"op": "save", "tracks": magicChanges}, &magicSaved)
	if len(magicSaved.Saved) != 2 || len(magicSaved.Joined) != 2 || len(magicSaved.Failures) != 0 {
		t.Fatal("The confirmed remote Magic metadata-and-cover batch did not save both tracks")
	}
	var magicRead struct {
		Tracks []lanInteropTrack `json:"tracks"`
	}
	call(map[string]any{"op": "read", "paths": paths}, &magicRead)
	if len(magicRead.Tracks) != 2 || magicRead.Tracks[0].DisplayAlbumID != magicRead.Tracks[1].DisplayAlbumID {
		t.Fatal("Confirmed tracks in different physical folders did not share the console's album identity")
	}
	joinedGroup := magicRead.Tracks[0].AlbumGroup
	if !strings.HasPrefix(joinedGroup, "remote-album:") {
		t.Fatal("The joined album did not receive a stable private grouping")
	}
	for index, row := range magicRead.Tracks {
		if row.Title != fmt.Sprintf("Magic track %d", index+1) || row.Album != magicAlbum || row.AlbumArtist != magicArtist || row.Genre != magicGenre ||
			row.Year != magicYear || row.Disc != 1 || row.Track != index+1 || row.TrackTotal != 2 || row.Artist != "Fixture artist" || row.AlbumGroup != joinedGroup {
			t.Fatal("The Magic batch lost per-track values, common metadata or untouched performer tags")
		}
		call(map[string]any{"op": "cover", "path": row.Path}, &artwork)
		if !bytes.Equal(artwork.Cover, cover) {
			t.Fatal("A Magic cover was not embedded in both selected files")
		}
	}
	checkOverrides(joinedGroup)
	if !bytes.Equal(mediaBefore, lanInteropMedia(t, paths[0])) || !bytes.Equal(secondMediaBefore, lanInteropMedia(t, paths[1])) ||
		!bytes.Equal(before, decode(paths[0], "after-magic-first")) || !bytes.Equal(secondBefore, decode(paths[1], "after-magic-second")) {
		t.Fatal("The remote multi-track Magic batch changed encoded or decoded AAC audio")
	}
	lanDisconnect(connection.ID)
	if _, err := lanRequest(connection.ID, []byte(`{"op":"list"}`)); err == nil {
		t.Fatal("The disconnected Rust session remained usable")
	}
	reconnected := lanConnectRegistered(request)
	if reconnected.Error != "" || reconnected.ID == 0 {
		t.Fatalf("The original unexpired code could not reconnect after disconnect: %s", reconnected.Error)
	}
	defer lanDisconnect(reconnected.ID)
	response, err := lanRequest(reconnected.ID, []byte(`{"op":"list","offset":0,"limit":256}`))
	var resumed struct {
		Total  int               `json:"total"`
		Tracks []lanInteropTrack `json:"tracks"`
	}
	if err != nil || json.Unmarshal(response, &resumed) != nil || resumed.Total != 2 {
		t.Fatal("The reconnected Rust session could not read the same library")
	}
	if len(resumed.Tracks) != 2 || resumed.Tracks[0].DisplayAlbumID != joinedGroup || resumed.Tracks[1].DisplayAlbumID != joinedGroup {
		t.Fatal("The cross-folder joined album did not persist after reconnection and a real Rust library rescan")
	}
	t.Log("Actual Rust/Go discovery, pairing/reconnection, private-override normalization, Magic batch, partial writes, cover edits, conflict checks and decoded-audio preservation passed")
}

func lanInteropPCM(t *testing.T, wave []byte) []byte {
	t.Helper()
	if len(wave) < 12 || string(wave[:4]) != "RIFF" || string(wave[8:12]) != "WAVE" {
		t.Fatal("The audio converter did not produce a valid WAV fixture")
	}
	for offset := 12; offset+8 <= len(wave); {
		length := int(binary.LittleEndian.Uint32(wave[offset+4 : offset+8]))
		start := offset + 8
		if length > len(wave)-start {
			t.Fatal("The audio converter produced a truncated WAV chunk")
		}
		if string(wave[offset:offset+4]) == "data" {
			return wave[start : start+length]
		}
		offset = start + length + length%2
	}
	t.Fatal("The audio converter produced no PCM payload")
	return nil
}

func lanInteropMedia(t *testing.T, path string) []byte {
	t.Helper()
	file, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	boxes, err := m4aTagBoxes(bytes.NewReader(file), 0, int64(len(file)))
	if err != nil {
		t.Fatal(err)
	}
	var media []byte
	for _, box := range boxes {
		if box.typ == "mdat" {
			media = append(media, file[box.body:box.end]...)
		}
	}
	if len(media) == 0 {
		t.Fatal("The AAC fixture contained no encoded media")
	}
	return media
}

func lanInteropWave(t *testing.T, path string) {
	t.Helper()
	var wave bytes.Buffer
	samples := 22050
	wave.WriteString("RIFF")
	_ = binary.Write(&wave, binary.LittleEndian, uint32(36+samples*2))
	wave.WriteString("WAVEfmt ")
	_ = binary.Write(&wave, binary.LittleEndian, uint32(16))
	for _, value := range []uint16{1, 1} {
		_ = binary.Write(&wave, binary.LittleEndian, value)
	}
	_ = binary.Write(&wave, binary.LittleEndian, uint32(44100))
	_ = binary.Write(&wave, binary.LittleEndian, uint32(88200))
	_ = binary.Write(&wave, binary.LittleEndian, uint16(2))
	_ = binary.Write(&wave, binary.LittleEndian, uint16(16))
	wave.WriteString("data")
	_ = binary.Write(&wave, binary.LittleEndian, uint32(samples*2))
	for i := 0; i < samples; i++ {
		_ = binary.Write(&wave, binary.LittleEndian, int16(math.Sin(float64(i)*2*math.Pi*440/44100)*10000))
	}
	if err := os.WriteFile(path, wave.Bytes(), 0600); err != nil {
		t.Fatal(err)
	}
}
