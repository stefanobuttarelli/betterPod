package main

// ij_ssart.go — the screensaver's covers, ready before they are shown:
// kept at the two sizes it draws (a cover, 110 px, and the big one, 220)
// in their own memory, filled in the background (from the scaled files on
// disk, made from the Cover Flow size the first time), so the mosaic
// appears whole and a cover only turns over to an album that is ready.
// While a cover grows, flies or turns it is drawn by a quick nearest-pixel
// scaling (drawNN), straight onto the screen.

import (
	"image"
	"math"
	"sync"
)

const ssMaxWarm = 240 // albums kept ready (a 110 px cover is 48 KB)

var ssArt = struct {
	sync.Mutex
	img     map[artKey]*image.RGBA
	pending map[artKey]bool
	big     []artKey // the big ones kept (a few: 190 KB each)
	warmed  *Library
}{img: map[artKey]*image.RGBA{}, pending: map[artKey]bool{}}

// ssArtGet: al's cover at size (ssCell or 2*ssCell), or nil while it is
// being got ready (it is asked for then).
func (ui *IpodUI) ssArtGet(al *Album, size int) *image.RGBA {
	if al == nil {
		return nil
	}
	k := artKey{al, size}
	ssArt.Lock()
	img := ssArt.img[k]
	if img == nil && !ssArt.pending[k] {
		ssArt.pending[k] = true
		go ui.ssArtLoad(k)
	}
	ssArt.Unlock()
	return img
}

// ssArtReady: the cover is in memory (without asking for it).
func ssArtReady(al *Album, size int) bool {
	ssArt.Lock()
	defer ssArt.Unlock()
	return ssArt.img[artKey{al, size}] != nil
}

func (ui *IpodUI) ssArtLoad(k artKey) {
	img := readArtDisk(k.a, k.dev)
	if img == nil {
		// from the Cover Flow size (on disk for every album shown once),
		// kept on disk at this size for next time
		src := ui.loadArt(k.a, 256)
		img = scaleImage(src, k.dev, k.dev)
		writeArtDisk(k.a, k.dev, img)
	}
	ssArt.Lock()
	ssArt.img[k] = img
	delete(ssArt.pending, k)
	if k.dev > ssCell {
		ssArt.big = append(ssArt.big, k)
		for len(ssArt.big) > 4 {
			delete(ssArt.img, ssArt.big[0])
			ssArt.big = ssArt.big[1:]
		}
	}
	ssArt.Unlock()
	ui.post(func() { ui.dirty = true })
}

// ssWarm gets the covers of (up to ssMaxWarm) albums ready, one at a time,
// once per library: started a little before the screensaver could start.
func (ui *IpodUI) ssWarm() {
	lib := ui.app.Library
	ssArt.Lock()
	if lib == nil || ssArt.warmed == lib {
		ssArt.Unlock()
		return
	}
	ssArt.warmed = lib
	// a new library: the old covers go
	ssArt.img = map[artKey]*image.RGBA{}
	ssArt.big = nil
	ssArt.Unlock()
	albums := lib.Albums
	if len(albums) > ssMaxWarm {
		albums = append([]*Album(nil), albums...)
		for i := range albums {
			j := i + int(uint32(i*2654435761)%uint32(len(albums)-i))
			albums[i], albums[j] = albums[j], albums[i]
		}
		albums = albums[:ssMaxWarm]
	}
	go func() {
		for _, a := range albums {
			if ui.app.Library != lib || !ui.app.Running {
				return
			}
			k := artKey{a, ssCell}
			ssArt.Lock()
			skip := ssArt.img[k] != nil || ssArt.pending[k]
			if !skip {
				ssArt.pending[k] = true
			}
			ssArt.Unlock()
			if !skip {
				ui.ssArtLoad(k)
			}
		}
	}()
}

// ssWarmAlbums: the albums whose covers are ready.
func ssWarmAlbums(all []*Album) []*Album {
	ssArt.Lock()
	defer ssArt.Unlock()
	var out []*Album
	for _, a := range all {
		if ssArt.img[artKey{a, ssCell}] != nil {
			out = append(out, a)
		}
	}
	return out
}

// drawNN draws src scaled into r (nearest pixel, opaque), darkened by dark
// (0: as it is) and faded by op over what is there.
func drawNN(dst, src *image.RGBA, r image.Rectangle, dark, op float64) {
	if src == nil || op <= 0 {
		return
	}
	cr := r.Intersect(dst.Bounds())
	if cr.Empty() {
		return
	}
	sw, sh := src.Bounds().Dx(), src.Bounds().Dy()
	k := 1 - math.Max(0, math.Min(1, dark))
	mul := uint32(k*256 + 0.5)
	a := uint32(math.Max(0, math.Min(1, op))*256 + 0.5)
	xs := make([]int, cr.Dx())
	for i := range xs {
		sx := (cr.Min.X + i - r.Min.X) * sw / r.Dx()
		xs[i] = min(max(sx, 0), sw-1) * 4
	}
	for y := cr.Min.Y; y < cr.Max.Y; y++ {
		sy := min(max((y-r.Min.Y)*sh/r.Dy(), 0), sh-1)
		srow := src.Pix[sy*src.Stride:]
		o := dst.PixOffset(cr.Min.X, y)
		drow := dst.Pix[o : o+cr.Dx()*4]
		for i, sx := range xs {
			d := drow[i*4 : i*4+4 : i*4+4]
			r8 := uint32(srow[sx]) * mul >> 8
			g8 := uint32(srow[sx+1]) * mul >> 8
			b8 := uint32(srow[sx+2]) * mul >> 8
			if a >= 256 {
				d[0], d[1], d[2], d[3] = uint8(r8), uint8(g8), uint8(b8), 255
			} else {
				d[0] = uint8((r8*a + uint32(d[0])*(256-a)) >> 8)
				d[1] = uint8((g8*a + uint32(d[1])*(256-a)) >> 8)
				d[2] = uint8((b8*a + uint32(d[2])*(256-a)) >> 8)
				d[3] = 255
			}
		}
	}
}
