package main

// scan.go — what was dropped on the app, as betterPod will see it: the
// songs it can play, grouped by album (the same Album and Album Artist, or
// Artist; without tags, the same folder), each album with its cover.
//
//	one album (or some of its songs)  sent as betterPod's own Share sends one
//	one song                          the song's title shows on the betterPod
//	songs of several albums           one share, a subfolder per album (each
//	                                  with its cover), a collage of the covers
//	                                  on the betterPod's screen

import (
	"bytes"
	"fmt"
	"image"
	"image/color"
	"image/jpeg"
	_ "image/png"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/dhowden/tag"
	xdraw "golang.org/x/image/draw"
)

// playable: what betterPod plays (src/ij_audio.go; a Miyoo plays them but
// .m4a, and says so before anything moves).
var playable = map[string]bool{".mp3": true, ".flac": true, ".ogg": true, ".wav": true, ".aif": true, ".aiff": true, ".m4a": true}

type Song struct {
	Path   string
	Title  string
	Artist string
	Disc   int
	Track  int
	Size   int64
}

type Group struct {
	Album    string
	Artist   string
	Songs    []Song
	CoverJPG []byte // 500 px, nil when none was found
	cover    image.Image
	folder   string
}

// Selection: everything dropped, ready to send.
type Selection struct {
	Kind     string // "album", "song" or "mix"
	Groups   []*Group
	Bytes    int64
	Songs    int
	Skipped  []string // files betterPod can't play (names)
	CoverJPG []byte   // the share's cover: the album's, or the collage
}

// Title and Subtitle: as the betterPod will show them.
func (s *Selection) Title() string {
	switch s.Kind {
	case "song":
		return s.Groups[0].Songs[0].Title
	case "album":
		return s.Groups[0].Album
	}
	return fmt.Sprintf("%d Albums", len(s.Groups))
}

func (s *Selection) Artist() string {
	a := s.Groups[0].Artist
	for _, g := range s.Groups[1:] {
		if !strings.EqualFold(g.Artist, a) {
			return "Various Artists"
		}
	}
	return a
}

// Scan reads the files and folders dropped (folders: everything in them).
func Scan(paths []string) (*Selection, error) {
	var files []string
	for _, p := range paths {
		st, err := os.Stat(p)
		if err != nil {
			continue
		}
		if !st.IsDir() {
			files = append(files, p)
			continue
		}
		filepath.Walk(p, func(path string, info os.FileInfo, err error) error {
			if err != nil {
				return nil
			}
			if info.IsDir() && path != p && strings.HasPrefix(info.Name(), ".") {
				return filepath.SkipDir
			}
			if !info.IsDir() && !strings.HasPrefix(info.Name(), ".") {
				files = append(files, path)
			}
			return nil
		})
	}
	sel := &Selection{}
	byKey := map[string]*Group{}
	seen := map[string]bool{}
	for _, f := range files {
		if seen[f] {
			continue
		}
		seen[f] = true
		ext := strings.ToLower(filepath.Ext(f))
		if !playable[ext] {
			if isAudioish(ext) {
				sel.Skipped = append(sel.Skipped, filepath.Base(f))
			}
			continue
		}
		st, err := os.Stat(f)
		if err != nil {
			continue
		}
		song := Song{Path: f, Size: st.Size(), Title: strings.TrimSuffix(filepath.Base(f), filepath.Ext(f))}
		var album, albumArtist string
		var pic *tag.Picture
		if fh, err := os.Open(f); err == nil {
			if m, err := tag.ReadFrom(fh); err == nil {
				if t := strings.TrimSpace(m.Title()); t != "" {
					song.Title = t
				}
				song.Artist = strings.TrimSpace(m.Artist())
				album = strings.TrimSpace(m.Album())
				albumArtist = strings.TrimSpace(m.AlbumArtist())
				song.Track, _ = m.Track()
				song.Disc, _ = m.Disc()
				pic = m.Picture()
			}
			fh.Close()
		}
		artist := albumArtist
		if artist == "" {
			artist = song.Artist
		}
		folder := filepath.Dir(f)
		key := "dir:" + strings.ToLower(folder)
		if album != "" {
			key = strings.ToLower(album) + "\x00" + strings.ToLower(artist)
		} else {
			album = filepath.Base(folder)
		}
		g := byKey[key]
		if g == nil {
			g = &Group{Album: album, Artist: artist, folder: folder}
			byKey[key] = g
			sel.Groups = append(sel.Groups, g)
		}
		if g.Artist == "" {
			g.Artist = artist
		}
		if g.cover == nil && pic != nil {
			if img, _, err := image.Decode(bytes.NewReader(pic.Data)); err == nil {
				g.cover = img
			}
		}
		g.Songs = append(g.Songs, song)
		sel.Bytes += song.Size
		sel.Songs++
	}
	if len(sel.Groups) == 0 {
		if len(sel.Skipped) > 0 {
			return nil, fmt.Errorf("betterPod can't play these files (MP3, FLAC, Ogg, WAV, AIFF and M4A only)")
		}
		return nil, fmt.Errorf("No songs here")
	}
	for _, g := range sel.Groups {
		sort.SliceStable(g.Songs, func(i, j int) bool {
			a, b := g.Songs[i], g.Songs[j]
			if a.Disc != b.Disc {
				return a.Disc < b.Disc
			}
			if a.Track != b.Track {
				return a.Track < b.Track
			}
			return a.Path < b.Path
		})
		if g.cover == nil {
			g.cover = folderCover(g.folder)
		}
		if g.cover != nil {
			g.CoverJPG = encodeJPG(scaleSquare(g.cover, 500))
		}
	}
	sort.SliceStable(sel.Groups, func(i, j int) bool {
		a, b := sel.Groups[i], sel.Groups[j]
		if !strings.EqualFold(a.Artist, b.Artist) {
			return strings.ToLower(a.Artist) < strings.ToLower(b.Artist)
		}
		return strings.ToLower(a.Album) < strings.ToLower(b.Album)
	})
	switch {
	case len(sel.Groups) > 1:
		sel.Kind = "mix"
		sel.CoverJPG = encodeJPG(collage(sel.Groups, 500))
	case sel.Songs == 1:
		sel.Kind = "song"
		sel.CoverJPG = sel.Groups[0].CoverJPG
	default:
		sel.Kind = "album"
		sel.CoverJPG = sel.Groups[0].CoverJPG
	}
	return sel, nil
}

