package main

// Album artwork is looked up in RAM on the UI thread. Two foreground
// workers read, decode and scale misses; background prefetch uses at most
// one decoding slot and yields to covers requested by the screen.

import (
	"bufio"
	"bytes"
	"container/list"
	"fmt"
	"hash/fnv"
	"image"
	"image/draw"
	"image/jpeg"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

type artKey struct {
	a   *Album
	dev int
}

const artQueueLimit = 256

type artCache struct {
	mu            sync.Mutex
	imgs          map[artKey]*list.Element
	lru           *list.List
	bytes, budget int
	pending       map[artKey]*artRequest
	epoch         uint64
	albumEpoch    map[*Album]uint64
	jobs          chan *artRequest
	start         sync.Once
	done          chan struct{}
	closed        bool
	foreground    atomic.Int32
	background    chan struct{}
	sem           chan struct{}
}

type artEntry struct {
	k     artKey
	img   *image.RGBA
	final bool
}

type artRequest struct {
	ui                  *IpodUI
	k                   artKey
	source              *Album // artwork fields captured before a later cover replacement
	fallback            *image.RGBA
	epoch, albumEpoch   uint64
	foreground, running bool // protected by the cache mutex
}

func newArtCache() *artCache {
	return &artCache{imgs: map[artKey]*list.Element{}, lru: list.New(), budget: 128 << 20,
		pending: map[artKey]*artRequest{}, albumEpoch: map[*Album]uint64{},
		jobs: make(chan *artRequest, artQueueLimit), done: make(chan struct{}),
		background: make(chan struct{}, 1), sem: make(chan struct{}, 2)}
}

// clear invalidates in-flight work as well as RAM entries. Disk entries
// remain available to the workers, without making the next frame read them.
func (c *artCache) clear() {
	c.mu.Lock()
	c.epoch++
	c.imgs = map[artKey]*list.Element{}
	c.pending = map[artKey]*artRequest{}
	c.albumEpoch = map[*Album]uint64{}
	c.lru.Init()
	c.bytes = 0
	c.mu.Unlock()
}

func (c *artCache) dropAlbum(a *Album) {
	c.mu.Lock()
	c.albumEpoch[a]++
	for k, e := range c.imgs {
		if k.a == a {
			c.bytes -= len(e.Value.(*artEntry).img.Pix)
			c.lru.Remove(e)
			delete(c.imgs, k)
		}
	}
	for k := range c.pending {
		if k.a == a {
			delete(c.pending, k)
		}
	}
	c.mu.Unlock()
}

// close stops idle workers and discards queued work. A decode already
// running may finish, but cannot publish after the cache is closed.
func (c *artCache) close() {
	c.mu.Lock()
	if c.closed {
		c.mu.Unlock()
		return
	}
	c.closed = true
	c.epoch++
	c.pending = map[artKey]*artRequest{}
	close(c.done)
	for {
		select {
		case <-c.jobs:
			c.foreground.Add(-1)
		default:
			c.mu.Unlock()
			return
		}
	}
}

// get and put are called with the cache mutex held.
func (c *artCache) get(k artKey) *image.RGBA {
	if e := c.imgs[k]; e != nil {
		c.lru.MoveToFront(e)
		return e.Value.(*artEntry).img
	}
	return nil
}
func (c *artCache) final(k artKey) bool {
	e := c.imgs[k]
	return e != nil && e.Value.(*artEntry).final
}
func (c *artCache) put(k artKey, img *image.RGBA) { c.putImage(k, img, true) }
func (c *artCache) putImage(k artKey, img *image.RGBA, final bool) {
	if img == nil {
		return
	}
	if e := c.imgs[k]; e != nil {
		entry := e.Value.(*artEntry)
		if entry.final && !final {
			return
		}
		c.bytes -= len(entry.img.Pix)
		entry.img = img
		entry.final = final
		c.bytes += len(img.Pix)
		c.lru.MoveToFront(e)
	} else {
		c.imgs[k] = c.lru.PushFront(&artEntry{k, img, final})
		c.bytes += len(img.Pix)
	}
	for c.bytes > c.budget && c.lru.Len() > 1 {
		e := c.lru.Back()
		entry := e.Value.(*artEntry)
		c.lru.Remove(e)
		delete(c.imgs, entry.k)
		c.bytes -= len(entry.img.Pix)
	}
}

func (c *artCache) valid(r *artRequest) bool {
	return !c.closed && r.epoch == c.epoch && r.albumEpoch == c.albumEpoch[r.k.a] && c.pending[r.k] == r
}
func (c *artCache) request(ui *IpodUI, k artKey) *artRequest {
	var source *Album
	if k.a != nil {
		source = &Album{ArtImg: k.a.ArtImg}
		// A retained decoded image is already the immutable source. Do not
		// tie it to a file identity that could change before this job runs.
		if source.ArtImg == nil {
			source.ArtPath = k.a.ArtPath
		}
	}
	return &artRequest{ui: ui, k: k, source: source, epoch: c.epoch, albumEpoch: c.albumEpoch[k.a], fallback: c.best(k)}
}

// best only returns existing RAM pixels. Their exact-size temporary copy
// is prepared by a worker, never by the caller drawing a frame.
func (c *artCache) best(k artKey) *image.RGBA {
	var best *image.RGBA
	for _, d := range []int{256, 2 * ssCell, 192, 96, ssCell, 512, 480} {
		if d == k.dev {
			continue
		}
		if img := c.get(artKey{k.a, d}); img != nil && (best == nil || img.Bounds().Dx() > best.Bounds().Dx()) {
			best = img
		}
	}
	if k.a != nil && c.epoch == 0 && c.albumEpoch[k.a] == 0 && (best == nil || best.Bounds().Dx() < 2*ssCell) {
		ssArt.Lock()
		for _, d := range []int{2 * ssCell, ssCell} {
			if img := ssArt.img[artKey{k.a, d}]; img != nil && (best == nil || img.Bounds().Dx() > best.Bounds().Dx()) {
				best = img
			}
		}
		ssArt.Unlock()
	}
	return best
}

// enqueue is nonblocking and coalesces one request per album/size. A full
// queue leaves the key retryable on the next frame instead of spawning an
// unbounded overflow goroutine. Background work can be promoted before it
// starts, preserving foreground priority without decoding it twice.
func (c *artCache) enqueue(r *artRequest) bool {
	if c.closed {
		return false
	}
	r.foreground = true
	c.foreground.Add(1)
	select {
	case c.jobs <- r:
		return true
	default:
		r.foreground = false
		c.foreground.Add(-1)
		return false
	}
}
func (c *artCache) startWorkers() {
	c.start.Do(func() {
		for i := 0; i < cap(c.sem); i++ {
			go func() {
				for {
					select {
					case <-c.done:
						return
					case r := <-c.jobs:
						c.run(r)
					}
				}
			}()
		}
	})
}

// art is a RAM lookup and a bounded queue submission only. It can return
// a worker-prepared temporary image until the full-quality result arrives.
func (ui *IpodUI) art(a *Album, devSize int) *image.RGBA {
	if devSize <= 0 {
		return nil
	}
	c := ui.artc
	k := artKey{a, devSize}
	c.mu.Lock()
	img := c.get(k)
	if c.final(k) || c.closed {
		c.mu.Unlock()
		return img
	}
	r := c.pending[k]
	queued := false
	if r == nil {
		r = c.request(ui, k)
		if c.enqueue(r) {
			c.pending[k] = r
			queued = true
		}
	} else if !r.foreground && !r.running {
		queued = c.enqueue(r)
	}
	c.mu.Unlock()
	if queued {
		c.startWorkers()
	}
	return img
}

func (c *artCache) begin(r *artRequest, foreground bool) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	if !c.valid(r) || r.running || r.foreground != foreground {
		return false
	}
	r.running = true
	return true
}
func (c *artCache) publish(r *artRequest, img *image.RGBA, final bool) bool {
	c.mu.Lock()
	if !c.valid(r) {
		c.mu.Unlock()
		return false
	}
	c.putImage(r.k, img, final)
	if final {
		delete(c.pending, r.k)
	}
	c.mu.Unlock()
	r.ui.post(func() { r.ui.onArtLoaded() })
	return true
}
func (c *artCache) run(r *artRequest) {
	defer c.foreground.Add(-1)
	if !c.begin(r, true) {
		return
	}
	out := r.ui.loadArtPrepared(r.source, r.k.dev, func() bool {
		if r.fallback == nil {
			return true
		}
		fallback := scaleImage(r.fallback, r.k.dev, r.k.dev)
		return c.publish(r, fallback, false)
	})
	c.publish(r, out, true)
}

