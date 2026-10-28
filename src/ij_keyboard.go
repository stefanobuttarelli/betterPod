package main

// ij_keyboard.go — the on-screen keyboard: a QWERTY grid that slides up
// from the bottom, made for a d-pad.
//
//	Q W E R T Y U I O P
//	 A S D F G H J K L
//	   Z X C V B N M
//	123 | space | ⌫ | Search
//
// The 123 key (or Y) switches to digits and symbols. The keyboard edits the
// caller's text itself, with a caret:
//
//	d-pad   move between keys (left/right wrap round a row; up/down go to
//	        the nearest key of the next row); held, it keeps moving
//	A       type the selected key
//	B       delete the character before the caret (held: close)
//	X       space          Y   letters / 123
//	L / R   move the caret START  search
//
// The keys are drawn like the app's other buttons: light glossy keys,
// grey ones for the actions, and the selected key in the list's blue.

import (
	"image"
	"math"

	"github.com/fogleman/gg"
	"time"
	"unicode/utf8"
)

type kbAction int

const (
	kbChar kbAction = iota
	kbSpace
	kbDelete
	kbLayer
	kbEnter
)

type kbDef struct {
	label string
	act   kbAction
	w     float64 // CSS px
}

const (
	kbKeyW   = 28.0
	kbKeyH   = 25.0
	kbGap    = 3.0
	kbPad    = 6.0
	kbPanelH = kbPad*2 + 4*kbKeyH + 3*kbGap
)

func kbCharRow(s string) []kbDef {
	var row []kbDef
	for _, r := range s {
		row = append(row, kbDef{string(r), kbChar, kbKeyW})
	}
	return row
}

// kbLayouts: letters, then digits and symbols; the bottom row is shared
// (its layer key names the other layer).
var kbLayouts = [2][][]kbDef{
	{kbCharRow("qwertyuiop"), kbCharRow("asdfghjkl"), kbCharRow("zxcvbnm")},
	{kbCharRow("1234567890"), kbCharRow("-._'&!?()"), kbCharRow("@#:,/+=")},
}

func kbBottomRow(layer int) []kbDef {
	lab := "123"
	if layer == 1 {
		lab = "ABC"
	}
	return []kbDef{{lab, kbLayer, 48}, {"space", kbSpace, 140}, {"⌫", kbDelete, 46}, {"Search", kbEnter, 64}}
}

// Keyboard is the on-screen keyboard and the text it edits.
type Keyboard struct {
	layer    int
	row, col int
	anim     *Anim // 0 hidden, 1 shown
	exiting  bool
	text     *string
	caret    int // in runes
	onChange func()
	onEnter  func()
}

// showKeyboard opens the keyboard on *text (the caret at its end);
// onChange runs after every edit, onEnter on Search / START.
func (ui *IpodUI) showKeyboard(text *string, onChange, onEnter func()) {
	kb := &Keyboard{anim: newAnim(0), text: text, caret: utf8.RuneCountInString(*text),
		onChange: onChange, onEnter: onEnter, row: 1, col: 4}
	kb.anim.To(1, 260*time.Millisecond, easeCSS, 0)
	ui.keyboard = kb
	ui.dirty = true
}

func (ui *IpodUI) hideKeyboard() {
	if kb := ui.keyboard; kb != nil && !kb.exiting {
		kb.exiting = true
		kb.anim.To(0, 220*time.Millisecond, easeInOut, 0)
		if kb.onChange != nil {
			kb.onChange() // the caller redraws without the caret
		}
	}
	ui.dirty = true
}

// keyboardEditing reports whether the keyboard is up on text.
func (ui *IpodUI) keyboardEditing(text *string) bool {
	kb := ui.keyboard
	return kb != nil && !kb.exiting && kb.text == text
}

// withCaret is text with a caret ("|") where the keyboard's caret is.
func (ui *IpodUI) withCaret(text *string) string {
	kb := ui.keyboard
	r := []rune(*text)
	c := len(r)
	if kb != nil && kb.text == text && kb.caret <= len(r) {
		c = kb.caret
	}
	return string(r[:c]) + "|" + string(r[c:])
}

func (kb *Keyboard) rows() [][]kbDef {
	return append(append([][]kbDef{}, kbLayouts[kb.layer]...), kbBottomRow(kb.layer))
}

// keyRects: the keys of each row, as x offsets (CSS px, from the panel's
// left edge) and widths; every row is centred.
func (kb *Keyboard) keyX(row []kbDef) []float64 {
	total := -kbGap
	for _, k := range row {
		total += k.w + kbGap
	}
	x := (320 - total) / 2
	xs := make([]float64, len(row))
	for i, k := range row {
		xs[i] = x
		x += k.w + kbGap
	}
	return xs
}

