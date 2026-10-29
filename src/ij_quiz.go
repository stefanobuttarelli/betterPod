package main

// ij_quiz.go — Guess the Song: a few seconds of one of your songs play,
// you pick its title among four. Ten rounds; the faster, the more points.
//
//	UP / DOWN  choose      A  answer, then the next song
//	B          back: the song you were listening to comes back where it was
//
// While it plays, the quiz has the player to itself: Now Playing is set
// aside (stopped, so the poller leaves it alone) and put back on leaving.

import (
	"fmt"
	"image"
	"math"
	"math/rand"
	"os"
	"strings"
	"sync"
	"time"

	"golang.org/x/image/font"
)

const (
	quizRounds = 10
	quizClip   = 15 * time.Second
)

type quizChoice struct {
	t *Track
}

type QuizGame struct {
	ui      *IpodUI
	rng     *rand.Rand
	pool    []*Track
	used    map[*Track]bool
	round   int // 1..quizRounds
	score   int
	best    int
	answer  *Track
	choices []*Track
	sel     int
	picked  int    // the row answered (-1: not yet)
	gained  int    // points of this round
	phase   string // loading, playing, reveal, done, empty
	started time.Time
	shownAt time.Time
	selAt   time.Time // the row the cursor is on since (its title scrolls when long)
	mu      sync.Mutex
	closed  bool
	gen     int // the round's load (a late load of an old round is ignored)

	saved NowPlaying // what was playing before
	wasOn bool
	bg    *image.RGBA
}

func quizBestPath() string { return dataPath("quiz_best.txt") }

func (ui *IpodUI) openQuiz() {
	app := ui.app
	q := &QuizGame{ui: ui, rng: rand.New(rand.NewSource(time.Now().UnixNano())), used: map[*Track]bool{}, picked: -1}
	if b, err := os.ReadFile(quizBestPath()); err == nil {
		fmt.Sscan(string(b), &q.best)
	}
	if lib := app.Library; lib != nil {
		for _, t := range lib.Tracks {
			if t.Title != "" && t.Duration >= 30 && !strings.Contains(t.Path, "/Podcasts/") {
				q.pool = append(q.pool, t)
			}
		}
	}
	// set what plays aside
	if app.Playing != nil {
		q.saved = *app.Playing
		q.wasOn = app.Playing.State == StatePlaying
		if app.Playing.State != StateStopped {
			audioPause()
		}
		app.Playing.Track, app.Playing.State = nil, StateStopped
	}
	v := &View{ID: "quiz", Title: "Guess the Song", Kind: KindFull, Custom: q}
	v.onClose = q.close
	if len(q.titles()) < 4 {
		q.phase = "empty"
	} else {
		q.next()
	}
	ui.showView(v, "")
}

// titles: the different titles in the pool (four are needed).
func (q *QuizGame) titles() map[string]bool {
	m := map[string]bool{}
	for _, t := range q.pool {
		m[strings.ToLower(strings.TrimSpace(t.Title))] = true
	}
	return m
}

// next: a new round (or the end).
func (q *QuizGame) next() {
	if q.round >= quizRounds {
		q.phase = "done"
		audioPause()
		if q.score > q.best {
			q.best = q.score
			writeFileAtomic(quizBestPath(), []byte(fmt.Sprint(q.best)))
		}
		return
	}
	q.round++
	// the song: one not asked yet
	var t *Track
	for i := 0; i < 50; i++ {
		c := q.pool[q.rng.Intn(len(q.pool))]
		if !q.used[c] {
			t = c
			break
		}
	}
	if t == nil {
		t = q.pool[q.rng.Intn(len(q.pool))]
	}
	q.used[t] = true
	q.answer = t
	// three other titles
	seen := map[string]bool{strings.ToLower(strings.TrimSpace(t.Title)): true}
	q.choices = []*Track{t}
	for i := 0; len(q.choices) < 4 && i < 400; i++ {
		c := q.pool[q.rng.Intn(len(q.pool))]
		k := strings.ToLower(strings.TrimSpace(c.Title))
		if !seen[k] {
			seen[k] = true
			q.choices = append(q.choices, c)
		}
	}
	q.rng.Shuffle(len(q.choices), func(i, j int) { q.choices[i], q.choices[j] = q.choices[j], q.choices[i] })
	q.sel, q.picked, q.gained = 0, -1, 0
	q.selAt = time.Now()
	q.phase = "loading"
	q.shownAt = time.Now()
	// a part from the middle of the song
	start := t.Duration * (0.25 + 0.3*q.rng.Float64())
	q.gen++
	gen := q.gen
	app := q.ui.app
	go func() {
		err := app.mpvLoadFile(t.Path)
		q.mu.Lock()
		ok := !q.closed && gen == q.gen
		q.mu.Unlock()
		if !ok {
			return
		}
		if err == nil {
			audioSeek(start)
		}
		q.ui.post(func() {
			if gen != q.gen || q.closed {
				return
			}
			q.phase = "playing"
			q.started = time.Now()
			q.ui.dirty = true
		})
	}()
}

