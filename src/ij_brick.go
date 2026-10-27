package main

// ij_brick.go — Brick: the iPod breakout game (wheel: paddle, centre: go).

import (
	"fmt"
	"image"
	"math"
	"time"

	"golang.org/x/image/font"
)

// brk is a brick: its rectangle (CSS px) and its row.
type brk struct {
	x, y, w, h float64
	c          int
}

// brickColors are the rows, top to bottom: the gradient of the bricks
// and the points a brick of the row is worth.
var brickColors = []brickColor{
	{Col{255, 196, 196, 1}, Col{239, 26, 26, 1}, 7},
	{Col{255, 202, 161, 1}, Col{255, 120, 30, 1}, 5},
	{Col{254, 242, 189, 1}, Col{240, 195, 31, 1}, 3},
	{Col{167, 255, 209, 1}, Col{23, 200, 112, 1}, 1},
	{Col{140, 200, 255, 1}, Col{0, 100, 222, 1}, 1},
}

type brickColor struct {
	top, bottom Col
	points      int
}

const brickStep = time.Second / 60

// BrickGame is the state of a game (CSS px, s scales the 340 px original).
type BrickGame struct {
	w, h, s          float64
	phase            string // waiting, playing, gameOver
	level, menuIndex int
	px, py, pw, ph   float64
	pvel, pspeed     float64
	score, lives     int
	bx, by, br       float64
	bspeed           float64
	dx, dy           float64
	bricks           []brk
	lastStep         time.Time
	acc              time.Duration
	onQuit           func()
	bg               *image.RGBA
}

// openBrick starts a new game full screen; Quit in the Game Over
// menu goes back to the Games menu.
func (ui *IpodUI) openBrick() {
	g := newBrickGame(320, 220)
	v := &View{ID: "brickGame", Title: "Brick", Kind: KindFull, Custom: g}
	g.onQuit = func() { ui.hideView() }
	ui.showView(v, "")
}

func newBrickGame(w, h float64) *BrickGame {
	g := &BrickGame{w: w, h: h, s: w / 340, phase: "waiting", level: 1}
	g.pw, g.ph = 70*g.s, 8*g.s
	g.pspeed = 3 * g.s
	g.lives = 3
	g.resetPlayerPos()
	g.br = 6 * g.s
	g.bspeed = 2 * g.s
	g.resetBall()
	g.setupBricks()
	return g
}

func (g *BrickGame) resetPlayerPos() {
	g.px = (g.w - g.pw) / 2
	g.py = g.h - g.ph - 3*g.s
}

func (g *BrickGame) resetBall() {
	g.bx = g.px + g.pw/2
	g.by = g.py - g.br - 1
	g.dx = math.Cos(math.Pi / 4)
	g.dy = -math.Abs(math.Sin(math.Pi / 4))
}

func (g *BrickGame) setupBricks() {
	g.bricks = nil
	left := 60 * g.s
	avail := g.w - 4*g.s - left
	sp := 3 * g.s
	bw := (avail - 7*sp) / 8
	bh := 12 * g.s
	for row := 0; row < 5; row++ {
		for i := 0; i < 8; i++ {
			g.bricks = append(g.bricks, brk{x: left + float64(i)*(bw+sp), y: 6*g.s + float64(row)*(bh+sp), w: bw, h: bh, c: row})
		}
	}
}

func aabb(ax, ay, aw, ah, bx, by, bw, bh float64) bool {
	return !(ay+ah < by || by+bh < ay || bx+bw < ax || ax+aw < bx)
}

