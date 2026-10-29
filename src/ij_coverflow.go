package main

// ij_coverflow.go — Cover Flow: the album covers in 3D (a small software
// rasterizer), the flip to the track list on the back, and Now Playing.

import (
	"fmt"
	"image"
	"math"
	"time"

	"golang.org/x/image/font"
)

type cfXform struct {
	X, Y, Z, S1, Z2, A, TY, S2 float64
	W                          float64 // wider by this fraction, to the right (Now Playing's tilted cover)
}

func lerpX(a, b cfXform, t float64) cfXform {
	l := func(x, y float64) float64 { return x + (y-x)*t }
	return cfXform{l(a.X, b.X), l(a.Y, b.Y), l(a.Z, b.Z), l(a.S1, b.S1), l(a.Z2, b.Z2), l(a.A, b.A), l(a.TY, b.TY), l(a.S2, b.S2), l(a.W, b.W)}
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
	fullBase   int // full-screen views when it opened: more are on top of it (Up Next)
	albums     []*Album
	active     int
	selected   *Album // flipped to its back
	playing    bool   // Now Playing shown over the flow
	covers     map[int]*cfCover
	back       *View // track list on the back of the selected cover
	backAnim   *Anim
	backAlbum  *Album
	infoFade   *Anim
	npFade     *Anim
	paused     bool
	tilt       *Anim
	backShown  *Album
	backSince  time.Time
	np         *NowPlayingView
	backFromNP bool // the back was reached from Now Playing (START goes back to it)
	fadeIn     *Anim
	exiting    bool
	layer      *image.RGBA
	npBuf      *image.RGBA
}

// The flow: the active cover faces the viewer in the middle; the others
// stand at ±70° in two rows of overlapping covers, 48 px apart. Moving
// animates every cover to its new pose (CSS-like transitions); selecting
// flips the active cover (-180°) to show its track list, and playing a
// track turns it to the Now Playing pose next to the track info.
//
// Covers are textured quads drawn with a perspective projection (mat4);
// the reflection is the same quad mirrored below, fading out. Art comes
// from the shared art cache at 128 px, the selected cover's back is a
// texture rebuilt only when its list scrolls (backsideTexture).
//
// easeFramer is the flip curve (a slower ease-out than CSS "ease").
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
	cf.fullBase = len(ui.full)
	ui.cover = cf
	ui.headerTitle = "Cover Flow"
	// the menus slide away and the preview hides
	ui.leftX.To(-1, durPanel, easeCSS, delayMenuHide)
	ui.previewHide.To(1, durPanel, easeCSS, 0)
	ui.dirty = true
}

// coverUp: Cover Flow, when it is open and on top (not under Up Next,
// opened from its Now Playing).
func (ui *IpodUI) coverUp() *CoverFlow {
	if cf := ui.cover; cf != nil && !cf.exiting && len(ui.full) <= cf.fullBase {
		return cf
	}
	return nil
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
				// next to the track info, tilted like Now Playing; the
				// angle is taken a full turn back so the cover keeps
				// turning the way it flipped (-180° → -325°) instead of
				// spinning back through the front
				p := npCoverPose(cf.tilt.Target())
				p.A -= 360
				return p, 500 * time.Millisecond
			}
			return backPose(), 500 * time.Millisecond
		}
		return x, 300 * time.Millisecond
	}
	a := -70.0
	if i < cf.active {
		a = 70
	}
	return cfXform{X: getOffsetPx(i-cf.active, 160), S1: 1.1, Z2: -65, A: a, S2: 1}, 250 * time.Millisecond
}

// backPose: the active cover flipped to show its track list.
func backPose() cfXform {
	return cfXform{X: 100, Y: 4, Z: 20, S1: 1, A: -180, TY: 32, S2: 0.96}
}