// reveal: the answer is in (row picked, or -1 when the time ran out).
func (q *QuizGame) reveal(row int) {
	q.picked = row
	q.gained = 0
	if row >= 0 && q.choices[row] == q.answer {
		left := 1 - math.Min(1, time.Since(q.started).Seconds()/quizClip.Seconds())
		q.gained = 20 + int(80*left+0.5)
		q.score += q.gained
	}
	q.phase = "reveal"
	q.shownAt = time.Now()
	// (the song goes on a moment, then stops with the next round)
}

func (q *QuizGame) close() {
	q.mu.Lock()
	q.closed = true
	q.mu.Unlock()
	audioStop()
	app := q.ui.app
	// what was playing comes back where it was, playing or paused as it
	// was (loaded again: the quiz's clips took the player)
	if app.Playing != nil {
		*app.Playing = q.saved
		if t := q.saved.Track; t != nil && q.saved.State != StateStopped {
			pos, on := q.saved.Position, q.wasOn
			go func() {
				if err := app.mpvLoadFile(t.Path); err != nil {
					return
				}
				if pos > 1 {
					audioSeek(pos)
				}
				if !on {
					audioPause()
				}
				q.ui.post(func() { q.ui.dirty = true })
			}()
		}
	}
	q.ui.onTrackChanged()
}

// ---- keys ----

func (q *QuizGame) OnKey(ui *IpodUI, k Key) bool {
	switch k {
	case UP, DOWN:
		if q.phase == "playing" || q.phase == "loading" {
			d := 1
			if k == UP {
				d = -1
			}
			q.sel = (q.sel + d + len(q.choices)) % len(q.choices)
			q.selAt = time.Now()
			ui.dirty = true
		}
		return true
	case LEFT, RIGHT:
		return true
	}
	return false
}

func (q *QuizGame) OnCenter(ui *IpodUI) {
	switch q.phase {
	case "playing":
		q.reveal(q.sel)
	case "reveal":
		q.next()
	case "done":
		q.round, q.score = 0, 0
		q.used = map[*Track]bool{}
		q.next()
	}
	ui.dirty = true
}

func (q *QuizGame) OnScroll(ui *IpodUI, dir int, velocity float64) {}

func (q *QuizGame) Animating(now time.Time) bool {
	return q.phase == "playing" || q.phase == "loading" || q.phase == "reveal" || now.Sub(q.shownAt) < 400*time.Millisecond
}

// ---- drawing ----

var quizRed = []GradStop{{0, Col{255, 120, 112, 1}}, {1, Col{200, 36, 40, 1}}}
var quizGreen = []GradStop{{0, Col{110, 214, 130, 1}}, {1, Col{24, 140, 66, 1}}}

