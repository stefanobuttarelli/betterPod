package main

// ij_prefs.go — settings of the ipod.js UI kept next to the library data.

import (
	"encoding/json"
	"os"
)

type ipodPrefs struct {
	Volume      float64 `json:"volume"`
	ShuffleMode string  `json:"shuffleMode"`
	RepeatMode  string  `json:"repeatMode"`
	HoldBright  int     `json:"holdBrightness,omitempty"`
	Theme       string  `json:"theme,omitempty"`
	YouTube     bool    `json:"youtube,omitempty"`
}

func ipodPrefsPath() string { return dataPath("ipodjs_settings.json") }

func (ui *IpodUI) loadPrefs() {
	data, err := os.ReadFile(ipodPrefsPath())
	if err != nil {
		return
	}
	var p ipodPrefs
	if json.Unmarshal(data, &p) != nil {
		return
	}
	switch p.Theme {
	case "light", "dark", "auto":
		themeMode = p.Theme
	}
	ytEnabled = p.YouTube
	if p.HoldBright >= 1 && p.HoldBright <= 100 {
		hold.level = p.HoldBright
	}
	if p.Volume > 0 && p.Volume <= 1 {
		ui.volume = p.Volume
	}
	switch p.ShuffleMode {
	case "off", "songs", "albums":
		ui.shuffleMode = p.ShuffleMode
	}
	if q := ui.app.Queue; q != nil {
		switch p.RepeatMode {
		case "one":
			q.Repeat = RepeatOne
		case "all":
			q.Repeat = RepeatAll
		case "off":
			q.Repeat = RepeatOff
		}
	}
}

func (ui *IpodUI) savePrefs() {
	rep := "off"
	if q := ui.app.Queue; q != nil {
		switch q.Repeat {
		case RepeatOne:
			rep = "one"
		case RepeatAll:
			rep = "all"
		}
	}
	data, _ := json.Marshal(ipodPrefs{Volume: ui.volume, ShuffleMode: ui.shuffleMode, RepeatMode: rep, HoldBright: hold.level, Theme: themeMode, YouTube: ytEnabled})
	_ = os.WriteFile(ipodPrefsPath(), data, 0644)
}
