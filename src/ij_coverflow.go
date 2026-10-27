package main

// ij_coverflow.go — Cover Flow: the album covers in 3D (a small software
// rasterizer), the flip to the track list on the back, and Now Playing.

import (
	"image"
	"math"
	// "sort"
	"time"
)

// cfXform is a cover pose: translation, scales, rotation (A, degrees).
type cfXform struct {
	X, Y, Z, S1, Z2, A, TY, S2 float64
}

func lerpX(a, b cfXform, t float64) cfXform {
	l := func(x, y float64) float64 { return x + (y-x)*t }
	return cfXform{l(a.X, b.X), l(a.Y, b.Y), l(a.Z, b.Z), l(a.S1, b.S1), l(a.Z2, b.Z2), l(a.A, b.A), l(a.TY, b.TY), l(a.S2, b.S2)}
}

type cfCover struct {
	from, to cfXform
	start    time.Time
	dur      time.Duration
	opacity  *Anim
	artAlpha *Anim
	inited   bool
}

func (c *cfCover) value(now time.Time) cfXform {
	if c.dur <= 0 {
		return c.to
	}
	p := float64(now.Sub(c.start)) / float64(c.dur)
	if p >= 1 {
		return c.to
	}
	if p <= 0 {
		return c.from
	}
	return lerpX(c.from, c.to, easeCSS(p))
}

func (c *cfCover) running(now time.Time) bool {
	return c.dur > 0 && now.Sub(c.start) < c.dur || c.opacity.Running() || c.artAlpha.Running()
}

func (c *cfCover) set(to cfXform, dur time.Duration, now time.Time) {
	if !c.inited {
		c.from, c.to, c.dur, c.inited = to, to, 0, true
		return
	}
	if c.to == to {
		return
	}
	c.from = c.value(now)
	c.to = to
	c.start = now
	c.dur = dur
}

// CoverFlow is the Cover Flow screen.
type CoverFlow struct {
	albums    []*Album
	active    int
	selected  *Album // flipped to its back
	playing   bool   // Now Playing shown over the flow
	covers    map[int]*cfCover
	back      *View // track list on the back of the selected cover
	backAnim  *Anim
	backAlbum *Album
	infoFade  *Anim
	npFade    *Anim
	paused    bool
	tilt      *Anim
	backShown *Album
	backSince time.Time
	np        *NowPlayingView
	fadeIn    *Anim
	exiting   bool
	layer     *image.RGBA
	npBuf     *image.RGBA
}

// The flow: the active cover faces the viewer in the middle; the others
// stand at ±70° in two rows of overlapping covers, 48 px apart. Moving
// animates every cover to its new pose (CSS-like transitions); selecting
// flips the active cover (-180°) to show its track list, and playing a
// track turns it to the Now Playing pose next to the track info.
//
//
//
//
//
//
//
var easeFramer = cubicBezier(0.25, 0.1, 0.35, 1)

func (ui *IpodUI) openCoverFlow() {
	if ui.cover != nil && !ui.cover.exiting {
		return
	}
	cf := &CoverFlow{
		albums:   ui.library().Albums,
		covers:   map[int]*cfCover{},
		backAnim: newAnim(0),
		infoFade: newAnim(1),
		npFade:   newAnim(0),
		tilt:     newAnim(1),
		fadeIn:   newAnim(0),
	}
	cf.fadeIn.To(1, 300*time.Millisecond, easeFramer, 0)
	ui.cover = cf
	ui.headerTitle = "Cover Flow"
	// the menus slide away and the preview hides
	ui.leftX.To(-1, durPanel, easeCSS, delayMenuHide)
	ui.previewHide.To(1, durPanel, easeCSS, 0)
	ui.dirty = true
}

func (ui *IpodUI) closeCoverFlow() {
	cf := ui.cover
	cf.exiting = true
	cf.fadeIn.To(0, 300*time.Millisecond, easeInOut, 0)
	ui.previewHide.To(0, durPanel, easeCSS, 0)
	ui.headerTitle = ui.topScreen().Title
	ui.updatePanels()
	ui.dirty = true
}

func getOffsetPx(i int, w float64) float64 {
	if i == 0 {
		return 0
	}
	o := w - 46 + float64(i)*48
	if i < 0 {
		return o - 48
	}
	return o + 24
}

