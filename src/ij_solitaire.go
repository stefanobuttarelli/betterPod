package main

// ij_solitaire.go — Solitaire: Klondike on the iPod screen. The wheel
// moves over the places of the table, the centre button picks up and puts
// down cards, Menu cancels or pauses, a long press undoes.

import (
	"encoding/json"
	"fmt"
	"image"
	"math"
	"os"
	"strconv"
	"strings"
	"time"

	"golang.org/x/image/font"
)

type Pos struct{ X, Y float64 }

var (
	solMenuItems = []string{"Resume", "Undo", "Redeal", "Quit"}
	solWinItems  = []string{"New Game", "Quit"}
	solLostItems = []string{"Redeal", "Quit"}
)

// solLayout: card size and the positions of the piles (CSS px).
type solLayout struct {
	cw, ch      float64
	topY, tabY  float64
	colSp, colX float64
	stockX      float64
	wasteX      float64
	fX, fSp     float64
	baseOv      float64
	downOv      float64
	maxTabH     float64
}

// Solitaire is a game: state, selection, animations, undo and menus.
type Solitaire struct {
	ui                          *IpodUI
	w, h, s                     float64
	l                           solLayout
	state                       SolState
	sel                         Selection
	anims                       solAnims
	undo                        []SolState
	menuIx, winIx, lostIx, wins int
	cursorAnim                  *struct {
		from, to Pos
		start    time.Time
		dur      time.Duration
	}
	sprites map[int]*image.RGBA
	bg      *image.RGBA
	onQuit  func()
}

func (ui *IpodUI) openSolitaire() {
	g := newSolitaire(ui, 320, 220)
	v := &View{ID: "solitaireGame", Title: "Solitaire", Kind: KindFull, Custom: g, DisableLongPress: true}
	g.onQuit = func() { ui.hideView() }
	ui.showView(v, "")
}

func newSolitaire(ui *IpodUI, w, h float64) *Solitaire {
	g := &Solitaire{ui: ui, w: w, h: h, s: h / 232, sprites: map[int]*image.RGBA{}}
	g.computeLayout()
	loaded := g.load()
	if !loaded {
		g.state = solDeal()
	}
	g.wins = g.loadWins()
	if loaded {
		g.sel.build(g.state)
	} else {
		g.animateDeal()
	}
	return g
}

func (g *Solitaire) computeLayout() {
	hs := g.h / 232
	ch := math.Floor(g.h * 0.22)
	cw := math.Floor(ch / 1.4)
	sp := math.Floor(cw * 0.2)
	content := 7*cw + 6*sp
	marginX := math.Floor((g.w - content) / 2)
	colX := math.Max(marginX, sp)
	topY := math.Floor(4 * hs)
	tabY := topY + ch + math.Floor(8*hs)
	right := colX + content
	g.l = solLayout{
		cw: cw, ch: ch, topY: topY, tabY: tabY, colSp: sp, colX: colX,
		stockX: colX, wasteX: colX + cw + sp,
		fX: right - 4*cw - 3*sp, fSp: cw + sp,
		baseOv: math.Floor(14 * hs), downOv: math.Floor(8 * hs),
		maxTabH: g.h - tabY - math.Floor(2*hs),
	}
}

func (g *Solitaire) overlaps(col []SCard) []float64 {
	l := g.l
	if len(col) <= 1 {
		return nil
	}
	ov := make([]float64, len(col)-1)
	total := 0.0
	for i := 0; i < len(col)-1; i++ {
		if col[i].FaceUp {
			ov[i] = l.baseOv
		} else {
			ov[i] = l.downOv
		}
		total += ov[i]
	}
	avail := l.maxTabH - l.ch
	if total > avail && avail > 0 {
		r := avail / total
		for i := range ov {
			ov[i] = math.Max(4*g.s, math.Floor(ov[i]*r))
		}
	}
	return ov
}

func (g *Solitaire) cardY(col []SCard, idx int) float64 {
	y := g.l.tabY
	ov := g.overlaps(col)
	for i := 0; i < idx; i++ {
		if i < len(ov) {
			y += ov[i]
		} else {
			y += g.l.baseOv
		}
	}
	return y
}

func (g *Solitaire) targetPos(s SolState, t Target) Pos {
	l := g.l
	switch t.T {
	case TStock:
		return Pos{l.stockX, l.topY}
	case TWaste:
		return Pos{l.wasteX, l.topY}
	case TFoundation:
		return Pos{l.fX + float64(t.Index)*l.fSp, l.topY}
	case TTableau:
		return Pos{l.colX + float64(t.Column)*(l.cw+l.colSp), g.cardY(s.Tableau[t.Column], t.CardIndex)}
	}
	return Pos{l.colX + float64(t.Column)*(l.cw+l.colSp), l.tabY}
}

