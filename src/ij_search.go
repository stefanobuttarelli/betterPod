package main

// ij_search.go — Search: the on-screen keyboard (a scrolling row of keys
// that pops up from the bottom) and the results list.

import (
	"fmt"
	"image"
	"math"
	"strings"
	"time"
)

// kbKey is a key of the on-screen keyboard: what it types, its label
type kbKey struct {
	key, label, icon string
}

var keyboardKeys = func() []kbKey {
	keys := []kbKey{{"Enter", "✓", "kb_enter"}, {"delete", "⌫", "kb_delete"}, {" ", "␣", "kb_space"}}
	for c := 'a'; c <= 'z'; c++ {
		keys = append(keys, kbKey{string(c), strings.ToUpper(string(c)), ""})
	}
	for _, c := range "1234567890" {
		keys = append(keys, kbKey{string(c), string(c), ""})
	}
	return keys
}()

// Keyboard is the on-screen keyboard: the wheel moves along the keys,
type Keyboard struct {
	index   int
	scroll  float64 // CSS px
	anim    *Anim   // 0 hidden, 1 shown
	exiting bool
	onKey   func(key string)
}

var springOut = cubicBezier(0.34, 1.32, 0.64, 1)

func (ui *IpodUI) showKeyboard(onKey func(string)) {
	kb := &Keyboard{anim: newAnim(0), onKey: onKey}
	kb.anim.To(1, 300*time.Millisecond, springOut, 0)
	ui.keyboard = kb
	ui.dirty = true
}

func (ui *IpodUI) hideKeyboard() {
	if kb := ui.keyboard; kb != nil && !kb.exiting {
		kb.exiting = true
		kb.anim.To(0, 300*time.Millisecond, easeInOut, 0)
	}
	ui.dirty = true
}

func (kb *Keyboard) scrollBy(dir int, velocity float64) {
	i := kb.index + dir*skipFor(velocity)
	if i < 0 {
		i = 0
	}
	if i > len(keyboardKeys)-1 {
		i = len(keyboardKeys) - 1
	}
	kb.index = i
}

func (kb *Keyboard) center(ui *IpodUI) {
	k := keyboardKeys[kb.index].key
	if k == "Enter" {
		ui.hideKeyboard()
	}
	if kb.onKey != nil {
		kb.onKey(k)
	}
}

// keyboard gradients and cached layers
var kbBgGrad = []GradStop{{0, hex("#ffffff")}, {0.097, hex("#a9a9a9")}, {0.5024, hex("#232323")}, {0.8958, hex("#181d1b")}}
var kbOptsGrad = []GradStop{{0, rgb(0, 0, 0)}, {1, rgba(102, 102, 102, 0)}}
var kbHiGrad = []GradStop{{0, hex("#8aebf7")}, {1, hex("#258af9")}}
var kbLayer, kbStatic, kbInset *image.RGBA

