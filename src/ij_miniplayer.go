package main

// ij_miniplayer.go — the Now Playing block at the bottom of the menus (as on
// the iPod nano): the cover, the song, the artist and the album on
// a dark woven panel.
//
// It belongs to the panel, not to a list: it stays put while the menus
// slide above it, and slides away (down) on the screens that do not show it
// (settings details, search, games, Now Playing itself).
//
//   - Playing: long lines scroll (marquee).
//   - Paused: the lines stop at their start, the cover shrinks a little,
//     dims and shows a pause sign; after 10 minutes of pause the block
//     slides away (it comes back when the song plays again).
//   - Next / previous song: the old song slides out and the new one in,
//     in the direction of the change.

import (
	"image"
	"math"
	"os"
	"strconv"
	"time"
)

// miniPlayerH is the height of the block (CSS px).
const miniPlayerH = 58.0

// miniViews: the lists that keep the block under them.
var miniViews = map[string]bool{
	"home": true, "music": true, "games": true, "settings": true,
	"artists": true, "artist": true, "albums": true, "album": true,
	"playlists": true, "playlist": true, "songs": true,
}

type miniState struct {
	show    map[ViewKind]*Anim // block shown in the split / full panel
	pause   *Anim              // 0 playing, 1 paused
	change  *Anim              // song change, 0 → 1
	shown   *Track             // the song on the block
	old     *Track             // the song sliding out
	dir     float64            // +1 next, -1 previous
	since   time.Time          // marquee start
	marqOn  bool               // a line was scrolling in the last frame
	drawnAt int                // frame the block was last drawn
}

var mini = miniState{
	show:   map[ViewKind]*Anim{KindSplit: newAnim(0), KindFull: newAnim(0)},
	pause:  newAnim(0),
	change: newAnim(1),
	dir:    1,
}

// skipDir is the direction of the last next/previous request (+1 / -1),
// read by the song-change animations; automatic advances count as next.
var skipDir = 1.0

// miniPauseHide: the block slides away after the song has been paused this
// long, and comes back when it plays again (MIYOOPOD_PAUSE_HIDE, in seconds,
// changes it).
var miniPauseHide = func() time.Duration {
	if v, err := strconv.Atoi(os.Getenv("MIYOOPOD_PAUSE_HIDE")); err == nil && v > 0 {
		return time.Duration(v) * time.Second
	}
	return 10 * time.Minute
}()

// pausedSince is when playback was last paused (zero while playing),
// followed by the UI loop (followPause).
var pausedSince time.Time

// pauseHideFired: the long pause has been noticed (the block sent away).
var pauseHideFired bool

// followPause notes when a pause starts; it marks the screen dirty when the
// block's pause time runs out, so it slides away even on an idle screen.
func (ui *IpodUI) followPause(now time.Time) {
	p := ui.app.Playing
	paused := p != nil && p.Track != nil && p.State == StatePaused
	switch {
	case !paused:
		if !pausedSince.IsZero() {
			pausedSince = time.Time{}
			ui.dirty = true
		}
	case pausedSince.IsZero():
		pausedSince = now
	case now.Sub(pausedSince) >= miniPauseHide && !pauseHideFired:
		pauseHideFired = true // once: the block slides away
		ui.dirty = true
	}
	if !paused {
		pauseHideFired = false
	}
}

// miniPlayerTrack: the track the block shows (nil: no block, also after a
// long pause).
func (ui *IpodUI) miniPlayerTrack() *Track {
	if p := ui.app.Playing; p != nil && p.Track != nil && p.State != StateStopped {
		if p.State == StatePaused && !pausedSince.IsZero() && animNow().Sub(pausedSince) >= miniPauseHide {
			return nil
		}
		return p.Track
	}
	return nil
}

// miniReserve: v keeps the bottom of its panel for the block.
func (ui *IpodUI) miniReserve(v *View) bool {
	return v.Custom == nil && miniViews[v.ID] && ui.miniPlayerTrack() != nil
}

// miniAnimating: the block needs more frames.
func (ui *IpodUI) miniAnimating(now time.Time) bool {
	for _, a := range mini.show {
		if a.Running() {
			return true
		}
	}
	if mini.pause.Running() || mini.change.Running() {
		return true
	}
	return mini.marqOn && !ui.noMarquee && mini.drawnAt >= ui.frameNo-1 && marqSyncNeedsFrame(now)
}

