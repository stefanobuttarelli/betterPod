package main

// Local library editing has no dependency on sharing or a connected betterPod.
// Changes are applied to a private sibling copy before replacing one original.

import (
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"image"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"github.com/dhowden/tag"
)

const libraryTagLimit = 16 << 20
const libraryArtworkLimit = 8 << 20

var libraryWriteMu sync.Mutex

type LibraryTrack struct {
	Path        string `json:"path"`
	FileName    string `json:"fileName"`
	Format      string `json:"format"`
	Writable    bool   `json:"writable"`
	Title       string `json:"title"`
	Artist      string `json:"artist"`
	Album       string `json:"album"`
	AlbumArtist string `json:"albumArtist"`
	Genre       string `json:"genre"`
	Year        int    `json:"year"`
	Track       int    `json:"track"`
	TrackTotal  int    `json:"trackTotal"`
	Disc        int    `json:"disc"`
	Cover       []byte `json:"cover,omitempty"`
	Size        int64  `json:"size"`
	Modified    int64  `json:"modified"`
	Warning     string `json:"warning,omitempty"`
}

type LibraryReadResult struct {
	Tracks  []LibraryTrack `json:"tracks"`
	Skipped []string       `json:"skipped"`
	Error   string         `json:"error,omitempty"`
}

type LibraryFields struct {
	Title       *string `json:"title,omitempty"`
	Artist      *string `json:"artist,omitempty"`
	Album       *string `json:"album,omitempty"`
	AlbumArtist *string `json:"albumArtist,omitempty"`
	Genre       *string `json:"genre,omitempty"`
	Year        *int    `json:"year,omitempty"`
	Track       *int    `json:"track,omitempty"`
	TrackTotal  *int    `json:"trackTotal,omitempty"`
	Disc        *int    `json:"disc,omitempty"`
}

type LibraryChange struct {
	Path        string        `json:"path"`
	Fields      LibraryFields `json:"fields"`
	Cover       []byte        `json:"cover,omitempty"`
	RemoveCover bool          `json:"removeCover,omitempty"`
	Size        int64         `json:"size"`
	Modified    int64         `json:"modified"`
}

type LibrarySaveRequest struct {
	Tracks []LibraryChange `json:"tracks"`
}
type LibraryFailure struct {
	Path  string `json:"path"`
	Error string `json:"error"`
}
type LibrarySaveResult struct {
	Saved    []string         `json:"saved"`
	Failures []LibraryFailure `json:"failures"`
	Error    string           `json:"error,omitempty"`
}

func readLibrary(paths []string) LibraryReadResult {
	out := LibraryReadResult{Tracks: []LibraryTrack{}, Skipped: []string{}}
	seen := map[string]bool{}
	add := func(path string) {
		path, err := filepath.Abs(path)
		if err != nil || seen[path] {
			return
		}
		seen[path] = true
		if !playable[strings.ToLower(filepath.Ext(path))] {
			if isAudioish(strings.ToLower(filepath.Ext(path))) {
				out.Skipped = append(out.Skipped, filepath.Base(path)+": unsupported audio format")
			}
			return
		}
		track, err := readLibraryTrack(path)
		if err != nil {
			out.Skipped = append(out.Skipped, filepath.Base(path)+": "+err.Error())
			return
		}
		out.Tracks = append(out.Tracks, track)
	}
	for _, path := range paths {
		st, err := os.Lstat(path)
		if err != nil {
			out.Skipped = append(out.Skipped, filepath.Base(path)+": "+err.Error())
			continue
		}
		if !st.IsDir() {
			add(path)
			continue
		}
		err = filepath.Walk(path, func(p string, info os.FileInfo, err error) error {
			if err != nil {
				out.Skipped = append(out.Skipped, p+": "+err.Error())
				return nil
			}
			if info.IsDir() && p != path && strings.HasPrefix(info.Name(), ".") {
				return filepath.SkipDir
			}
			if !info.IsDir() && !strings.HasPrefix(info.Name(), ".") {
				add(p)
			}
			return nil
		})
		if err != nil {
			out.Skipped = append(out.Skipped, path+": "+err.Error())
		}
	}
	sort.SliceStable(out.Tracks, func(i, j int) bool {
		a, b := out.Tracks[i], out.Tracks[j]
		// Keep a damaged album's tracks together by their folder, not their tags.
		if filepath.Dir(a.Path) != filepath.Dir(b.Path) {
			return a.Path < b.Path
		}
		if a.Disc != b.Disc {
			return a.Disc < b.Disc
		}
		if a.Track != b.Track {
			return a.Track < b.Track
		}
		return a.FileName < b.FileName
	})
	if len(out.Tracks) == 0 {
		out.Error = "No supported audio files found. Choose MP3, FLAC, Ogg, M4A, WAV or AIFF files."
	}
	return out
}