func (ui *IpodUI) drawKeyboard(dst *image.RGBA, now time.Time) {
	kb := ui.keyboard
	if kb == nil {
		return
	}
	v := kb.anim.Get(now)
	if kb.exiting && !kb.anim.Running() {
		ui.keyboard = nil
		return
	}
	// The keyboard is a 304x51.6 CSS px panel at the bottom of the screen
	// (8 px margin, with a soft shadow): a glossy dark frame and an inset
	// strip of 20 px keys that scrolls to keep the selected key visible.
	// The static part is drawn once (kbStatic); every frame copies it into
	// kbLayer, draws the keys and the inset shadow, and blits the layer,
	// scaled while it pops up.
	//
	//
	left := float64(kb.index) * 20
	if left < kb.scroll {
		kb.scroll = left
	} else if left+20 > kb.scroll+286 {
		kb.scroll = left + 20 - 286
	}
	ox := 8.0
	x0 := ox
	x1, y1 := ox+304, ox+51.6
	// shadow, frame and key strip
	//
	if kbStatic == nil {
		st := newCanvas(px(x1-x0)+px(ox*2), px(y1-x0)+px(ox*2))
		// box-shadow
		for i := 0; i < px(5); i++ {
			a := (1 - float64(i)/float64(px(5))) * 0.39 / 2
			fillRoundedGradient(st, x0*DPR-float64(i)/2, x0*DPR-float64(i)/2, x1*DPR+float64(i)/2, y1*DPR+float64(i)/2, 8*DPR+float64(i), []GradStop{{0, Col{0, 0, 0, a}}, {1, Col{0, 0, 0, a}}}, 1)
		}
		fillRoundedGradient(st, x0*DPR, (x0-4)*DPR, x1*DPR, (y1-4)*DPR, 8*DPR, kbBgGrad, 1)
		// key strip
		fillRoundedGradient(st, (x0+8)*DPR, (x0+8)*DPR, (x1-8)*DPR, (y1-16)*DPR, 4*DPR, kbOptsGrad, 1)
		kbStatic = st
	}
	layer := newCanvasCached(&kbLayer, px(x1-x0)+px(ox*2), px(y1-x0)+px(ox*2))
	copy(layer.Pix, kbStatic.Pix)
	clip := image.Rect(px(x0+8)+px(1), px(x0+8)+px(1), px(x1-8)-px(1), px(y1-16)-px(1))
	in := layer.SubImage(clip).(*image.RGBA)
	f := face(W700, 16)
	_, asc := lineBox(16)
	baseline := asc + 3.115
	for i, k := range keyboardKeys {
		bx := 17 + float64(i)*20 - kb.scroll
		if bx+20 < 16 || bx > 304 {
			continue
		}
		// selected key: blue
		if i == kb.index {
			fillRoundedGradient(in, bx*DPR, 17*DPR, (bx+20)*DPR, 42.6*DPR, 4*DPR, kbHiGrad, 1)
		}
		if k.icon != "" {
			img := ui.asset(k.icon)
			iw := float64(img.Bounds().Dx())
			blit(in, img, px(bx+(20-iw/DPR)/2), px(17+baseline-15.8), 1)
			continue
		}
		tw := textWidth(f, k.label)
		drawText(in, f, k.label, (bx+(20-tw/DPR)/2)*DPR, (17+baseline)*DPR, colWhite)
	}
	// inset shadow of the key strip
	if kbInset == nil {
		kbInset = newCanvas(layer.Bounds().Dx(), layer.Bounds().Dy())
		for i := 0; i < px(3); i++ {
			a := (1 - float64(i)/float64(px(3))) * 0.6
			strokeRounded(kbInset, (x0+8)*DPR+float64(i), (x0+8)*DPR+float64(i), (x1-8)*DPR-float64(i), (y1-16)*DPR-float64(i), 4*DPR, 1, Col{0, 0, 0, a}, 1)
		}
		strokeRounded(kbInset, (x0+8)*DPR, (x0+8)*DPR, (x1-8)*DPR, (y1-16)*DPR, 4*DPR, 1*DPR, hex("#454545"), 1)
	}
	blit(layer, kbInset, 0, 0, 1)
	// pop-up: scale 0.6 → 1 around the centre of the panel
	sc := 0.6 + 0.4*v
	dx, dy := 0.0, 240-(y1+ox)
	//
	if sc > 0.995 && v >= 1 {
		blit(dst, layer, px(dx), px(dy), 1)
		return
	}
	scaled := scaleImage(layer, int(float64(layer.Bounds().Dx())*sc), int(float64(layer.Bounds().Dy())*sc))
	cx := dx + float64(layer.Bounds().Dx())/DPR/2
	cy := dy + float64(layer.Bounds().Dy())/DPR/2
	blit(dst, scaled, px(cx)-scaled.Bounds().Dx()/2, px(cy)-scaled.Bounds().Dy()/2, math.Max(0, math.Min(1, v)))
}

// searchState is the text typed in Search.
type searchState struct {
	term string
}

// pluralize returns one or many depending on n.
// Only English plurals are needed ("1 song", "2 songs"): the
// interface is in English like the iPod.
func pluralize(n int, one, many string) string {
	if n == 1 {
		return one
	}
	return many
}