// step advances the game by one 60 Hz tick.
func (g *BrickGame) step() {
	if g.phase == "gameOver" {
		return
	}
	// paddle
	g.px += g.pvel
	g.pvel *= 0.85
	if math.Abs(g.pvel) < 0.1 {
		g.pvel = 0
	}
	g.px = math.Max(0, math.Min(g.w-g.pw, g.px))
	// walls
	if g.bx < 0 || g.bx > g.w {
		g.dx = -g.dx
	}
	if g.by < 0 {
		g.dy = -g.dy
	}
	if g.by > g.h {
		g.die()
	}
	if g.phase == "playing" {
		// the paddle sends the ball off between 30° and 150°
		if aabb(g.bx, g.by, g.br*2, g.br*2, g.px, g.py, g.pw, g.ph) {
			hit := g.bx - g.px
			ang := (hit-0)*(5*math.Pi/6-math.Pi/6)/(g.pw-0) + math.Pi/6
			g.dx = -math.Cos(ang)
			g.dy = -math.Sin(ang)
		}
		for i := len(g.bricks) - 1; i >= 0; i-- {
			b := g.bricks[i]
			if !aabb(g.bx, g.by, g.br*2, g.br*2, b.x, b.y, b.w, b.h) {
				continue
			}
			g.score += brickColors[b.c].points
			g.bricks = append(g.bricks[:i], g.bricks[i+1:]...)
			if len(g.bricks) == 0 {
				g.advanceLevel()
				return
			}
			g.bounceOff(b)
			break
		}
		if g.phase == "playing" {
			g.bx += g.bspeed * g.dx
			g.by += g.bspeed * g.dy
		}
	}
}

func (g *BrickGame) bounceOff(b brk) {
	// which side was hit depends on where the ball is coming from
	if (g.dx > 0 && g.dy > 0) || (g.dx < 0 && g.dy > 0) {
		if g.by > b.y {
			g.dx = -g.dx
		} else {
			g.dy = -g.dy
		}
	} else if g.dx > 0 && g.dy < 0 {
		if g.bx > b.x {
			g.dy = -g.dy
		} else {
			g.dx = -g.dx
		}
	} else if g.dx < 0 && g.dy < 0 {
		if g.bx > b.x+b.w {
			g.dx = -g.dx
		} else {
			g.dy = -g.dy
		}
	}
}

func (g *BrickGame) die() {
	g.lives--
	if g.lives <= 0 {
		g.phase = "gameOver"
		g.menuIndex = 0
		return
	}
	g.resetPlayerPos()
	g.resetBall()
	g.phase = "waiting"
}

func (g *BrickGame) advanceLevel() {
	g.level++
	g.setupBricks()
	g.bspeed = (2 + float64(g.level-1)*0.3) * g.s
	g.resetPlayerPos()
	g.resetBall()
	g.phase = "waiting"
}

func (g *BrickGame) reset() {
	g.level = 1
	g.lives = 3
	g.score = 0
	g.resetPlayerPos()
	g.bspeed = 2 * g.s
	g.resetBall()
	g.setupBricks()
	g.phase = "waiting"
}

// Animating: the game redraws every frame (CustomView); the input
// methods move the paddle and drive the menu.
func (g *BrickGame) Animating(now time.Time) bool { return true }

func (g *BrickGame) OnScroll(ui *IpodUI, dir int, velocity float64) {
	if g.phase == "gameOver" {
		g.menuIndex = int(math.Max(0, math.Min(1, float64(g.menuIndex+dir))))
		return
	}
	g.pvel += float64(dir) * g.pspeed
	maxV := 3 * g.pspeed
	g.pvel = math.Max(-maxV, math.Min(maxV, g.pvel))
}

func (g *BrickGame) OnCenter(ui *IpodUI) {
	switch g.phase {
	case "gameOver":
		if g.menuIndex == 0 {
			g.reset()
		} else if g.onQuit != nil {
			g.onQuit()
		}
	case "waiting":
		g.phase = "playing"
	}
}

