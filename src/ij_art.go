package main

// ij_art.go — album artwork for the UI: decoded and scaled in the background
// (two at a time), kept in an LRU of scaled images (by bytes) and cached on
// disk as raw RGBA (conf/.ipod_art); covers already on disk are read at
// once within a per-frame budget, and the list thumbnails are prefetched.

import (
	"bytes"
	"container/list"
	"fmt"
	"hash/fnv"
	"image"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"time"
)

type artKey struct {
	a   *Album
	dev int // size in device pixels
}

// artCache keeps scaled artwork in memory, least recently used out first,
// within a byte budget (a 96 px list thumbnail is 36 KB, a Cover Flow
// cover 256 KB, a full-height preview 900 KB).
type artCache struct {
	mu      sync.Mutex
	imgs    map[artKey]*list.Element
	lru     *list.List // front: most recently used
	bytes   int
	budget  int
	pending map[artKey]bool
	sem     chan struct{}
}

type artEntry struct {
	k   artKey
	img *image.RGBA
}

// 128 MB of scaled images, at most 2 decoded at the same time
func newArtCache() *artCache {
	return &artCache{
		imgs:    map[artKey]*list.Element{},
		lru:     list.New(),
		budget:  128 << 20,
		pending: map[artKey]bool{},
		sem:     make(chan struct{}, 2),
	}
}

// clear drops every image in memory (the scaled files on disk stay, so
// they come back at once).
func (c *artCache) clear() {
	c.mu.Lock()
	c.imgs = map[artKey]*list.Element{}
	c.lru.Init()
	c.bytes = 0
	c.mu.Unlock()
}

// dropAlbum forgets every size of one album's artwork (it has a new cover).
func (c *artCache) dropAlbum(a *Album) {
	c.mu.Lock()
	for k, e := range c.imgs {
		if k.a == a {
			c.bytes -= len(e.Value.(*artEntry).img.Pix)
			c.lru.Remove(e)
			delete(c.imgs, k)
		}
	}
	c.mu.Unlock()
}

// get returns a cached image and marks it as just used.
func (c *artCache) get(k artKey) *image.RGBA {
	if e, ok := c.imgs[k]; ok {
		c.lru.MoveToFront(e)
		return e.Value.(*artEntry).img
	}
	return nil
}

// put adds an image, dropping the least recently used ones over budget.
func (c *artCache) put(k artKey, img *image.RGBA) {
	if e, ok := c.imgs[k]; ok {
		c.bytes -= len(e.Value.(*artEntry).img.Pix)
		e.Value.(*artEntry).img = img
		c.bytes += len(img.Pix)
		c.lru.MoveToFront(e)
	} else {
		c.imgs[k] = c.lru.PushFront(&artEntry{k, img})
		c.bytes += len(img.Pix)
	}
	for c.bytes > c.budget && c.lru.Len() > 1 {
		e := c.lru.Back()
		en := e.Value.(*artEntry)
		c.lru.Remove(e)
		delete(c.imgs, en.k)
		c.bytes -= len(en.img.Pix)
	}
}

// Artwork already scaled on disk is read at once, without waiting for a
// background load, up to artSyncBudget bytes per frame (the rest loads in
// the background): lists and Cover Flow show the covers straight away
// instead of the default cover first.
const artSyncBudget = 700 << 10

var artSyncLeft = artSyncBudget

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
	if img := c.get(k); img != nil {
		c.mu.Unlock()
		return img
	}
	if c.pending[k] {
		c.mu.Unlock()
		return nil
	}
	// scaled before: read it now if the frame's budget allows
	if need := devSize * devSize * 4; artSyncLeft >= need {
		if img := readArtDisk(a, devSize); img != nil {
			artSyncLeft -= need
			c.put(k, img)
			c.mu.Unlock()
			return img
		}
	}
	c.pending[k] = true
	c.mu.Unlock()

	// wanted on the screen now: ahead of the background work
	atomic.AddInt32(&artFG, 1)
	artFGStart.Do(func() {
		for w := 0; w < cap(c.sem); w++ {
			go func() {
				for job := range artFGQueue {
					job()
				}
			}()
		}
	})
	job := func() {
		defer atomic.AddInt32(&artFG, -1)
		out := ui.loadArt(a, devSize)
		c.mu.Lock()
		c.put(k, out)
		delete(c.pending, k)
		c.mu.Unlock()
		ui.post(func() { ui.onArtLoaded() })
	}
	select {
	case artFGQueue <- job:
	default:
		go job() // (the queue is full: don't block the frame)
	}
	return nil
}

// artFGQueue: the covers asked for by the screen, loaded in the order they
// were asked (first in, first out) by as many workers as loadArt allows at
// once. (One goroutine each would come in no particular order, often the
// newest first.)
var (
	artFGQueue = make(chan func(), 256)
	artFGStart sync.Once
)

// artFG: covers wanted on the screen being loaded. The background work
// (prefetchArt, the screensaver's warm-up) waits while there are any, and
// takes at most one of the two decoding places (artBG), so a cover asked
// for by the screen never waits behind it.
var (
	artFG int32
	artBG = make(chan struct{}, 1)
)

