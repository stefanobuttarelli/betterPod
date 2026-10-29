package main

// ij_puzzles.go — Apps → Puzzles: a dozen puzzles from Simon Tatham's
// Portable Puzzle Collection (MIT, src/puzzles/LICENCE). The puzzles run
// as they are (their C code, through their "midend"); this is their front
// end: they draw into a list of commands (src/puzzles/pzfe.c) that is
// replayed here on a canvas, and the keys go to their keyboard cursor.
//
//	arrows  move the cursor (Fifteen: slide)
//	A       the puzzle's action (open, rotate, fill…); number puzzles:
//	        place the chosen digit
//	Y       the second action (flag, mark…); number puzzles: pencil marks
//	L       undo       R  redo (number puzzles: the next digit, or clear)
//	START   new puzzle      B  back (it's kept)     X  play / pause, as anywhere

/*
#include <stdlib.h>
#include "puzzles/pzfe.c" // (rev 2: change it after editing a file in puzzles/: Go does not see them change)
*/
import "C"

import (
	"fmt"
	"image"
	"image/draw"
	"math"
	"os"
	"path/filepath"
	"time"
	"unsafe"

	"golang.org/x/image/vector"
)

type pzInfo struct {
	id, name, about string
	numeric         bool   // digits are placed (L / R choose, A places)
	hint            string // the keys, under the puzzle
}

// pzList: the puzzles, in pzfe.c's order.
var pzList = []pzInfo{
	{"mines", "Mines", "Find the mines: numbers count those around", false, "A open · Y flag · L undo · START new"},
	{"solo", "Sudoku", "Every row, column and box holds each digit once", true, ""},
	{"net", "Net", "Turn the tiles to connect the whole network", false, "A turn · Y turn back · L undo · START new"},
	{"flood", "Flood", "Fill the board with one colour in few moves", false, "A flood with this colour · L undo · START new"},
	{"samegame", "Same Game", "Clear groups of the same colour", false, "A select, A again remove · L undo · START new"},
	{"fifteen", "Fifteen", "Slide the tiles into order", false, "Arrows slide · L undo · START new"},
	{"lightup", "Light Up", "Light every square with bulbs that don't see each other", false, "A bulb · Y mark · L undo · START new"},
	{"unruly", "Unruly", "Black and white: no three in a row, as many of each", false, "A cycle · Y cycle back · L undo · START new"},
	{"towers", "Towers", "Place the towers: the clues count those you can see", true, ""},
	{"keen", "Keen", "A Latin square whose cages add, multiply…", true, ""},
	{"pattern", "Pattern", "Fill the squares the clues describe (nonogram)", false, "A fill · Y empty · L undo · START new"},
	{"tents", "Tents", "A tent next to each tree, never touching", false, "A tent · Y grass · L undo · START new"},
}

const pzNumHint = "R digit · A place · Y pencil · L undo · START new"

// puzzlesView: the list of puzzles.
func (ui *IpodUI) puzzlesView() *View {
	v := &View{ID: "puzzles", Title: "Puzzles", Kind: KindSplit}
	for i, p := range pzList {
		i := i
		v.Items = append(v.Items, Item{Label: p.name, Sublabel: p.about, Preview: PrevPuzzles,
			OnSelect: func() { ui.openPuzzle(i) }})
	}
	return v
}

type PuzzleGame struct {
	idx    int
	fe     *C.frontend
	me     *C.midend
	canvas *image.RGBA
	w, h   int
	pal    []Col
	blits  map[int]*image.RGBA
	clip   image.Rectangle
	ras    vector.Rasterizer
	last   time.Time
	digit  int // number puzzles: the digit A places (0: clear)
	bg     *image.RGBA
	dirty  bool
}

const (
	pzAreaW = 640
	pzAreaH = 440 - 44 // (the keys' line under it)
)

func pzSavePath(id string) string { return dataPath(filepath.Join("puzzles", id+".sav")) }

func (ui *IpodUI) openPuzzle(i int) {
	g := &PuzzleGame{idx: i, blits: map[int]*image.RGBA{}, digit: 1}
	// the default background: the app's light grey (the puzzles derive
	// their colours from it)
	g.fe = C.pz_fe_new(0.88, 0.89, 0.91)
	g.me = C.pz_new(g.fe, C.int(i))
	loaded := false
	if data, err := os.ReadFile(pzSavePath(pzList[i].id)); err == nil && len(data) > 0 {
		cd := C.CBytes(data)
		if e := C.pz_load(g.me, (*C.char)(cd), C.int(len(data))); e == nil {
			loaded = true
		}
		C.free(cd)
	}
	if !loaded {
		C.midend_new_game(g.me)
	}
	g.layout()
	if pzList[i].numeric {
		// the cursor shows from the start (digits go where it is)
		C.pz_key(g.me, 2)
		C.pz_key(g.me, 1)
		C.midend_redraw(g.me)
		g.replay()
	}
	v := &View{ID: "puzzle", Title: pzList[i].name, Kind: KindFull, Custom: g}
	v.onClose = g.close
	ui.showView(v, "")
}