// cursorCenter is where the hand points for a target.
func (g *Solitaire) cursorCenter(s SolState, sel *Selection, t Target) Pos {
	l := g.l
	p := g.targetPos(s, t)
	if sel.phase == PhBrowsing && t.T == TTableau {
		col := s.Tableau[t.Column]
		bottom := g.cardY(col, len(col)-1) + l.ch
		return Pos{p.X + l.cw/2, p.Y + (bottom-p.Y)/2}
	}
	if sel.phase == PhHolding && t.T == TTableau {
		col := s.Tableau[t.Column]
		if len(col) > 0 {
			drop := g.cardY(col, len(col)-1) + l.baseOv
			return Pos{p.X + l.cw/2, drop + l.ch/2}
		}
	}
	return Pos{p.X + l.cw/2, p.Y + l.ch/2}
}

func (g *Solitaire) animateCursor(from, to Pos, dur time.Duration) {
	g.cursorAnim = &struct {
		from, to Pos
		start    time.Time
		dur      time.Duration
	}{from, to, time.Now(), dur}
}

// Animating: the game redraws every frame (cards, cursor and menus
// animate).
func (g *Solitaire) Animating(now time.Time) bool { return true }

func (g *Solitaire) OnScroll(ui *IpodUI, dir int, velocity float64) {
	if g.anims.animating() {
		return
	}
	clamp := func(i, n int) int { return int(math.Max(0, math.Min(float64(n-1), float64(i)))) }
	switch g.sel.phase {
	case PhWon:
		g.winIx = clamp(g.winIx+dir, len(solWinItems))
	case PhLost:
		g.lostIx = clamp(g.lostIx+dir, len(solLostItems))
	case PhMenu:
		g.menuIx = clamp(g.menuIx+dir, len(solMenuItems))
	default:
		g.sel.scroll(dir)
	}
}

func (g *Solitaire) OnCenter(ui *IpodUI) {
	if g.anims.animating() {
		return
	}
	switch g.sel.phase {
	case PhWon:
		if g.winIx == 0 {
			g.redeal()
		} else {
			g.clearSave()
			g.onQuit()
		}
		return
	case PhLost:
		if g.lostIx == 0 {
			g.redeal()
		} else {
			g.clearSave()
			g.onQuit()
		}
		return
	case PhMenu:
		switch g.menuIx {
		case 0:
			g.sel.phase = PhBrowsing
		case 1:
			g.sel.phase = PhBrowsing
			g.OnLongPress(ui)
		case 2:
			g.redeal()
		case 3:
			g.onQuit()
		}
		return
	case PhHolding:
		g.placeCards()
		return
	}
	t := g.sel.current()
	if t == nil {
		return
	}
	if t.T == TStock {
		g.stockDraw()
		return
	}
	if t.T == TTableauEmpty || !canPickUp(g.state, *t) {
		return
	}
	held := cardsFromTarget(g.state, *t)
	if len(held) == 0 {
		return
	}
	valid := validDropTargets(g.state, held, *t)
	from := g.cursorCenter(g.state, &g.sel, *t)
	g.sel.enterHolding(held, *t, valid)
	if first := g.sel.current(); first != nil && g.sel.phase == PhHolding {
		g.animateCursor(from, g.cursorCenter(g.state, &g.sel, *first), 150*time.Millisecond)
	}
}

func (g *Solitaire) stockDraw() {
	old := g.state
	g.pushUndo()
	g.state = drawFromStock(g.state)
	g.sel.rebuildAndClamp(g.state)
	if len(g.state.Waste) > len(old.Waste) {
		c := g.state.Waste[len(g.state.Waste)-1]
		g.anims.queue = append(g.anims.queue, cardAnim{
			cards: []SCard{c}, from: g.targetPos(old, Target{T: TStock}), to: g.targetPos(g.state, Target{T: TWaste}),
			start: time.Now(), dur: 150 * time.Millisecond,
		})
	}
	g.save()
	g.scheduleAutoMoves(false, nil)
}

func (g *Solitaire) placeCards() {
	drop := g.sel.current()
	if drop == nil || g.sel.heldFrom == nil {
		return
	}
	to := *drop
	from := *g.sel.heldFrom
	held := g.sel.held
	res, ok := moveCards(g.state, from, to)
	if !ok {
		return
	}
	fromPos := g.targetPos(g.state, from)
	old := g.state
	g.pushUndo()
	g.state = res
	g.sel.exitHolding(g.state)
	// the moved cards fly from where they were to their new place
	var toPos Pos
	var hide Target
	if to.T == TFoundation {
		toPos = g.targetPos(g.state, to)
		hide = to
	} else {
		col := g.state.Tableau[to.Column]
		first := len(col) - len(held)
		hide = Target{T: TTableau, Column: to.Column, CardIndex: first}
		toPos = g.targetPos(g.state, hide)
	}
	g.anims.queue = append(g.anims.queue, cardAnim{
		cards: held, from: fromPos, to: toPos, start: time.Now(),
		dur: 200 * time.Millisecond, overlap: math.Min(14*g.s, 12*g.s), hideTarget: &hide,
	})
	g.save()
	flip := g.findRevealed(old, g.state)
	if checkWin(g.state) {
		g.wins++
		g.saveWins()
		g.sel.phase = PhWon
		g.winIx = 0
		return
	}
	var skip []int
	if to.T == TTableau || to.T == TTableauEmpty {
		skip = []int{to.Column}
	}
	if flip != nil {
		g.anims.flips = append(g.anims.flips, *flip)
		g.anims.onComplete = func() { g.performAutoMoves(false, skip) }
	} else {
		g.scheduleAutoMoves(false, skip)
	}
}

