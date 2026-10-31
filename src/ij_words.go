package main

// ij_words.go — Five Letters: guess a five-letter English word in six tries.
// After each guess the tiles turn green (right letter, right place), orange
// (in the word, elsewhere) or grey (not in the word); the keyboard keeps the
// best colour of each letter. The game in progress and the statistics are
// kept when you leave.
//
//	arrows  move on the keyboard     A      type the key; with the row full, send
//	Y       send the row             B      delete a letter
//	START   the menu: New Word (at any time: a word left counts as missed),
//	        History (the words played, found or not)
//
// The letters found in place (green) are already in the next row; typing
// fills the other tiles, and deleting leaves them.
//
// The words: the answers are the common five-letter words of SCOWL (Kevin
// Atkinson, size 35, American and British), without plurals and past
// tenses; any word of ENABLE (public domain) is accepted as a guess.

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"image"
	"math"
	"math/rand"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"

	"golang.org/x/image/vector"
)

//go:embed words/answers.txt
var wlAnswersTxt string

//go:embed words/allowed.txt
var wlAllowedTxt string

var wlAnswers []string
var wlValid map[string]bool

func wlLoad() {
	if wlValid != nil {
		return
	}
	wlValid = map[string]bool{}
	for _, w := range strings.Fields(wlAnswersTxt) {
		wlAnswers = append(wlAnswers, strings.ToUpper(w))
		wlValid[strings.ToUpper(w)] = true
	}
	for _, w := range strings.Fields(wlAllowedTxt) {
		wlValid[strings.ToUpper(w)] = true
	}
}

const (
	wlLen   = 5
	wlTries = 6
	wlFlip  = 300 * time.Millisecond // one tile turning
	wlStep  = 180 * time.Millisecond // between the tiles of a row
	wlShake = 420 * time.Millisecond
)

// letter states
const (
	wlNone = iota
	wlAbsent
	wlPresent
	wlCorrect
)

type wlStats struct {
	Played    int          `json:"played"`
	Wins      int          `json:"wins"`
	Streak    int          `json:"streak"`
	MaxStreak int          `json:"maxStreak"`
	Dist      [wlTries]int `json:"dist"`
	History   []wlPast     `json:"history,omitempty"` // the words played, newest last
}

// wlPast: a word played: found (in how many tries) or not, and when.
type wlPast struct {
	Word  string `json:"word"`
	Won   bool   `json:"won"`
	Tries int    `json:"tries"`          // found in; or how many were tried
	Gave  bool   `json:"gave,omitempty"` // a new word asked for before the end
	When  int64  `json:"when"`
}

const wlHistoryMax = 300

type wlSave struct {
	Answer  string   `json:"answer"`
	Guesses []string `json:"guesses"`
	Typed   string   `json:"typed"`
	Stats   wlStats  `json:"stats"`
}

type WordGame struct {
	answer  string
	guesses []string
	typed   string
	stats   wlStats
	over    bool
	won     bool
	kr, kc  int // the keyboard cursor
	rng     *rand.Rand
	// animations
	revealAt time.Time // the last row started turning
	shakeAt  time.Time
	typedAt  time.Time
	lastAt   int // the tile typed last (its pop)
	toast    string
	toastAt  time.Time
	bg       *image.RGBA
	ky0      float64 // the keyboard's top
}

var wlRows = []string{"QWERTYUIOP", "ASDFGHJKL", "+ZXCVBNM-"} // + ENTER, - delete

func wlPath() string { return dataPath("fiveletters.json") }

func (ui *IpodUI) openWords() {
	wlLoad()
	g := &WordGame{rng: rand.New(rand.NewSource(time.Now().UnixNano())), kr: 0, kc: 0}
	if !g.load() {
		g.newGame()
	}
	v := &View{ID: "fiveletters", Title: "Five Letters", Kind: KindFull, Custom: g}
	v.onClose = g.save
	ui.showView(v, "")
}

