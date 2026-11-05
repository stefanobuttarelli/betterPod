//go:build ignore

// Copied into the scratch Go 1.1 test tree; never linked into the Rust app.
package main

import (
	"encoding/json"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func qaExportIcons(t *testing.T, ui *IpodUI, out string) {
	t.Helper()
	directory := filepath.Join(out, "icons")
	if err := os.MkdirAll(directory, 0755); err != nil {
		t.Fatal(err)
	}
	for name, picture := range map[string]*image.RGBA{
		"preview_brick": brickIcon(),
		"preview_solitaire": ui.solitaireIcon(),
		"preview_2048": g48Icon(),
		"preview_words": wordsIcon(),
		"preview_quiz": quizIcon(),
		"preview_puzzles": pzIcon(),
		"preview_videos": videosIcon(),
	} {
		file, err := os.Create(filepath.Join(directory, name+".png"))
		if err != nil { t.Fatal(err) }
		err = png.Encode(file, picture)
		closeErr := file.Close()
		if err != nil { t.Fatal(err) }
		if closeErr != nil { t.Fatal(closeErr) }
	}
}

func qaSeedGame(t *testing.T, ui *IpodUI, screen, fixtureRoot string) bool {
	t.Helper()
	now := animNow()
	var view *View
	var fixture any
	switch screen {
	case "game2048":
		board := [4][4]int{{2, 4, 8, 16}, {32, 64, 128, 256}, {512, 1024, 2048, 0}, {2, 4, 8, 0}}
		game := &Game2048{score: 1234, best: 5678, movedAt: now.Add(-time.Second)}
		for row := 0; row < 4; row++ {
			for column := 0; column < 4; column++ {
				if value := board[row][column]; value != 0 {
					game.tiles = append(game.tiles, &g48Tile{v: value, r: row, c: column, fr: row, fc: column})
				}
			}
		}
		fixture = g48Save{Board: board, Score: game.score, Best: game.best}
		view = &View{ID: screen, Title: "2048", Kind: KindFull, Custom: game}
	case "gamebrick":
		game := newBrickGame(320, 220)
		game.onQuit = func() {}
		bricks := make([]map[string]any, 0, len(game.bricks))
		for _, brick := range game.bricks {
			bricks = append(bricks, map[string]any{"x": brick.x, "y": brick.y, "w": brick.w, "h": brick.h, "c": brick.c})
		}
		fixture = map[string]any{"px": game.px, "pvel": game.pvel, "bx": game.bx, "by": game.by, "dx": game.dx, "dy": game.dy, "phase": game.phase, "level": game.level, "score": game.score, "lives": game.lives, "bricks": bricks}
		view = &View{ID: screen, Title: "Brick", Kind: KindFull, Custom: game}
	case "gamesolitaire":
		// An affine permutation pins the actual deal, independently of either RNG.
		var deck []SCard
		for i := 0; i < 52; i++ {
			id := (i*17 + 11) % 52
			deck = append(deck, SCard{Suit: id / 13, Rank: id%13 + 1})
		}
		state := SolState{}
		index := 0
		for column := 0; column < 7; column++ {
			for row := 0; row <= column; row++ {
				card := deck[index]
				card.FaceUp = row == column
				state.Tableau[column] = append(state.Tableau[column], card)
				index++
			}
		}
		state.Stock = append([]SCard{}, deck[index:]...)
		game := &Solitaire{ui: ui, w: 320, h: 220, s: 220.0 / 232, state: state, wins: 3, sprites: map[int]*image.RGBA{}, onQuit: func() {}}
		game.computeLayout()
		game.sel.build(state)
		fixture = map[string]any{"gameState": state, "undoStack": []SolState{}, "wins": game.wins}
		view = &View{ID: screen, Title: "Solitaire", Kind: KindFull, Custom: game, DisableLongPress: true}
	case "gamewords":
		wlLoad()
		game := &WordGame{answer: "APPLE", guesses: []string{"ALLEY"}, kr: 0, kc: 0, revealAt: now.Add(-2 * time.Second)}
		game.stats = wlStats{Played: 12, Wins: 8, Streak: 3, MaxStreak: 4, Dist: [6]int{1, 2, 3, 1, 1, 0}}
		game.blankRow()
		game.settle()
		fixture = map[string]any{"answer": game.answer, "guesses": game.guesses, "typed": game.typed, "stats": game.stats, "kr": game.kr, "kc": game.kc, "revealElapsed": 2}
		view = &View{ID: screen, Title: "Five Letters", Kind: KindFull, Custom: game}
	default:
		return false
	}
	directory := filepath.Join(fixtureRoot, "games")
	if err := os.MkdirAll(directory, 0755); err != nil {
		t.Fatal(err)
	}
	bytes, err := json.MarshalIndent(fixture, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, screen+".json"), bytes, 0644); err != nil {
		t.Fatal(err)
	}
	ui.showView(view, "")
	return true
}
