package main

// ij_sol_engine.go — Klondike solitaire rules: dealing, moves,
// auto-moves, the wheel selection and the card animations.

import (
	"math"
	"math/rand"
	"os"
	"strconv"
	"time"
)

// solSeed: MIYOOPOD_SOL_SEED makes the deals reproducible (tests);
// -1 uses math/rand. The generator is the classic LCG so that a seed
// gives the same deal as the JavaScript version.
var solSeed = func() int64 {
	if v, err := strconv.ParseInt(os.Getenv("MIYOOPOD_SOL_SEED"), 10, 64); err == nil {
		return v
	}
	return -1
}()

func solRandom() float64 {
	if solSeed < 0 {
		return rand.Float64()
	}
	solSeed = (solSeed*1103515245 + 12345) % 2147483648
	return float64(solSeed) / 2147483648
}

// SCard is a card: suit 0-3 (0 and 1 are red), rank 1-13.
type SCard struct {
	Suit   int  `json:"suit"`
	Rank   int  `json:"rank"`
	FaceUp bool `json:"faceUp"`
}

// SolState is a game position (saved as JSON).
type SolState struct {
	Stock       []SCard    `json:"stock"`
	Waste       []SCard    `json:"waste"`
	Foundations [4][]SCard `json:"foundations"`
	Tableau     [7][]SCard `json:"tableau"`
	StockPasses int        `json:"stockPasses"`
}

func (c SCard) red() bool { return c.Suit == 0 || c.Suit == 1 }

// clone is a deep copy of the state: the moves build a new state and
// leave the old one (undo, animations) untouched.
//
// Empty piles become empty, non-nil slices (they are saved as [] rather
// than null), so that a saved game round-trips exactly through JSON and
// the comparison with the previous state (findStateDiff) never sees a
// difference that is only nil against empty. The stock-pass counter is
// copied as is.
func (s SolState) clone() SolState {
	n := SolState{StockPasses: s.StockPasses}
	n.Stock = append([]SCard{}, s.Stock...)
	n.Waste = append([]SCard{}, s.Waste...)
	for i := range s.Foundations {
		n.Foundations[i] = append([]SCard{}, s.Foundations[i]...)
	}
	for i := range s.Tableau {
		n.Tableau[i] = append([]SCard{}, s.Tableau[i]...)
	}
	return n
}

// TargetType is the kind of place cards are taken from or put on.
type TargetType int

const (
	TStock TargetType = iota
	TWaste
	TFoundation
	TTableau
	TTableauEmpty
)

// Target is a place: a foundation, a card of a column or an empty column.
type Target struct {
	T                        TargetType
	Index, Column, CardIndex int
}

func targetsEqual(a, b Target) bool {
	if a.T != b.T {
		return false
	}
	switch a.T {
	case TFoundation:
		return a.Index == b.Index
	case TTableau:
		return a.Column == b.Column && a.CardIndex == b.CardIndex
	case TTableauEmpty:
		return a.Column == b.Column
	}
	return true
}

func solDeal() SolState {
	var deck []SCard
	for s := 0; s < 4; s++ {
		for r := 1; r <= 13; r++ {
			deck = append(deck, SCard{Suit: s, Rank: r})
		}
	}
	for i := len(deck) - 1; i > 0; i-- {
		j := int(math.Floor(solRandom() * float64(i+1)))
		deck[i], deck[j] = deck[j], deck[i]
	}
	var st SolState
	k := 0
	for col := 0; col < 7; col++ {
		for row := 0; row <= col; row++ {
			c := deck[k]
			c.FaceUp = row == col
			st.Tableau[col] = append(st.Tableau[col], c)
			k++
		}
	}
	for _, c := range deck[k:] {
		c.FaceUp = false
		st.Stock = append(st.Stock, c)
	}
	return st
}

func canMoveToFoundation(c SCard, f []SCard, suit int) bool {
	if c.Suit != suit {
		return false
	}
	if len(f) == 0 {
		return c.Rank == 1
	}
	return c.Rank == f[len(f)-1].Rank+1
}

func canMoveToTableau(cards []SCard, col []SCard) bool {
	if len(cards) == 0 {
		return false
	}
	c := cards[0]
	if len(col) == 0 {
		return c.Rank == 13
	}
	top := col[len(col)-1]
	if !top.FaceUp {
		return false
	}
	return c.red() != top.red() && c.Rank == top.Rank-1
}

func drawFromStock(s SolState) SolState {
	n := s.clone()
	if len(n.Stock) > 0 {
		c := n.Stock[len(n.Stock)-1]
		n.Stock = n.Stock[:len(n.Stock)-1]
		c.FaceUp = true
		n.Waste = append(n.Waste, c)
		return n
	}
	if len(n.Waste) > 0 {
		var stock []SCard
		for i := len(n.Waste) - 1; i >= 0; i-- {
			c := n.Waste[i]
			c.FaceUp = false
			stock = append(stock, c)
		}
		n.Stock = stock
		n.Waste = nil
		n.StockPasses++
	}
	return n
}