func (g *WordGame) load() bool {
	data, err := os.ReadFile(wlPath())
	if err != nil {
		return false
	}
	var s wlSave
	if json.Unmarshal(data, &s) != nil {
		return false
	}
	g.stats = s.Stats
	if len(s.Answer) != wlLen || !wlValid[s.Answer] {
		return false
	}
	g.answer, g.guesses = s.Answer, s.Guesses
	g.blankRow()
	// the letters typed (blanks for the empty tiles; before 0.86 they were
	// kept in a row from the left: they go into the free tiles)
	if len(s.Typed) == wlLen {
		b := []byte(g.typed)
		for i := 0; i < wlLen; i++ {
			if b[i] == ' ' && s.Typed[i] >= 'A' && s.Typed[i] <= 'Z' {
				b[i] = s.Typed[i]
			}
		}
		g.typed = string(b)
	} else {
		for i := 0; i < len(s.Typed); i++ {
			g.typeLetter(s.Typed[i])
		}
	}
	g.settle()
	if g.over {
		return false // a finished game: a new word
	}
	return true
}

func (g *WordGame) save() {
	s := wlSave{Answer: g.answer, Guesses: g.guesses, Typed: g.typed, Stats: g.stats}
	if g.over {
		s.Answer, s.Guesses, s.Typed = "", nil, ""
	}
	if data, err := json.Marshal(s); err == nil {
		writeFileAtomic(wlPath(), data)
	}
}

func (g *WordGame) newGame() {
	prev := g.answer
	for {
		g.answer = wlAnswers[g.rng.Intn(len(wlAnswers))]
		if g.answer != prev {
			break
		}
	}
	g.guesses, g.over, g.won = nil, false, false
	g.revealAt, g.toast = time.Time{}, ""
	g.blankRow()
}

// known: the letters found in place (green) in the guesses so far: the
// next row starts with them (0: not known). A letter found twice in the
// word is there twice only where both were found.
func (g *WordGame) known() [wlLen]byte {
	var k [wlLen]byte
	for _, w := range g.guesses {
		if len(w) != wlLen {
			continue
		}
		for i := 0; i < wlLen; i++ {
			if w[i] == g.answer[i] {
				k[i] = w[i]
			}
		}
	}
	return k
}

// blankRow: the row being typed, empty but for the letters known (blanks
// for the tiles still to type).
func (g *WordGame) blankRow() {
	k := g.known()
	b := []byte(strings.Repeat(" ", wlLen))
	for i, c := range k {
		if c != 0 {
			b[i] = c
		}
	}
	g.typed = string(b)
}

// rowFull: every tile of the row has its letter.
func (g *WordGame) rowFull() bool { return !strings.Contains(g.typed, " ") }

// typeLetter: ch in the first free tile.
func (g *WordGame) typeLetter(ch byte) {
	if i := strings.IndexByte(g.typed, ' '); i >= 0 {
		b := []byte(g.typed)
		b[i] = ch
		g.typed = string(b)
		g.typedAt, g.lastAt = time.Now(), i
	}
}

// wlRecord: a word ends (found, missed, or given up): the statistics and
// the history.
func (g *WordGame) record(won, gave bool) {
	g.stats.Played++
	if won {
		g.stats.Wins++
		g.stats.Streak++
		g.stats.MaxStreak = max(g.stats.MaxStreak, g.stats.Streak)
		g.stats.Dist[len(g.guesses)-1]++
	} else {
		g.stats.Streak = 0
	}
	g.stats.History = append(g.stats.History, wlPast{Word: g.answer, Won: won, Tries: len(g.guesses), Gave: gave, When: time.Now().Unix()})
	if n := len(g.stats.History); n > wlHistoryMax {
		g.stats.History = g.stats.History[n-wlHistoryMax:]
	}
}

// giveUp: a new word before the end: this one counts as missed, and its
// answer is shown.
func (g *WordGame) giveUp() {
	if !g.over && len(g.guesses) > 0 {
		g.record(false, true)
	}
	word := g.answer
	g.newGame()
	g.say("It was " + word)
	g.save()
}