var miniPlayerBg *image.RGBA

// miniPlayerTexture: the dark woven fabric (assets/ipod/woven.png, a
// seamless 256 px tile) repeated at device resolution, with a dark top
// edge and a faint light line under it. Built once per size.
func (ui *IpodUI) miniPlayerTexture(w, h int) *image.RGBA {
	// built once at the full width; a narrower panel takes its left part
	// (both panels can show the block in the same frame)
	if miniPlayerBg != nil && miniPlayerBg.Bounds().Dx() >= w && miniPlayerBg.Bounds().Dy() == h {
		return miniPlayerBg.SubImage(image.Rect(0, 0, w, h)).(*image.RGBA)
	}
	want := w
	w = max(w, 640)
	img := newCanvas(w, h)
	tile := ui.asset("woven")
	tw, th := tile.Bounds().Dx(), tile.Bounds().Dy()
	if tw <= 1 || th <= 1 {
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
	return img.SubImage(image.Rect(0, 0, want, h)).(*image.RGBA)
}

// drawMiniPlayer draws the block of a panel (x, w device px) whose stack
// has top on top: shown, or sliding away.
func (ui *IpodUI) drawMiniPlayer(kind ViewKind, top *View, x, w int, now time.Time) {
	t := ui.miniPlayerTrack()
	want := 0.0
	if t != nil && top != nil && ui.miniReserve(top) {
		want = 1
	}
	sh := mini.show[kind]
	if sh.Target() != want {
		sh.To(want, durSlide, easeCSS, 0)
	}
	s := sh.Get(now)
	if t == nil {
		t = mini.shown
	}
	if s <= 0.001 || t == nil {
		return
	}
	ui.updateMini(t, now)
	h := px(miniPlayerH)
	y0 := 480 - h + int(float64(h)*(1-s)+0.5)
	clip := image.Rect(x, y0, x+w, 480).Intersect(ui.fb.Bounds())
	if clip.Empty() {
		return
	}
	dst := ui.fb.SubImage(clip).(*image.RGBA)
	blitOpaque(dst, ui.miniPlayerTexture(w, h), x, y0)
	mini.drawnAt = ui.frameNo
	k := mini.change.Get(now)
	if k < 1 && mini.old != nil {
		// the old song slides out, then the new one comes in; the cover
		// moves with it only when the album changes
		off := 26 * mini.dir
		out, in := changePhases(k)
		same := ui.trackAlbum(mini.old) == ui.trackAlbum(t)
		if same {
			ui.drawMiniCover(dst, t, float64(x)/DPR, float64(y0)/DPR, 1, 0, mini.pause.Get(now))
		}
		ui.drawMiniContent(dst, mini.old, float64(x)/DPR, float64(y0)/DPR, float64(w)/DPR, -off*out, 1-out, !same, false, now)
		ui.drawMiniContent(dst, t, float64(x)/DPR, float64(y0)/DPR, float64(w)/DPR, off*(1-in), in, !same, false, now)
		return
	}
	ui.drawMiniContent(dst, t, float64(x)/DPR, float64(y0)/DPR, float64(w)/DPR, 0, 1, true, true, now)
}

// updateMini follows the song and the play / pause state.
func (ui *IpodUI) updateMini(t *Track, now time.Time) {
	if t != mini.shown {
		if mini.shown != nil && mini.drawnAt >= ui.frameNo-1 {
			mini.old = mini.shown
			mini.dir = skipDir
			mini.change.Set(0)
			mini.change.To(1, 380*time.Millisecond, easeLinear, 0) // the phases ease themselves
		}
		mini.shown = t
		mini.since = now
	}
	paused := 0.0
	if p := ui.app.Playing; p != nil && p.State == StatePaused {
		paused = 1
	}
	if mini.pause.Target() != paused {
		mini.pause.To(paused, 300*time.Millisecond, easeInOut, 0)
		if paused == 0 {
			mini.since = now // the lines start scrolling again from the start
		}
	}
	if mini.drawnAt < ui.frameNo-1 {
		mini.pause.Set(paused)
		mini.change.Set(1)
	}
}

// drawMiniContent draws the cover and the three lines of t with the block's
// left edge at x (CSS px), at opacity a. live: lines may scroll.
func (ui *IpodUI) drawMiniContent(dst *image.RGBA, t *Track, x, top, w, dx, a float64, withArt, live bool, now time.Time) {
	pz := mini.pause.Get(now)
	if withArt {
		ui.drawMiniCover(dst, t, x, top, a, dx, pz)
	}
	if a <= 0.002 {
		return
	}
	// the lines stay inside their column while they slide
	colL := x + 7 + 40 + 4
	b := dst.Bounds()
	if r := image.Rect(px(colL), b.Min.Y, b.Max.X, b.Max.Y).Intersect(b); !r.Empty() {
		dst = dst.SubImage(r).(*image.RGBA)
	} else {
		return
	}
	x += dx
	art := 40.0
	// song, artist, album
	tx := x + 7 + art + 7
	tw := x + w - 8 - tx
	lines := []struct {
		s    string
		w    FontWeight
		size float64
		a    float64
	}{
		{t.Title, W700, 12, 1},
		{t.Artist, W600, 11, 0.86},
		{t.Album, W500, 11, 0.86},
	}
	total := 0.0
	for _, l := range lines {
		lh, _ := lineBox(l.size)
		total += lh
	}
	y := top + (miniPlayerH-total)/2 + 0.5
	scroll := live && pz < 0.5
	// the lines that scroll share one cycle (they start together and wait
	// for the longest)
	cyc := 0.0
	for _, l := range lines {
		if l.s != "" {
			cyc = math.Max(cyc, marqueeCycle(face(l.w, l.size), l.s, tw))
		}
	}
	for _, l := range lines {
		lh, asc := lineBox(l.size)
		if l.s != "" {
			f := face(l.w, l.size)
			c := rgba(255, 255, 255, l.a*a)
			sc := rgba(0, 0, 0, 0.35*a)
			if scroll && textWidth(f, l.s)/DPR > tw {
				// a 1 px dark shadow keeps the white text crisp on the
				// fabric
				drawMarqueeSync(dst, f, l.s, tx, y+asc+0.5, tw, sc, mini.since, now, cyc)
				drawMarqueeSync(dst, f, l.s, tx, y+asc, tw, c, mini.since, now, cyc)
				mini.marqOn = true
			} else {
				s := ellipsize(f, l.s, tw*DPR)
				drawText(dst, f, s, tx*DPR, (y+asc)*DPR+1, sc)
				drawText(dst, f, s, tx*DPR, (y+asc)*DPR, c)
			}
		}
		y += lh
	}
}

// drawMiniCover: the cover with a thin light frame (moved by dx and faded
// to a during a song change); paused it shrinks, dims and shows ‖.
func (ui *IpodUI) drawMiniCover(dst *image.RGBA, t *Track, x, top, a, dx, pz float64) {
	if a <= 0.002 {
		return
	}
	art := 40.0
	x += dx
	sz := art * (1 - 0.14*pz)
	ax := x + 7 + (art-sz)/2
	ay := top + (miniPlayerH-sz)/2 + 0.5
	fillRect(dst, px(ax-1), px(ay-1), px(sz+2), px(sz+2), rgba(255, 255, 255, 0.85*a))
	img := ui.thumb(ui.trackAlbum(t), 40)
	if d := px(sz); d != img.Bounds().Dx() {
		img = scaledThumb(img, d)
	}
	blit(dst, img, px(ax), px(ay), a)
	if pz > 0.01 {
		fillRect(dst, px(ax), px(ay), px(sz), px(sz), rgba(0, 0, 0, 0.42*pz*a))
		bw, bh, gap := 3.5, 13.0, 3.0
		cx, cy := ax+sz/2, ay+sz/2
		for _, bx := range []float64{cx - gap/2 - bw, cx + gap/2} {
			fillRoundedGradient(dst, bx*DPR, (cy-bh/2)*DPR, (bx+bw)*DPR, (cy+bh/2)*DPR, 1.2*DPR,
				[]GradStop{{0, rgb(255, 255, 255)}, {1, rgb(225, 228, 233)}}, pz*a)
		}
	}
}

// scaledThumb: img resized to d device px (the paused cover), cached for
// the last image and size.
func scaledThumb(img *image.RGBA, d int) *image.RGBA {
	if d < 2 {
		return img
	}
	if thumbScaleSrc == img && thumbScaleOut != nil && thumbScaleOut.Bounds().Dx() == d {
		return thumbScaleOut
	}
	thumbScaleSrc, thumbScaleOut = img, scaleImage(img, d, d)
	return thumbScaleOut
}

var thumbScaleSrc, thumbScaleOut *image.RGBA