func cardsFromTarget(s SolState, t Target) []SCard {
	switch t.T {
	case TWaste:
		if len(s.Waste) > 0 {
			return []SCard{s.Waste[len(s.Waste)-1]}
		}
	case TFoundation:
		f := s.Foundations[t.Index]
		if len(f) > 0 {
			return []SCard{f[len(f)-1]}
		}
	case TTableau:
		col := s.Tableau[t.Column]
		if t.CardIndex >= 0 && t.CardIndex <= len(col) {
			return append([]SCard{}, col[t.CardIndex:]...)
		}
	}
	return nil
}

func isValidRun(col []SCard, from int) bool {
	for i := from; i < len(col); i++ {
		if !col[i].FaceUp {
			return false
		}
		if i > from {
			prev, cur := col[i-1], col[i]
			if cur.red() == prev.red() || cur.Rank != prev.Rank-1 {
				return false
			}
		}
	}
	return true
}

func canPickUp(s SolState, t Target) bool {
	switch t.T {
	case TWaste:
		return len(s.Waste) > 0
	case TFoundation:
		return len(s.Foundations[t.Index]) > 0
	case TTableau:
		col := s.Tableau[t.Column]
		if t.CardIndex < 0 || t.CardIndex >= len(col) || !col[t.CardIndex].FaceUp {
			return false
		}
		return isValidRun(col, t.CardIndex)
	}
	return false
}

func moveCards(s SolState, from, to Target) (SolState, bool) {
	cards := cardsFromTarget(s, from)
	if len(cards) == 0 {
		return s, false
	}
	n := s.clone()
	switch to.T {
	case TFoundation:
		if len(cards) != 1 || !canMoveToFoundation(cards[0], s.Foundations[to.Index], to.Index) {
			return s, false
		}
		n.Foundations[to.Index] = append(n.Foundations[to.Index], cards[0])
	case TTableau, TTableauEmpty:
		if !canMoveToTableau(cards, s.Tableau[to.Column]) {
			return s, false
		}
		n.Tableau[to.Column] = append(n.Tableau[to.Column], cards...)
	default:
		return s, false
	}
	switch from.T {
	case TWaste:
		n.Waste = n.Waste[:len(n.Waste)-1]
	case TFoundation:
		n.Foundations[from.Index] = n.Foundations[from.Index][:len(n.Foundations[from.Index])-1]
	case TTableau:
		col := append([]SCard{}, s.Tableau[from.Column][:from.CardIndex]...)
		if len(col) > 0 && !col[len(col)-1].FaceUp {
			col[len(col)-1].FaceUp = true
		}
		n.Tableau[from.Column] = col
	}
	return n, true
}

func isSameTarget(a, b Target) bool {
	if a.T != b.T {
		return false
	}
	switch a.T {
	case TFoundation:
		return a.Index == b.Index
	case TTableauEmpty:
		return a.Column == b.Column
	}
	return a.T != TTableau && a.T != TStock
}

func validDropTargets(s SolState, held []SCard, from Target) []Target {
	var out []Target
	if len(held) == 0 {
		return nil
	}
	h := held[0]
	if len(held) == 1 {
		for i := 0; i < 4; i++ {
			t := Target{T: TFoundation, Index: i}
			if !isSameTarget(t, from) && canMoveToFoundation(h, s.Foundations[i], i) {
				out = append(out, t)
			}
		}
	}
	for c := 0; c < 7; c++ {
		col := s.Tableau[c]
		if len(col) == 0 {
			if h.Rank == 13 {
				t := Target{T: TTableauEmpty, Column: c}
				if !isSameTarget(t, from) {
					out = append(out, t)
				}
			}
			continue
		}
		top := col[len(col)-1]
		if top.FaceUp && h.red() != top.red() && h.Rank == top.Rank-1 {
			sameCol := (from.T == TTableau || from.T == TTableauEmpty) && from.Column == c
			if !sameCol {
				out = append(out, Target{T: TTableau, Column: c, CardIndex: len(col) - 1})
			}
		}
	}
	return out
}

func checkWin(s SolState) bool {
	for _, f := range s.Foundations {
		if len(f) != 13 {
			return false
		}
	}
	return true
}