func (g *BrickGame) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	// fixed 60 Hz steps
	if g.lastStep.IsZero() {
		g.lastStep = now
	}
	g.acc += now.Sub(g.lastStep)
	g.lastStep = now
	// never more than 10 steps behind
	if g.acc > 10*brickStep {
		g.acc = 10 * brickStep
	}
	for g.acc >= brickStep {
		g.step()
		g.acc -= brickStep
	}
	if v != nil && (v.exiting || ui.topScreen() != v) {
		g.lastStep = time.Time{}
		g.acc = 0
	}
	// background
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	if g.bg == nil {
		g.bg = newCanvas(W, H)
		fillVGradient(g.bg, 0, 0, W, H, []GradStop{{0, Col{64, 162, 247, 1}}, {0.5, Col{64, 162, 247, 1}}, {1, Col{141, 204, 254, 1}}})
	}
	blitOpaque(dst, g.bg, 0, 0)
	s := g.s
	// bricks
	sp := 3 * s
	for _, b := range g.bricks {
		c := brickColors[b.c]
		bw, bh := b.w-sp, b.h-sp
		softShadow(dst, b.x, b.y+2*s, bw, bh, sp, 0.3)
		fillVGradient(dst, px(b.x), px(b.y), px(b.x+bw)-px(b.x), px(b.y+bh)-px(b.y), []GradStop{{0, c.top}, {0.5, c.bottom}, {1, c.bottom}})
		strokeRect(dst, b.x, b.y, bw, bh, 1, c.bottom)
	}
	// paddle
	softShadow(dst, g.px, g.py+2*s, g.pw, g.ph, 4*s, 0.4)
	fillVGradient(dst, px(g.px), px(g.py), px(g.px+g.pw)-px(g.px), px(g.py+g.ph)-px(g.py),
		[]GradStop{{0, Col{199, 199, 199, 1}}, {0.5, Col{85, 85, 85, 1}}, {1, Col{111, 111, 111, 1}}})
	strokeRect(dst, g.px, g.py, g.pw, g.ph, s, Col{0, 0, 0, 0.5})

	g.drawBall(dst)

	g.drawHUD(dst)
	if g.phase == "gameOver" {
		g.drawGameOver(dst)
	}
}

// softShadow: a blurred dark rectangle under (x, y, w, h).
func softShadow(dst *image.RGBA, x, y, w, h, blur, alpha float64) {
	steps := int(math.Max(1, blur*DPR/2))
	for i := steps; i > 0; i-- {
		e := float64(i) / float64(steps) * blur / 2
		a := alpha / float64(steps) * 0.9
		fillRoundedGradient(dst, (x-e)*DPR, (y-e)*DPR, (x+w+e)*DPR, (y+h+e)*DPR, e*DPR, []GradStop{{0, Col{0, 0, 0, a}}, {1, Col{0, 0, 0, a}}}, 1)
	}
}

func strokeRect(dst *image.RGBA, x, y, w, h, lw float64, c Col) {
	strokeRounded(dst, (x-lw/2)*DPR, (y-lw/2)*DPR, (x+w+lw/2)*DPR, (y+h+lw/2)*DPR, 0, lw*DPR, c, 1)
}

func (g *BrickGame) drawBall(dst *image.RGBA) {
	r := g.br
	// shadow
	for i := 3; i > 0; i-- {
		e := float64(i) * g.s
		fillCircle(dst, g.bx, g.by+2*g.s, r+e/2, Col{0, 0, 0, 0.1})
	}
	// radial gradient from a focus up and to the left
	fx, fy := g.bx-r/4, g.by-r/4
	fr := r * 0.05
	stops := []GradStop{{0, Col{171, 171, 171, 1}}, {0.3, Col{98, 98, 98, 1}}, {1, Col{67, 67, 67, 1}}}
	x0, x1 := int((g.bx-r-1)*DPR), int((g.bx+r+1)*DPR)
	y0, y1 := int((g.by-r-1)*DPR), int((g.by+r+1)*DPR)
	bb := dst.Bounds()
	for py := y0; py <= y1; py++ {
		if py < bb.Min.Y || py >= bb.Max.Y {
			continue
		}
		for pxx := x0; pxx <= x1; pxx++ {
			if pxx < bb.Min.X || pxx >= bb.Max.X {
				continue
			}
			cx, cy := (float64(pxx)+0.5)/DPR, (float64(py)+0.5)/DPR
			d := math.Hypot(cx-g.bx, cy-g.by)
			cov := math.Max(0, math.Min(1, (r-d)*DPR+0.5))
			if cov <= 0 {
				continue
			}

			t := radialT(cx, cy, fx, fy, fr, g.bx, g.by, r)
			c := gradAt(stops, t)
			o := dst.PixOffset(pxx, py)
			blendPx(dst.Pix[o:o+4], c, cov)
		}
	}
}