func (q *QuizGame) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	if q.bg == nil || q.bg.Bounds().Dx() != W {
		q.bg = newCanvas(W, H)
		fillVGradient(q.bg, 0, 0, W, H, []GradStop{{0, Col{238, 241, 245, 1}}, {1, Col{210, 216, 225, 1}}})
	}
	blitOpaque(dst, q.bg, 0, 0)
	ink, sub := Col{36, 40, 48, 1}, Col{96, 104, 118, 1}
	center := func(f fontFace, s string, cx, y float64, c Col) {
		drawText(dst, f.f, s, cx-textWidth(f.f, s)/2, y+f.asc, c)
	}
	switch q.phase {
	case "empty":
		center(ff(W700, 16), "Not enough songs", 320, 170, ink)
		center(ff(W500, 13), "Guess the Song needs at least four songs", 320, 220, sub)
		center(ff(W500, 13), "with different titles in your library.", 320, 252, sub)
		return
	case "done":
		center(ff(W700, 14), "Your score", 320, 90, sub)
		center(ff(W700, 40), fmt.Sprint(q.score), 320, 130, ink)
		msg := fmt.Sprintf("Best: %d", q.best)
		if q.score >= q.best && q.score > 0 {
			msg = "A new best!"
		}
		center(ff(W600, 14), msg, 320, 250, sub)
		center(ff(W500, 13), "A: play again · B: back", 320, 330, sub)
		return
	}
	// the round and the score
	f12 := ff(W700, 12)
	drawText(dst, f12.f, fmt.Sprintf("Song %d of %d", q.round, quizRounds), 24, 22+f12.asc, sub)
	sc := fmt.Sprintf("Score %d", q.score)
	drawText(dst, f12.f, sc, 616-textWidth(f12.f, sc), 22+f12.asc, sub)
	// the time left
	left := 1.0
	switch q.phase {
	case "playing":
		left = 1 - math.Min(1, now.Sub(q.started).Seconds()/quizClip.Seconds())
		if left <= 0 {
			q.reveal(-1)
			left = 0
		}
	case "reveal":
		left = 0
	}
	fillRoundedGradient(dst, 24, 56, 616, 66, 5, []GradStop{{0, Col{0, 0, 0, 0.12}}, {1, Col{0, 0, 0, 0.06}}}, 1)
	if left > 0 {
		g := selGrad
		if left < 0.25 {
			g = quizRed
		}
		fillRoundedGradient(dst, 24, 56, 24+math.Max(10, 592*left), 66, 5, g, 1)
	}
	// the picture: a question mark while it plays, the cover after
	ax, ay, as := 24.0, 92.0, 260.0
	if q.phase == "reveal" {
		al := ui.trackAlbum(q.answer)
		img := ui.art(al, int(as))
		if img == nil {
			img = ui.artFallback(al, int(as))
		}
		if img != nil {
			drawNN(dst, img, image.Rect(int(ax), int(ay), int(ax+as), int(ay+as)), 0, 1)
		} else {
			fillRoundedGradient(dst, ax, ay, ax+as, ay+as, 16, []GradStop{{0, Col{96, 104, 122, 1}}, {1, Col{52, 58, 72, 1}}}, 1)
		}
	} else {
		fillRoundedGradient(dst, ax, ay, ax+as, ay+as, 18, []GradStop{{0, Col{110, 120, 140, 1}}, {1, Col{43, 50, 64, 1}}}, 1)
		fillRoundedGradient(dst, ax+3, ay+3, ax+as-3, ay+as/2, 16, []GradStop{{0, Col{255, 255, 255, 0.35}}, {1, Col{255, 255, 255, 0.04}}}, 1)
		if q.phase == "playing" {
			// bars moving with the time (no spoiler)
			t := now.Sub(q.started).Seconds()
			for i := 0; i < 5; i++ {
				h := 30 + 50*(0.5+0.5*math.Sin(t*5.3+float64(i)*1.7))*(0.6+0.4*math.Sin(t*2.1+float64(i)))
				x := ax + 54 + float64(i)*26
				fillRoundedGradient(dst, x, ay+as-46-h, x+16, ay+as-46, 5, selGrad, 1)
			}
		} else {
			center(ff(W700, 52), "?", ax+as/2, ay+60, colWhite)
		}
	}
	// the four titles
	rx, rw, rh, gap := 304.0, 312.0, 59.0, 8.0
	for i, c := range q.choices {
		y := ay + float64(i)*(rh+gap)
		var g []GradStop
		tc, scol := ink, sub
		switch {
		case q.phase == "reveal" && c == q.answer:
			g = quizGreen
		case q.phase == "reveal" && i == q.picked:
			g = quizRed
		case q.phase != "reveal" && i == q.sel:
			g = selGrad
		}
		if g != nil {
			fillRoundedGradient(dst, rx, y, rx+rw, y+rh, 10, g, 1)
			tc, scol = colWhite, Col{255, 255, 255, 0.85}
		} else {
			fillRoundedGradient(dst, rx, y, rx+rw, y+rh, 10, []GradStop{{0, colWhite}, {1, Col{244, 246, 249, 1}}}, 1)
			strokeRounded(dst, rx, y, rx+rw, y+rh, 10, 2, Col{0, 0, 0, 0.12}, 1)
		}
		tf, sf := ff(W700, 13), ff(W500, 11)
		ty := y + (rh-tf.asc)/2 - 2
		if q.phase == "reveal" {
			ty = y + 8
		}
		// the row with the cursor (or, after the answer, the right one)
		// scrolls its title when it is too long
		live := (q.phase != "reveal" && i == q.sel) || (q.phase == "reveal" && c == q.answer)
		if live && textWidth(tf.f, c.Title) > rw-28 {
			since := q.selAt
			if q.phase == "reveal" {
				since = q.shownAt
			}
			sub := dst.SubImage(image.Rect(int(rx+14), int(y), int(rx+rw-14), int(y+rh))).(*image.RGBA)
			drawMarquee(sub, tf.f, c.Title, (rx+14)/DPR, (ty+tf.asc)/DPR, (rw-28)/DPR, tc, since, now)
		} else {
			drawText(dst, tf.f, ellipsize(tf.f, c.Title, rw-28), rx+14, ty+tf.asc, tc)
		}
		if q.phase == "reveal" {
			drawText(dst, sf.f, ellipsize(sf.f, c.Artist, rw-28), rx+14, y+33+sf.asc, scol)
		}
	}
	// what to do / how it went
	hf := ff(W600, 12)
	var msg string
	switch q.phase {
	case "loading":
		msg = "Listen…"
	case "playing":
		msg = "UP / DOWN choose · A answer"
	case "reveal":
		switch {
		case q.picked < 0:
			msg = "Time's up · A: next song"
		case q.gained > 0:
			msg = fmt.Sprintf("Right! +%d · A: next song", q.gained)
		default:
			msg = "Not this one · A: next song"
		}
		if q.round >= quizRounds {
			msg = strings.Replace(msg, "next song", "see your score", 1)
		}
	}
	center(hf, msg, 320, 372, sub)
	if q.phase == "reveal" {
		al := q.answer.Album
		s := q.answer.Artist
		if al != "" {
			s += " · " + al
		}
		center(ff(W500, 11), ellipsize(ff(W500, 11).f, s, 590), 320, 404, sub)
	}
}

