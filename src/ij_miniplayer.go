package main

// ij_miniplayer.go — the Now Playing block at the bottom of the main menu
// (as on the iPod nano): the cover, the song in quotes, the artist and the
// album on a dark woven panel.

import (
	"image"
	"math"
)

// miniPlayerH is the height of the block (CSS px).
const miniPlayerH = 58.0

// miniPlayerTrack: the track the block shows (nil: no block).
func (ui *IpodUI) miniPlayerTrack() *Track {
	if p := ui.app.Playing; p != nil && p.Track != nil && p.State != StateStopped {
		return p.Track
	}
	return nil
}

var miniPlayerBg *image.RGBA

// miniPlayerTexture: the dark woven fabric (assets/ipod/woven.png, a
// seamless 256 px tile) repeated at device resolution, with a dark top
// edge and a faint light line under it. Built once per size.
func (ui *IpodUI) miniPlayerTexture(w, h int) *image.RGBA {
	if miniPlayerBg != nil && miniPlayerBg.Bounds().Dx() == w && miniPlayerBg.Bounds().Dy() == h {
		return miniPlayerBg
	}
	img := newCanvas(w, h)
	tile := ui.asset("woven")
	tw, th := tile.Bounds().Dx(), tile.Bounds().Dy()
	if tw == 0 || th == 0 {
		fillRect(img, 0, 0, w, h, rgb(44, 48, 60))
	} else {
		for y := 0; y < h; y++ {
			src := tile.Pix[(y%th)*tile.Stride:]
			dst := img.Pix[y*img.Stride:]
			for x := 0; x < w; x++ {
				copy(dst[x*4:x*4+4], src[(x%tw)*4:(x%tw)*4+4])
				dst[x*4+3] = 255
			}
		}
	}
	fillRect(img, 0, 0, w, px(1), rgb(8, 9, 12))
	fillRect(img, 0, px(1), w, 1, rgba(255, 255, 255, 0.12))
	miniPlayerBg = img
	return img
}

func clamp8(v float64) uint8 {
	return uint8(math.Max(0, math.Min(255, v+0.5)))
}

// drawMiniPlayer draws the block into dst, full width, at its bottom.
func (ui *IpodUI) drawMiniPlayer(dst *image.RGBA, t *Track) {
	w := dst.Bounds().Dx()
	h := px(miniPlayerH)
	y0 := dst.Bounds().Dy() - h
	blitOpaque(dst, ui.miniPlayerTexture(w, h), 0, y0)
	top := float64(y0) / DPR
	// cover with a thin light frame and a soft shadow under it
	art := 40.0
	ax, ay := 7.0, top+(miniPlayerH-art)/2+0.5
	fillRect(dst, px(ax), px(ay+art), px(art), px(1.5), rgba(0, 0, 0, 0.35))
	fillRect(dst, px(ax-1), px(ay-1), px(art+2), px(art+2), rgba(255, 255, 255, 0.85))
	blit(dst, ui.thumb(ui.trackAlbum(t), art), px(ax), px(ay), 1)
	// three lines: “Song”, artist, album
	tx := ax + art + 7
	tw := (float64(w)/DPR - tx - 6) * DPR
	lines := []struct {
		s    string
		w    FontWeight
		size float64
		a    float64
	}{
		{"“" + t.Title + "”", W700, 12, 1},
		{t.Artist, W600, 11, 0.86},
		{t.Album, W500, 11, 0.86},
	}
	total := 0.0
	for _, l := range lines {
		lh, _ := lineBox(l.size)
		total += lh
	}
	y := top + (miniPlayerH-total)/2 + 0.5
	for _, l := range lines {
		lh, asc := lineBox(l.size)
		if l.s != "" {
			f := face(l.w, l.size)
			s := ellipsize(f, l.s, tw)
			// a 1 px dark shadow keeps the white text crisp on the blue
			drawText(dst, f, s, tx*DPR, (y+asc)*DPR+1, rgba(0, 0, 0, 0.35))
			drawText(dst, f, s, tx*DPR, (y+asc)*DPR, rgba(255, 255, 255, l.a))
		}
		y += lh
	}
}