func (g *Solitaire) scheduleAutoMoves(skipWaste bool, skip []int) {
	if g.anims.animating() {
		g.anims.onComplete = func() { g.performAutoMoves(skipWaste, skip) }
	} else {
		g.performAutoMoves(skipWaste, skip)
	}
}

func (g *Solitaire) performAutoMoves(skipWaste bool, skip []int) {
	am := findAutoMove(g.state, skipWaste, skip)
	if am == nil {
		g.sel.rebuildAndClamp(g.state)
		g.checkLoss()
		return
	}
	ft := Target{T: TFoundation, Index: am.fi}
	fromPos := g.targetPos(g.state, am.from)
	res, ok := moveCards(g.state, am.from, ft)
	if !ok {
		g.checkLoss()
		return
	}
	g.anims.queue = append(g.anims.queue, cardAnim{
		cards: []SCard{am.card}, from: fromPos, to: g.targetPos(res, ft), start: time.Now(), dur: 150 * time.Millisecond,
	})
	g.anims.onComplete = func() {
		g.pushUndo()
		g.state = res
		g.sel.rebuildAndClamp(g.state)
		g.save()
		if checkWin(g.state) {
			g.wins++
			g.saveWins()
			g.sel.phase = PhWon
			g.winIx = 0
			return
		}
		g.performAutoMoves(skipWaste, skip)
	}
}

// OnLongPress undoes the last move (animating the cards back).
func (g *Solitaire) OnLongPress(ui *IpodUI) {
	if g.anims.animating() || g.sel.phase == PhWon || g.sel.phase == PhLost || len(g.undo) == 0 {
		return
	}
	prev := g.undo[len(g.undo)-1]
	g.undo = g.undo[:len(g.undo)-1]
	cur := g.state
	if g.sel.phase == PhHolding {
		g.sel.exitHolding(g.state)
	} else if g.sel.phase == PhMenu {
		g.sel.phase = PhBrowsing
	}
	if d := findStateDiff(prev, cur); d != nil {
		ov := 0.0
		if len(d.cards) > 1 {
			ov = math.Min(14*g.s, 12*g.s)
		}
		g.anims.queue = append(g.anims.queue, cardAnim{
			cards: d.cards, from: g.targetPos(cur, d.to), to: g.targetPos(prev, d.from), start: time.Now(), dur: 200 * time.Millisecond, overlap: ov,
		})
		g.anims.onComplete = func() {
			g.state = prev
			g.sel.rebuildAndClamp(g.state)
			g.save()
		}
		return
	}
	g.state = prev
	g.sel.rebuildAndClamp(g.state)
	g.save()
}

// OnKey: START opens or closes the game's menu (every game: START).
func (g *Solitaire) OnKey(ui *IpodUI, k Key) bool {
	if k != START || g.anims.animating() {
		return k == START
	}
	switch g.sel.phase {
	case PhMenu:
		g.sel.phase = PhBrowsing
	case PhBrowsing:
		g.sel.phase = PhMenu
		g.menuIx = 0
	default:
		return true // (holding cards, won, lost: B, or its own buttons)
	}
	ui.dirty = true
	return true
}

// OnMenu cancels a move, closes the menu or opens it.
func (g *Solitaire) OnMenu(ui *IpodUI) {
	if g.anims.animating() {
		return
	}
	switch g.sel.phase {
	case PhHolding:
		cur := g.sel.current()
		if cur != nil {
			from := g.cursorCenter(g.state, &g.sel, *cur)
			g.sel.exitHolding(g.state)
			if nt := g.sel.current(); nt != nil {
				g.animateCursor(from, g.cursorCenter(g.state, &g.sel, *nt), 150*time.Millisecond)
			}
			return
		}
		g.sel.exitHolding(g.state)
	case PhMenu:
		g.sel.phase = PhBrowsing
	case PhBrowsing:
		g.sel.phase = PhMenu
		g.menuIx = 0
	}
}

func (g *Solitaire) redeal() {
	g.anims.clear()
	g.state = solDeal()
	g.undo = nil
	g.menuIx, g.winIx, g.lostIx = 0, 0, 0
	g.sel.resetToStock(g.state)
	g.save()
	g.animateDeal()
}