// settle: over / won from the guesses.
func (g *WordGame) settle() {
	g.won = len(g.guesses) > 0 && g.guesses[len(g.guesses)-1] == g.answer
	g.over = g.won || len(g.guesses) >= wlTries
}

// score: the colours of a guess (two passes, so repeated letters are right).
func wlScore(guess, answer string) [wlLen]int {
	var res [wlLen]int
	var left [26]int
	for i := 0; i < wlLen; i++ {
		if guess[i] == answer[i] {
			res[i] = wlCorrect
		} else {
			left[answer[i]-'A']++
		}
	}
	for i := 0; i < wlLen; i++ {
		if res[i] == wlCorrect {
			continue
		}
		if c := guess[i] - 'A'; left[c] > 0 {
			res[i] = wlPresent
			left[c]--
		} else {
			res[i] = wlAbsent
		}
	}
	return res
}

// revealing: the last row is still turning.
func (g *WordGame) revealing(now time.Time) bool {
	return !g.revealAt.IsZero() && now.Sub(g.revealAt) < wlStep*(wlLen-1)+wlFlip
}

// keyStates: the best colour of each letter, from the rows already shown.
func (g *WordGame) keyStates(now time.Time) [26]int {
	var st [26]int
	n := len(g.guesses)
	if g.revealing(now) {
		n-- // (the keyboard changes once the row has turned)
	}
	for _, w := range g.guesses[:max(n, 0)] {
		sc := wlScore(w, g.answer)
		for i := 0; i < wlLen; i++ {
			c := w[i] - 'A'
			st[c] = max(st[c], sc[i])
		}
	}
	return st
}

func (g *WordGame) say(s string) {
	g.toast, g.toastAt = s, time.Now()
}

func (g *WordGame) typeKey(ch byte) {
	if g.over || g.revealing(time.Now()) {
		return
	}
	switch ch {
	case '+':
		g.submit()
	case '-':
		g.del()
	default:
		g.typeLetter(ch)
	}
}

// del: the last letter typed goes (the known ones stay).
func (g *WordGame) del() {
	if g.over {
		return
	}
	k := g.known()
	b := []byte(g.typed)
	for i := wlLen - 1; i >= 0; i-- {
		if k[i] == 0 && b[i] != ' ' {
			b[i] = ' '
			g.typed = string(b)
			return
		}
	}
}

func (g *WordGame) submit() {
	if g.over || g.revealing(time.Now()) {
		return
	}
	if !g.rowFull() {
		g.say("Not enough letters")
		g.shakeAt = time.Now()
		return
	}
	if !wlValid[g.typed] {
		g.say("Not in the word list")
		g.shakeAt = time.Now()
		return
	}
	g.guesses = append(g.guesses, g.typed)
	g.revealAt = time.Now()
	g.settle()
	g.blankRow() // (with the letters found in place)
	if g.over {
		g.record(g.won, false)
	}
	g.save() // (kept even if the console is switched off)
}

// wordsHistoryView: START → History: the words played, newest first.
func (ui *IpodUI) wordsHistoryView(g *WordGame) *View {
	v := &View{ID: "fiveletters-history", Title: "History", Kind: KindFull, Empty: "No words played yet"}
	v.Build = func() []Item {
		h := g.stats.History
		var items []Item
		if n := len(h); n > 0 {
			found := 0
			for _, p := range h {
				if p.Won {
					found++
				}
			}
			items = append(items, Item{Header: true, Label: fmt.Sprintf("%d %s · %d found", n, pluralize(n, "word", "words"), found)})
		}
		for i := len(h) - 1; i >= 0; i-- {
			p := h[i]
			when := time.Unix(p.When, 0).Format("2 Jan")
			sub := ""
			switch {
			case p.Won && p.Tries == 1:
				sub = "Found at the first try · " + when
			case p.Won:
				sub = fmt.Sprintf("Found in %d tries · %s", p.Tries, when)
			case p.Gave:
				sub = fmt.Sprintf("Missed · a new word after %d %s · %s", p.Tries, pluralize(p.Tries, "try", "tries"), when)
			default:
				sub = "Missed · " + when
			}
			items = append(items, Item{Label: p.Word[:1] + strings.ToLower(p.Word[1:]), Sublabel: sub, NoArrow: true})
		}
		return items
	}
	v.Items = v.Build()
	if len(v.Items) > 1 {
		v.Index = 1
	}
	return v
}