// layout: the puzzle's size for the screen, its colours, a fresh canvas.
func (g *PuzzleGame) layout() {
	w, h := C.int(pzAreaW-16), C.int(pzAreaH-12)
	C.midend_size(g.me, &w, &h, 1) // (as big as the screen allows)
	g.w, g.h = int(w), int(h)
	g.canvas = newCanvas(g.w, g.h)
	var n C.int
	cols := C.midend_colours(g.me, &n)
	fs := unsafe.Slice((*C.float)(cols), int(n)*3)
	g.pal = make([]Col, int(n))
	for i := range g.pal {
		g.pal[i] = Col{float64(fs[i*3]) * 255, float64(fs[i*3+1]) * 255, float64(fs[i*3+2]) * 255, 1}
	}
	C.free(unsafe.Pointer(cols))
	g.clip = g.canvas.Bounds()
	C.midend_force_redraw(g.me)
	g.replay()
}

func (g *PuzzleGame) close() {
	var n C.int
	p := C.pz_save(g.me, &n)
	if p != nil && n > 0 {
		os.MkdirAll(filepath.Dir(pzSavePath(pzList[g.idx].id)), 0755)
		writeFileAtomic(pzSavePath(pzList[g.idx].id), C.GoBytes(unsafe.Pointer(p), n))
	}
	C.free(unsafe.Pointer(p))
	C.midend_free(g.me)
	g.me = nil
}

// ---- replaying the drawing ----

type pzCmd struct {
	op, a, b, c, d, e, f, g, off C.int
	fl                           C.float
}

func (g *PuzzleGame) col(i C.int) Col {
	if int(i) < 0 || int(i) >= len(g.pal) {
		return Col{0, 0, 0, 1}
	}
	return g.pal[i]
}

// replay draws what the puzzle asked for since the last time.
func (g *PuzzleGame) replay() {
	n := int(C.pz_ncmd(g.fe))
	if n == 0 {
		return
	}
	cmds := unsafe.Slice((*pzCmd)(unsafe.Pointer(C.pz_cmds(g.fe))), n)
	ints := C.pz_intpool(g.fe)
	chars := C.pz_charpool(g.fe)
	for _, c := range cmds {
		switch c.op {
		case 1: // text
			s := C.GoString((*C.char)(unsafe.Add(unsafe.Pointer(chars), int(c.off))))
			g.text(int(c.a), int(c.b), int(c.d), int(c.e), g.col(c.f), s)
		case 2: // rect
			r := image.Rect(int(c.a), int(c.b), int(c.a+c.c), int(c.b+c.d)).Intersect(g.clip)
			if !r.Empty() {
				fillRect(g.canvas, r.Min.X, r.Min.Y, r.Dx(), r.Dy(), g.col(c.e))
			}
		case 3: // line
			g.line(float64(c.a)+0.5, float64(c.b)+0.5, float64(c.c)+0.5, float64(c.d)+0.5, 1.2, g.col(c.e))
		case 4: // polygon
			pts := unsafe.Slice((*C.int)(unsafe.Add(unsafe.Pointer(ints), int(c.off)*4)), int(c.a)*2)
			xy := make([]float32, len(pts))
			for i, v := range pts {
				xy[i] = float32(v) + 0.5
			}
			if c.b >= 0 {
				g.poly(xy, g.col(c.b))
			}
			if c.c >= 0 {
				for i := 0; i < int(c.a); i++ {
					j := (i + 1) % int(c.a)
					g.line(float64(xy[i*2]), float64(xy[i*2+1]), float64(xy[j*2]), float64(xy[j*2+1]), 1.2, g.col(c.c))
				}
			}
		case 5: // circle
			cx, cy, r := float64(c.a)+0.5, float64(c.b)+0.5, float64(c.c)
			if c.d >= 0 {
				g.poly(circlePts(cx, cy, r), g.col(c.d))
			}
			if c.e >= 0 {
				pts := circlePts(cx, cy, r)
				for i := 0; i < len(pts)/2; i++ {
					j := (i + 1) % (len(pts) / 2)
					g.line(float64(pts[i*2]), float64(pts[i*2+1]), float64(pts[j*2]), float64(pts[j*2+1]), 1.2, g.col(c.e))
				}
			}
		case 6: // clip
			g.clip = image.Rect(int(c.a), int(c.b), int(c.a+c.c), int(c.b+c.d)).Intersect(g.canvas.Bounds())
		case 7: // unclip
			g.clip = g.canvas.Bounds()
		case 8: // blitter save
			if b := g.blits[int(c.a)]; b != nil {
				draw.Draw(b, b.Bounds(), g.canvas, image.Pt(int(c.b), int(c.c)), draw.Src)
			}
		case 9: // blitter load
			if b := g.blits[int(c.a)]; b != nil {
				r := image.Rect(int(c.b), int(c.c), int(c.b)+b.Bounds().Dx(), int(c.c)+b.Bounds().Dy()).Intersect(g.clip)
				draw.Draw(g.canvas, r, b, r.Min.Sub(image.Pt(int(c.b), int(c.c))), draw.Src)
			}
		case 10: // thick line
			g.line(float64(c.a)/16+0.5, float64(c.b)/16+0.5, float64(c.c)/16+0.5, float64(c.d)/16+0.5, math.Max(1.2, float64(c.fl)), g.col(c.e))
		case 11: // blitter new
			g.blits[int(c.a)] = newCanvas(int(c.b), int(c.c))
		case 12:
			delete(g.blits, int(c.a))
		}
	}
	C.pz_reset(g.fe)
	g.dirty = true
}

