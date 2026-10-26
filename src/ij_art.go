package main

// ij_art.go — album artwork for the UI: decoded and scaled in the background
// (two at a time), kept in an LRU of scaled images and cached on disk as raw
// RGBA (conf/.ipod_art) so lists and Cover Flow fill in fast next time.

import (
	"bytes"
	"fmt"
	"hash/fnv"
	"image"
	"os"
	"path/filepath"
	"sync"
)

type artKey struct {
	a   *Album
	dev int // size in device pixels
}

type artCache struct {
	mu      sync.Mutex
	imgs    map[artKey]*image.RGBA
	pending map[artKey]bool
	order   []artKey
	limit   int
	sem     chan struct{}
}

// 400 scaled images, at most 2 decoded at the same time
func newArtCache() *artCache {
	return &artCache{
		imgs:    map[artKey]*image.RGBA{},
		pending: map[artKey]bool{},
		limit:   400,
		sem:     make(chan struct{}, 2),
	}
}

// decodeAlbumSource returns the album's full-size artwork (nil if none).
func decodeAlbumSource(a *Album) image.Image {
	if a == nil {
		return nil
	}
	if a.ArtImg != nil {
		return a.ArtImg
	}
	if a.ArtPath == "" {
		return nil
	}
	data, err := os.ReadFile(a.ArtPath)
	if err != nil {
		return nil
	}
	img, _, err := image.Decode(bytes.NewReader(data))
	if err != nil {
		return nil
	}
	return img
}

// art returns the album artwork scaled to devSize, or nil while it is being
// prepared (onArtLoaded redraws when it's ready).
func (ui *IpodUI) art(a *Album, devSize int) *image.RGBA {
	c := ui.artc
	k := artKey{a, devSize}
	c.mu.Lock()
	if img, ok := c.imgs[k]; ok {
		c.mu.Unlock()
		return img
	}
	if c.pending[k] {
		c.mu.Unlock()
		return nil
	}
	c.pending[k] = true
	c.mu.Unlock()

	go func() {
		c.sem <- struct{}{}
		out := readArtDisk(a, devSize)
		if out == nil {
			src := decodeAlbumSource(a)
			if src == nil {
				out = scaleImage(ui.defaultArt(), devSize, devSize)
			} else {
				out = scaleImage(src, devSize, devSize)
				writeArtDisk(a, devSize, out)
			}
		}
		<-c.sem
		c.mu.Lock()
		c.imgs[k] = out
		delete(c.pending, k)
		c.order = append(c.order, k)
		for len(c.order) > c.limit {
			old := c.order[0]
			c.order = c.order[1:]
			delete(c.imgs, old)
		}
		c.mu.Unlock()
		ui.post(func() { ui.onArtLoaded() })
	}()
	return nil
}

// artOrDefault: the artwork, or the default cover while it loads.
func (ui *IpodUI) artOrDefault(a *Album, devSize int) *image.RGBA {
	if img := ui.art(a, devSize); img != nil {
		return img
	}
	return ui.defaultArtSized(devSize)
}

var defaultArtMu sync.Mutex
var defaultArtSizes = map[int]*image.RGBA{}

func (ui *IpodUI) defaultArt() image.Image { return ui.assetSafe("default_album_artwork") }

func (ui *IpodUI) defaultArtSized(dev int) *image.RGBA {
	defaultArtMu.Lock()
	defer defaultArtMu.Unlock()
	if img, ok := defaultArtSizes[dev]; ok {
		return img
	}
	img := scaleImage(ui.defaultArt(), dev, dev)
	defaultArtSizes[dev] = img
	return img
}

func (ui *IpodUI) onArtLoaded() {
	for _, st := range [][]*View{ui.split, ui.full} {
		for _, v := range st {
			v.dirty = true
		}
	}
	ui.dirty = true
}

// trackAlbum finds the album of a track.
func (ui *IpodUI) trackAlbum(t *Track) *Album {
	if t == nil || ui.app.Library == nil {
		return nil
	}
	lib := ui.app.Library
	if a, ok := lib.AlbumsByKey[albumKeyFor(t)]; ok {
		return a
	}
	for _, key := range []string{t.AlbumArtist + "|" + t.Album, t.Artist + "|" + t.Album} {
		if a, ok := lib.AlbumsByKey[key]; ok {
			return a
		}
	}
	for _, a := range lib.Albums {
		if a.Name == t.Album {
			return a
		}
	}
	return nil
}

// artCached returns the scaled artwork only if it is already in memory.
func (ui *IpodUI) artCached(a *Album, devSize int) *image.RGBA {
	c := ui.artc
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.imgs[artKey{a, devSize}]
}

// ---- disk cache of scaled artwork ----
//
// Files are named after the artwork file (path, size, mtime) and the size,
// so a changed cover gets a new entry. Only sizes up to 320 px are cached.

func artDiskPath(a *Album, dev int) string {
	if a == nil || a.ArtPath == "" || dev > 320 {
		return ""
	}
	st, err := os.Stat(a.ArtPath)
	if err != nil {
		return ""
	}
	h := fnv.New64a()
	fmt.Fprintf(h, "%s|%d|%d", a.ArtPath, st.Size(), st.ModTime().UnixNano())
	return filepath.Join(dataPath(".ipod_art"), fmt.Sprintf("%016x_%d.rgba", h.Sum64(), dev))
}

func readArtDisk(a *Album, dev int) *image.RGBA {
	p := artDiskPath(a, dev)
	if p == "" {
		return nil
	}
	data, err := os.ReadFile(p)
	if err != nil || len(data) != dev*dev*4 {
		return nil
	}
	return &image.RGBA{Pix: data, Stride: dev * 4, Rect: image.Rect(0, 0, dev, dev)}
}

func writeArtDisk(a *Album, dev int, img *image.RGBA) {
	p := artDiskPath(a, dev)
	if p == "" || img.Bounds().Dx() != dev || img.Bounds().Dy() != dev || img.Stride != dev*4 {
		return
	}
	_ = os.MkdirAll(filepath.Dir(p), 0755)
	tmp := p + ".tmp"
	if os.WriteFile(tmp, img.Pix, 0644) == nil {
		_ = os.Rename(tmp, p)
	}
}