// ---- keys ----

func (g *WordGame) OnKey(ui *IpodUI, k Key) bool {
	switch k {
	case UP, DOWN:
		d := 1
		if k == UP {
			d = -1
		}
		nr := (g.kr + d + len(wlRows)) % len(wlRows)
		// keep about the same place across rows of different lengths
		x := g.keyX(g.kr, g.kc)
		g.kr, g.kc = nr, g.keyAt(nr, x)
	case LEFT:
		g.kc = (g.kc - 1 + len(wlRows[g.kr])) % len(wlRows[g.kr])
	case RIGHT:
		g.kc = (g.kc + 1) % len(wlRows[g.kr])
	case Y: // sends the row (B deletes)
		g.submit()
	case START: // the game's menu (every game: START)
		newWord := SheetOption{Label: "New Word", OnSelect: func() { g.newGame(); g.save(); ui.dirty = true }}
		if !g.over && len(g.guesses) > 0 {
			// stuck: another word (this one counts as missed)
			newWord = SheetOption{Label: "New Word (this one is missed)", OnSelect: func() { g.giveUp(); ui.dirty = true }}
		}
		ui.gameMenu("Five Letters", newWord,
			SheetOption{Label: "History", OnSelect: func() { ui.showView(ui.wordsHistoryView(g), "") }})
	default:
		return false
	}
	ui.dirty = true
	return true
}

// OnCenter: A types the key, as on a keyboard; with the row full, A on a
// letter sends it.
func (g *WordGame) OnCenter(ui *IpodUI) {
	ch := wlRows[g.kr][g.kc]
	switch {
	case g.over && !g.revealing(time.Now()):
		g.newGame()
	case g.rowFull() && ch != '-':
		g.submit()
	default:
		g.typeKey(ch)
	}
	ui.dirty = true
}

// OnMenu: B deletes a letter (it doesn't leave: START → Quit does).
func (g *WordGame) OnMenu(ui *IpodUI) {
	g.del()
	ui.dirty = true
}

func (g *WordGame) OnScroll(ui *IpodUI, dir int, velocity float64) {}

func (g *WordGame) Animating(now time.Time) bool {
	return g.revealing(now) || now.Sub(g.shakeAt) < wlShake ||
		now.Sub(g.typedAt) < 150*time.Millisecond || now.Sub(g.toastAt) < 1800*time.Millisecond ||
		(g.over && now.Sub(g.revealAt) < wlStep*(wlLen-1)+wlFlip+600*time.Millisecond)
}

// ---- layout (pixels, 640 × 480) ----

const (
	wlTile = 36.0
	wlGap  = 5.0
	wlY0   = 10.0
	wlKeyW = 56.0
	wlKeyH = 42.0
	wlKGap = 6.0
	wlWide = 86.0 // ENTER and delete
)

func wlBoardX0() float64 { return (640 - (wlLen*wlTile + (wlLen-1)*wlGap)) / 2 }

// keyRect: where key c of row r is.
func (g *WordGame) keyRect(r, c int) (x0, y0, x1, y1 float64) {
	row := wlRows[r]
	w := func(i int) float64 {
		if row[i] == '+' || row[i] == '-' {
			return wlWide
		}
		return wlKeyW
	}
	total := 0.0
	for i := range row {
		total += w(i)
	}
	total += float64(len(row)-1) * wlKGap
	x := (640 - total) / 2
	for i := 0; i < c; i++ {
		x += w(i) + wlKGap
	}
	y := g.ky0 + float64(r)*(wlKeyH+6)
	return x, y, x + w(c), y + wlKeyH
}