// fontFace: a face and its ascent (device px), for drawing at a top y.
type fontFace struct {
	f   font.Face
	asc float64
}

func ff(w FontWeight, size float64) fontFace {
	_, a := lineBox(size)
	return fontFace{face(w, size), a * DPR}
}

// quizIcon: the preview's picture (made once).
var quizIconImg *image.RGBA

func quizIcon() *image.RGBA {
	if quizIconImg != nil {
		return quizIconImg
	}
	const S = 192.0
	img := newCanvas(int(S), int(S))
	fillRoundedGradient(img, 12, 12, S-12, S-12, 36, []GradStop{{0, Col{255, 120, 170, 1}}, {1, Col{200, 40, 110, 1}}}, 1)
	fillRoundedGradient(img, 15, 15, S-15, S/2, 32, []GradStop{{0, Col{255, 255, 255, 0.45}}, {1, Col{255, 255, 255, 0.05}}}, 1)
	// a question mark over a few bars
	for i := 0; i < 5; i++ {
		h := []float64{34, 56, 44, 62, 30}[i]
		x := 46 + float64(i)*22
		fillRoundedGradient(img, x, S-40-h, x+14, S-40, 5, []GradStop{{0, Col{255, 255, 255, 0.55}}, {1, Col{255, 255, 255, 0.3}}}, 1)
	}
	f := face(W700, 50)
	_, a := lineBox(50)
	drawText(img, f, "?", S/2-textWidth(f, "?")/2, S/2+a*DPR/2-14, colWhite)
	quizIconImg = img
	return img
}