func (g *Solitaire) animateDeal() {
	now := time.Now()
	from := g.targetPos(g.state, Target{T: TStock})
	n := 0
	for c := 0; c < 7; c++ {
		col := g.state.Tableau[c]
		for r := 0; r <= c; r++ {
			card := col[r]
			g.anims.queue = append(g.anims.queue, cardAnim{
				cards: []SCard{card}, from: from,
				to:    g.targetPos(g.state, Target{T: TTableau, Column: c, CardIndex: r}),
				start: now.Add(time.Duration(n) * 30 * time.Millisecond), dur: 120 * time.Millisecond, faceDown: !card.FaceUp,
			})
			n++
		}
	}
	g.anims.onComplete = func() { g.sel.build(g.state) }
}

func (g *Solitaire) findRevealed(old, cur SolState) *flipAnim {
	for c := 0; c < 7; c++ {
		oc, nc := old.Tableau[c], cur.Tableau[c]
		if len(nc) > 0 && len(nc) < len(oc) {
			top := nc[len(nc)-1]
			was := oc[len(nc)-1]
			if top.FaceUp && !was.FaceUp {
				t := Target{T: TTableau, Column: c, CardIndex: len(nc) - 1}
				return &flipAnim{card: top, pos: g.targetPos(cur, t), start: time.Now().Add(200 * time.Millisecond), dur: 200 * time.Millisecond, hideTarget: t}
			}
		}
	}
	return nil
}

func (g *Solitaire) pushUndo() {
	g.undo = append(g.undo, g.state.clone())
	if len(g.undo) > 20 {
		g.undo = g.undo[1:]
	}
}

func (g *Solitaire) checkLoss() {
	if !hasAvailableMoves(g.state) {
		g.sel.phase = PhLost
		g.lostIx = 0
	}
}

// The game in progress (with its undo stack) and the number of wins are
// saved next to the settings.
func solSavePath() string { return dataPath("ipodSolitaireSaveState.json") }
func solWinsPath() string { return dataPath("ipodSolitaireWins.txt") }

func (g *Solitaire) save() {
	data, err := json.Marshal(struct {
		GameState SolState   `json:"gameState"`
		UndoStack []SolState `json:"undoStack"`
	}{g.state, g.undo})
	if err == nil {
		os.WriteFile(solSavePath(), data, 0o644)
	}
}

func (g *Solitaire) load() bool {
	data, err := os.ReadFile(solSavePath())
	if err != nil {
		return false
	}
	var d struct {
		GameState *SolState  `json:"gameState"`
		UndoStack []SolState `json:"undoStack"`
	}
	if json.Unmarshal(data, &d) != nil || d.GameState == nil || checkWin(*d.GameState) {
		g.clearSave()
		return false
	}
	g.state = *d.GameState
	g.undo = d.UndoStack
	return true
}

func (g *Solitaire) clearSave() { os.Remove(solSavePath()) }

func (g *Solitaire) loadWins() int {
	data, err := os.ReadFile(solWinsPath())
	if err != nil {
		return 0
	}
	n, _ := strconv.Atoi(strings.TrimSpace(string(data)))
	return n
}

func (g *Solitaire) saveWins() { os.WriteFile(solWinsPath(), []byte(strconv.Itoa(g.wins)), 0o644) }

// Render: the green table, the piles, the cards in flight, the hand and
// the menus. Each card is drawn once into a cached sprite (with its
// shadow); the outline of an empty pile is cached too, and so are the
// scaled suit and hand images.
func (g *Solitaire) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	g.anims.tick(now)
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	if g.bg == nil {
		g.bg = newCanvas(W, H)
		fillVGradient(g.bg, 0, 0, W, H, []GradStop{{0, hex("#2D5E3F")}, {0.5, hex("#1E4D30")}, {1, hex("#163D25")}})
	}
	blitOpaque(dst, g.bg, 0, 0)
	hidden := g.anims.hidden()
	g.drawStock(dst)
	g.drawWaste(dst, hidden)
	g.drawFoundations(dst, hidden)
	g.drawTableau(dst, hidden)
	g.drawFlips(dst, now)
	g.drawAnims(dst, now)
	if g.sel.phase == PhBrowsing || g.sel.phase == PhHolding {
		if t := g.sel.current(); t != nil {
			if p, ok := g.cursorAnimPos(now); ok {
				g.drawCursor(dst, p)
			} else {
				g.drawCursor(dst, g.cursorCenter(g.state, &g.sel, *t))
			}
		}
	}
	switch g.sel.phase {
	case PhMenu:
		sub := fmt.Sprintf("%d wins", g.wins)
		if g.wins == 1 {
			sub = "1 win"
		}
		g.drawMenu(dst, solMenuItems, g.menuIx, "Paused", sub)
	case PhWon:
		sub := fmt.Sprintf("%d wins total", g.wins)
		if g.wins == 1 {
			sub = "1 win total"
		}
		g.drawMenu(dst, solWinItems, g.winIx, "You Win!", sub)
	case PhLost:
		g.drawMenu(dst, solLostItems, g.lostIx, "No More Moves", "")
	}
}