func (g *WordGame) keyX(r, c int) float64 {
	x0, _, x1, _ := g.keyRect(r, c)
	return (x0 + x1) / 2
}

func (g *WordGame) keyAt(r int, x float64) int {
	best, bd := 0, math.Inf(1)
	for c := range wlRows[r] {
		if d := math.Abs(g.keyX(r, c) - x); d < bd {
			best, bd = c, d
		}
	}
	return best
}

// ---- drawing ----

func wlColors(st int) (Col, Col, Col) {
	switch st {
	case wlCorrect:
		return Col{118, 206, 120, 1}, Col{40, 150, 62, 1}, colWhite
	case wlPresent:
		return Col{255, 196, 92, 1}, Col{230, 132, 16, 1}, colWhite
	case wlAbsent:
		return Col{150, 158, 172, 1}, Col{104, 112, 128, 1}, colWhite
	}
	return Col{255, 255, 255, 1}, Col{232, 236, 242, 1}, Col{40, 44, 52, 1}
}

func wlTileDraw(dst *image.RGBA, cx, cy, hw, hh float64, st int, ch string, size float64) {
	if hh < 0.5 {
		return
	}
	top, bot, tc := wlColors(st)
	r := math.Min(7, hh)
	fillRoundedGradient(dst, cx-hw, cy-hh, cx+hw, cy+hh, r, []GradStop{{0, top}, {1, bot}}, 1)
	if st == wlNone {
		strokeRounded(dst, cx-hw, cy-hh, cx+hw, cy+hh, r, 2, Col{150, 160, 176, 1}, 1)
	} else {
		fillRoundedGradient(dst, cx-hw+2, cy-hh+2, cx+hw-2, cy, math.Max(0, r-1),
			[]GradStop{{0, Col{255, 255, 255, 0.42}}, {1, Col{255, 255, 255, 0.05}}}, 1)
	}
	if ch != "" && hh > hw*0.45 {
		f := face(W700, size)
		_, asc := lineBox(size)
		drawText(dst, f, ch, cx-textWidth(f, ch)/2, cy+asc*DPR/2-2, tc)
	}
}