func hasAvailableMoves(s SolState) bool {
	if len(s.Stock) > 0 || len(s.Waste) > 0 {
		return true
	}
	for c := 0; c < 7; c++ {
		col := s.Tableau[c]
		for i := 0; i < len(col); i++ {
			if !col[i].FaceUp || !isValidRun(col, i) {
				continue
			}
			run := col[i:]
			last := run[len(run)-1]
			for f := 0; f < 4; f++ {
				if canMoveToFoundation(last, s.Foundations[f], f) {
					return true
				}
			}
			for tc := 0; tc < 7; tc++ {
				if tc == c {
					continue
				}
				if canMoveToTableau(run, s.Tableau[tc]) {
					if col[i].Rank == 13 && i == 0 && len(s.Tableau[tc]) == 0 {
						continue
					}
					return true
				}
			}
		}
	}
	return false
}

func isGuaranteedWin(s SolState) bool {
	if len(s.Stock) > 0 || len(s.Waste) > 0 {
		return false
	}
	for _, col := range s.Tableau {
		for _, c := range col {
			if !c.FaceUp {
				return false
			}
		}
	}
	return true
}

func isSafeToAutoMove(c SCard, s SolState) bool {
	if isGuaranteedWin(s) {
		return true
	}
	if c.Rank <= 3 {
		return true
	}
	need := c.Rank - 2
	for i := 0; i < 4; i++ {
		if (i == 0 || i == 1) == c.red() {
			continue
		}
		top := 0
		if f := s.Foundations[i]; len(f) > 0 {
			top = f[len(f)-1].Rank
		}
		if top < need {
			return false
		}
	}
	return true
}

type autoMove struct {
	card SCard
	from Target
	fi   int
}

func findAutoMove(s SolState, skipWaste bool, skipCols []int) *autoMove {
	type cand struct {
		c SCard
		f Target
	}
	var cs []cand
	if !skipWaste && len(s.Waste) > 0 {
		cs = append(cs, cand{s.Waste[len(s.Waste)-1], Target{T: TWaste}})
	}
	for c := 0; c < 7; c++ {
		if containsInt(skipCols, c) {
			continue
		}
		col := s.Tableau[c]
		if len(col) > 0 && col[len(col)-1].FaceUp {
			cs = append(cs, cand{col[len(col)-1], Target{T: TTableau, Column: c, CardIndex: len(col) - 1}})
		}
	}
	for _, x := range cs {
		// foundation i holds suit i
		if !canMoveToFoundation(x.c, s.Foundations[x.c.Suit], x.c.Suit) {
			continue
		}
		if isSafeToAutoMove(x.c, s) {
			return &autoMove{card: x.c, from: x.f, fi: x.c.Suit}
		}
	}
	return nil
}

type stateDiff struct {
	cards    []SCard
	from, to Target
}

func findStateDiff(old, cur SolState) *stateDiff {
	for i := 0; i < 4; i++ {
		if len(cur.Foundations[i]) > len(old.Foundations[i]) {
			cards := cur.Foundations[i][len(old.Foundations[i]):]
			from, ok := findCardSource(old, cur, cards[0])
			if !ok {
				from = Target{T: TFoundation, Index: i}
			}
			return &stateDiff{cards: cards, from: from, to: Target{T: TFoundation, Index: i}}
		}
	}
	for c := 0; c < 7; c++ {
		oc, cc := old.Tableau[c], cur.Tableau[c]
		if len(cc) > len(oc) {
			cards := cc[len(oc):]
			from, ok := findCardSource(old, cur, cards[0])
			if !ok {
				from = Target{T: TTableau, Column: c, CardIndex: len(oc)}
			}
			return &stateDiff{cards: cards, from: from, to: Target{T: TTableau, Column: c, CardIndex: len(oc)}}
		}
	}
	if len(cur.Waste) > len(old.Waste) && len(cur.Stock) < len(old.Stock) {
		return &stateDiff{cards: []SCard{cur.Waste[len(cur.Waste)-1]}, from: Target{T: TStock}, to: Target{T: TWaste}}
	}
	return nil
}

func findCardSource(old, cur SolState, c SCard) (Target, bool) {
	if len(old.Waste) > len(cur.Waste) {
		w := old.Waste[len(old.Waste)-1]
		if w.Suit == c.Suit && w.Rank == c.Rank {
			return Target{T: TWaste}, true
		}
	}
	for i := 0; i < 4; i++ {
		if len(old.Foundations[i]) > len(cur.Foundations[i]) {
			f := old.Foundations[i][len(old.Foundations[i])-1]
			if f.Suit == c.Suit && f.Rank == c.Rank {
				return Target{T: TFoundation, Index: i}, true
			}
		}
	}
	for col := 0; col < 7; col++ {
		if len(old.Tableau[col]) > len(cur.Tableau[col]) {
			for i, x := range old.Tableau[col] {
				if x.FaceUp && x.Suit == c.Suit && x.Rank == c.Rank {
					return Target{T: TTableau, Column: col, CardIndex: i}, true
				}
			}
		}
	}
	return Target{}, false
}

type SolPhase int