func (g *Solitaire) cursorAnimPos(now time.Time) (Pos, bool) {
	a := g.cursorAnim
	if a == nil {
		return Pos{}, false
	}
	el := now.Sub(a.start)
	if el >= a.dur {
		g.cursorAnim = nil
		return Pos{}, false
	}
	t := easeOutCubic(float64(el) / float64(a.dur))
	return Pos{a.from.X + (a.to.X-a.from.X)*t, a.from.Y + (a.to.Y-a.from.Y)*t}, true
}

func (g *Solitaire) highlightFor(t Target) string {
	if g.sel.phase != PhBrowsing && g.sel.phase != PhHolding {
		return ""
	}
	cur := g.sel.current()
	if cur != nil && targetsEqual(*cur, t) {
		return "orange"
	}
	return ""
}

func (g *Solitaire) drawStock(dst *image.RGBA) {
	l := g.l
	hl := g.highlightFor(Target{T: TStock})
	// a face-down card, the recycle sign when only the waste is left
	if len(g.state.Stock) > 0 {
		g.blitCard(dst, 100, l.stockX, l.topY)
		g.drawHighlight(dst, l.stockX, l.topY, l.cw, l.ch, hl)
	} else if len(g.state.Waste) > 0 {
		g.drawEmpty(dst, l.stockX, l.topY, -1)
		ic := g.ui.asset("sol_recycle")
		size := math.Min(l.cw, l.ch) * 0.45
		img := scaledAsset(ic, px(size), px(size))
		blit(dst, img, px(l.stockX+(l.cw-size)/2), px(l.topY+(l.ch-size)/2), 0.5)
		g.drawHighlight(dst, l.stockX, l.topY, l.cw, l.ch, hl)
	} else {
		g.drawEmpty(dst, l.stockX, l.topY, -1)
	}
}

func (g *Solitaire) isHidden(hidden []Target, t Target) bool {
	for _, h := range hidden {
		if targetsEqual(h, t) {
			return true
		}
	}
	return false
}

func (g *Solitaire) drawWaste(dst *image.RGBA, hidden []Target) {
	l := g.l
	w := g.state.Waste
	if len(w) == 0 {
		return
	}
	for _, h := range hidden {
		if h.T == TWaste {
			if len(w) > 1 {
				g.blitCard(dst, cardKey(w[len(w)-2]), l.wasteX, l.topY)
			}
			return
		}
	}
	hl := g.highlightFor(Target{T: TWaste})
	if g.sel.phase == PhHolding && g.sel.heldFrom != nil && g.sel.heldFrom.T == TWaste {
		hl = "blue"
	}
	g.blitCard(dst, cardKey(w[len(w)-1]), l.wasteX, l.topY)
	g.drawHighlight(dst, l.wasteX, l.topY, l.cw, l.ch, hl)
}

func (g *Solitaire) drawFoundations(dst *image.RGBA, hidden []Target) {
	l := g.l
	for i := 0; i < 4; i++ {
		x := l.fX + float64(i)*l.fSp
		f := g.state.Foundations[i]
		held := g.sel.phase == PhHolding && g.sel.heldFrom != nil && g.sel.heldFrom.T == TFoundation && g.sel.heldFrom.Index == i
		hl := g.highlightFor(Target{T: TFoundation, Index: i})
		isHid := g.isHidden(hidden, Target{T: TFoundation, Index: i})
		// while its top card flies, the card under it shows
		if isHid && len(f) > 0 {
			if len(f) > 1 {
				g.blitCard(dst, cardKey(f[len(f)-2]), x, l.topY)
				g.drawHighlight(dst, x, l.topY, l.cw, l.ch, hl)
			} else {
				g.drawEmpty(dst, x, l.topY, i)
				g.drawHighlight(dst, x, l.topY, l.cw, l.ch, hl)
			}
		} else if len(f) == 0 {
			g.drawEmpty(dst, x, l.topY, i)
			g.drawHighlight(dst, x, l.topY, l.cw, l.ch, hl)
		} else {
			if held {
				hl = "blue"
			}
			g.blitCard(dst, cardKey(f[len(f)-1]), x, l.topY)
			g.drawHighlight(dst, x, l.topY, l.cw, l.ch, hl)
		}
	}
}