// targetFor: the pose of cover i, and how long to animate to it.
func (cf *CoverFlow) targetFor(i int) (cfXform, time.Duration) {
	if i == cf.active {
		x := cfXform{X: 100, Y: 4, Z: 20, S1: 1, S2: 1}
		if cf.selected != nil && cf.albums[i] == cf.selected {
			if cf.playing {
				// next to the track info, tilted like Now Playing
				return npCoverPose(cf.tilt.Target()), 500 * time.Millisecond
			}
			x.A, x.TY, x.S2 = -180, 32, 0.96
			return x, 500 * time.Millisecond
		}
		return x, 300 * time.Millisecond
	}
	a := -70.0
	if i < cf.active {
		a = 70
	}
	return cfXform{X: getOffsetPx(i-cf.active, 160), S1: 1.1, Z2: -65, A: a, S2: 1}, 250 * time.Millisecond
}

func (cf *CoverFlow) sync(now time.Time) {
	for i := cf.active - 15; i <= cf.active+15; i++ {
		if i < 0 || i >= len(cf.albums) {
			continue
		}
		c, ok := cf.covers[i]
		if !ok {
			c = &cfCover{opacity: newAnim(1), artAlpha: newAnim(1)}
			cf.covers[i] = c
		}
		to, dur := cf.targetFor(i)
		c.set(to, dur, now)
		hidden := i != cf.active && cf.playing
		if hidden {
			c.opacity.To(0, 350*time.Millisecond, easeCSS, 0)
		} else {
			c.opacity.To(1, 350*time.Millisecond, easeCSS, 0)
		}
		if cf.selected != nil && cf.albums[i] == cf.selected &&
			!cf.playing {
			c.artAlpha.To(0, 500*time.Millisecond, easeCSS, 0)
		} else {
			c.artAlpha.To(1, 500*time.Millisecond, easeCSS, 0)
		}
	}
	for i := range cf.covers {
		if i < cf.active-15 || i > cf.active+15 {
			delete(cf.covers, i)
		}
	}
}

// scroll: Now Playing volume / scrubbing, the back's track list, or the
// flow itself.
func (cf *CoverFlow) scroll(ui *IpodUI, dir int, velocity float64) {
	//
	if cf.playing {
		cf.np.OnScroll(ui, dir, velocity)
	} else if cf.selected != nil {
		if cf.back != nil && len(cf.back.Items) > 0 {
			ni := cf.back.Index + dir*skipFor(velocity)
			if ni < 0 {
				ni = 0
			}
			if ni >= len(cf.back.Items) {
				ni = len(cf.back.Items) - 1
			}
			cf.back.Index = ni
			cf.back.dirty = true
		}
	} else {
		ni := cf.active + dir
		if ni >= 0 && ni < len(cf.albums) {
			cf.active = ni
		}
	}
	ui.dirty = true
}

func (cf *CoverFlow) center(ui *IpodUI) {
	// Now Playing / play the selected track / flip the active cover
	if cf.playing {
		cf.np.OnCenter(ui)
	} else if cf.selected != nil {
		// on the back: play the track and turn to Now Playing
		if cf.back != nil && len(cf.back.Items) > 0 {
			it := cf.back.Items[cf.back.Index]
			ui.playSong(it.Tracks, it.TrackIdx, false)
			//
			cf.playing = true
			cf.backAnim.To(0, 300*time.Millisecond, easeInOut, 0)
			cf.np = newNowPlayingView()
			cf.np.hideArt = true
			cf.npFade.To(1, 300*time.Millisecond, easeFramer, 0)
			cf.infoFade.To(0, 300*time.Millisecond, easeInOut, 0)
			ui.headerTitle = "Now Playing"
		}
	} else if len(cf.albums) == 0 {
		return
	} else {
		cf.selected = cf.albums[cf.active]
		cf.back = ui.backsideList(cf.selected)
		cf.backAlbum = cf.selected
		cf.backAnim.Set(0)
		cf.backAnim.To(1, 500*time.Millisecond, easeInOut, 0)
	}
	ui.dirty = true
}

func (cf *CoverFlow) menu(ui *IpodUI) {
	// back out: Now Playing → track list → flow → menus
	if cf.selected != nil && cf.playing {
		cf.playing = false
		cf.npFade.To(0, 300*time.Millisecond, easeInOut, 0)
		cf.infoFade.To(1, 300*time.Millisecond, easeFramer, 0)
		ui.headerTitle = "Cover Flow"
		// flip back to the track list
		cf.back = ui.backsideList(cf.selected)
		cf.backAnim.Set(0)
		cf.backAnim.To(1, 500*time.Millisecond, easeInOut, 0)
	} else if cf.selected != nil {
		cf.selected = nil
		cf.backAnim.To(0, 300*time.Millisecond, easeInOut, 0)
		ui.headerTitle = "Cover Flow"
	} else {
		ui.closeCoverFlow()
	}
	ui.dirty = true
}