const (
	PhBrowsing SolPhase = iota
	PhHolding
	PhMenu
	PhWon
	PhLost
)

// Selection is the wheel cursor over the places of the table.
// While holding cards it moves over the places they can go.
type Selection struct {
	phase      SolPhase
	cursor     int
	targets    []Target
	held       []SCard
	heldFrom   *Target
	prevBrowse int
}

func (sel *Selection) current() *Target {
	if len(sel.targets) == 0 || sel.cursor >= len(sel.targets) {
		return nil
	}
	return &sel.targets[sel.cursor]
}

func (sel *Selection) scroll(dir int) {
	n := len(sel.targets)
	if n == 0 {
		return
	}
	sel.cursor = ((sel.cursor+dir)%n + n) % n
}

func (sel *Selection) build(s SolState) {
	ts := []Target{{T: TStock}}
	if len(s.Waste) > 0 {
		ts = append(ts, Target{T: TWaste})
	}
	for i := 0; i < 4; i++ {
		if len(s.Foundations[i]) > 0 {
			ts = append(ts, Target{T: TFoundation, Index: i})
		}
	}
	for c := 0; c < 7; c++ {
		col := s.Tableau[c]
		if len(col) == 0 {
			ts = append(ts, Target{T: TTableauEmpty, Column: c})
			continue
		}
		first := -1
		for i, x := range col {
			if x.FaceUp {
				first = i
				break
			}
		}
		if first == -1 {
			continue
		}
		for i := first; i < len(col); i++ {
			if isValidRun(col, i) {
				ts = append(ts, Target{T: TTableau, Column: c, CardIndex: i})
			}
		}
	}
	sel.targets = ts
	sel.cursor = int(math.Min(float64(sel.cursor), math.Max(0, float64(len(ts)-1))))
}

func (sel *Selection) enterHolding(held []SCard, from Target, valid []Target) bool {
	if len(valid) == 0 {
		return false
	}
	sel.prevBrowse = sel.cursor
	sel.phase = PhHolding
	sel.cursor = 0
	sel.targets = valid
	sel.held = held
	f := from
	sel.heldFrom = &f
	return true
}

func (sel *Selection) exitHolding(s SolState) {
	sel.phase = PhBrowsing
	sel.held = nil
	sel.heldFrom = nil
	sel.build(s)
	sel.cursor = int(math.Min(float64(sel.prevBrowse), math.Max(0, float64(len(sel.targets)-1))))
}

func (sel *Selection) resetToStock(s SolState) {
	sel.phase = PhBrowsing
	sel.cursor = 0
	sel.held = nil
	sel.heldFrom = nil
	sel.build(s)
}

func (sel *Selection) rebuildAndClamp(s SolState) {
	prev := sel.cursor
	sel.build(s)
	sel.cursor = int(math.Min(float64(prev), math.Max(0, float64(len(sel.targets)-1))))
}

// cardAnim moves cards from one place to another (CSS px positions,
// overlap is the spacing of a run).
type cardAnim struct {
	cards      []SCard
	from, to   Pos
	start      time.Time
	dur        time.Duration
	overlap    float64
	faceDown   bool
	hideTarget *Target
}

// flipAnim turns a card face up where it lies.
type flipAnim struct {
	card       SCard
	pos        Pos
	start      time.Time
	dur        time.Duration
	hideTarget Target
}

// solAnims: the moves in flight, the flips, and what to do after.
type solAnims struct {
	queue      []cardAnim
	flips      []flipAnim
	onComplete func()
}

func easeOutCubic(t float64) float64 { return 1 - math.Pow(1-t, 3) }

func (a *solAnims) animating() bool { return len(a.queue) > 0 || len(a.flips) > 0 }

func (a *solAnims) hidden() []Target {
	var out []Target
	for _, x := range a.queue {
		if x.hideTarget != nil {
			out = append(out, *x.hideTarget)
		}
	}
	for _, f := range a.flips {
		out = append(out, f.hideTarget)
	}
	return out
}

func (a *solAnims) tick(now time.Time) {
	before := len(a.queue) + len(a.flips)
	q := a.queue[:0]
	for _, x := range a.queue {
		if now.Sub(x.start) < x.dur {
			q = append(q, x)
		}
	}
	a.queue = q
	f := a.flips[:0]
	for _, x := range a.flips {
		if now.Sub(x.start) < x.dur {
			f = append(f, x)
		}
	}
	a.flips = f
	if before > 0 && len(a.queue)+len(a.flips) == 0 && a.onComplete != nil {
		cb := a.onComplete
		a.onComplete = nil
		cb()
	}
}

func (a *solAnims) clear() {
	a.queue = nil
	a.flips = nil
	a.onComplete = nil
}

// suits (Hearts and Diamonds are red)
const (
	Hearts = iota
	Diamonds
	Clubs
	Spades
)