func circlePts(cx, cy, r float64) []float32 {
	n := int(math.Max(16, math.Min(64, r*1.5)))
	out := make([]float32, 0, n*2)
	for i := 0; i < n; i++ {
		a := float64(i) / float64(n) * 2 * math.Pi
		out = append(out, float32(cx+r*math.Cos(a)), float32(cy+r*math.Sin(a)))
	}
	return out
}

// poly: a filled polygon (anti-aliased), within the clip.
func (g *PuzzleGame) poly(xy []float32, c Col) {
	r := g.clip
	if r.Empty() || len(xy) < 6 {
		return
	}
	g.ras.Reset(r.Dx(), r.Dy())
	ox, oy := float32(r.Min.X), float32(r.Min.Y)
	g.ras.MoveTo(xy[0]-ox, xy[1]-oy)
	for i := 2; i+1 < len(xy); i += 2 {
		g.ras.LineTo(xy[i]-ox, xy[i+1]-oy)
	}
	g.ras.ClosePath()
	g.ras.Draw(g.canvas, r, image.NewUniform(c8(c)), image.Point{})
}

// line: a segment as a thin quad (anti-aliased).
func (g *PuzzleGame) line(x1, y1, x2, y2, w float64, c Col) {
	dx, dy := x2-x1, y2-y1
	l := math.Hypot(dx, dy)
	if l < 0.01 {
		g.poly([]float32{float32(x1 - w/2), float32(y1 - w/2), float32(x1 + w/2), float32(y1 - w/2), float32(x1 + w/2), float32(y1 + w/2), float32(x1 - w/2), float32(y1 + w/2)}, c)
		return
	}
	// a little longer at each end, so joined lines meet
	ux, uy := dx/l, dy/l
	x1, y1, x2, y2 = x1-ux*w/2, y1-uy*w/2, x2+ux*w/2, y2+uy*w/2
	nx, ny := -uy*w/2, ux*w/2
	g.poly([]float32{float32(x1 + nx), float32(y1 + ny), float32(x2 + nx), float32(y2 + ny), float32(x2 - nx), float32(y2 - ny), float32(x1 - nx), float32(y1 - ny)}, c)
}

func (g *PuzzleGame) text(x, y, size, align int, c Col, s string) {
	f := face(W700, float64(size)/DPR)
	tw := textWidth(f, s)
	fx, fy := float64(x), float64(y)
	switch align & 0x0f {
	case 1:
		fx -= tw / 2
	case 2:
		fx -= tw
	}
	if align&0x100 != 0 {
		fy += float64(size) * 0.36 // (the middle of the digits)
	}
	sub, ok := g.canvas.SubImage(g.clip).(*image.RGBA)
	if !ok || sub.Bounds().Empty() {
		return
	}
	drawText(sub, f, s, fx, fy, c)
}

// ---- keys ----

func (g *PuzzleGame) send(k int) {
	if g.me == nil {
		return
	}
	C.pz_key(g.me, C.int(k))
	C.midend_redraw(g.me)
	g.replay()
}