func (ui *IpodUI) searchView() *View {
	st := &searchState{}
	v := &View{ID: "search", Title: "Search", Kind: KindFull, Empty: "No results"}
	v.Build = func() []Item {
		sub := "Enter text to search"
		if st.term != "" {
			sub = "Results for: " + st.term
		}
		items := []Item{{Label: "Search", Sublabel: sub, Icon: "search_icon", OnSelect: func() {
			ui.showKeyboard(func(k string) { ui.searchKey(v, st, k) })
		}}}
		q := strings.ToLower(strings.TrimSpace(st.term))
		if q == "" {
			return items
		}
		lib := ui.library()
		var artists []*Artist
		for _, a := range sortedArtists(lib.Artists) {
			if strings.Contains(strings.ToLower(a.Name), q) {
				artists = append(artists, a)
			}
		}
		var albums []*Album
		for _, a := range lib.Albums {
			if strings.Contains(strings.ToLower(a.Name), q) {
				albums = append(albums, a)
			}
		}
		var songs []*Track
		for _, t := range lib.Tracks {
			if strings.Contains(strings.ToLower(t.Title), q) {
				songs = append(songs, t)
			}
		}
		var playlists []*Playlist
		for _, p := range lib.Playlists {
			if strings.Contains(strings.ToLower(p.Name), q) {
				playlists = append(playlists, p)
			}
		}
		if len(artists) > 0 {
			items = append(items, Item{Label: "Artists", Icon: "artists_icon",
				Sublabel: fmt.Sprintf("%d %s", len(artists), pluralize(len(artists), "artist", "artists")),
				OnSelect: func() { ui.showView(ui.artistsListView(artists, true), "") }})
		}
		if len(albums) > 0 {
			items = append(items, Item{Label: "Albums", Icon: "albums_icon",
				Sublabel: fmt.Sprintf("%d %s", len(albums), pluralize(len(albums), "album", "albums")),
				OnSelect: func() { ui.showView(ui.albumsView(albums), "") }})
		}
		if len(songs) > 0 {
			items = append(items, Item{Label: "Songs", Icon: "song_icon",
				Sublabel: fmt.Sprintf("%d %s", len(songs), pluralize(len(songs), "song", "songs")),
				OnSelect: func() { ui.showView(ui.songsView(songs), "") }})
		}
		if len(playlists) > 0 {
			items = append(items, Item{Label: "Playlists", Icon: "playlist_icon",
				Sublabel: fmt.Sprintf("%d %s", len(playlists), pluralize(len(playlists), "playlist", "playlists")),
				OnSelect: func() { ui.showView(ui.playlistsListView(playlists), "") }})
		}
		return items
	}
	v.Items = v.Build()
	// the keyboard opens right away
	ui.showKeyboard(func(k string) { ui.searchKey(v, st, k) })
	return v
}

func (ui *IpodUI) searchKey(v *View, st *searchState, k string) {
	switch k {
	case "Enter":
	case "delete":
		if r := []rune(st.term); len(r) > 0 {
			st.term = string(r[:len(r)-1])
		}
	default:
		st.term += k
	}
	v.Items = v.Build()
	if v.Index >= len(v.Items) {
		v.Index = 0
	}
	v.dirty = true
	ui.dirty = true
}

// artistsListView lists artists (with the first album's cover if thumbs).
func (ui *IpodUI) artistsListView(artists []*Artist, thumbs bool) *View {
	v := &View{ID: "artists", Title: "Artists", Kind: KindFull, Empty: "No artists"}
	for _, ar := range artists {
		//
		it := Item{Label: ar.Name, OnSelect: func() { ui.showView(ui.artistView(ar), ar.Name) }}
		if thumbs {
			if len(ar.Albums) > 0 {
				it.Album = ar.Albums[0]
			} else {
				it.Icon = "artists_icon"
			}
		}
		v.Items = append(v.Items, it)
	}
	return v
}

func (ui *IpodUI) playlistsListView(pls []*Playlist) *View {
	v := ui.playlistsView()
	var items []Item
	for _, it := range v.Items {
		for _, p := range pls {
			if it.Label == p.Name {
				items = append(items, it)
				break
			}
		}
	}
	v.Items = items
	return v
}
