//go:build ignore

package main

import (
	"bytes"
	"image"
	"image/color"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"
)

func artTestUI(t *testing.T, workers bool) *IpodUI {
	t.Helper()
	c := newArtCache()
	if !workers {
		c.start.Do(func() {})
	}
	t.Cleanup(c.close)
	return &IpodUI{artc: c, events: make(chan func(), 512)}
}

func TestArtLookupDoesNotReadWarmDiskOnUI(t *testing.T) {
	ui := artTestUI(t, false)
	old := dataDirOverride
	dataDirOverride = t.TempDir()
	t.Cleanup(func() { dataDirOverride = old })
	a := &Album{ArtPath: filepath.Join(t.TempDir(), "cover.png")}
	if err := os.WriteFile(a.ArtPath, []byte("cover identity"), 0600); err != nil {
		t.Fatal(err)
	}
	disk := image.NewRGBA(image.Rect(0, 0, 96, 96))
	disk.SetRGBA(4, 7, color.RGBA{12, 34, 56, 255})
	writeArtDisk(a, 96, disk)
	if readArtDisk(a, 96) == nil {
		t.Fatal("test did not create a warm disk entry")
	}
	if got := ui.art(a, 96); got != nil {
		t.Fatal("UI lookup synchronously read the disk entry")
	}
	if len(ui.artc.jobs) != 1 {
		t.Fatalf("queued %d jobs", len(ui.artc.jobs))
	}
	for i := 0; i < 20; i++ {
		if got := ui.art(a, 96); got != nil {
			t.Fatal("pending lookup read disk")
		}
	}
	if len(ui.artc.jobs) != 1 || len(ui.artc.pending) != 1 {
		t.Fatal("repeated frame requests were not coalesced")
	}
	r := <-ui.artc.jobs
	ui.artc.run(r)
	if got := ui.artCached(a, 96); got == nil || !bytes.Equal(got.Pix, disk.Pix) {
		t.Fatal("worker did not publish original disk pixels")
	}
}

func TestArtFallbackReturnsExistingRAMWithoutResizing(t *testing.T) {
	ui := artTestUI(t, false)
	src := image.NewRGBA(image.Rect(0, 0, 16, 16))
	for y := 0; y < 16; y++ {
		for x := 0; x < 16; x++ {
			src.SetRGBA(x, y, color.RGBA{uint8(x * 13), uint8(y * 11), 70, 255})
		}
	}
	a := &Album{ArtImg: src}
	lower := scaleImage(src, 96, 96)
	ui.artc.mu.Lock()
	ui.artc.put(artKey{a, 96}, lower)
	ui.artc.mu.Unlock()
	for i := 0; i < 20; i++ {
		if got := ui.artFallback(a, 256); got != lower {
			t.Fatal("fallback was resized or replaced on the UI caller")
		}
	}
	if len(ui.artc.jobs) != 1 {
		t.Fatal("fallback requests did not coalesce")
	}
}

func TestArtQueueIsBoundedAndRetriesOverflow(t *testing.T) {
	ui := artTestUI(t, false)
	albums := make([]*Album, artQueueLimit+10)
	for i := range albums {
		albums[i] = &Album{}
		ui.art(albums[i], 96)
	}
	if got := len(ui.artc.jobs); got != artQueueLimit {
		t.Fatalf("queue=%d", got)
	}
	if len(ui.artc.pending) != artQueueLimit || ui.artc.foreground.Load() != artQueueLimit {
		t.Fatal("overflow left unbounded work or pending entries")
	}
	overflow := artKey{albums[len(albums)-1], 96}
	if ui.artc.pending[overflow] != nil {
		t.Fatal("overflow key cannot retry")
	}
	// Completing one queued item makes room for the previously rejected key.
	r := <-ui.artc.jobs
	ui.artc.mu.Lock()
	delete(ui.artc.pending, r.k)
	ui.artc.mu.Unlock()
	ui.artc.foreground.Add(-1)
	ui.art(overflow.a, overflow.dev)
	if ui.artc.pending[overflow] == nil || len(ui.artc.jobs) != artQueueLimit {
		t.Fatal("overflow request did not retry")
	}
	ui.artc.close()
	if ui.artc.foreground.Load() != 0 || len(ui.artc.pending) != 0 {
		t.Fatal("close did not cancel queued work")
	}
}