// radialT is the parameter of a two-circle radial gradient at (x, y).
func radialT(x, y, x0, y0, r0, x1, y1, r1 float64) float64 {
	cdx, cdy, dr := x1-x0, y1-y0, r1-r0
	pdx, pdy := x-x0, y-y0
	a := cdx*cdx + cdy*cdy - dr*dr
	bq := pdx*cdx + pdy*cdy + r0*dr
	c := pdx*pdx + pdy*pdy - r0*r0
	if math.Abs(a) < 1e-9 {
		return math.Max(0, math.Min(1, c/(2*bq)))
	}
	disc := bq*bq - a*c
	if disc < 0 {
		return 1
	}
	sq := math.Sqrt(disc)
	t1, t2 := (bq+sq)/a, (bq-sq)/a
	t := math.Max(t1, t2)
	if r0+t*dr < 0 {
		t = math.Min(t1, t2)
	}
	return math.Max(0, math.Min(1, t))
}

func fillCircle(dst *image.RGBA, cx, cy, r float64, c Col) {
	x0, x1 := int((cx-r-1)*DPR), int((cx+r+1)*DPR)
	y0, y1 := int((cy-r-1)*DPR), int((cy+r+1)*DPR)
	bb := dst.Bounds()
	for py := y0; py <= y1; py++ {
		if py < bb.Min.Y || py >= bb.Max.Y {
			continue
		}
		for pxx := x0; pxx <= x1; pxx++ {
			if pxx < bb.Min.X || pxx >= bb.Max.X {
				continue
			}
			d := math.Hypot((float64(pxx)+0.5)/DPR-cx, (float64(py)+0.5)/DPR-cy)
			cov := math.Max(0, math.Min(1, (r-d)*DPR+0.5))
			if cov > 0 {
				o := dst.PixOffset(pxx, py)
				blendPx(dst.Pix[o:o+4], c, c.A*cov)
			}
		}
	}
}

func (g *BrickGame) drawHUD(dst *image.RGBA) {
	s := g.s
	mx := 30 * s
	fs := math.Round(10 * s)
	f := face(W700, fs)
	txt := fmt.Sprintf("%d", g.score)
	tw := textWidth(f, txt) / DPR

	drawText(dst, f, txt, (mx-tw/2)*DPR, (8*s+fs*0.8)*DPR, Col{255, 255, 255, 0.85})

	total := float64(g.lives-1) * (8 * s)
	sx := mx - total/2
	for i := 0; i < g.lives; i++ {
		fillCircle(dst, sx+float64(i)*(8*s), 24*s, 2.5*s, Col{255, 255, 255, 0.85})
	}
}

func (g *BrickGame) drawGameOver(dst *image.RGBA) {
	s := g.s
	fillRect(dst, 0, 0, dst.Bounds().Dx(), dst.Bounds().Dy(), Col{0, 0, 0, 0.7})
	// panel: title, score, two items
	th, sh, ih, bp := 28*s, 20*s, 22*s, 8*s
	ph := th + sh + ih*2 + bp
	x := (g.w - 160*s) / 2
	y := (g.h - ph) / 2
	solid := func(c Col) []GradStop { return []GradStop{{0, c}, {1, c}} }
	fillRoundedGradient(dst, x*DPR, y*DPR, (x+160*s)*DPR, (y+ph)*DPR, bp*DPR, solid(Col{0, 0, 0, 0.85}), 1)
	text := func(f font.Face, txt string, cy float64, c Col) {
		tw := textWidth(f, txt) / DPR
		fs := faceSize(f)
		drawText(dst, f, txt, (g.w/2-tw/2)*DPR, (cy+fs*0.35)*DPR, c)
	}
	text(sizedFace(W700, math.Round(14*s)), "Game Over", y+th/2, colWhite)
	text(sizedFace(W400, math.Round(11*s)), fmt.Sprintf("Score: %d", g.score), y+th+sh/2, Col{255, 255, 255, 0.7})
	items := []string{"New Game", "Quit"}
	iy := y + th + sh
	pad := 10 * s
	for i, it := range items {
		yy := iy + float64(i)*ih
		if i == g.menuIndex {
			fillRoundedGradient(dst, (x+pad)*DPR, yy*DPR, (x+160*s-pad)*DPR, (yy+ih-2*s)*DPR, 4*s*DPR, solid(Col{64, 162, 247, 0.9}), 1)
		}
		text(sizedFace(W400, math.Round(12*s)), it, yy+(ih-2*s)/2, colWhite)
	}
}
