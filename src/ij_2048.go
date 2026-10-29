package main

// ij_2048.go — 2048 (after Gabriele Cirulli's game, MIT): the arrows slide
// the tiles, two equal ones merge, a new 2 (or 4) comes after each move.
// The game is kept when you leave (and the best score), so it goes on.
//
//	arrows  slide            START  new game
//	A       play again / keep going (on the messages)
//	B       back (the game is kept)

import (
	"encoding/json"
	"fmt"
	"image"
	"math"
	"math/rand"
	"os"
	"time"
)

const (
	g48N     = 4
	g48Slide = 110 * time.Millisecond
	g48Pop   = 140 * time.Millisecond
)

type g48Tile struct {
	v            int
	r, c, fr, fc int  // where it is, and where it slid from
	born, merged bool // new (grows in) / made by a merge (pops)
	gone         bool // merged into another: drawn sliding, then dropped
	gr, gc       int  // (gone: where it slid to)
}

type Game2048 struct {
	tiles   []*g48Tile
	score   int
	best    int
	won     bool // reached 2048 (the message shown once)
	over    bool
	keepOn  bool // went on after 2048
	movedAt time.Time
	rng     *rand.Rand
	bg      *image.RGBA
}

type g48Save struct {
	Board  [g48N][g48N]int `json:"board"`
	Score  int             `json:"score"`
	Best   int             `json:"best"`
	Won    bool            `json:"won"`
	KeepOn bool            `json:"keepOn"`
}

func g48Path() string { return dataPath("2048.json") }

func (ui *IpodUI) open2048() {
	g := &Game2048{rng: rand.New(rand.NewSource(time.Now().UnixNano()))}
	if !g.load() {
		g.newGame()
	}
	v := &View{ID: "2048", Title: "2048", Kind: KindFull, Custom: g}
	v.onClose = g.save
	ui.showView(v, "")
}

func (g *Game2048) load() bool {
	data, err := os.ReadFile(g48Path())
	if err != nil {
		return false
	}
	var s g48Save
	if json.Unmarshal(data, &s) != nil {
		return false
	}
	g.best = s.Best
	n := 0
	for r := 0; r < g48N; r++ {
		for c := 0; c < g48N; c++ {
			if v := s.Board[r][c]; v > 0 {
				g.tiles = append(g.tiles, &g48Tile{v: v, r: r, c: c, fr: r, fc: c})
				n++
			}
		}
	}
	if n == 0 {
		return false
	}
	g.score, g.won, g.keepOn = s.Score, s.Won, s.KeepOn
	g.over = !g.canMove()
	return true
}

func (g *Game2048) save() {
	var s g48Save
	for _, t := range g.tiles {
		if !t.gone {
			s.Board[t.r][t.c] = t.v
		}
	}
	if g.over {
		s.Board = [g48N][g48N]int{} // (a finished game starts again)
	}
	s.Score, s.Best, s.Won, s.KeepOn = g.score, g.best, g.won, g.keepOn
	if g.over {
		s.Score, s.Won, s.KeepOn = 0, false, false
	}
	if data, err := json.Marshal(s); err == nil {
		writeFileAtomic(g48Path(), data)
	}
}

func (g *Game2048) newGame() {
	g.tiles, g.score, g.won, g.over, g.keepOn = nil, 0, false, false, false
	g.spawn()
	g.spawn()
	g.movedAt = time.Now()
}

func (g *Game2048) grid() [g48N][g48N]*g48Tile {
	var b [g48N][g48N]*g48Tile
	for _, t := range g.tiles {
		if !t.gone {
			b[t.r][t.c] = t
		}
	}
	return b
}

// spawn: a 2 (or, one time in ten, a 4) on a free cell.
func (g *Game2048) spawn() {
	b := g.grid()
	var free [][2]int
	for r := 0; r < g48N; r++ {
		for c := 0; c < g48N; c++ {
			if b[r][c] == nil {
				free = append(free, [2]int{r, c})
			}
		}
	}
	if len(free) == 0 {
		return
	}
	p := free[g.rng.Intn(len(free))]
	v := 2
	if g.rng.Intn(10) == 0 {
		v = 4
	}
	g.tiles = append(g.tiles, &g48Tile{v: v, r: p[0], c: p[1], fr: p[0], fc: p[1], born: true})
}