func TestArtClearAndAlbumReplacementRejectStaleResults(t *testing.T) {
	for _, invalidate := range []string{"clear", "album"} {
		t.Run(invalidate, func(t *testing.T) {
			ui := artTestUI(t, false)
			a := &Album{ArtImg: image.NewRGBA(image.Rect(0, 0, 16, 16))}
			ui.art(a, 96)
			old := <-ui.artc.jobs
			if invalidate == "clear" {
				ui.artc.clear()
			} else {
				ui.artc.dropAlbum(a)
			}
			a.ArtImg = image.NewRGBA(image.Rect(0, 0, 32, 32))
			ui.art(a, 96)
			fresh := ui.artc.pending[artKey{a, 96}]
			if fresh == nil || fresh == old {
				t.Fatal("invalidation did not allow a new request")
			}
			if ui.artc.publish(old, image.NewRGBA(image.Rect(0, 0, 96, 96)), true) {
				t.Fatal("stale completion repopulated cache")
			}
			if ui.artc.pending[old.k] != fresh || ui.artCached(a, 96) != nil {
				t.Fatal("stale completion changed the replacement request")
			}
			ui.artc.run(old) // stale queued work exits without loading or clearing fresh work
			if fresh.source.ArtImg != a.ArtImg {
				t.Fatal("replacement request did not capture the new source")
			}
		})
	}
}

func TestArtForegroundPromotesQueuedPrefetchWithoutDuplicateDecode(t *testing.T) {
	ui := artTestUI(t, false)
	a := &Album{ArtImg: image.NewRGBA(image.Rect(0, 0, 16, 16))}
	k := artKey{a, 96}
	ui.artc.mu.Lock()
	r := ui.artc.request(ui, k)
	ui.artc.pending[k] = r
	ui.artc.mu.Unlock()
	ui.art(a, 96)
	if len(ui.artc.jobs) != 1 || ui.artc.pending[k] != r || !r.foreground {
		t.Fatal("foreground lookup did not promote the pending background request")
	}
	if ui.artc.begin(r, false) {
		t.Fatal("background decoded work already promoted to the foreground")
	}
	job := <-ui.artc.jobs
	ui.artc.run(job)
	if ui.artCached(a, 96) == nil || ui.artc.foreground.Load() != 0 {
		t.Fatal("promoted foreground work did not finish")
	}
}

type artBlockingImage struct {
	image.Image
	once            sync.Once
	started, resume chan struct{}
}

func (i *artBlockingImage) At(x, y int) color.Color {
	i.once.Do(func() { close(i.started); <-i.resume })
	return i.Image.At(x, y)
}

func TestArtDecodeCannotWriteOldPixelsUnderReplacementDiskIdentity(t *testing.T) {
	ui := artTestUI(t, false)
	old := dataDirOverride
	dataDirOverride = t.TempDir()
	t.Cleanup(func() { dataDirOverride = old })
	path := filepath.Join(t.TempDir(), "cover")
	if err := os.WriteFile(path, []byte("old cover"), 0600); err != nil {
		t.Fatal(err)
	}
	src := image.NewRGBA(image.Rect(0, 0, 16, 16))
	src.SetRGBA(7, 8, color.RGBA{200, 30, 10, 255})
	blocking := &artBlockingImage{Image: src, started: make(chan struct{}), resume: make(chan struct{})}
	a := &Album{ArtPath: path, ArtImg: blocking}
	finished := make(chan *image.RGBA, 1)
	go func() { finished <- ui.loadArt(a, 96) }()
	select {
	case <-blocking.started:
	case <-time.After(3 * time.Second):
		t.Fatal("decode did not reach scaling")
	}
	if err := os.WriteFile(path, []byte("a different replacement cover"), 0600); err != nil {
		close(blocking.resume)
		t.Fatal(err)
	}
	future := time.Now().Add(2 * time.Second)
	os.Chtimes(path, future, future)
	replacement := artDiskPath(a, 96)
	close(blocking.resume)
	select {
	case result := <-finished:
		if result == nil {
			t.Fatal("decode failed")
		}
	case <-time.After(3 * time.Second):
		t.Fatal("decode did not finish")
	}
	if _, err := os.Stat(replacement); !os.IsNotExist(err) {
		t.Fatal("old decode wrote pixels under the replacement's new disk identity", err)
	}
}