func (cf *CoverFlow) sync(now time.Time) {
	// the album title under the flow shows only when no cover is flipped
	want := 1.0
	if cf.selected != nil {
		want = 0
	}
	if cf.infoFade.Target() != want {
		cf.infoFade.To(want, 300*time.Millisecond, easeInOut, 0)
	}
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
		hidden := i != cf.active && (cf.playing || cf.backFromNP) // (turned over from Now Playing: still on its own)
		if hidden {
			c.opacity.To(0, 350*time.Millisecond, easeCSS, 0)
		} else {
			c.opacity.To(1, 350*time.Millisecond, easeCSS, 0)
		}
		flipped := cf.selected != nil && cf.albums[i] == cf.selected && !cf.playing
		if flipped {
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
			if ni != cf.back.Index {
				cf.back.scrolledAt = animNow()
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
		cf.backFromNP = false
		// on the back: play the track and turn to Now Playing
		if cf.back != nil && len(cf.back.Items) > 0 {
			it := cf.back.Items[cf.back.Index]
			ui.playSong(it.Tracks, it.TrackIdx, false)
		}
		cf.playing = true
		cf.backAnim.To(0, 250*time.Millisecond, easeOutCubic, 0)
		cf.np = newNowPlayingView()
		cf.np.hideArt = true
		cf.npFade.To(1, 250*time.Millisecond, easeInOut, 250*time.Millisecond) // (once the card is mostly gone)
		ui.headerTitle = "Now Playing"
		// (the cover art stays in the flow: Now Playing shows only the info)
	} else if len(cf.albums) == 0 {
		return
	} else {
		cf.backFromNP = false
		cf.selected = cf.albums[cf.active]
		cf.back = ui.backsideList(cf.selected)
		cf.backAlbum = cf.selected
		cf.backAnim.Set(0)
		cf.backAnim.To(1, 380*time.Millisecond, easeOutCubic, 170*time.Millisecond) // (once the cover has turned away)
	}
	ui.dirty = true
}

// startKey: START on Cover Flow's Now Playing turns the cover over to its
// album's songs, still in Now Playing (not back to the flow), and START or
// B there turn it back to Now Playing (the song goes on), as on the other
// Now Playing; A plays the song picked.
func (cf *CoverFlow) startKey(ui *IpodUI) bool {
	switch {
	case cf.playing && cf.selected != nil:
		if np := cf.np; np != nil && np.opts { // (the options row goes with it)
			np.opts, np.volEnabled = false, true
			np.updateControls()
		}
		cf.backFromNP = true
		cf.menu(ui)
		ui.headerTitle = "Now Playing" // (still Now Playing: only the cover turns over)
		if cf.back != nil {
			if p := ui.app.Playing; p != nil {
				for i, it := range cf.back.Items {
					if it.Tracks[it.TrackIdx] == p.Track {
						cf.back.Index = i
					}
				}
			}
		}
		return true
	case !cf.playing && cf.selected != nil && cf.backFromNP:
		cf.backFromNP = false
		cf.playing = true
		cf.backAnim.To(0, 250*time.Millisecond, easeOutCubic, 0)
		cf.np = newNowPlayingView()
		cf.np.hideArt = true
		cf.npFade.To(1, 250*time.Millisecond, easeInOut, 250*time.Millisecond) // (once the card is mostly gone)
		ui.headerTitle = "Now Playing"
		ui.dirty = true
		return true
	}
	return false
}

func (cf *CoverFlow) menu(ui *IpodUI) {
	// the options row on Now Playing goes first
	if cf.playing && cf.np != nil && cf.np.opts {
		cf.np.OnMenu(ui)
		return
	}
	// back out: Now Playing → track list → flow → menus
	if cf.selected != nil && cf.playing {
		// start the turn back from where the cover is now (Now Playing
		// may have tilted it), a full turn back like targetFor
		if c := cf.covers[cf.active]; c != nil {
			p := npCoverPose(cf.tilt.Get(animNow()))
			p.A -= 360
			c.from, c.to, c.dur = p, p, 0
		}
		cf.playing = false
		cf.npFade.To(0, 160*time.Millisecond, easeOutCubic, 0) // (gone before the card shows)
		ui.headerTitle = "Cover Flow"
		// flip back to the track list
		cf.back = ui.backsideList(cf.selected)
		cf.backAnim.Set(0)
		cf.backAnim.To(1, 380*time.Millisecond, easeOutCubic, 170*time.Millisecond) // (once the cover has turned away)
	} else if cf.selected != nil && cf.backFromNP {
		// turned over from Now Playing: B turns it back there, as START does
		cf.startKey(ui)
		return
	} else if cf.selected != nil {
		cf.selected = nil
		cf.backFromNP = false
		cf.backAnim.To(0, 250*time.Millisecond, easeOutCubic, 0)
		ui.headerTitle = "Cover Flow"
	} else {
		ui.closeCoverFlow()
	}
	ui.dirty = true
}

func (ui *IpodUI) backsideList(al *Album) *View {
	v := &View{ID: "coverFlowBack", Kind: KindFull, TrackList: true}
	tracks := albumTracks(al)
	for i, t := range tracks {
		d := ""
		if t.Duration > 0 {
			d = formatTrackLength(t.Duration)
		}
		v.Items = append(v.Items, Item{Label: t.Title, Tracks: tracks, TrackIdx: i, Duration: d, NoArrow: true})
	}
	// libraries scanned before durations were read: read the missing ones
	// in the background and update the list
	var missing []*Track
	for _, t := range tracks {
		if t.Duration <= 0 {
			missing = append(missing, t)
		}
	}
	if len(missing) > 0 {
		go func() {
			durs := make([]float64, len(missing))
			for i, t := range missing {
				durs[i] = probeDuration(t.Path)
			}
			ui.post(func() {
				for i, t := range missing {
					if t.Duration <= 0 && durs[i] > 0 {
						t.Duration = durs[i]
					}
				}
				for i := range v.Items {
					if t := v.Items[i].Tracks[v.Items[i].TrackIdx]; t.Duration > 0 {
						v.Items[i].Duration = formatTrackLength(t.Duration)
					}
				}
				v.dirty = true
				ui.dirty = true
			})
		}()
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
	if cf.back != nil && cf.back.scrollbarAnimating(now) {
		return true
	}
	return cf.np != nil && cf.np.Animating(now)
}

// 4x4 matrices (row-major) for the cover transforms, like CSS 3D
// transforms with a 500 px perspective around the screen centre.
type mat4 [16]float64

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
	if x.W != 0 {
		// wider from the left edge (which stays where it is)
		full = mMul(full, mat4{1 + x.W, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1})
	}
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

type texMap struct {
	img       *image.RGBA
	su, ou    float64 // image x = u*su + ou
	sv, ov    float64
	u0, v0    float64 // visible part of the plane
	u1, v1    float64
	reflFrom  float64 // v where the reflection starts (0: none)
	reflMax   float64 // reflection opacity at the edge
	reflStart float64 // fraction where it fades out
}

func drawTex(dst *image.RGBA, m mat4, t texMap, opacity float64) {
	if opacity <= 0.002 {
		return
	}
	H := planeH(m)
	Hi, ok := inv3(H)
	if !ok {
		return
	}
	minX, minY, maxX, maxY := math.Inf(1), math.Inf(1), math.Inf(-1), math.Inf(-1)
	for _, c := range [4][2]float64{{t.u0, t.v0}, {t.u1, t.v0}, {t.u0, t.v1}, {t.u1, t.v1}} {
		w := H[6]*c[0] + H[7]*c[1] + H[8]
		if w <= 0 {
			return
		}
		x := (H[0]*c[0] + H[1]*c[1] + H[2]) / w
		y := (H[3]*c[0] + H[4]*c[1] + H[5]) / w
		minX, maxX = math.Min(minX, x), math.Max(maxX, x)
		minY, maxY = math.Min(minY, y), math.Max(maxY, y)
	}
	b := dst.Bounds()
	x0 := int(math.Max(float64(b.Min.X), math.Floor(minX*DPR)))
	x1 := int(math.Min(float64(b.Max.X), math.Ceil(maxX*DPR)))
	y0 := int(math.Max(float64(b.Min.Y), math.Floor(minY*DPR)))
	y1 := int(math.Min(float64(b.Max.Y), math.Ceil(maxY*DPR)))
	if x1 <= x0 || y1 <= y0 {
		return
	}
	img := t.img
	iw, ih := img.Bounds().Dx(), img.Bounds().Dy()
	sp, sst := pix32(img)
	dp, dst32 := pix32(dst)
	op := uint32(opacity*256 + 0.5)
	if op > 256 {
		op = 256
	}
	reflH := t.reflFrom
	// rows in parallel; along a row the mapping is linearised every 8 px
	parallelRows(y0, y1, func(ya, yb int) {
		if cDraw {
			cDrawTex(dp, dst32, x0, x1, ya, yb, sp, sst, iw, ih, Hi, t, op)
			return
		}
		for py := ya; py < yb; py++ {
			sy := (float64(py) + 0.5) / DPR
			sx0 := (float64(x0) + 0.5) / DPR
			du := Hi[0] / DPR
			dv := Hi[3] / DPR
			dw := Hi[6] / DPR
			U := Hi[0]*sx0 + Hi[1]*sy + Hi[2]
			V := Hi[3]*sx0 + Hi[4]*sy + Hi[5]
			W := Hi[6]*sx0 + Hi[7]*sy + Hi[8]
			row := dp[py*dst32:]
			u, v := U/W, V/W
			for px0 := x0; px0 < x1; px0 += 8 {
				n := 8
				if px0+8 > x1 {
					n = x1 - px0
				}
				U2, V2, W2 := U+du*float64(n), V+dv*float64(n), W+dw*float64(n)
				u2, v2 := U2/W2, V2/W2
				su, sv := (u2-u)/float64(n), (v2-v)/float64(n)
				uu, vv := u, v
				for k := 0; k < n; k, uu, vv = k+1, uu+su, vv+sv {
					if uu < t.u0 || uu >= t.u1 || vv < t.v0 || vv >= t.v1 {
						continue
					}
					alpha := op
					vs := vv
					if t.reflFrom > 0 && vv >= t.reflFrom {
						f := (2*t.reflFrom - vv) / reflH
						if f < t.reflStart {
							continue
						}
						alpha = uint32(float64(op) * t.reflMax * (f - t.reflStart) / (1 - t.reflStart))
						if alpha == 0 {
							continue
						}
						vs = 2*t.reflFrom - vv
					}
					tx := uu*t.su + t.ou - 0.5
					ty := vs*t.sv + t.ov - 0.5
					if tx < 0 {
						tx = 0
					}
					if ty < 0 {
						ty = 0
					}
					ix, iy := int(tx), int(ty)
					fx := uint32((tx - float64(ix)) * 256)
					fy := uint32((ty - float64(iy)) * 256)
					ix1, iy1 := ix+1, iy+1
					if ix >= iw-1 {
						ix, ix1, fx = iw-1, iw-1, 0
					}
					if iy >= ih-1 {
						iy, iy1, fy = ih-1, ih-1, 0
					}
					r0, r1 := iy*sst, iy1*sst
					c := lerp32(lerp32(sp[r0+ix], sp[r0+ix1], fx), lerp32(sp[r1+ix], sp[r1+ix1], fx), fy)
					if alpha < 256 {
						c = scale32(c, alpha)
					}
					if c>>24 == 0 {
						continue
					}
					o := px0 + k
					row[o] = over32(c, row[o])
				}
				U, V, W, u, v = U2, V2, W2, u2, v2
			}
		}
	})
}

func (ui *IpodUI) renderCoverFlow(dst *image.RGBA, now time.Time) {
	// covers lean back (tilt 0) while paused
	cf := ui.cover
	if p := ui.app.Playing; p != nil {
		cf.paused = p.State != StatePlaying
	}
	tiltT := 1.0
	if cf.paused {
		tiltT = 0
	}
	if cf.tilt.Target() != tiltT {
		if cf.playing {
			cf.tilt.To(tiltT, 700*time.Millisecond, easeInOut, 0)
		} else {
			cf.tilt.Set(tiltT)
		}
	}
	cf.sync(now)
	W, H := px(320), px(220)
	layer := newCanvasCached(&cf.layer, W, H)
	fillRect(layer, 0, 0, W, H, th.bg)
	// album title and artist under the flow, fading out while the cover
	// is flipped
	if fo := cf.infoFade.Get(now); fo > 0.002 && len(cf.albums) > 0 && cf.active < len(cf.albums) {
		al := cf.albums[cf.active]
		// album in bold, the artist under it a little smaller and grey
		lh, asc := lineBox(16)
		alh, aasc := lineBox(13)
		y := 204 - lh - alh
		lines := []struct {
			s       string
			f       font.Face
			lh, asc float64
			c       Col
		}{
			{al.Name, face(W700, 16), lh, asc, th.text},
			{al.Artist, face(W600, 13), alh, aasc, th.sub},
		}
		for _, l := range lines {
			s := ellipsize(l.f, l.s, 576)
			tw := textWidth(l.f, s)
			drawText(layer, l.f, s, (float64(W)-tw)/2, (y+l.asc)*DPR, Col{l.c.R, l.c.G, l.c.B, fo})
			y += l.lh
		}
	}
	// back to front: covers further from the active one first, then by index
	type item struct {
		i, z int
	}
	//
	var order []item
	for i := range cf.covers {
		z := 1 - absInt(i-cf.active)
		order = append(order, item{i, z})
	}
	for a := 1; a < len(order); a++ {
		for b := a; b > 0 && (order[b].z < order[b-1].z || order[b].z == order[b-1].z && order[b].i < order[b-1].i); b-- {
			order[b], order[b-1] = order[b-1], order[b]
		}
	}
	size := px(128)
	// at most 3 missing covers are asked for per frame, in the shelf's
	// order (left to right: numbers first, then A to Z), before anything
	// is drawn, so they come in that order too
	asked := 0
	for i := max(cf.active-15, 0); i <= cf.active+15 && i < len(cf.albums) && asked < 3; i++ {
		c, ok := cf.covers[i]
		if !ok || c.opacity.Get(now) <= 0.001 {
			continue
		}
		if ui.artCached(cf.albums[i], size) == nil {
			ui.art(cf.albums[i], size)
			asked++
		}
	}
	for _, it := range order {
		c := cf.covers[it.i]
		op := c.opacity.Get(now)
		if op <= 0.001 {
			continue
		}
		x := c.value(now)
		if cf.playing && it.i == cf.active && cf.selected != nil && cf.albums[it.i] == cf.selected &&
			cf.npFade.Target() == 1 && !cf.npFade.Running() &&
			(cf.tilt.Running() || !(c.dur > 0 && now.Sub(c.start) < c.dur)) {
			x = npCoverPose(cf.tilt.Get(now))
		}
		m := coverMatrix(x)
		art := ui.artCached(cf.albums[it.i], size)
		if art == nil {
			art = ui.defaultArtSized(size)
		}
		aa := 1.0
		selected := cf.selected != nil && cf.albums[it.i] == cf.selected && it.i == cf.active
		if !facesViewer(m) {
			// turned away: the plain back of the cover, never its art
			// seen mirrored
			art = coverBackFace()
		}
		sc := float64(art.Bounds().Dx()) / 128
		tm := texMap{img: art, su: sc, sv: sc, u1: 128, v1: 128}
		onBack := it.i == cf.active && cf.back != nil && cf.albums[it.i] == cf.backAlbum && !facesViewer(m)
		if onBack {
			// turned away: its back is the album's card
			ui.drawBackside(layer, m, coverMatrix(backPose()), cardGrow(x.A), aa*op, now)
		} else if selected {
			// the reflection of the selected cover fades with Now
			// Playing
			//
			if np := cf.npFade.Get(now); np > 0.001 {
				tm.v1 = 166.4
				tm.reflFrom, tm.reflMax, tm.reflStart = 128, 0.2*np, 0.7
			}
			drawTex(layer, m, tm, aa*op)
		} else {
			// the other covers always have their reflection
			tm.v1 = 166.4
			tm.reflFrom, tm.reflMax, tm.reflStart = 128, 0.2, 0.7
			drawTex(layer, m, tm, aa*op)
		}
	}
	// Now Playing (track info and controls) over the flow
	//
	//
	if fo := cf.npFade.Get(now); fo > 0 && cf.np != nil {
		nb := newCanvasCached(&cf.npBuf, W, H)
		clearRGBA(nb)
		cf.np.Render(ui, nil, nb, now)
		blit(layer, nb, 0, 0, fo)
	}
	blitOpaque(dst, layer, 0, px(20))
}

// formatTrackLength: m:ss (h:mm:ss from an hour), like the iPod lists.
func formatTrackLength(sec float64) string {
	s := int(sec + 0.5)
	if s >= 3600 {
		return fmt.Sprintf("%d:%02d:%02d", s/3600, s/60%60, s%60)
	}
	return fmt.Sprintf("%d:%02d", s/60, s%60)
}

func absInt(x int) int {
	if x < 0 {
		return -x
	}
	return x
}

// drawBackside draws the back of the flipped cover (album info, tracks).
func (ui *IpodUI) drawBackside(layer *image.RGBA, m, final mat4, g, alpha float64, now time.Time) {
	cf := ui.cover
	if cf.back == nil {
		return
	}
	if cf.backShown != cf.backAlbum {
		cf.backShown, cf.backSince = cf.backAlbum, now
	}
	drawBacksideCard(layer, m, final, alpha, g, func(w, h float64) *image.RGBA {
		return ui.backsideTex(&backsideBuf, &backsideList, cf.backAlbum, cf.back, w, h, cf.backSince, now)
	})
}

// drawBacksideCard draws an album's card on the back of a cover (Cover
// Flow's, and Now Playing's turned over), s: 0 → 1 as it comes (bigger
// and stronger). tex makes the card's picture at w×h (CSS px).
func drawBacksideCard(layer *image.RGBA, m, final mat4, alpha, g float64, texFn func(w, h float64) *image.RGBA) {
	if alpha <= 0.001 {
		return
	}
	// the card, in the cover's plane coordinates (wider than the cover and
	// centred on it); its height comes from where its bottom edge lands on
	// the screen once the flip has finished
	bu0, bv0 := -64.0, -35.84
	bw := 256.0
	bh := backsideHeight(final, bu0, bv0, bw)
	tex := texFn(bw, bh)
	// it is the back of the cover: just turned (g = 0) it is the cover's
	// own square, growing to the whole card as it turns to face us
	l := func(a, b float64) float64 { return a + (b-a)*g }
	u0, u1 := l(0, bu0), l(128, bu0+bw)
	v0, v1 := l(0, bv0), l(128, bv0+bh)
	tw, th := float64(tex.Bounds().Dx()), float64(tex.Bounds().Dy())
	// mirrored horizontally (we see it from behind)
	tm := texMap{
		img: tex,
		su:  -tw / (u1 - u0), ou: u1 * tw / (u1 - u0),
		sv: th / (v1 - v0), ov: -v0 * th / (v1 - v0)}
	tm.u0, tm.u1 = u0, u1
	tm.v0, tm.v1 = v0, v1
	drawTex(layer, m, tm, alpha)
}

// cardGrow: how far the back of a cover at angle a (degrees) has turned
// to face us: 0 edge-on, 1 facing us.
func cardGrow(a float64) float64 {
	return math.Max(0, -math.Cos(a*math.Pi/180))
}

// backsideInfoH: height of the album / artist block on the back.
// Title and artist lines (16 and 14 px bold) with 5 px padding above,
// 4 below; the track rows start right after it and a 1 px separator.
// Shared by the card's height search and its texture.
func backsideInfoH() float64 {
	tlh, _ := lineBox(16)
	slh, _ := lineBox(14)
	return tlh + 4 + slh + 4 + 1
}

// backsideHeight finds by bisection the card height whose bottom edge
// lands just past the bottom of the screen (y = 221 in the 220 px layer,
// so the card touches the edge and its bottom border is off screen), at least the info block plus a row.
// The bisection runs on the final flip pose, so the card does not change
// size while it turns.
func backsideHeight(final mat4, bu0, bv0, bw float64) float64 {
	H := planeH(final)
	bottom := func(v float64) float64 {
		m := -1e9
		for _, u := range [2]float64{bu0, bu0 + bw} {
			w := H[6]*u + H[7]*v + H[8]
			if w <= 0 {
				return 1e9
			}
			m = math.Max(m, (H[3]*u+H[4]*v+H[5])/w)
		}
		return m
	}
	target := 221.0
	lo, hi := bv0+60, bv0+600
	for i := 0; i < 40; i++ {
		mid := (lo + hi) / 2
		if bottom(mid) > target {
			hi = mid
		} else {
			lo = mid
		}
	}
	h := lo - bv0
	// never smaller than the album info, the separator and one row
	//
	if min := backsideInfoH() + 1 + 25 + 1; h < min {
		h = min
	}
	return h
}

// drawMarquee draws s clipped to maxW, scrolling it (pause 1.5 s, then
// 32 px/s) when it does not fit.
// The text scrolls left until its start comes round again (a second copy
// follows 40 px behind), waits, and starts over; since is when the text
// appeared (the cycle restarts from there).
// The clip is done per glyph: nothing is drawn outside x..x+maxW.
func drawMarquee(dst *image.RGBA, f font.Face, s string, x, baseline, maxW float64, c Col, since, now time.Time) {
	drawMarqueeSync(dst, f, s, x, baseline, maxW, c, since, now, 0)
}

const (
	marqueePause = 1.5  // s at the start of each cycle
	marqueeSpeed = 32.0 // CSS px/s
	marqueeGap   = 40.0 // CSS px between the two copies
)

// marqueeCycle is the length of one scroll of s in maxW (pause + travel),
// 0 when it fits.
func marqueeCycle(f font.Face, s string, maxW float64) float64 {
	tw := textWidth(f, s) / DPR
	if tw <= maxW {
		return 0
	}
	return (tw+marqueeGap)/marqueeSpeed + marqueePause
}

// drawMarqueeSync is drawMarquee on a shared cycle: lines that scroll
// together (title, artist, album) all start at once and, once a shorter one
// has come round, it waits at its start until the longest has finished, so
// they never drift apart. cycle 0 uses the line's own.
// marqSyncMoving: a synced marquee moved in the frame being drawn;
// marqSyncWake: otherwise, the earliest time one starts moving again (reset
// each frame), so the screens can idle through the pauses.
var (
	marqSyncMoving bool
	marqSyncWake   time.Time
)

func marqSyncWakeAt(t time.Time) {
	if marqSyncWake.IsZero() || t.Before(marqSyncWake) {
		marqSyncWake = t
	}
}

// marqSyncNeedsFrame: a synced marquee is moving or due to move.
func marqSyncNeedsFrame(now time.Time) bool {
	return marqSyncMoving || (!marqSyncWake.IsZero() && !now.Before(marqSyncWake))
}

func drawMarqueeSync(dst *image.RGBA, f font.Face, s string, x, baseline, maxW float64, c Col, since, now time.Time, cycle float64) {
	tw := textWidth(f, s) / DPR
	b := dst.Bounds()
	r := image.Rect(px(x), b.Min.Y, px(x+maxW), b.Max.Y).Intersect(b)
	clip, ok := dst.SubImage(r).(*image.RGBA)
	if !ok {
		return
	}
	if tw <= maxW {
		drawText(clip, f, s, x*DPR, baseline*DPR, c)
		return
	}
	marqueeActive = true
	// two copies 40 px apart scroll left together
	loop := tw + marqueeGap
	own := loop/marqueeSpeed + marqueePause
	if cycle < own {
		cycle = own
	}
	t := math.Mod(now.Sub(since).Seconds(), cycle)
	off := 0.0
	if t > marqueePause {
		// done early: wait with the start showing (the second copy has
		// taken the first one's place)
		off = math.Min((t-marqueePause)*marqueeSpeed, loop)
	}
	// moving now, or when it will move again (no frames needed meanwhile)
	switch {
	case t > marqueePause && off < loop:
		marqSyncMoving = true
	case t <= marqueePause:
		marqSyncWakeAt(now.Add(time.Duration((marqueePause - t) * float64(time.Second))))
	default:
		marqSyncWakeAt(now.Add(time.Duration((cycle - t) * float64(time.Second))))
	}
	drawText(clip, f, s, (x-off)*DPR, baseline*DPR, c)
	drawText(clip, f, s, (x-off+loop)*DPR, baseline*DPR, c)
}

// backsideTex: an album's card: its name and artist on blue (scrolling
// together when long, from since), then its songs (back).
func (ui *IpodUI) backsideTex(buf, listBuf **image.RGBA, al *Album, back *View, w, h float64, since, now time.Time) *image.RGBA {
	img := newCanvasCached(buf, px(w), px(h))
	fillRect(img, 0, 0, px(w), px(h), th.bg)
	// album info on a blue gradient, then the track list
	tf := face(W700, 16)
	sf := face(W700, 14)
	tlh, tasc := lineBox(16)
	_, sasc := lineBox(14)
	inner := w - 2
	infoH := backsideInfoH()
	fillVGradient(img, px(1), px(1), px(inner), px(infoH-1), []GradStop{{0, hex("#6585ad")}, {1, hex("#789ab3")}})
	fillRect(img, px(1), px(infoH), px(inner), px(1), hex("#6d87a3"))
	y := 5.0
	// album and artist scroll on one cycle
	cyc := math.Max(marqueeCycle(tf, al.Name, inner-16), marqueeCycle(sf, al.Artist, inner-16))
	drawMarqueeSync(img, tf, al.Name, 9, y+tasc, inner-16, colWhite, since, now, cyc)
	y += tlh
	drawMarqueeSync(img, sf, al.Artist, 9, y+sasc, inner-16, colWhite, since, now, cyc)
	//
	lt := infoH + 1
	listH := h - 1 - lt
	if listH > 0 {
		lst := newCanvasCached(listBuf, px(inner), px(listH))
		ui.renderList(back, lst)
		blitOpaque(img, lst, px(1), px(lt))
	}
	// 1px border
	fillRect(img, 0, 0, px(w), px(1), th.cardBorder)
	fillRect(img, 0, px(h)-px(1), px(w), px(1), th.cardBorder)
	fillRect(img, 0, 0, px(1), px(h), th.cardBorder)
	fillRect(img, px(w)-px(1), 0, px(1), px(h), th.cardBorder)
	return img
}

// npCoverPose: the active cover in Cover Flow's Now Playing, at the left
// next to the track info, turned by 35°×t like the Now Playing screen.
// t goes 0 → 1 while Now Playing fades in.
func npCoverPose(t float64) cfXform {
	th0 := 35 * math.Pi / 180
	th := th0 * t
	p := 160.0 / 9
	// keep the cover's left edge where it is at the resting angle
	//
	return cfXform{
		X: p + 64*math.Cos(th0) - 64*math.Cos(th),
		Y: 8,
		Z: -64*math.Sin(th0) + 64*math.Sin(th),
		// paused (t = 0) the cover also shrinks to 85%
		S1: 1, A: 35 * t, S2: 0.85 + 0.15*t,
		// tilted, wider so it looks square: 9 px
		// here, where the perspective is milder (measured: 247×247 on
		// screen, as Now Playing's)
		W: cfNPWiden / 128 * t}
}

// backsideBuf is the reused canvas of the flipped cover's track list.
var backsideBuf, backsideList *image.RGBA

// facesViewer reports whether the cover's front faces the screen under m
// (the projected quad keeps its winding).
func facesViewer(m mat4) bool {
	H := planeH(m)
	pr := func(u, v float64) (float64, float64) {
		w := H[6]*u + H[7]*v + H[8]
		return (H[0]*u + H[1]*v + H[2]) / w, (H[3]*u + H[4]*v + H[5]) / w
	}
	x0, y0 := pr(0, 0)
	x1, y1 := pr(128, 0)
	x2, y2 := pr(0, 128)
	return (x1-x0)*(y2-y0)-(y1-y0)*(x2-x0) > 0
}

var coverBack *image.RGBA
var coverBackDark bool

// coverBackFace: the back of a cover, a plain card in the theme's colours.
func coverBackFace() *image.RGBA {
	if coverBack == nil || coverBackDark != th.dark {
		coverBack, coverBackDark = newCanvas(16, 16), th.dark
		c := hex("#e9eaec")
		if th.dark {
			c = rgb(46, 48, 52)
		}
		fillRect(coverBack, 0, 0, 16, 16, c)
	}
	return coverBack
}

// cfNPWiden: how much wider Cover Flow's Now Playing cover is when tilted
// (CSS px).
const cfNPWiden = 9.0