func (ui *IpodUI) backsideList(al *Album) *View {
	v := &View{ID: "coverFlowBack", Kind: KindFull}
	tracks := albumTracks(al)
	for i, t := range tracks {
		v.Items = append(v.Items, Item{Label: t.Title, Tracks: tracks, TrackIdx: i})
	}
	v.dirty = true
	return v
}

func (cf *CoverFlow) animating(now time.Time) bool {
	if cf.backAnim.Running() || cf.infoFade.Running() || cf.npFade.Running() || cf.fadeIn.Running() || cf.tilt.Running() {
		return true
	}
	for _, c := range cf.covers {
		if c.running(now) {
			return true
		}
	}
	if cf.backAlbum != nil && marqueeActive {
		return true
	}
	return cf.np != nil && cf.np.Animating(now)
}

// 4x4 matrices (row-major) for the cover transforms, like CSS 3D
// transforms with a 500 px perspective around the screen centre.
type mat4 [16]float64

func mIdent() mat4 { return mat4{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1} }
func mMul(a, b mat4) mat4 {
	var r mat4
	for i := 0; i < 4; i++ {
		for j := 0; j < 4; j++ {
			var s float64
			for k := 0; k < 4; k++ {
				s += a[i*4+k] * b[k*4+j]
			}
			r[i*4+j] = s
		}
	}
	return r
}
func mTrans(x, y, z float64) mat4 { return mat4{1, 0, 0, x, 0, 1, 0, y, 0, 0, 1, z, 0, 0, 0, 1} }
func mScale(s float64) mat4       { return mat4{s, 0, 0, 0, 0, s, 0, 0, 0, 0, s, 0, 0, 0, 0, 1} }
func mRotY(deg float64) mat4 {
	r := deg * math.Pi / 180
	c, s := math.Cos(r), math.Sin(r)
	//
	return mat4{c, 0, s, 0, 0, 1, 0, 0, -s, 0, c, 0, 0, 0, 0, 1}
}
func mPersp(d float64) mat4 { return mat4{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, -1 / d, 1} }

// coverMatrix maps a 128x128 cover (origin at its top-left corner) to
// the screen for pose x.
func coverMatrix(x cfXform) mat4 {
	m := mTrans(x.X, x.Y, x.Z)
	m = mMul(m, mScale(x.S1))
	m = mMul(m, mTrans(0, 0, x.Z2))
	m = mMul(m, mRotY(x.A))
	m = mMul(m, mTrans(0, x.TY, 0))
	m = mMul(m, mScale(x.S2))
	// around the cover's centre, then the perspective about the screen's
	o := 0.0
	full := mMul(mTrans(64, 84, o), mMul(m, mTrans(-64, -64, o)))
	proj := mMul(mTrans(160, 110, o), mMul(mPersp(500), mTrans(-160, -110, o)))
	return mMul(proj, full)
}

// planeH: the homography of the z = 0 plane under m (x, y, w rows).
func planeH(m mat4) [9]float64 {
	return [9]float64{m[0], m[1], m[3], m[4], m[5], m[7], m[12], m[13], m[15]}
}

func inv3(a [9]float64) ([9]float64, bool) {
	det := a[0]*(a[4]*a[8]-a[5]*a[7]) - a[1]*(a[3]*a[8]-a[5]*a[6]) + a[2]*(a[3]*a[7]-a[4]*a[6])
	if math.Abs(det) < 1e-12 {
		return a, false
	}
	id := 1 / det
	return [9]float64{
		(a[4]*a[8] - a[5]*a[7]) * id, (a[2]*a[7] - a[1]*a[8]) * id, (a[1]*a[5] - a[2]*a[4]) * id,
		(a[5]*a[6] - a[3]*a[8]) * id, (a[0]*a[8] - a[2]*a[6]) * id, (a[2]*a[3] - a[0]*a[5]) * id,
		(a[3]*a[7] - a[4]*a[6]) * id, (a[1]*a[6] - a[0]*a[7]) * id, (a[0]*a[4] - a[1]*a[3]) * id,
	}, true
}