func TestArtRunningWorkerCannotOverwriteReplacement(t *testing.T) {
	ui := artTestUI(t, true)
	oldPixels := image.NewRGBA(image.Rect(0, 0, 16, 16))
	blocked := &artBlockingImage{Image: oldPixels, started: make(chan struct{}), resume: make(chan struct{})}
	var release sync.Once
	unblock := func() { release.Do(func() { close(blocked.resume) }) }
	defer unblock()
	a := &Album{ArtImg: blocked}
	ui.art(a, 96)
	select {
	case <-blocked.started:
	case <-time.After(3 * time.Second):
		t.Fatal("worker did not start")
	}
	fresh := image.NewRGBA(image.Rect(0, 0, 16, 16))
	for y := 0; y < 16; y++ {
		for x := 0; x < 16; x++ {
			fresh.SetRGBA(x, y, color.RGBA{20, 80, 170, 255})
		}
	}
	a.ArtImg = fresh
	ui.artc.dropAlbum(a)
	ui.art(a, 96)
	deadline := time.Now().Add(3 * time.Second)
	for ui.artCached(a, 96) == nil && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	want := scaleImage(fresh, 96, 96)
	if got := ui.artCached(a, 96); got == nil || !bytes.Equal(got.Pix, want.Pix) {
		t.Fatal("second worker did not publish the replacement")
	}
	unblock()
	for ui.artc.foreground.Load() != 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if ui.artc.foreground.Load() != 0 {
		t.Fatal("old worker did not finish")
	}
	if got := ui.artCached(a, 96); got == nil || !bytes.Equal(got.Pix, want.Pix) {
		t.Fatal("old worker overwrote the replacement")
	}
}

func TestArtWorkerPublishesTemporaryBeforeBlockedFullDecode(t *testing.T) {
	ui := artTestUI(t, true)
	src := image.NewRGBA(image.Rect(0, 0, 16, 16))
	for y := 0; y < 16; y++ {
		for x := 0; x < 16; x++ {
			src.SetRGBA(x, y, color.RGBA{uint8(x * 13), uint8(y * 11), 70, 255})
		}
	}
	blocked := &artBlockingImage{Image: src, started: make(chan struct{}), resume: make(chan struct{})}
	var release sync.Once
	unblock := func() { release.Do(func() { close(blocked.resume) }) }
	defer unblock()
	a := &Album{ArtImg: blocked}
	lower := scaleImage(src, 96, 96)
	ui.artc.mu.Lock()
	ui.artc.put(artKey{a, 96}, lower)
	ui.artc.mu.Unlock()
	ui.art(a, 256)
	select {
	case <-blocked.started:
	case <-time.After(3 * time.Second):
		t.Fatal("full source decode did not start")
	}
	temporary := ui.artCached(a, 256)
	if temporary == nil || temporary.Bounds().Dx() != 256 {
		t.Fatal("worker did not publish the resized temporary before full decode")
	}
	ui.artc.mu.Lock()
	final := ui.artc.final(artKey{a, 256})
	ui.artc.mu.Unlock()
	if final {
		t.Fatal("temporary was mistaken for the final image")
	}
	for i := 0; i < 20; i++ {
		if ui.art(a, 256) != temporary {
			t.Fatal("temporary cover was resized again")
		}
	}
	unblock()
	deadline := time.Now().Add(3 * time.Second)
	for ui.artc.foreground.Load() != 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if ui.artc.foreground.Load() != 0 {
		t.Fatal("worker did not finish")
	}
	exact := ui.artCached(a, 256)
	if exact == nil || exact == temporary || !bytes.Equal(exact.Pix, scaleImage(src, 256, 256).Pix) {
		t.Fatal("temporary did not yield to full-fidelity artwork")
	}
}