func (ui *IpodUI) loadArtBG(a *Album, devSize int) *image.RGBA {
	c := ui.artc
	c.background <- struct{}{}
	defer func() { <-c.background }()
	for i := 0; c.foreground.Load() > 0 && i < 500; i++ {
		time.Sleep(10 * time.Millisecond)
	}
	return ui.loadArt(a, devSize)
}

// Existing callers that draw at an explicit destination size may show
// another RAM image immediately. No Catmull-Rom scaling happens here.
func (ui *IpodUI) artFallback(a *Album, devSize int) *image.RGBA {
	if img := ui.art(a, devSize); img != nil {
		return img
	}
	c := ui.artc
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.best(artKey{a, devSize})
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
	f, err := os.Open(a.ArtPath)
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
	return ui.loadArtPrepared(a, devSize, nil)
}

// A warm full-quality disk entry wins immediately. Only a disk miss needs
// a temporary resized fallback while the original is decoded.
func (ui *IpodUI) loadArtPrepared(a *Album, devSize int, prepare func() bool) *image.RGBA {
	c := ui.artc
	c.sem <- struct{}{}
	defer func() { <-c.sem }()
	// Capture the disk identity once. A cover replaced while decoding must
	// not write old pixels under the replacement's newer cache identity.
	prefix := artDiskPrefix(a)
	if out := readArtDiskPath(artDiskPathFrom(prefix, devSize), devSize); out != nil {
		return out
	}
	if prepare != nil && !prepare() {
		return nil
	}
	src := decodeAlbumSource(a)
	if src == nil {
		return scaleImage(ui.defaultArt(), devSize, devSize)
	}
	// one smaller copy for all the sizes below: the full picture (a 3000 px
	// cover is 13 MB decoded) can go now
	big := max(devSize, slices.Max(prefetchSizes))
	src = shrinkImage(src, big, big)
	out := scaleImage(src, devSize, devSize)
	writeArtDiskPath(artDiskPathFrom(prefix, devSize), devSize, out)
	// the other common sizes from the same decode, for next time
	for _, d := range prefetchSizes {
		if path := artDiskPathFrom(prefix, d); d != devSize && path != "" && !fileExists(path) {
			writeArtDiskPath(path, d, scaleImage(src, d, d))
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
	c := ui.artc
	for _, a := range lib.Albums {
		if ui.app.Library != lib || !ui.app.Running {
			return
		}
		k := artKey{a, 96}
		c.mu.Lock()
		if c.closed {
			c.mu.Unlock()
			return
		}
		if c.final(k) || c.pending[k] != nil {
			c.mu.Unlock()
			continue
		}
		r := c.request(ui, k)
		c.pending[k] = r
		c.mu.Unlock()
		c.background <- struct{}{}
		for i := 0; c.foreground.Load() > 0 && i < 500; i++ {
			time.Sleep(10 * time.Millisecond)
		}
		if c.begin(r, false) {
			c.publish(r, ui.loadArt(r.source, 96), true)
		}
		<-c.background
		time.Sleep(3 * time.Millisecond)
	}
	ui.post(func() { ui.onArtLoaded() })
	ui.post(func() {
		if saverAfter > 0 {
			ui.ssWarm()
		}
	})
}

// prepareDefaultArt keeps the usual placeholders ready before Run starts.
// An uncommon dynamically sized placeholder retains the exact-size slow
// path below; regular frame lookups never load or resize the default asset.
func (ui *IpodUI) prepareDefaultArt() {
	for _, d := range []int{80, 96, 112, 150, 192, 200, 256, 480, 512, ssCell, 2 * ssCell} {
		ui.defaultArtSized(d)
	}
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
// so a changed cover gets a new entry. Sizes up to 320 px are raw RGBA;
// bigger ones (the 480 px previews) are JPEG: raw they would take 900 KB
// an album on the card, and the slideshow would decode the original cover
// at every step without them.

// artJPEGFrom: sizes above this are kept as JPEG.
const artJPEGFrom = 320

func artDiskPrefix(a *Album) string {
	if a == nil || a.ArtPath == "" {
		return ""
	}
	st, err := os.Stat(a.ArtPath)
	if err != nil {
		return ""
	}
	h := fnv.New64a()
	fmt.Fprintf(h, "%s|%d|%d", a.ArtPath, st.Size(), st.ModTime().UnixNano())
	return filepath.Join(dataPath(".ipod_art"), fmt.Sprintf("%016x", h.Sum64()))
}
func artDiskPathFrom(prefix string, dev int) string {
	if prefix == "" {
		return ""
	}
	ext := "rgba"
	if dev > artJPEGFrom {
		ext = "jpg"
	}
	return fmt.Sprintf("%s_%d.%s", prefix, dev, ext)
}
func artDiskPath(a *Album, dev int) string { return artDiskPathFrom(artDiskPrefix(a), dev) }

func readArtDisk(a *Album, dev int) *image.RGBA {
	return readArtDiskPath(artDiskPath(a, dev), dev)
}
func readArtDiskPath(p string, dev int) *image.RGBA {
	if p == "" {
		return nil
	}
	if dev > artJPEGFrom {
		return readArtJPEG(p, dev)
	}
	data, err := os.ReadFile(p)
	if err != nil || len(data) != dev*dev*4 {
		return nil
	}
	return &image.RGBA{Pix: data, Stride: dev * 4, Rect: image.Rect(0, 0, dev, dev)}
}

func writeArtDisk(a *Album, dev int, img *image.RGBA) {
	writeArtDiskPath(artDiskPath(a, dev), dev, img)
}
func writeArtDiskPath(p string, dev int, img *image.RGBA) {
	if p == "" || img.Bounds().Dx() != dev || img.Bounds().Dy() != dev || img.Stride != dev*4 {
		return
	}
	_ = os.MkdirAll(filepath.Dir(p), 0755)
	data := img.Pix
	if dev > artJPEGFrom {
		var b bytes.Buffer
		if jpeg.Encode(&b, img, &jpeg.Options{Quality: 90}) != nil {
			return
		}
		data = b.Bytes()
	}
	// Different sizes of the same album can prepare shared cache entries
	// concurrently. Give each writer its own temporary file so a reader
	// sees either complete image, never another worker's partial write.
	f, err := os.CreateTemp(filepath.Dir(p), ".art-*.tmp")
	if err != nil {
		return
	}
	tmp := f.Name()
	defer os.Remove(tmp)
	_ = f.Chmod(0644)
	_, err = f.Write(data)
	if closeErr := f.Close(); err == nil {
		err = closeErr
	}
	if err == nil {
		_ = os.Rename(tmp, p)
	}
}

// readArtJPEG: a big size from the disk cache (nil if missing or not
// dev x dev).
func readArtJPEG(p string, dev int) *image.RGBA {
	f, err := os.Open(p)
	if err != nil {
		return nil
	}
	defer f.Close()
	src, err := jpeg.Decode(bufio.NewReader(f))
	if err != nil || src.Bounds().Dx() != dev || src.Bounds().Dy() != dev {
		return nil
	}
	out := newCanvas(dev, dev)
	draw.Draw(out, out.Bounds(), src, src.Bounds().Min, draw.Src)
	return out
}

// pruneArtCaches deletes the cached covers no album uses any more (after a
// scan, in the background): .ipod_art's scaled copies are named from their
// cover file's path, size and time, the artwork cache's from the album's key,
// so a changed cover or a renamed album leaves its old files behind for
// good. Only files older than 10 minutes go (not one just written for an
// album the scan is still filling in).
func pruneArtCaches(lib *Library) {
	if lib == nil || len(lib.Albums) == 0 {
		return
	}
	scaled := map[string]bool{}
	keys := map[string]bool{}
	for _, a := range lib.Albums {
		keys[albumArtKey(a)] = true
		if p := artDiskPath(a, 1); p != "" {
			scaled[filepath.Base(p)[:16]] = true
		}
	}
	old := time.Now().Add(-10 * time.Minute)
	prune := func(dir string, n int, keep map[string]bool) int {
		es, err := os.ReadDir(dir)
		if err != nil {
			return 0
		}
		gone := 0
		for _, e := range es {
			name := e.Name()
			if e.IsDir() || len(name) < n || !isHex(name[:n]) || keep[name[:n]] {
				continue
			}
			if info, err := e.Info(); err != nil || info.ModTime().After(old) {
				continue
			}
			if os.Remove(filepath.Join(dir, name)) == nil {
				gone++
			}
		}
		return gone
	}
	n := prune(dataPath(".ipod_art"), 16, scaled) + prune(strings.TrimSuffix(ARTWORK_DIR(), "/"), 32, keys)
	if n > 0 {
		logMsg(fmt.Sprintf("Artwork: %d cached covers no album uses were deleted", n))
	}
}

func isHex(s string) bool {
	for _, c := range s {
		if (c < '0' || c > '9') && (c < 'a' || c > 'f') {
			return false
		}
	}
	return true
}