func readLibraryTrack(path string) (track LibraryTrack, err error) {
	// Bad tags must not take down the app or make a file silently writable.
	defer func() {
		if p := recover(); p != nil {
			err = fmt.Errorf("could not read metadata: %v", p)
		}
	}()
	st, err := os.Stat(path)
	if err != nil {
		return track, err
	}
	if !st.Mode().IsRegular() {
		return track, errors.New("not a regular audio file")
	}
	track = LibraryTrack{Path: path, FileName: filepath.Base(path), Format: strings.ToUpper(strings.TrimPrefix(filepath.Ext(path), ".")), Writable: canWriteTags(path), Size: st.Size(), Modified: st.ModTime().UnixNano()}
	track.Title = strings.TrimSuffix(track.FileName, filepath.Ext(path))
	f, err := os.Open(path)
	if err != nil {
		return track, err
	}
	defer f.Close()
	if err := validateLibraryContainer(f, path); err != nil {
		track.Writable = false
		track.Warning = "This metadata layout is read-only: " + err.Error()
		return track, nil
	}
	m, tagErr := tag.ReadFrom(f)
	if tagErr == nil {
		// An empty title stays empty, so saving another field never adds a title.
		track.Title = m.Title()
		track.Artist, track.Album, track.AlbumArtist = m.Artist(), m.Album(), m.AlbumArtist()
		track.Genre, track.Year = m.Genre(), m.Year()
		track.Track, track.TrackTotal = m.Track()
		track.Disc, _ = m.Disc()
		if p := m.Picture(); p != nil && len(p.Data) <= libraryArtworkLimit && validateArtwork(p.Data) == nil {
			track.Cover = p.Data
		}
	} else {
		track.Warning = "Metadata could not be read: " + tagErr.Error()
		if !errors.Is(tagErr, tag.ErrNoTagsFound) && strings.ToLower(filepath.Ext(path)) != ".mp3" {
			track.Writable = false
		}
	}
	if !canWriteTags(path) {
		track.Warning = "Metadata editing is supported for MP3, FLAC, Ogg Vorbis and M4A. This file is read-only."
	}
	if _, err := libraryRegularPath(path); err != nil {
		track.Writable = false
		track.Warning = err.Error()
	}
	return track, nil
}

func libraryRegularPath(path string) (os.FileInfo, error) {
	absolute, err := filepath.Abs(path)
	if err != nil {
		return nil, err
	}
	st, err := os.Lstat(absolute)
	if err != nil {
		return nil, err
	}
	if st.Mode()&os.ModeSymlink != 0 {
		return nil, errors.New("Symbolic links are read-only. Open the original file to edit it.")
	}
	if !st.Mode().IsRegular() {
		return nil, errors.New("Only regular audio files can be edited")
	}
	return st, nil
}

func saveLibrary(req LibrarySaveRequest) LibrarySaveResult {
	libraryWriteMu.Lock()
	defer libraryWriteMu.Unlock()
	out := LibrarySaveResult{Saved: []string{}, Failures: []LibraryFailure{}}
	seen := map[string]bool{}
	for _, change := range req.Tracks {
		path, err := filepath.Abs(change.Path)
		if err == nil && seen[path] {
			err = errors.New("The same file was submitted more than once")
		}
		seen[path] = true
		if err == nil {
			err = saveLibraryTrack(change)
		}
		if err != nil {
			out.Failures = append(out.Failures, LibraryFailure{Path: change.Path, Error: err.Error()})
		} else {
			out.Saved = append(out.Saved, change.Path)
		}
	}
	return out
}

