package main

// ij_gameclock.go — the time in the header while a game is played: each
// game's START menu has Show the Time / Hide the Time, kept for that game
// (in the preferences, by the game's name). The time sits in the middle
// of the header, as Hold's does, and is drawn again when the minute
// changes (no frame more than that).

import (
	"sort"
	"time"
)

// gameClock: the games that show the time, by name ("Brick", "Mines"…).
var gameClock = map[string]bool{}

// gameViews: the screens that are games (their Title is the game's name,
// as gameMenu gets it).
var gameViews = map[string]bool{
	"brickGame": true, "solitaireGame": true, "2048": true, "fiveletters": true, "quiz": true, "puzzle": true,
}

// gameClockOption: the START menu's row that shows or hides the time.
func (ui *IpodUI) gameClockOption(name string) SheetOption {
	label := "Show the Time"
	if gameClock[name] {
		label = "Hide the Time"
	}
	return SheetOption{Label: label, OnSelect: func() { ui.toggleGameClock(name) }}
}

func (ui *IpodUI) toggleGameClock(name string) {
	if gameClock[name] {
		delete(gameClock, name)
	} else {
		gameClock[name] = true
	}
	ui.savePrefs()
	ui.dirty = true
}

// gameClockShown: the screen on top is a game that shows the time.
func (ui *IpodUI) gameClockShown() bool {
	if len(ui.full) == 0 {
		return false
	}
	v := ui.full[len(ui.full)-1]
	return gameViews[v.ID] && gameClock[v.Title]
}

// gameClockText: the time, as the header shows it.
func gameClockText() string { return time.Now().Format("15:04") }

// gameClockList: the games that show the time, for the preferences.
func gameClockList() []string {
	var l []string
	for g := range gameClock {
		l = append(l, g)
	}
	sort.Strings(l)
	return l
}
