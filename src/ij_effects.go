package main

// ij_effects.go — Settings → Effects: Full (the interface as it is, the
// default on Knulli) or Light, the default on the Miyoo, whose Cortex-A7
// can't redraw the slow pans 20 times a second and still keep up with
// everything else:
//
//   - the album covers beside the menus stand still (no Ken Burns pan) and
//     cross-fade in 0.6 s instead of 2 s;
//   - the Now Playing preview shows the cover without panning;
//   - the Weather icons stand still (in Games and in the forecast): an
//     animated one is 60 frames drawn with gg, about a second of the A7
//     and up to 16 MB, then redrawn 20 times a second;
//   - option sheets and toasts appear and go at once: their slide redraws
//     the whole screen under them, dimmed pixel by pixel, at every frame,
//     which the Miyoo shows as a few jerky steps;
//
// so a screen nobody touches is not redrawn at all. Menu slides, Cover
// Flow and Now Playing are the same in both.

import "time"

var effectsMode = defaultEffects()

func defaultEffects() string {
	if isOnion {
		return "light"
	}
	return "full"
}

func lightEffects() bool { return effectsMode == "light" }

func effectsLabel(m string) string {
	if m == "light" {
		return "Light"
	}
	return "Full"
}

// lightDur: d, or no time at all with Light effects (sheets, toasts).
func lightDur(d time.Duration) time.Duration {
	if lightEffects() {
		return 0
	}
	return d
}

// kbFade: how long the music preview's covers take to cross-fade.
func kbFade() time.Duration {
	if lightEffects() {
		return 600 * time.Millisecond
	}
	return 2 * time.Second
}

// effectsView: Full / Light.
func (ui *IpodUI) effectsView() *View {
	v := &View{ID: "effects", Title: "Effects", Kind: KindSplit}
	v.Build = func() []Item {
		var items []Item
		for _, m := range []string{"full", "light"} {
			items = append(items, Item{Label: effectsLabel(m), Checked: effectsMode == m,
				Preview: PrevSettings, NoArrow: true, OnSelect: func() { ui.setEffects(m) }})
		}
		return items
	}
	v.Items = v.Build()
	for i, it := range v.Items {
		if it.Checked {
			v.Index = i
		}
	}
	return v
}

func (ui *IpodUI) setEffects(m string) {
	if m == effectsMode {
		return
	}
	effectsMode = m
	ui.kb = newKenBurns() // start the preview again in the new mode
	ui.savePrefs()
	ui.refreshBuilt()
	logMsg("Effects: " + effectsLabel(m))
}