// move slides everything towards (dr, dc); it reports whether anything moved.
func (g *Game2048) move(dr, dc int) bool {
	// the tiles of the last move settle first
	keep := g.tiles[:0]
	for _, t := range g.tiles {
		if !t.gone {
			t.fr, t.fc, t.born, t.merged = t.r, t.c, false, false
			keep = append(keep, t)
		}
	}
	g.tiles = keep
	b := g.grid()
	moved := false
	// lines are walked from the side the tiles go to
	for line := 0; line < g48N; line++ {
		var cells [g48N][2]int
		for i := 0; i < g48N; i++ {
			r, c := line, i
			if dr != 0 {
				r, c = i, line
			}
			if dr > 0 || dc > 0 {
				if dr != 0 {
					r = g48N - 1 - i
				} else {
					c = g48N - 1 - i
				}
			}
			cells[i] = [2]int{r, c}
		}
		pos := 0
		var last *g48Tile
		for i := 0; i < g48N; i++ {
			t := b[cells[i][0]][cells[i][1]]
			if t == nil {
				continue
			}
			if last != nil && last.v == t.v && !last.merged {
				// merge: t slides onto last, which doubles
				t.gone, t.gr, t.gc = true, last.r, last.c
				last.v *= 2
				last.merged = true
				g.score += last.v
				if last.v == 2048 && !g.won {
					g.won = true
				}
				moved = true
				continue
			}
			dst := cells[pos]
			if t.r != dst[0] || t.c != dst[1] {
				moved = true
			}
			t.r, t.c = dst[0], dst[1]
			last = t
			pos++
		}
	}
	if !moved {
		return false
	}
	g.best = max(g.best, g.score)
	g.spawn()
	g.movedAt = time.Now()
	g.over = !g.canMove()
	return true
}

func (g *Game2048) canMove() bool {
	b := g.grid()
	for r := 0; r < g48N; r++ {
		for c := 0; c < g48N; c++ {
			t := b[r][c]
			if t == nil {
				return true
			}
			if c+1 < g48N && b[r][c+1] != nil && b[r][c+1].v == t.v {
				return true
			}
			if r+1 < g48N && b[r+1][c] != nil && b[r+1][c].v == t.v {
				return true
			}
		}
	}
	return false
}

func (g *Game2048) message() string {
	switch {
	case g.over:
		return "over"
	case g.won && !g.keepOn:
		return "won"
	}
	return ""
}

// ---- keys ----

func (g *Game2048) OnKey(ui *IpodUI, k Key) bool {
	switch k {
	case UP, DOWN, LEFT, RIGHT:
		if g.message() != "" {
			return true
		}
		dr, dc := 0, 0
		switch k {
		case UP:
			dr = -1
		case DOWN:
			dr = 1
		case LEFT:
			dc = -1
		case RIGHT:
			dc = 1
		}
		g.move(dr, dc)
		ui.dirty = true
		return true
	case START:
		g.newGame()
		ui.dirty = true
		return true
	}
	return false
}

func (g *Game2048) OnCenter(ui *IpodUI) {
	switch g.message() {
	case "over":
		g.newGame()
	case "won":
		g.keepOn = true
	}
	ui.dirty = true
}

func (g *Game2048) OnScroll(ui *IpodUI, dir int, velocity float64) {}

func (g *Game2048) Animating(now time.Time) bool {
	return now.Sub(g.movedAt) < g48Slide+g48Pop+30*time.Millisecond
}

// ---- drawing ----