func isAudioish(ext string) bool {
	switch ext {
	case ".mp4", ".aac", ".alac", ".opus", ".oga", ".wma", ".ape", ".wv", ".mpc", ".dsf", ".dff", ".mka":
		return true
	}
	return false
}

// folderCover: a picture next to the songs (cover, folder, front… first).
func folderCover(dir string) image.Image {
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil
	}
	var best string
	bestRank := 99
	for _, e := range entries {
		n := strings.ToLower(e.Name())
		ext := filepath.Ext(n)
		if e.IsDir() || (ext != ".jpg" && ext != ".jpeg" && ext != ".png") || strings.HasPrefix(n, ".") {
			continue
		}
		rank := 5
		for i, w := range []string{"cover", "folder", "front", "album"} {
			if strings.HasPrefix(strings.TrimSuffix(n, ext), w) {
				rank = i
				break
			}
		}
		if rank < bestRank {
			best, bestRank = filepath.Join(dir, e.Name()), rank
		}
	}
	if best == "" {
		return nil
	}
	f, err := os.Open(best)
	if err != nil {
		return nil
	}
	defer f.Close()
	img, _, err := image.Decode(f)
	if err != nil {
		return nil
	}
	return img
}

// scaleSquare: the middle square of img, n px.
func scaleSquare(img image.Image, n int) *image.RGBA {
	b := img.Bounds()
	side := min(b.Dx(), b.Dy())
	src := image.Rect(b.Min.X+(b.Dx()-side)/2, b.Min.Y+(b.Dy()-side)/2, 0, 0)
	src.Max = src.Min.Add(image.Pt(side, side))
	dst := image.NewRGBA(image.Rect(0, 0, n, n))
	xdraw.CatmullRom.Scale(dst, dst.Bounds(), img, src, xdraw.Src, nil)
	return dst
}

func encodeJPG(img image.Image) []byte {
	var b bytes.Buffer
	if jpeg.Encode(&b, img, &jpeg.Options{Quality: 85}) != nil {
		return nil
	}
	return b.Bytes()
}

// collage: the albums' covers in one square, for a mix: 2 side by side
// halves, 3 as a big one and two small ones, 4 or more as a 2 × 2 grid (the
// first four). An album without a cover is a grey tile.
func collage(groups []*Group, n int) *image.RGBA {
	dst := image.NewRGBA(image.Rect(0, 0, n, n))
	tile := func(i int, r image.Rectangle) {
		if i >= len(groups) || groups[i].cover == nil {
			shade := uint8(70 + 25*(i%3))
			xdraw.Draw(dst, r, &image.Uniform{color.RGBA{shade, shade, shade + 8, 255}}, image.Point{}, xdraw.Src)
			return
		}
		img := groups[i].cover
		b := img.Bounds()
		// the middle of the cover, at the tile's proportions
		w, h := b.Dx(), b.Dy()
		if w*r.Dy() > h*r.Dx() {
			w = h * r.Dx() / r.Dy()
		} else {
			h = w * r.Dy() / r.Dx()
		}
		src := image.Rect(b.Min.X+(b.Dx()-w)/2, b.Min.Y+(b.Dy()-h)/2, 0, 0)
		src.Max = src.Min.Add(image.Pt(w, h))
		xdraw.CatmullRom.Scale(dst, r, img, src, xdraw.Src, nil)
	}
	h := n / 2
	switch len(groups) {
	case 2:
		tile(0, image.Rect(0, 0, h, n))
		tile(1, image.Rect(h, 0, n, n))
	case 3:
		tile(0, image.Rect(0, 0, h, n))
		tile(1, image.Rect(h, 0, n, h))
		tile(2, image.Rect(h, h, n, n))
	default:
		tile(0, image.Rect(0, 0, h, h))
		tile(1, image.Rect(h, 0, n, h))
		tile(2, image.Rect(0, h, h, n))
		tile(3, image.Rect(h, h, n, n))
	}
	// thin lines between the tiles
	line := color.RGBA{255, 255, 255, 255}
	for y := 0; y < n; y++ {
		dst.SetRGBA(h, y, line)
	}
	if len(groups) >= 3 {
		x0 := 0
		if len(groups) == 3 {
			x0 = h
		}
		for x := x0; x < n; x++ {
			dst.SetRGBA(x, h, line)
		}
	}
	return dst
}