// move: dx along the row (wrapping), dy to the nearest key of the row
// above / below.
func (kb *Keyboard) move(dx, dy int) {
	rows := kb.rows()
	if dx != 0 {
		n := len(rows[kb.row])
		kb.col = ((kb.col+dx)%n + n) % n
		return
	}
	nr := kb.row + dy
	if nr < 0 || nr >= len(rows) {
		return
	}
	cur := rows[kb.row]
	xs := kb.keyX(cur)
	cx := xs[kb.col] + cur[kb.col].w/2
	tx := kb.keyX(rows[nr])
	best, bd := 0, math.Inf(1)
	for i, k := range rows[nr] {
		if d := math.Abs(tx[i] + k.w/2 - cx); d < bd {
			best, bd = i, d
		}
	}
	kb.row, kb.col = nr, best
}

// step moves to the next / previous key in reading order (the stick turned
// as a click wheel).
func (kb *Keyboard) step(dir int) {
	rows := kb.rows()
	kb.col += dir
	if kb.col >= len(rows[kb.row]) {
		kb.row, kb.col = (kb.row+1)%len(rows), 0
	} else if kb.col < 0 {
		kb.row = (kb.row - 1 + len(rows)) % len(rows)
		kb.col = len(rows[kb.row]) - 1
	}
}

func (kb *Keyboard) changed() {
	if kb.onChange != nil {
		kb.onChange()
	}
}

func (kb *Keyboard) insert(s string) {
	r := []rune(*kb.text)
	if kb.caret > len(r) {
		kb.caret = len(r)
	}
	*kb.text = string(r[:kb.caret]) + s + string(r[kb.caret:])
	kb.caret += utf8.RuneCountInString(s)
	kb.changed()
}

func (kb *Keyboard) backspace() {
	r := []rune(*kb.text)
	if kb.caret > len(r) {
		kb.caret = len(r)
	}
	if kb.caret == 0 {
		return
	}
	*kb.text = string(r[:kb.caret-1]) + string(r[kb.caret:])
	kb.caret--
	kb.changed()
}

func (kb *Keyboard) moveCaret(d int) {
	n := utf8.RuneCountInString(*kb.text)
	kb.caret = int(math.Max(0, math.Min(float64(n), float64(kb.caret+d))))
	kb.changed()
}

func (kb *Keyboard) toggleLayer() {
	kb.layer = 1 - kb.layer
	rows := kb.rows()
	if kb.col >= len(rows[kb.row]) {
		kb.col = len(rows[kb.row]) - 1
	}
}

func (kb *Keyboard) enter(ui *IpodUI) {
	f := kb.onEnter
	ui.hideKeyboard()
	if f != nil {
		f()
	}
}

// press: the selected key.
func (kb *Keyboard) press(ui *IpodUI) {
	k := kb.rows()[kb.row][kb.col]
	switch k.act {
	case kbChar:
		kb.insert(k.label)
	case kbSpace:
		kb.insert(" ")
	case kbDelete:
		kb.backspace()
	case kbLayer:
		kb.toggleLayer()
	case kbEnter:
		kb.enter(ui)
	}
}

// keyboardKey handles a key press while the keyboard is up; it reports
// whether the key was used.
func (ui *IpodUI) keyboardKey(k Key) bool {
	kb := ui.keyboard
	if kb == nil || kb.exiting {
		return false
	}
	switch k {
	case LEFT:
		kb.move(-1, 0)
	case RIGHT:
		kb.move(1, 0)
	case UP:
		kb.move(0, -1)
	case DOWN:
		kb.move(0, 1)
	case WHEEL_CW:
		kb.step(1)
	case WHEEL_CCW:
		kb.step(-1)
	case X:
		kb.insert(" ")
	case Y:
		kb.toggleLayer()
	case L:
		kb.moveCaret(-1)
	case R:
		kb.moveCaret(1)
	case START:
		kb.enter(ui)
	default:
		return false
	}
	ui.dirty = true
	return true
}

var kbPanel *image.RGBA