func (g *Solitaire) drawTableau(dst *image.RGBA, hidden []Target) {
	l := g.l
	cur := g.sel.current()
	for c := 0; c < 7; c++ {
		col := g.state.Tableau[c]
		x := l.colX + float64(c)*(l.cw+l.colSp)
		if len(col) == 0 {
			g.drawEmpty(dst, x, l.tabY, -1)
			g.drawHighlight(dst, x, l.tabY, l.cw, l.ch, g.highlightFor(Target{T: TTableauEmpty, Column: c}))
			continue
		}
		ov := g.overlaps(col)
		browse, heldFrom := -1, -1
		if g.sel.phase == PhBrowsing && cur != nil && cur.T == TTableau && cur.Column == c {
			browse = cur.CardIndex
		}
		if g.sel.phase == PhHolding && g.sel.heldFrom != nil && g.sel.heldFrom.T == TTableau && g.sel.heldFrom.Column == c {
			heldFrom = g.sel.heldFrom.CardIndex
		}
		isDrop := g.sel.phase == PhHolding && cur != nil && cur.T == TTableau && cur.Column == c
		hidFrom := math.MaxInt32
		for _, h := range hidden {
			if h.T == TTableau && h.Column == c && h.CardIndex < hidFrom {
				hidFrom = h.CardIndex
			}
		}
		y := l.tabY
		ys := make([]float64, len(col))
		for i, card := range col {
			ys[i] = y
			if i < hidFrom {
				if card.FaceUp {
					g.blitCard(dst, cardKey(card), x, y)
				} else {
					g.blitCard(dst, 100, x, y)
				}
			}
			if i < len(col)-1 {
				if i < len(ov) {
					y += ov[i]
				} else {
					y += l.baseOv
				}
			}
		}
		last := ys[len(col)-1] + l.ch
		if browse >= 0 {
			g.drawHighlight(dst, x, ys[browse], l.cw, last-ys[browse], "blue")
		}
		if heldFrom >= 0 {
			g.drawHighlight(dst, x, ys[heldFrom], l.cw, last-ys[heldFrom], "orange")
		}
		if isDrop {
			g.drawHighlight(dst, x, ys[len(col)-1]+l.baseOv, l.cw, l.ch, "orange")
		}
	}
}

func (g *Solitaire) drawFlips(dst *image.RGBA, now time.Time) {
	for _, f := range g.anims.flips {
		el := now.Sub(f.start)
		if el < 0 {
			continue
		}
		raw := math.Min(1, float64(el)/float64(f.dur))
		half := raw * 2
		sx := 0.0
		back := half <= 1
		if back {
			sx = 1 - easeOutCubic(half)
		} else {
			sx = easeOutCubic(half - 1)
		}
		sx = math.Max(0.01, sx)
		key := cardKey(f.card)
		if back {
			key = 100
		}
		sp := g.sprite(key)
		w := int(math.Max(1, float64(sp.Bounds().Dx())*sx))
		scaled := scaleImage(sp, w, sp.Bounds().Dy())
		cx := f.pos.X + g.l.cw/2
		blit(dst, scaled, px(cx)-w/2, px(f.pos.Y-5), 1)
	}
}

func (g *Solitaire) drawAnims(dst *image.RGBA, now time.Time) {
	for _, a := range g.anims.queue {
		el := now.Sub(a.start)
		if el < 0 {
			continue
		}
		t := easeOutCubic(math.Min(1, float64(el)/float64(a.dur)))
		x := a.from.X + (a.to.X-a.from.X)*t
		y := a.from.Y + (a.to.Y-a.from.Y)*t
		for i, c := range a.cards {
			yy := y + float64(i)*a.overlap
			// lifted cards cast a bigger shadow
			softShadow(dst, x+g.s, yy+2*g.s, g.l.cw, g.l.ch, 4*g.s, 0.3)
			if a.faceDown {
				g.blitCard(dst, 100, x, yy)
			} else {
				g.blitCard(dst, cardKey(c), x, yy)
			}
		}
	}
}

func cardKey(c SCard) int { return c.Suit*13 + c.Rank - 1 }

func (g *Solitaire) blitCard(dst *image.RGBA, key int, x, y float64) {
	blit(dst, g.sprite(key), px(x-5), px(y-5), 1)
}

var scaledAssetCache = map[scaledKey]*image.RGBA{}

type scaledKey struct {
	src  *image.RGBA
	w, h int
}

func scaledAsset(src *image.RGBA, w, h int) *image.RGBA {
	k := scaledKey{src, w, h}
	if img, ok := scaledAssetCache[k]; ok {
		return img
	}
	img := scaleImage(src, w, h)
	scaledAssetCache[k] = img
	return img
}

var suitAssets = []string{"sol_heart", "sol_diamond", "sol_club", "sol_spade"}

func (g *Solitaire) drawSuit(dst *image.RGBA, suit int, cx, cy, size, alpha float64) {
	img := scaledAsset(g.ui.asset(suitAssets[suit]), px(size), px(size))
	blit(dst, img, px(cx-size/2), px(cy-size/2), alpha)
}