func saveLibraryTrack(c LibraryChange) (err error) {
	defer func() {
		if p := recover(); p != nil {
			err = fmt.Errorf("metadata could not be saved: %v", p)
		}
	}()
	if !canWriteTags(c.Path) {
		return errNoTags
	}
	// Resolve the parent once: macOS itself aliases /var and /tmp. A final
	// symbolic link is still refused, and a changing parent cannot redirect us.
	parent, err := filepath.EvalSymlinks(filepath.Dir(c.Path))
	if err != nil {
		return err
	}
	c.Path = filepath.Join(parent, filepath.Base(c.Path))
	original, err := libraryRegularPath(c.Path)
	if err != nil {
		return err
	}
	matches := func(st os.FileInfo) bool {
		return os.SameFile(original, st) && st.Size() == c.Size && st.ModTime().UnixNano() == c.Modified
	}
	if !matches(original) {
		return errors.New("This file changed since it was opened. Reload it before saving.")
	}
	if original.Mode().Perm()&0222 == 0 {
		return errors.New("This audio file is read-only")
	}
	current, err := readLibraryTrack(c.Path)
	if err != nil {
		return err
	}
	if !current.Writable {
		return errors.New(current.Warning)
	}
	t := songTags{Title: current.Title, Artist: current.Artist, Album: current.Album, AlbumArtist: current.AlbumArtist, Genre: current.Genre, Year: current.Year, Track: current.Track, TrackTotal: current.TrackTotal, Disc: current.Disc}
	var mask tagMask
	for _, f := range []struct {
		p   *string
		dst *string
		bit tagMask
	}{
		{c.Fields.Title, &t.Title, tagTitle}, {c.Fields.Artist, &t.Artist, tagArtist}, {c.Fields.Album, &t.Album, tagAlbum}, {c.Fields.AlbumArtist, &t.AlbumArtist, tagAlbumArtist}, {c.Fields.Genre, &t.Genre, tagGenre},
	} {
		if f.p != nil {
			if len(*f.p) > 65536 || strings.ContainsRune(*f.p, '\x00') {
				return errors.New("Metadata text is too long or contains a null character")
			}
			*f.dst = *f.p
			mask |= f.bit
		}
	}
	for _, f := range []struct {
		p   *int
		dst *int
		bit tagMask
	}{
		{c.Fields.Year, &t.Year, tagYear}, {c.Fields.Track, &t.Track, tagTrack}, {c.Fields.TrackTotal, &t.TrackTotal, tagTrackTotal}, {c.Fields.Disc, &t.Disc, tagDisc},
	} {
		if f.p != nil {
			if *f.p < 0 || *f.p > 65535 {
				return errors.New("Track, disc, total and year values must be between 0 and 65535")
			}
			*f.dst = *f.p
			mask |= f.bit
		}
	}
	if c.Cover != nil && c.RemoveCover {
		return errors.New("A cover cannot be replaced and removed in the same edit")
	}
	if c.Cover != nil {
		if err := validateArtwork(c.Cover); err != nil {
			return err
		}
		t.Cover = c.Cover
		mask |= tagCover
	}
	if c.RemoveCover {
		mask |= tagCover
	}
	if mask == 0 {
		return nil
	}
	in, err := os.Open(c.Path)
	if err != nil {
		return err
	}
	defer in.Close()
	// A random sibling also isolates the old writers' internal .tagtmp paths.
	tmp, err := os.CreateTemp(filepath.Dir(c.Path), ".betterpod-edit-*")
	if err != nil {
		return err
	}
	name := tmp.Name()
	defer os.Remove(name)
	defer os.Remove(name + ".tagtmp")
	defer tmp.Close()
	if _, err := io.Copy(tmp, in); err != nil {
		return err
	}
	if err := tmp.Chmod(original.Mode().Perm()); err != nil {
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	if err := writeTagsMasked(name, filepath.Ext(c.Path), t, mask); err != nil {
		return err
	}
	staged, err := os.OpenFile(name, os.O_RDWR, 0)
	if err != nil {
		return err
	}
	err = validateLibraryContainer(staged, c.Path)
	if err == nil {
		err = staged.Chmod(original.Mode().Perm())
	}
	if err == nil {
		err = preserveLibraryFileMetadata(c.Path, name)
	}
	if err == nil {
		err = staged.Sync()
	}
	closeErr := staged.Close()
	if err != nil {
		return err
	}
	if closeErr != nil {
		return closeErr
	}
	// Recheck immediately before replacing, including identity and symlinks.
	latest, err := libraryRegularPath(c.Path)
	if err != nil {
		return err
	}
	if !matches(latest) {
		return errors.New("The original changed during saving. No changes were written.")
	}
	return os.Rename(name, c.Path)
}

func validateArtwork(data []byte) error {
	if len(data) == 0 || len(data) > libraryArtworkLimit {
		return errors.New("Choose a JPEG or PNG cover smaller than 8 MB")
	}
	conf, kind, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil || (kind != "jpeg" && kind != "png") || conf.Width <= 0 || conf.Height <= 0 || int64(conf.Width)*int64(conf.Height) > 40000000 {
		return errors.New("Choose a valid JPEG or PNG cover up to 40 megapixels")
	}
	return nil
}

func artworkMIME(data []byte) string {
	if bytes.HasPrefix(data, []byte("\x89PNG")) {
		return "image/png"
	}
	return "image/jpeg"
}

func id3FrontCover(data []byte) bool {
	if len(data) < 3 {
		return false
	}
	end := bytes.IndexByte(data[1:], 0)
	return end >= 0 && end+2 < len(data) && data[end+2] == 3
}

func flacPicture(data []byte) []byte {
	conf, _, _ := image.DecodeConfig(bytes.NewReader(data))
	var out bytes.Buffer
	put := func(n int) { var b [4]byte; binary.BigEndian.PutUint32(b[:], uint32(n)); out.Write(b[:]) }
	mime := artworkMIME(data)
	put(3)
	put(len(mime))
	out.WriteString(mime)
	put(0)
	put(conf.Width)
	put(conf.Height)
	put(24)
	put(0)
	put(len(data))
	out.Write(data)
	return out.Bytes()
}