func (g *WordGame) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	if g.bg == nil || g.bg.Bounds().Dx() != W || g.bg.Bounds().Dy() != H {
		g.bg = newCanvas(W, H)
		fillVGradient(g.bg, 0, 0, W, H, []GradStop{{0, Col{238, 241, 245, 1}}, {1, Col{206, 212, 221, 1}}})
	}
	blitOpaque(dst, g.bg, 0, 0)
	g.ky0 = float64(H) - 3*wlKeyH - 2*6 - 8

	// the board
	x0 := wlBoardX0()
	ht := wlTile / 2
	for r := 0; r < wlTries; r++ {
		cy := wlY0 + float64(r)*(wlTile+wlGap) + ht
		word := ""
		var sc [wlLen]int
		shown := r < len(g.guesses)
		if shown {
			word = g.guesses[r]
			sc = wlScore(word, g.answer)
		} else if r == len(g.guesses) && !g.over {
			word = g.typed
		}
		dx := 0.0
		if r == len(g.guesses) && now.Sub(g.shakeAt) < wlShake {
			p := float64(now.Sub(g.shakeAt)) / float64(wlShake)
			dx = 10 * math.Sin(p*math.Pi*6) * (1 - p)
		}
		for c := 0; c < wlLen; c++ {
			cx := x0 + float64(c)*(wlTile+wlGap) + ht + dx
			ch := ""
			if c < len(word) && word[c] != ' ' {
				ch = word[c : c+1]
			}
			st, hh := wlNone, ht
			if shown {
				st = sc[c]
				if r == len(g.guesses)-1 && !g.revealAt.IsZero() {
					el := now.Sub(g.revealAt) - time.Duration(c)*wlStep
					switch {
					case el < 0:
						st = wlNone
					case el < wlFlip:
						p := float64(el) / float64(wlFlip)
						hh = ht * math.Abs(math.Cos(p*math.Pi))
						if p < 0.5 {
							st = wlNone
						}
					}
				}
			}
			hw := ht
			if !shown && ch != "" && c == g.lastAt && now.Sub(g.typedAt) < 150*time.Millisecond {
				s := 1 + 0.1*math.Sin(math.Pi*float64(now.Sub(g.typedAt))/float64(150*time.Millisecond))
				hw, hh = ht*s, ht*s
			}
			if st == wlNone && ch == "" {
				// an empty square; the one the next letter goes in, blue
				// (the letters found in place are skipped)
				if r == len(g.guesses) && !g.over && c == strings.IndexByte(g.typed, ' ') {
					strokeRounded(dst, cx-hw, cy-hh, cx+hw, cy+hh, 7, 3, Col{40, 130, 230, 1}, 1)
					continue
				}
				strokeRounded(dst, cx-hw, cy-hh, cx+hw, cy+hh, 7, 2, Col{186, 194, 206, 1}, 1)
				continue
			}
			wlTileDraw(dst, cx, cy, hw, hh, st, ch, 13)
		}
	}

	// the keyboard
	ks := g.keyStates(now)
	for r, row := range wlRows {
		for c := range row {
			kx0, ky0, kx1, ky1 := g.keyRect(r, c)
			ch := row[c]
			st := wlNone
			label := string(ch)
			if ch >= 'A' && ch <= 'Z' {
				st = ks[ch-'A']
			}
			top, bot, tc := wlColors(st)
			if st == wlNone {
				top, bot = Col{252, 253, 255, 1}, Col{222, 227, 235, 1}
			}
			sel := r == g.kr && c == g.kc
			if sel && st == wlNone {
				top, bot, tc = Col{88, 172, 248, 1}, Col{28, 110, 214, 1}, colWhite
			}
			fillRoundedGradient(dst, kx0, ky0, kx1, ky1, 8, []GradStop{{0, top}, {1, bot}}, 1)
			if st == wlNone && !sel {
				strokeRounded(dst, kx0, ky0, kx1, ky1, 8, 1, Col{170, 178, 192, 1}, 1)
			}
			fillRoundedGradient(dst, kx0+2, ky0+2, kx1-2, (ky0+ky1)/2, 7,
				[]GradStop{{0, Col{255, 255, 255, 0.4}}, {1, Col{255, 255, 255, 0.04}}}, 1)
			if sel && st != wlNone {
				// a coloured key keeps its colour: the cursor is a ring
				strokeRounded(dst, kx0-3, ky0-3, kx1+3, ky1+3, 10, 3, Col{28, 120, 230, 1}, 1)
			}
			size := 13.0
			switch ch {
			case '+':
				label, size = "ENTER", 10
			case '-':
				label, size = "⌫", 13
			}
			f := face(W700, size)
			_, asc := lineBox(size)
			cx, cy := (kx0+kx1)/2, (ky0+ky1)/2
			if ch == '-' {
				wlBackspace(dst, cx, cy, tc)
				continue
			}
			drawText(dst, f, label, cx-textWidth(f, label)/2, cy+asc*DPR/2-2, tc)
		}
	}

	// the hint at the side of the board
	hf := face(W600, 10)
	_, ha := lineBox(10)
	hints := []string{"[A] type", "[Y] send", "[B] delete", "[START] menu"}
	if g.over {
		hints = []string{"[A] new word", "", "", "[START] menu"}
	}
	for i, l := range hints {
		drawText(dst, hf, l, 452, 40+float64(i)*28+ha*DPR/2, Col{110, 118, 132, 1})
	}
	// the statistics, at the left
	sf := face(W700, 20)
	lf := face(W600, 9)
	_, sa := lineBox(20)
	_, la := lineBox(9)
	stat := func(y float64, n string, label string) {
		cx := 92.0
		drawText(dst, sf, n, cx-textWidth(sf, n)/2, y+sa*DPR/2, Col{58, 64, 76, 1})
		drawText(dst, lf, label, cx-textWidth(lf, label)/2, y+30+la*DPR/2, Col{110, 118, 132, 1})
	}
	pct := 0
	if g.stats.Played > 0 {
		pct = g.stats.Wins * 100 / g.stats.Played
	}
	stat(44, strconv.Itoa(g.stats.Played), "PLAYED")
	stat(124, strconv.Itoa(pct)+"%", "WON")
	stat(204, strconv.Itoa(g.stats.Streak), "STREAK")

	// the end, once the row has turned
	done := g.over && !g.revealing(now)
	if done {
		msg := []string{"Genius!", "Magnificent!", "Impressive!", "Splendid!", "Great!", "Phew!"}
		title := "The word was " + g.answer
		if g.won {
			title = msg[len(g.guesses)-1]
		}
		g.banner(dst, title, now.Sub(g.revealAt)-(wlStep*(wlLen-1)+wlFlip), len(g.guesses))
	} else if g.toast != "" && now.Sub(g.toastAt) < 1800*time.Millisecond {
		g.banner(dst, g.toast, now.Sub(g.toastAt), len(g.guesses)+1)
	}
}