// g48Colors: a tile's gradient and its number's colour, by value.
func g48Colors(v int) (Col, Col, Col) {
	dark := Col{58, 62, 70, 1}
	switch v {
	case 2:
		return Col{250, 251, 253, 1}, Col{218, 223, 231, 1}, dark
	case 4:
		return Col{250, 244, 226, 1}, Col{228, 214, 176, 1}, dark
	case 8:
		return Col{255, 190, 120, 1}, Col{240, 122, 30, 1}, colWhite
	case 16:
		return Col{255, 158, 112, 1}, Col{232, 80, 42, 1}, colWhite
	case 32:
		return Col{255, 124, 112, 1}, Col{217, 48, 48, 1}, colWhite
	case 64:
		return Col{255, 92, 104, 1}, Col{184, 20, 31, 1}, colWhite
	case 128:
		return Col{255, 232, 136, 1}, Col{240, 191, 34, 1}, colWhite
	case 256:
		return Col{255, 220, 100, 1}, Col{230, 168, 0, 1}, colWhite
	case 512:
		return Col{150, 232, 154, 1}, Col{47, 174, 75, 1}, colWhite
	case 1024:
		return Col{130, 200, 255, 1}, Col{47, 127, 224, 1}, colWhite
	case 2048:
		return Col{200, 160, 255, 1}, Col{122, 63, 224, 1}, colWhite
	}
	return Col{110, 120, 140, 1}, Col{43, 50, 64, 1}, colWhite
}

const (
	g48X0   = 24.0
	g48Y0   = 20.0
	g48Size = 400.0
	g48Gap  = 12.0
)

func g48Cell(r, c float64) (float64, float64) {
	t := (g48Size - 5*g48Gap) / 4
	return g48X0 + g48Gap + c*(t+g48Gap), g48Y0 + g48Gap + r*(t+g48Gap)
}

func (g *Game2048) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	if g.bg == nil || g.bg.Bounds().Dx() != W || g.bg.Bounds().Dy() != H {
		g.bg = newCanvas(W, H)
		fillVGradient(g.bg, 0, 0, W, H, []GradStop{{0, Col{238, 241, 245, 1}}, {1, Col{206, 212, 221, 1}}})
		// the board
		fillRoundedGradient(g.bg, g48X0, g48Y0, g48X0+g48Size, g48Y0+g48Size, 14,
			[]GradStop{{0, Col{122, 132, 148, 1}}, {1, Col{92, 101, 117, 1}}}, 1)
		ts := (g48Size - 5*g48Gap) / 4
		for r := 0; r < g48N; r++ {
			for c := 0; c < g48N; c++ {
				x, y := g48Cell(float64(r), float64(c))
				fillRoundedGradient(g.bg, x, y, x+ts, y+ts, 9,
					[]GradStop{{0, Col{108, 117, 133, 1}}, {1, Col{120, 130, 146, 1}}}, 1)
			}
		}
	}
	blitOpaque(dst, g.bg, 0, 0)
	ts := (g48Size - 5*g48Gap) / 4
	el := now.Sub(g.movedAt)
	slide := math.Min(1, float64(el)/float64(g48Slide))
	slide = easeOutCubic(slide)
	pop := math.Min(1, math.Max(0, float64(el-g48Slide)/float64(g48Pop)))
	draw := func(t *g48Tile) {
		r := float64(t.fr) + float64(t.r-t.fr)*slide
		c := float64(t.fc) + float64(t.c-t.fc)*slide
		if t.gone {
			r = float64(t.fr) + float64(t.gr-t.fr)*slide
			c = float64(t.fc) + float64(t.gc-t.fc)*slide
			if slide >= 1 {
				return
			}
		}
		x, y := g48Cell(r, c)
		sc := 1.0
		val := t.v
		switch {
		case t.born:
			if el < g48Slide {
				return
			}
			sc = easeOutCubic(pop)
		case t.merged:
			if el < g48Slide {
				val = t.v / 2 // (still the two halves sliding together)
			} else {
				sc = 1 + 0.12*math.Sin(math.Pi*pop)
			}
		}
		if sc <= 0.02 {
			return
		}
		cx, cy := x+ts/2, y+ts/2
		h := ts / 2 * sc
		top, bot, tc := g48Colors(val)
		fillRoundedGradient(dst, cx-h, cy-h, cx+h, cy+h, 9*sc, []GradStop{{0, top}, {1, bot}}, 1)
		// gloss on the top half
		fillRoundedGradient(dst, cx-h+2, cy-h+2, cx+h-2, cy, 8*sc,
			[]GradStop{{0, Col{255, 255, 255, 0.45}}, {1, Col{255, 255, 255, 0.05}}}, 1)
		s := fmt.Sprint(val)
		size := map[int]float64{1: 24, 2: 22, 3: 18, 4: 14}[len(s)]
		if size == 0 {
			size = 11
		}
		f := face(W800or700(), size*sc)
		_, asc := lineBox(size * sc)
		drawText(dst, f, s, cx-textWidth(f, s)/2, cy+asc*DPR/2-2*sc, tc)
	}
	for _, t := range g.tiles {
		if t.gone {
			draw(t)
		}
	}
	for _, t := range g.tiles {
		if !t.gone {
			draw(t)
		}
	}
	// the scores, at the right
	box := func(y float64, label string, n int) {
		x0, x1 := 448.0, 618.0
		fillRoundedGradient(dst, x0, y, x1, y+78, 12, []GradStop{{0, Col{122, 132, 148, 1}}, {1, Col{92, 101, 117, 1}}}, 1)
		lf := face(W700, 11)
		_, la := lineBox(11)
		drawText(dst, lf, label, (x0+x1)/2-textWidth(lf, label)/2, y+14+la*DPR/2, Col{230, 234, 240, 1})
		nf := face(W700, 22)
		_, na := lineBox(22)
		s := fmt.Sprint(n)
		drawText(dst, nf, s, (x0+x1)/2-textWidth(nf, s)/2, y+48+na*DPR/2, colWhite)
	}
	box(g48Y0, "SCORE", g.score)
	box(g48Y0+92, "BEST", g.best)
	hf := face(W600, 11)
	_, ha := lineBox(11)
	for i, l := range []string{"Arrows  slide", "START  new game", "B  back"} {
		drawText(dst, hf, l, 452, (g48Y0+g48Size-76+float64(i)*30)+ha*DPR/2, Col{90, 98, 112, 1})
	}
	// a message over the board
	if m := g.message(); m != "" && el > g48Slide+g48Pop {
		fillRoundedGradient(dst, g48X0, g48Y0, g48X0+g48Size, g48Y0+g48Size, 14,
			[]GradStop{{0, Col{255, 255, 255, 0.72}}, {1, Col{255, 255, 255, 0.72}}}, 1)
		title, sub := "No more moves", "A: play again"
		if m == "won" {
			title, sub = "You made 2048!", "A: keep going · START: new game"
		}
		tf := face(W700, 22)
		_, ta := lineBox(22)
		cx := g48X0 + g48Size/2
		drawText(dst, tf, title, cx-textWidth(tf, title)/2, g48Y0+g48Size/2-10+ta*DPR/2, Col{40, 44, 52, 1})
		sf := face(W500, 13)
		_, sa := lineBox(13)
		drawText(dst, sf, sub, cx-textWidth(sf, sub)/2, g48Y0+g48Size/2+36+sa*DPR/2, Col{70, 76, 88, 1})
	}
}

