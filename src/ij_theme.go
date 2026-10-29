package main

// ij_theme.go — light (ipod.js) and dark themes. Settings → Theme:
// Light, Dark or Automatic (dark from 20:00 to 7:00).

import (
	"image"
	"time"
)

type UITheme struct {
	dark         bool
	bg           Col // list / screen background
	text         Col
	sub          Col // secondary text
	npSub        Col // Now Playing artist / album
	empty        Col
	header       []GradStop
	headerBorder Col
	headerText   Col
	preview      []GradStop // grey icon previews
	artBorder    Col        // 1px border around Now Playing artwork
	cardBorder   Col        // Cover Flow backside card
	stripe       Col        // every other row of the Cover Flow track list
	sheetOpt     []GradStop
	sheetBorder  Col
	sheetText    Col
	sheetShadow  Col
	padlock      []GradStop
	padShackle   Col
}

var lightTheme = UITheme{
	bg:           rgb(255, 255, 255),
	text:         rgb(0, 0, 0),
	sub:          rgb(100, 100, 100),
	npSub:        rgb(99, 101, 103),
	empty:        rgb(100, 100, 100),
	header:       []GradStop{{0, rgb(254, 255, 255)}, {1, rgb(177, 182, 185)}},
	headerBorder: hex("#7995a3"),
	headerText:   rgb(0, 0, 0),
	preview:      []GradStop{{0, hex("#b1b5c0")}, {1, hex("#686e7a")}},
	artBorder:    hex("#f3f3f3"),
	cardBorder:   hex("#d3d3d3"),
	stripe:       hex("#f2f3f5"),
	sheetOpt:     []GradStop{{0, hex("#ffffff")}, {0.4844, hex("#cbccce")}, {1, hex("#dfdfdf")}},
	sheetBorder:  hex("#3e4249"),
	sheetText:    rgb(0, 0, 0),
	sheetShadow:  hex("#505050"),
	padlock:      []GradStop{{0, rgb(120, 126, 130)}, {1, rgb(40, 44, 48)}},
	padShackle:   rgb(70, 74, 78),
}

var darkTheme = UITheme{
	dark:         true,
	bg:           rgb(22, 23, 25),
	text:         rgb(238, 239, 241),
	sub:          rgb(150, 153, 158),
	npSub:        rgb(155, 158, 163),
	empty:        rgb(140, 143, 148),
	header:       []GradStop{{0, rgb(78, 81, 86)}, {1, rgb(36, 38, 42)}},
	headerBorder: rgb(10, 11, 12),
	headerText:   rgb(240, 241, 243),
	preview:      []GradStop{{0, hex("#3d414a")}, {1, hex("#17191d")}},
	artBorder:    rgb(48, 50, 54),
	cardBorder:   rgb(60, 62, 66),
	stripe:       rgb(30, 31, 34),
	sheetOpt:     []GradStop{{0, hex("#55585f")}, {0.4844, hex("#34373d")}, {1, hex("#3c3f45")}},
	sheetBorder:  hex("#15171a"),
	sheetText:    rgb(240, 241, 243),
	sheetShadow:  hex("#000000"),
	padlock:      []GradStop{{0, rgb(235, 237, 240)}, {1, rgb(170, 175, 180)}},
	padShackle:   rgb(215, 218, 222),
}

var th = &lightTheme

// themeMode: "light", "dark", "auto"
var themeMode = "light"

func wantDark(mode string, now time.Time) bool {
	switch mode {
	case "dark":
		return true
	case "auto":
		h := now.Hour()
		return h >= 20 || h < 7
	}
	return false
}

// updateTheme switches palette when needed (called at start, on change and
// every minute for the automatic mode). Returns true if it changed.
func (ui *IpodUI) updateTheme() bool {
	t := &lightTheme
	if wantDark(themeMode, time.Now()) {
		t = &darkTheme
	}
	if t == th {
		return false
	}
	th = t
	// drop everything rendered with the old palette
	headerBg = nil
	greyPreviewBg = nil
	ui.assetsMu.Lock()
	for k := range ui.assets {
		if len(k) > 6 && k[:6] == "theme:" {
			delete(ui.assets, k)
		}
	}
	ui.assetsMu.Unlock()
	for _, st := range [][]*View{ui.split, ui.full} {
		for _, v := range st {
			v.dirty = true
			if np, ok := v.Custom.(*NowPlayingView); ok {
				np.stageBuf = nil // (the cover's border and back follow the theme)
			}
			if a, ok := v.Custom.(*AboutView); ok {
				a.list.dirty = true
			}
		}
	}
	if cf := ui.cover; cf != nil && cf.np != nil {
		cf.np.stageBuf = nil
	}
	ui.dirty = true
	return true
}

func (ui *IpodUI) setThemeMode(mode string) {
	themeMode = mode
	ui.updateTheme()
	ui.savePrefs()
	ui.refreshBuilt()
}

func themeLabel(m string) string {
	switch m {
	case "dark":
		return "Dark"
	case "auto":
		return "Automatic"
	}
	return "Light"
}

func (ui *IpodUI) themeView() *View {
	v := &View{ID: "theme", Title: "Theme", Kind: KindSplit}
	v.Build = func() []Item {
		var items []Item
		for _, m := range []string{"light", "dark", "auto"} {
			m := m
			it := Item{Label: themeLabel(m), Checked: themeMode == m, NoArrow: true,
				Preview: PrevSettings, OnSelect: func() { ui.setThemeMode(m) }}
			if m == "auto" {
				it.Sublabel = "Dark from 20:00 to 7:00"
			}
			items = append(items, it)
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

// Dark versions of the grey glyph assets (lightened) and of the light ones
// that would glare (darkened). Other assets are used as they are.
var themeLighten = map[string]bool{
	"bluetooth": true, "hdr_shuffle": true, "hdr_repeat": true, "hdr_repeat_one": true,
	"volume_full_bar": true, "volume_mute_bar": true, "volume_full_16": true, "battery_shell": true,
}
var themeDarken = map[string]bool{"gloss_track": true}

// themed returns the asset for the current theme.
func (ui *IpodUI) themed(name string) *image.RGBA {
	base := ui.asset(name)
	if !th.dark || (!themeLighten[name] && !themeDarken[name]) {
		return base
	}
	key := "theme:" + name
	ui.assetsMu.Lock()
	if a, ok := ui.assets[key]; ok {
		ui.assetsMu.Unlock()
		return a
	}
	ui.assetsMu.Unlock()
	out := newCanvas(base.Bounds().Dx(), base.Bounds().Dy())
	copy(out.Pix, base.Pix)
	for i := 0; i+3 < len(out.Pix); i += 4 {
		a := uint32(out.Pix[i+3])
		if a == 0 {
			continue
		}
		for c := 0; c < 3; c++ {
			v := uint32(out.Pix[i+c]) // premultiplied
			if themeLighten[name] {
				// straight value v' = 255 - (255-v)*0.4, then premultiply
				sv := v * 255 / a
				sv = 255 - (255-sv)*2/5
				v = sv * a / 255
			} else {
				v = v * 3 / 5
			}
			out.Pix[i+c] = uint8(v)
		}
	}
	ui.assetsMu.Lock()
	ui.assets[key] = out
	ui.assetsMu.Unlock()
	return out
}