func (g *PuzzleGame) OnKey(ui *IpodUI, k Key) bool {
	num := pzList[g.idx].numeric
	switch k {
	case UP:
		g.send(1)
	case DOWN:
		g.send(2)
	case LEFT:
		g.send(3)
	case RIGHT:
		g.send(4)
	case Y:
		if num {
			g.send(5) // pencil marks on / off
		} else {
			g.send(6)
		}
	case L:
		g.send(7)
	case R:
		if num {
			g.digit = (g.digit + 1) % 10
		} else {
			g.send(8)
		}
	case START:
		g.send(9)
	default:
		return false
	}
	ui.dirty = true
	return true
}

func (g *PuzzleGame) OnCenter(ui *IpodUI) {
	if pzList[g.idx].numeric {
		if g.digit == 0 {
			g.send(6) // clear
		} else {
			if g.me != nil {
				C.midend_process_key(g.me, 0, 0, C.int('0'+g.digit))
				C.midend_redraw(g.me)
				g.replay()
			}
		}
	} else {
		g.send(5)
	}
	ui.dirty = true
}

func (g *PuzzleGame) OnScroll(ui *IpodUI, dir int, velocity float64) {}

func (g *PuzzleGame) Animating(now time.Time) bool {
	return g.fe != nil && C.pz_timer_on(g.fe) != 0
}

// ---- the screen ----

func (g *PuzzleGame) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	if g.me != nil && C.pz_timer_on(g.fe) != 0 {
		dt := 1.0 / 30
		if !g.last.IsZero() {
			dt = math.Min(0.1, now.Sub(g.last).Seconds())
		}
		C.midend_timer(g.me, C.float(dt))
		g.replay()
		g.last = now
	} else {
		g.last = time.Time{}
	}
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	bgc := Col{224, 227, 232, 1}
	if len(g.pal) > 0 {
		bgc = g.pal[0]
	}
	fillRect(dst, 0, 0, W, H, bgc)
	ox, oy := (W-g.w)/2, (pzAreaH-g.h)/2
	blitOpaque(dst, g.canvas, ox, oy)
	// the keys, or the puzzle's own line, under it
	fillRoundedGradient(dst, 0, float64(pzAreaH), float64(W), float64(H), 0,
		[]GradStop{{0, Col{0, 0, 0, 0.06}}, {1, Col{0, 0, 0, 0.12}}}, 1)
	f := face(W600, 10)
	_, a := lineBox(10)
	hint := pzList[g.idx].hint
	if pzList[g.idx].numeric {
		hint = pzNumHint
	}
	left := ""
	if g.fe != nil {
		left = C.GoString(C.pz_status(g.fe))
	}
	if g.me != nil && C.midend_status(g.me) > 0 {
		left = "Solved!"
	}
	if pzList[g.idx].numeric {
		d := fmt.Sprint(g.digit)
		if g.digit == 0 {
			d = "clear"
		}
		left = "Digit: " + d + "   " + left
	}
	y := float64(pzAreaH) + 22 + a*DPR/2
	ink := Col{70, 76, 88, 1}
	if left != "" {
		lw := math.Min(textWidth(f, left), 200)
		drawText(dst, f, ellipsize(f, left, 200), 14, y, Col{30, 34, 42, 1})
		hw := float64(W) - 14 - (14 + lw + 20)
		drawText(dst, f, ellipsize(f, hint, hw), float64(W)-14-math.Min(hw, textWidth(f, hint)), y, ink)
	} else {
		drawText(dst, f, hint, float64(W)/2-textWidth(f, hint)/2, y, ink)
	}
}

// pzIcon: the Puzzles preview (made once): a small glossy grid.
var pzIconImg *image.RGBA

func pzIcon() *image.RGBA {
	if pzIconImg != nil {
		return pzIconImg
	}
	const S = 192.0
	img := newCanvas(int(S), int(S))
	fillRoundedGradient(img, 12, 12, S-12, S-12, 36, []GradStop{{0, Col{255, 196, 92, 1}}, {1, Col{226, 120, 10, 1}}}, 1)
	cells := [][3]int{{1, 0, 1}, {0, 1, 0}, {1, 1, 0}}
	for r := 0; r < 3; r++ {
		for c := 0; c < 3; c++ {
			x, y := 40+float64(c)*40, 40+float64(r)*40
			col := []GradStop{{0, Col{255, 255, 255, 0.95}}, {1, Col{255, 244, 226, 0.95}}}
			if cells[r][c] == 1 {
				col = []GradStop{{0, Col{120, 70, 20, 0.85}}, {1, Col{90, 50, 10, 0.85}}}
			}
			fillRoundedGradient(img, x, y, x+34, y+34, 7, col, 1)
		}
	}
	fillRoundedGradient(img, 15, 15, S-15, S/2, 32, []GradStop{{0, Col{255, 255, 255, 0.4}}, {1, Col{255, 255, 255, 0.04}}}, 1)
	pzIconImg = img
	return img
}