// W800or700: the boldest Inter weight shipped.
func W800or700() FontWeight { return W700 }

// g48Icon: the preview's picture, a few tiles.
var g48IconImg *image.RGBA

func g48Icon() *image.RGBA {
	if g48IconImg != nil {
		return g48IconImg
	}
	const S = 192.0
	img := newCanvas(int(S), int(S))
	fillRoundedGradient(img, 8, 8, S-8, S-8, 26, []GradStop{{0, Col{122, 132, 148, 1}}, {1, Col{92, 101, 117, 1}}}, 1)
	vals := [][2]int{{2, 4}, {8, 2048}}
	ts := (S - 16 - 3*10) / 2
	for r := 0; r < 2; r++ {
		for c := 0; c < 2; c++ {
			x := 8 + 10 + float64(c)*(ts+10)
			y := 8 + 10 + float64(r)*(ts+10)
			top, bot, tc := g48Colors(vals[r][c])
			fillRoundedGradient(img, x, y, x+ts, y+ts, 14, []GradStop{{0, top}, {1, bot}}, 1)
			fillRoundedGradient(img, x+2, y+2, x+ts-2, y+ts/2, 12,
				[]GradStop{{0, Col{255, 255, 255, 0.45}}, {1, Col{255, 255, 255, 0.05}}}, 1)
			s := fmt.Sprint(vals[r][c])
			size := map[int]float64{1: 26, 4: 12}[len(s)]
			f := face(W700, size)
			_, asc := lineBox(size)
			drawText(img, f, s, x+ts/2-textWidth(f, s)/2, y+ts/2+asc*DPR/2-2, tc)
		}
	}
	g48IconImg = img
	return img
}