// sprite: a card (key = suit*13+rank-1, 100 the back) with its shadow.
func (g *Solitaire) sprite(key int) *image.RGBA {
	if sp, ok := g.sprites[key]; ok {
		return sp
	}
	l := g.l
	s := g.s
	img := newCanvas(px(l.cw+10), px(l.ch+10))
	x, y := 5.0, 5.0
	solid := func(c Col) []GradStop { return []GradStop{{0, c}, {1, c}} }
	// shadow and white card
	softShadow(img, x, y+s, l.cw, l.ch, 3*s, 0.2)
	fillRoundedGradient(img, x*DPR, y*DPR, (x+l.cw)*DPR, (y+l.ch)*DPR, 3*DPR, solid(colWhite), 1)
	if key == 100 {
		b := 2 * s
		ix, iw, ih := x+b, l.cw-2*b, l.ch-2*b
		inner := newCanvas(img.Bounds().Dx(), img.Bounds().Dy())
		fillRoundedGradient(inner, ix*DPR, (y+b)*DPR, (ix+iw)*DPR, (y+b+ih)*DPR, 2*DPR, []GradStop{{0, hex("#4A90D9")}, {1, hex("#1B5AAF")}}, 1)
		// stripes
		sh, gap := 1.5*s, 2.5*s
		for sy := y + b + s; sy < y+b+ih-s; sy += sh + gap {
			hh := math.Min(sh, y+b+ih-s-sy)
			if hh > 0 {
				fillRectF(inner, ix+s, sy, iw-b, hh, Col{106, 171, 224, 0.3})
			}
		}
		blit(img, inner, 0, 0, 1)
		logoH := ih * 0.34
		logoW := logoH * 814 / 1000
		logo := scaledAsset(g.ui.asset("sol_apple"), px(logoW), px(logoH))
		blit(img, logo, px(x+(l.cw-logoW)/2), px(y+(l.ch-logoH)/2), 0.3)
	} else {
		suit, rank := key/13, key%13
		fillRoundedGradient(img, x*DPR, y*DPR, (x+l.cw)*DPR, (y+l.ch)*DPR, 3*DPR, []GradStop{{0, hex("#FFFFFF")}, {1, hex("#E8E8E8")}}, 1)
		strokeRounded(img, (x-0.5)*DPR, (y-0.5)*DPR, (x+l.cw+0.5)*DPR, (y+l.ch+0.5)*DPR, 3.5*DPR, 1*DPR, hex("#AAAAAA"), 1)
		col := hex("#000000")
		if suit == 0 || suit == 1 {
			col = hex("#CC0000")
		}
		fs := math.Floor(10 * s)
		small := math.Floor(9 * s)
		large := math.Floor(12 * s)
		f := face(W700, fs)
		rx, ry := x+3*s, y+2*s
		label := rankNames[rank]
		// rank at the top left, a small suit next to it, a large one in the middle
		asc := fs * 0.96875
		drawText(img, f, label, rx*DPR, (ry+asc*0.84)*DPR, col)
		b, _ := fontBounds(f, label)
		tw := textWidth(f, label)
		centerY := ry + asc*0.84 + (b.minY+b.maxY)/2/DPR
		g.drawSuit(img, suit, rx+tw/DPR+s+small/2, centerY, small, 1)
		g.drawSuit(img, suit, x+l.cw/2, y+l.ch/2, large, 1)
	}
	g.sprites[key] = img
	return img
}

type glyphBounds struct{ minY, maxY float64 }

// fontBounds: the vertical ink extent of s (device px).
func fontBounds(f font.Face, s string) (glyphBounds, bool) {
	b, _ := font.BoundString(f, s)
	return glyphBounds{float64(b.Min.Y) / 64, float64(b.Max.Y) / 64}, true
}

func fillRectF(dst *image.RGBA, x, y, w, h float64, c Col) {
	fillRoundedGradient(dst, x*DPR, y*DPR, (x+w)*DPR, (y+h)*DPR, 0, []GradStop{{0, c}, {1, c}}, 1)
}

var rankNames = []string{"A", "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K"}
var emptyOutline *image.RGBA

func (g *Solitaire) drawEmpty(dst *image.RGBA, x, y float64, suit int) {
	l := g.l
	if emptyOutline == nil {
		// a dashed outline
		emptyOutline = newCanvas(px(l.cw+2), px(l.ch+2))
		dashedRoundRect(emptyOutline, 1, 1, l.cw, l.ch, 3, colWhite)
	}
	blit(dst, emptyOutline, px(x-1), px(y-1), 0.3)
	if suit >= 0 {
		g.drawSuit(dst, suit, x+l.cw/2, y+l.ch/2, math.Floor(22*g.s), 0.2)
	}
}