// loadArtBG is loadArt for the background work.
func (ui *IpodUI) loadArtBG(a *Album, devSize int) *image.RGBA {
	artBG <- struct{}{}
	defer func() { <-artBG }()
	for i := 0; atomic.LoadInt32(&artFG) > 0 && i < 500; i++ {
		time.Sleep(10 * time.Millisecond)
	}
	return ui.loadArt(a, devSize)
}

// artFallback: a cover to show while the one at devSize is being made:
// the same album at another size already in memory, scaled (not kept).
func (ui *IpodUI) artFallback(a *Album, devSize int) *image.RGBA {
	// (asked each frame until the real one comes: made once)
	if a == artFbAl && devSize == artFbSize && artFbImg != nil {
		return artFbImg
	}
	img := ui.artFallbackMake(a, devSize)
	if img != nil {
		artFbAl, artFbSize, artFbImg = a, devSize, img
	}
	return img
}

var (
	artFbAl   *Album
	artFbSize int
	artFbImg  *image.RGBA
)

func (ui *IpodUI) artFallbackMake(a *Album, devSize int) *image.RGBA {
	c := ui.artc
	var best *image.RGBA
	c.mu.Lock()
	for _, d := range []int{256, 2 * ssCell, 192, 96, ssCell, 512} {
		if d == devSize {
			continue
		}
		if img := c.get(artKey{a, d}); img != nil && (best == nil || img.Bounds().Dx() > best.Bounds().Dx()) {
			best = img
		}
	}
	c.mu.Unlock()
	if best == nil || best.Bounds().Dx() < 2*ssCell {
		ssArt.Lock()
		for _, d := range []int{2 * ssCell, ssCell} {
			if img := ssArt.img[artKey{a, d}]; img != nil && (best == nil || img.Bounds().Dx() > best.Bounds().Dx()) {
				best = img
			}
		}
		ssArt.Unlock()
	}
	if best == nil {
		return nil
	}
	return scaleImage(best, devSize, devSize)
}

// preloadPlaying asks for the covers Now Playing will want: the song's
// album, and those of the songs before and after it in the queue.
func (ui *IpodUI) preloadPlaying() {
	q := ui.app.Queue
	if q == nil || len(q.Tracks) == 0 {
		return
	}
	for _, d := range []int{0, 1, -1} {
		i := q.CurrentIndex + d
		if i >= 0 && i < len(q.Tracks) {
			if al := ui.trackAlbum(q.Tracks[i]); al != nil {
				ui.art(al, px(128))
			}
		}
	}
}

// loadArt reads the scaled artwork from the disk cache, or decodes and
// scales the original (at most two at a time) and caches it on disk.
func (ui *IpodUI) loadArt(a *Album, devSize int) *image.RGBA {
	c := ui.artc
	c.sem <- struct{}{}
	defer func() { <-c.sem }()
	if out := readArtDisk(a, devSize); out != nil {
		return out
	}
	src := decodeAlbumSource(a)
	if src == nil {
		return scaleImage(ui.defaultArt(), devSize, devSize)
	}
	out := scaleImage(src, devSize, devSize)
	writeArtDisk(a, devSize, out)
	// the other common sizes from the same decode, for next time
	for _, d := range prefetchSizes {
		if d != devSize && artDiskPath(a, d) != "" && !fileExists(artDiskPath(a, d)) {
			writeArtDisk(a, d, scaleImage(src, d, d))
		}
	}
	return out
}

// prefetchSizes: list thumbnails (48 CSS px) and Cover Flow covers (128).
// The screensaver's two (ssCell, 2*ssCell) are made too.
var prefetchSizes = []int{96, 256, ssCell, 2 * ssCell}

// prefetchArt loads every album's list thumbnail into memory in the
// background, one at a time, and prepares the scaled files on disk for the
// ones never shown before, so lists and Cover Flow never wait for them.
// It runs once per library (a new scan starts it again).
func (ui *IpodUI) prefetchArt(lib *Library) {
	for _, a := range lib.Albums {
		if ui.app.Library != lib || !ui.app.Running {
			return
		}
		k := artKey{a, 96}
		c := ui.artc
		c.mu.Lock()
		have := c.get(k) != nil || c.pending[k]
		if !have {
			c.pending[k] = true
		}
		c.mu.Unlock()
		if have {
			continue
		}
		img := ui.loadArtBG(a, 96)
		c.mu.Lock()
		c.put(k, img)
		delete(c.pending, k)
		c.mu.Unlock()
		time.Sleep(3 * time.Millisecond) // stay out of the way of the UI
	}
	ui.post(func() { ui.onArtLoaded() })
	// then the screensaver's covers, so it never waits for them
	ui.post(func() {
		if saverAfter > 0 { // (only when it may start by itself; by hand it gets them then)
			ui.ssWarm()
		}
	})
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
	return c.get(artKey{a, devSize})
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