// drawKeyboard draws the panel at the bottom of the screen, sliding up.
func (ui *IpodUI) drawKeyboard(dst *image.RGBA, now time.Time) {
	kb := ui.keyboard
	if kb == nil {
		return
	}
	v := kb.anim.Get(now)
	if kb.exiting && !kb.anim.Running() {
		ui.keyboard = nil
		return
	}
	if v <= 0.001 {
		return
	}
	d := DPR
	p := newCanvasCached(&kbPanel, px(320), px(kbPanelH))
	fillVGradient(p, 0, 0, px(320), px(kbPanelH), []GradStop{{0, rgb(74, 78, 85)}, {1, rgb(28, 30, 34)}})
	fillRect(p, 0, 0, px(320), px(1), rgb(18, 19, 22))
	fillRect(p, 0, px(1), px(320), 1, rgba(255, 255, 255, 0.10))
	keyGrad := []GradStop{{0, rgb(255, 255, 255)}, {1, rgb(212, 214, 218)}}
	actGrad := []GradStop{{0, rgb(150, 155, 162)}, {1, rgb(104, 109, 117)}}
	lf := face(W600, 14)
	sf := face(W600, 11)
	for r, row := range kb.rows() {
		xs := kb.keyX(row)
		y := kbPad + float64(r)*(kbKeyH+kbGap)
		for c, k := range row {
			x := xs[c]
			sel := r == kb.row && c == kb.col
			grad, border, tc := keyGrad, rgb(118, 122, 128), rgb(20, 21, 24)
			if k.act != kbChar {
				grad, border, tc = actGrad, rgb(70, 74, 80), colWhite
			}
			if sel {
				grad, border, tc = selGrad, rgb(36, 98, 156), colWhite
			}
			// a soft shadow under each key
			fillRoundedGradient(p, x*d, (y+1)*d, (x+k.w)*d, (y+kbKeyH+1)*d, 5*d, []GradStop{{0, rgba(0, 0, 0, 0.35)}, {1, rgba(0, 0, 0, 0.35)}}, 1)
			fillRoundedGradient(p, x*d, y*d, (x+k.w)*d, (y+kbKeyH)*d, 5*d, grad, 1)
			strokeRounded(p, x*d, y*d, (x+k.w)*d, (y+kbKeyH)*d, 5*d, 1.5, border, 1)
			f := lf
			label := k.label
			if k.act == kbChar {
				label = upperASCII(label)
			} else {
				f = sf
			}
			if k.act == kbDelete {
				f = lf
			}
			if k.act == kbEnter {
				label = "" // a magnifying glass instead of the word
			}
			tw := textWidth(f, label) / DPR
			if k.act == kbEnter {
				tw = 13
			}
			size := 14.0
			if f == sf {
				size = 11
			}
			// the console button that does the same, in a little square
			btn := kbButtonFor(k.act)
			bw := 0.0
			if btn != "" {
				bw = kbBadgeW(btn) + 4
			}
			lx := x + (k.w-tw-bw)/2
			if k.act == kbEnter {
				drawMagnifier(p, lx, y+kbKeyH/2, 13, tc)
			} else {
				drawText(p, f, label, lx*d, (y+kbKeyH/2+lineCapHeight(size)/2)*d, tc)
			}
			if btn != "" {
				kbBadge(p, btn, lx+tw+4, y+kbKeyH/2, sel)
			}
		}
	}
	y := 240 - kbPanelH*v
	// a shadow above the panel
	prof := boxShadowProfile(8, 1)
	for i, a := range prof {
		fillRect(dst, 0, px(y)-1-i, 640, 1, Col{0, 0, 0, a * 0.6 * v})
	}
	blitOpaque(dst, p, 0, px(y))
}

func upperASCII(s string) string {
	b := []byte(s)
	for i, c := range b {
		if c >= 'a' && c <= 'z' {
			b[i] = c - 32
		}
	}
	return string(b)
}

// kbButtonFor: the button that is a shortcut for an action key (as labelled
// on the console: X top, Y left, B bottom).
func kbButtonFor(a kbAction) string {
	switch a {
	case kbSpace:
		return "X"
	case kbDelete:
		return "B"
	case kbLayer:
		return "Y"
	case kbEnter:
		return "START"
	}
	return ""
}

func kbBadgeW(btn string) float64 {
	if len(btn) == 1 {
		return 12
	}
	return textWidth(face(W700, 7), btn)/DPR + 7
}

// kbBadge draws the button's label in a small rounded square (a pill for
// START), centred on cy.
func kbBadge(p *image.RGBA, btn string, x, cy float64, sel bool) {
	d := DPR
	w := kbBadgeW(btn)
	h := 12.0
	y := cy - h/2
	bg, fg := Col{255, 255, 255, 0.92}, rgb(70, 74, 80)
	if sel {
		fg = rgb(36, 98, 156)
	}
	r := 3.0
	if len(btn) > 1 {
		r = h / 2
	}
	fillRoundedGradient(p, x*d, y*d, (x+w)*d, (y+h)*d, r*d, []GradStop{{0, bg}, {1, bg}}, 1)
	size := 9.0
	if len(btn) > 1 {
		size = 7
	}
	f := face(W700, size)
	tw := textWidth(f, btn) / d
	drawText(p, f, btn, (x+(w-tw)/2)*d, (cy+lineCapHeight(size)/2)*d, fg)
}

// drawMagnifier: a magnifying glass s CSS px wide, from x, centred on cy.
func drawMagnifier(dst *image.RGBA, x, cy, s float64, c Col) {
	d := DPR
	dc := gg.NewContextForRGBA(dst)
	r := s * 0.34
	cx, ccy := x+r+1, cy-s*0.12
	dc.SetColor(c8(c))
	dc.SetLineWidth(1.9 * d)
	dc.DrawCircle(cx*d, ccy*d, r*d)
	dc.Stroke()
	dc.SetLineCapRound()
	dc.SetLineWidth(2.4 * d)
	a := math.Pi / 4
	dc.MoveTo((cx+r*math.Cos(a)+0.6)*d, (ccy+r*math.Sin(a)+0.6)*d)
	dc.LineTo((x+s-1)*d, (cy+s*0.5-1)*d)
	dc.Stroke()
}