func (g *Solitaire) drawHighlight(dst *image.RGBA, x, y, w, h float64, kind string) {
	if kind == "" {
		return
	}
	pad := 2 * g.s
	fill, stroke := rgba(255, 140, 0, 0.15), hex("#FF8C00")
	if kind == "blue" {
		fill, stroke = rgba(46, 107, 230, 0.15), hex("#2E6BE6")
	}
	x0, y0, x1, y1 := (x-pad)*DPR, (y-pad)*DPR, (x+w+pad)*DPR, (y+h+pad)*DPR
	fillRoundedGradient(dst, x0, y0, x1, y1, 4*DPR, []GradStop{{0, fill}, {1, fill}}, 1)
	lw := 2 * g.s * DPR
	strokeRounded(dst, x0-lw/2, y0-lw/2, x1+lw/2, y1+lw/2, 4*DPR+lw/2, lw, stroke, 1)
}

func (g *Solitaire) drawCursor(dst *image.RGBA, p Pos) {
	size := 26 * g.s
	w := size * 435 / 512
	img := scaledAsset(g.ui.asset("sol_hand"), px(w), px(size))
	blit(dst, img, px(p.X-w*0.4), px(p.Y-size*0.55+20*g.s), 1)
}

func (g *Solitaire) drawMenu(dst *image.RGBA, labels []string, sel int, title, subtitle string) {
	s := g.s
	fillRect(dst, 0, 0, dst.Bounds().Dx(), dst.Bounds().Dy(), Col{0, 0, 0, 0.7})
	ih, th := 24*s, 28*s
	sh := 0.0
	if subtitle != "" {
		sh = 18 * s
	}
	total := th + sh + float64(len(labels))*ih + 8*s
	// a 160 px panel in the middle
	sx := (g.w - 160*s) / 2
	sy := (g.h - total) / 2
	solid := func(c Col) []GradStop { return []GradStop{{0, c}, {1, c}} }
	fillRoundedGradient(dst, sx*DPR, sy*DPR, (sx+160*s)*DPR, (sy+total)*DPR, 6*DPR, solid(Col{40, 40, 40, 0.95}), 1)
	text := func(f font.Face, txt string, cy float64, c Col) {
		tw := textWidth(f, txt) / DPR
		fs := faceSize(f)
		drawText(dst, f, txt, (g.w/2-tw/2)*DPR, (cy+fs*0.35)*DPR, c)
	}
	text(face(W700, math.Floor(12*s)), title, sy+th/2, colWhite)
	if subtitle != "" {
		text(face(W400, math.Floor(9*s)), subtitle, sy+th+sh/2, Col{255, 255, 255, 0.6})
	}
	is := sy + th + sh
	for i, lab := range labels {
		iy := is + float64(i)*ih
		if i == sel {
			fillRoundedGradient(dst, (sx+6*s)*DPR, (iy+2*s)*DPR, (sx+160*s-6*s)*DPR, (iy+ih-2*s)*DPR, 4*DPR, solid(hex("#4A90D9")), 1)
		}
		text(face(W400, math.Floor(11*s)), lab, iy+ih/2, colWhite)
	}
}

// dashedRoundRect strokes a dashed rounded rectangle (CSS px).
func dashedRoundRect(dst *image.RGBA, x, y, w, h, r float64, c Col) {
	// the point at distance d along the outline, clockwise from the top left
	per := 2*(w+h) - 8*r + 2*math.Pi*r
	pointAt := func(d float64) (float64, float64) {
		d = math.Mod(d, per)
		segs := []float64{w - 2*r, r * math.Pi / 2, h - 2*r, r * math.Pi / 2, w - 2*r, r * math.Pi / 2, h - 2*r, r * math.Pi / 2}
		for i, L := range segs {
			if d <= L {
				switch i {
				case 0:
					return x + r + d, y
				case 1:
					a := -math.Pi/2 + d/r
					return x + w - r + r*math.Cos(a), y + r + r*math.Sin(a)
				case 2:
					return x + w, y + r + d
				case 3:
					a := d / r
					return x + w - r + r*math.Cos(a), y + h - r + r*math.Sin(a)
				case 4:
					return x + w - r - d, y + h
				case 5:
					a := d/r + math.Pi/2
					return x + r + r*math.Cos(a), y + h - r + r*math.Sin(a)
				case 6:
					return x, y + h - r - d
				case 7:
					a := d/r + math.Pi
					return x + r + r*math.Cos(a), y + r + r*math.Sin(a)
				}
			}
			d -= L
		}
		return x, y
	}
	for d := 0.0; d < per; d += 7 {
		for e := d; e < math.Min(d+4, per); e += 0.5 {
			x0, y0 := pointAt(e)
			x1, y1 := pointAt(math.Min(e+0.5, per))
			drawThickLine(dst, x0*DPR, y0*DPR, x1*DPR, y1*DPR, 2, c)
		}
	}
}