// banner: a dark pill over the board.
func (g *WordGame) banner(dst *image.RGBA, s string, el time.Duration, row int) {
	a := math.Min(1, math.Max(0, float64(el)/float64(150*time.Millisecond)))
	if a <= 0 {
		return
	}
	f := face(W700, 14)
	_, asc := lineBox(14)
	w := textWidth(f, s) + 44
	cx, cy := 320.0, wlY0+float64(min(row, wlTries-1))*(wlTile+wlGap)+wlTile/2
	fillRoundedGradient(dst, cx-w/2, cy-20, cx+w/2, cy+20, 20,
		[]GradStop{{0, Col{62, 68, 80, 0.94}}, {1, Col{30, 34, 42, 0.94}}}, a)
	drawText(dst, f, s, cx-textWidth(f, s)/2, cy+asc*DPR/2-2, Col{255, 255, 255, a})
}

// wlBackspace: the delete key's arrow.
func wlBackspace(dst *image.RGBA, cx, cy float64, c Col) {
	w, h := 30.0, 20.0
	x0, x1 := cx-w/2, cx+w/2
	pts := [][2]float64{{x0, cy}, {x0 + 9, cy - h/2}, {x1, cy - h/2}, {x1, cy + h/2}, {x0 + 9, cy + h/2}}
	strokePoly(dst, pts, 2.4, c)
	strokeLine(dst, cx-2, cy-5, cx+8, cy+5, 2.4, c)
	strokeLine(dst, cx+8, cy-5, cx-2, cy+5, 2.4, c)
}

// wordsIcon: the preview's picture, a row being solved.
var wlIconImg *image.RGBA

func wordsIcon() *image.RGBA {
	if wlIconImg != nil {
		return wlIconImg
	}
	const S = 192.0
	img := newCanvas(int(S), int(S))
	fillRoundedGradient(img, 12, 12, S-12, S-12, 36, []GradStop{{0, Col{98, 108, 126, 1}}, {1, Col{44, 50, 64, 1}}}, 1)
	rows := []struct {
		w  string
		st [3]int
	}{
		{"TEA", [3]int{wlAbsent, wlAbsent, wlPresent}},
		{"OAK", [3]int{wlAbsent, wlCorrect, wlAbsent}},
		{"PAW", [3]int{wlCorrect, wlCorrect, wlCorrect}},
	}
	t, gap := 38.0, 8.0
	x0 := (S - 3*t - 2*gap) / 2
	for r, row := range rows {
		for c := 0; c < 3; c++ {
			cx := x0 + float64(c)*(t+gap) + t/2
			cy := x0 + float64(r)*(t+gap) + t/2
			wlTileDraw(img, cx, cy, t/2, t/2, row.st[c], row.w[c:c+1], 13)
		}
	}
	fillRoundedGradient(img, 15, 15, S-15, S/2, 32, []GradStop{{0, Col{255, 255, 255, 0.3}}, {1, Col{255, 255, 255, 0.03}}}, 1)
	wlIconImg = img
	return img
}

