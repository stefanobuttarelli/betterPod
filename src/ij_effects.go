package main

// ij_effects.go — Settings → Effects: Full (the default) or Light.
// Light keeps covers and Weather icons still and shows sheets and toasts
// immediately, so an idle screen can avoid continuous redraws.

import "time"

var effectsMode = defaultEffects()

func defaultEffects() string {
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
