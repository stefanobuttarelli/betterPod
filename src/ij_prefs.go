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
	Effects     string  `json:"effects,omitempty"`
	YouTube     bool    `json:"youtube,omitempty"`
	NoPodcasts  bool    `json:"hidePodcasts,omitempty"` // Podcasts taken out of the main menu
	SaverAfter  int     `json:"screensaverMinutes,omitempty"`
	SaverDim    bool    `json:"screensaverDim,omitempty"`
	SaverLevel  int     `json:"screensaverBrightness,omitempty"`
	SaverStyle  string  `json:"screensaverStyle,omitempty"`
	SaverEffect int     `json:"screensaverEffect,omitempty"`
	SaverColors int     `json:"screensaverColors,omitempty"`
	Apple       bool    `json:"turnItApple,omitempty"`
	ShareName   string  `json:"shareName,omitempty"` // this betterPod's name, for Share
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
	switch p.Effects {
	case "full", "light":
		effectsMode = p.Effects
	}
	ytEnabled = p.YouTube
	appleMode = p.Apple
	shareName = p.ShareName
	podsInMenu = !p.NoPodcasts
	saverAfter, saverDim = p.SaverAfter, p.SaverDim
	if p.SaverStyle == "viz" {
		saverStyle = "viz"
	}
	if p.SaverEffect >= 0 && p.SaverEffect < len(vizNames) {
		saverEffect = p.SaverEffect
	}
	if p.SaverColors >= 0 && p.SaverColors < len(vizSchemes) {
		saverColors = p.SaverColors
	}
	if p.SaverLevel >= 1 && p.SaverLevel <= 100 {
		saverLevel = p.SaverLevel
	}
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
	data, _ := json.Marshal(ipodPrefs{Volume: ui.volume, ShuffleMode: ui.shuffleMode, RepeatMode: rep, HoldBright: hold.level, Theme: themeMode, YouTube: ytEnabled, NoPodcasts: !podsInMenu, Effects: effectsMode,
		SaverAfter: saverAfter, SaverDim: saverDim, SaverLevel: saverLevel,
		SaverStyle: saverStyle, SaverEffect: saverEffect, SaverColors: saverColors, Apple: appleMode, ShareName: shareName})
	_ = writeFileAtomic(ipodPrefsPath(), data)
}