// aaPoly: a filled polygon, anti-aliased.
func aaPoly(dst *image.RGBA, pts [][2]float64, c Col) {
	r := polyBox(dst, pts)
	if r.Empty() {
		return
	}
	ras := aaRaster(r)
	defer aaRasters.Put(ras)
	ox, oy := float64(r.Min.X), float64(r.Min.Y)
	ras.MoveTo(float32(pts[0][0]-ox), float32(pts[0][1]-oy))
	for _, p := range pts[1:] {
		ras.LineTo(float32(p[0]-ox), float32(p[1]-oy))
	}
	ras.ClosePath()
	ras.Draw(dst, r.Add(dst.Bounds().Min), image.NewUniform(c8(c)), image.Point{})
}

// aaRasters: the rasterizers, reused: their buffers are as big as the area
// they draw, and a new one over the whole screen was 2.4 MB for a gloss.
var aaRasters = sync.Pool{New: func() any { return new(vector.Rasterizer) }}

// aaRaster: a rasterizer for the box r (its corner at r.Min), at least
// 513 px wide so that it keeps to floating-point maths: vector's fixed-point
// one, for areas up to 512 px, is up to 17% off on thin slanted edges.
func aaRaster(r image.Rectangle) *vector.Rasterizer {
	w := r.Dx()
	if w <= 512 && r.Dy() <= 512 {
		w = 513
	}
	ras := aaRasters.Get().(*vector.Rasterizer)
	ras.Reset(w, r.Dy())
	return ras
}

// polyBox: the pixels pts can cover, within dst (from its corner, like the
// points).
func polyBox(dst *image.RGBA, pts [][2]float64) image.Rectangle {
	x0, y0, x1, y1 := pts[0][0], pts[0][1], pts[0][0], pts[0][1]
	for _, p := range pts[1:] {
		x0, y0 = math.Min(x0, p[0]), math.Min(y0, p[1])
		x1, y1 = math.Max(x1, p[0]), math.Max(y1, p[1])
	}
	b := dst.Bounds()
	r := image.Rect(int(math.Floor(x0)), int(math.Floor(y0)), int(math.Ceil(x1)), int(math.Ceil(y1)))
	return r.Intersect(image.Rect(0, 0, b.Dx(), b.Dy()))
}

// strokeLine: a segment w pixels wide, anti-aliased.
func strokeLine(dst *image.RGBA, x1, y1, x2, y2, w float64, c Col) {
	dx, dy := x2-x1, y2-y1
	l := math.Hypot(dx, dy)
	if l < 0.01 {
		return
	}
	ux, uy := dx/l, dy/l
	x1, y1, x2, y2 = x1-ux*w/2, y1-uy*w/2, x2+ux*w/2, y2+uy*w/2
	nx, ny := -uy*w/2, ux*w/2
	aaPoly(dst, [][2]float64{{x1 + nx, y1 + ny}, {x2 + nx, y2 + ny}, {x2 - nx, y2 - ny}, {x1 - nx, y1 - ny}}, c)
}

// strokePoly: a closed outline.
func strokePoly(dst *image.RGBA, pts [][2]float64, w float64, c Col) {
	for i := range pts {
		q := pts[(i+1)%len(pts)]
		strokeLine(dst, pts[i][0], pts[i][1], q[0], q[1], w, c)
	}
}
