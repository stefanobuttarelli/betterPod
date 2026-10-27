package main

// ij_coverpick.go — Settings → Library → Missing Covers: the albums
// without a cover, one by one.
//
//	Missing Covers            Hercules: An Original…
//	  Search All Automatically  ⌕ Edit search
//	  Hercules: An Original…    ▣ Hercules — Alan Menken · 1997 · Best match
//	  Live at the Pier          ▣ Hercules — Hercules and Love Affair · 2008
//
// Choosing an album searches MusicBrainz for it at once (title and artist,
// then the title alone, then its main part before ":") and lists everything
// MusicBrainz answers, unfiltered: the ones that pass the automatic search's
// checks first ("Best match"), each with its cover from the Cover Art
// Archive as a thumbnail. Choosing one downloads that cover (1200 px) and
// gives it to the album. If nothing fits, "Edit search" opens the keyboard
// on the search to look for anything by hand (plain words, matched against
// title, artist and the rest). Errors are shown as they are, so a missing
// connection doesn't look like "no results".

import (
	"bytes"
	"fmt"
	"image"
	"strings"
)

// missingCoversView: "Search All Automatically" (the old one-shot search)
// and the albums without a cover.
func (ui *IpodUI) missingCoversView() *View {
	v := &View{ID: "missingCovers", Title: "Missing Covers", Kind: KindFull}
	var saved string // the album that just got a cover
	v.Build = func() []Item {
		albums := ui.missingCovers()
		var items []Item
		if saved != "" {
			items = append(items, Item{Label: "Cover saved", Sublabel: saved, NoArrow: true})
		}
		if len(albums) == 0 {
			items = append(items, Item{Label: "Every album has a cover", NoArrow: true})
			return items
		}
		items = append(items, Item{Label: "Search All Automatically",
			Sublabel: fmt.Sprintf("%d %s, best matches only", len(albums), pluralize(len(albums), "album", "albums")),
			OnSelect: func() { ui.showView(ui.coverView(), "") }})
		for _, a := range albums {
			a := a
			sub := a.Artist
			if n := len(a.Tracks); n > 0 {
				sub = strings.Trim(sub+fmt.Sprintf(" · %d %s", n, pluralize(n, "song", "songs")), " ·")
			}
			items = append(items, Item{Label: a.Name, Sublabel: sub, Album: a,
				OnSelect: func() {
					ui.showView(ui.coverPickView(a, func() { saved = a.Name }), "")
				}})
		}
		return items
	}
	v.Items = v.Build()
	return v
}

// a result of the cover search
type coverHit struct {
	g     mbGroup
	best  bool   // passes the automatic search's checks
	thumb *Album // stand-in album for the thumbnail (ArtImg once loaded)
	state string // "" · "loading" (thumbnail) · "none" (no cover) · "error"
}

func (h *coverHit) sublabel() string {
	parts := []string{}
	if a := h.g.artist(); a != "" {
		parts = append(parts, a)
	}
	if len(h.g.First) >= 4 {
		parts = append(parts, h.g.First[:4])
	}
	kind := h.g.Primary
	if h.g.soundtrack() {
		kind = "Soundtrack"
	}
	if kind != "" && kind != "Album" {
		parts = append(parts, kind)
	}
	switch h.state {
	case "none":
		parts = append(parts, "No cover image")
	case "error":
		parts = append(parts, "Cover not loaded")
	default:
		if h.best {
			parts = append(parts, "Best match")
		}
	}
	return strings.Join(parts, " · ")
}

// coverPickView: the search for one album and what MusicBrainz answers.
func (ui *IpodUI) coverPickView(a *Album, onSaved func()) *View {
	q := newAlbumQuery(a)
	st := &struct {
		term    string // the search row's text
		manual  bool   // the search was typed (else: the automatic one)
		ran     string // what the results are for
		busy    bool
		status  string // an error, "No results", or the download
		hits    []*coverHit
		gen     int
		getting *coverHit // the cover being downloaded
	}{}
	st.term = strings.TrimSpace(cleanTitle(a.Name))
	if len(q.artists) > 0 {
		st.term += " " + cleanArtist(q.artists[0])
	}
	v := &View{ID: "coverPick", Title: a.Name, Kind: KindFull}
	var run func(manual bool)
	var pick func(h *coverHit)
	v.Build = func() []Item {
		editing := ui.keyboardEditing(&st.term)
		label, sub := "Edit search", st.term
		switch {
		case editing:
			label, sub = "Search MusicBrainz", ui.withCaret(&st.term)
		case st.ran != "" && !st.busy:
			sub = fmt.Sprintf("%s · %d %s", st.ran, len(st.hits), pluralize(len(st.hits), "result", "results"))
		}
		edit := func() {
			ui.showKeyboard(&st.term, func() { ui.refreshView(v) }, func() { run(true) })
			ui.refreshView(v)
		}
		items := []Item{{Label: label, Sublabel: sub, Icon: "search_icon", OnSelect: edit,
			LongPress: []SheetOption{
				{Label: "Clear search", OnSelect: func() { st.term = ""; edit() }},
				{Label: "Search automatically again", OnSelect: func() { run(false) }},
			}}}
		switch {
		case st.busy:
			items = append(items, Item{Label: "Searching MusicBrainz…", NoArrow: true})
		case st.status != "":
			label, sub, _ := strings.Cut(st.status, " · ")
			items = append(items, Item{Label: label, Sublabel: sub, NoArrow: true, OnSelect: func() {
				if strings.HasSuffix(st.status, "press to retry") {
					run(st.manual)
				}
			}})
		}
		for _, h := range st.hits {
			h := h
			sub := h.sublabel()
			if st.getting == h {
				sub = "Downloading the cover…"
			}
			items = append(items, Item{Label: h.g.Title, Sublabel: sub, Album: h.thumb,
				OnSelect: func() { pick(h) }})
		}
		if !st.busy && st.ran != "" && len(st.hits) > 0 {
			items = append(items, Item{Label: "None of these?", Sublabel: "Edit the search and try other words", OnSelect: edit})
		}
		return items
	}

	// thumbnails: one by one, front-250 from the Cover Art Archive
	loadThumbs := func(gen int, hits []*coverHit) {
		go func() {
			for _, h := range hits {
				if gen != st.gen || !ui.app.Running {
					return
				}
				data, err := caaImage(h.g.ID, "front-250")
				var img image.Image
				if err == nil && data != nil {
					img, _, err = image.Decode(bytes.NewReader(data))
				}
				h := h
				ui.post(func() {
					switch {
					case err != nil:
						h.state = "error"
					case img == nil:
						h.state = "none"
					default:
						h.state = ""
						h.thumb.ArtImg = img
						ui.artc.dropAlbum(h.thumb)
					}
					if gen == st.gen {
						ui.refreshView(v)
					}
				})
			}
		}()
	}

	run = func(manual bool) {
		term := strings.TrimSpace(st.term)
		if manual && term == "" {
			return
		}
		st.gen++
		gen := st.gen
		st.manual, st.busy, st.status, st.hits, st.getting = manual, true, "", nil, nil
		st.ran = term
		if !manual {
			st.ran = "Automatic search"
		}
		v.Index = 0
		ui.refreshView(v)
		go func() {
			var groups []mbGroup
			var err error
			seen := map[string]bool{}
			add := func(gs []mbGroup) {
				for _, g := range gs {
					if !seen[g.ID] && len(groups) < 25 {
						seen[g.ID] = true
						groups = append(groups, g)
					}
				}
			}
			if manual {
				var gs []mbGroup
				gs, err = mbSearch(term, true, 25)
				add(gs)
			} else {
				title := cleanTitle(q.title)
				var queries []string
				if len(q.artists) > 0 {
					queries = append(queries, "releasegroup:("+lucene(title)+") AND artist:("+lucene(cleanArtist(q.artists[0]))+")")
				}
				queries = append(queries, "releasegroup:("+lucene(title)+")")
				if m := cleanTitle(mainTitle(q.title)); m != title && m != "" {
					queries = append(queries, "releasegroup:("+lucene(m)+")")
				}
				for _, qs := range queries {
					if gen != st.gen {
						return
					}
					gs, e := mbSearch(qs, false, 15)
					if e != nil {
						err = e
						break
					}
					add(gs)
					logMsg(fmt.Sprintf("Covers: %q: %d results for %s", a.Name, len(gs), qs))
					if len(groups) >= 10 {
						break
					}
				}
			}
			ui.post(func() {
				if gen != st.gen {
					return
				}
				st.busy = false
				var best, rest []*coverHit
				for _, g := range groups {
					h := &coverHit{g: g, best: q.titleOK(g) && (q.artistOK(g) || q.soundtrack || q.various),
						thumb: &Album{Name: g.Title, Artist: g.artist()}, state: "loading"}
					if h.best {
						best = append(best, h)
					} else {
						rest = append(rest, h)
					}
				}
				st.hits = append(best, rest...)
				switch {
				case err != nil && len(st.hits) == 0:
					st.status = "MusicBrainz didn't answer · " + iaErrText(err) + " · press to retry"
					logMsg("Covers: " + a.Name + ": " + err.Error())
				case len(st.hits) == 0 && manual:
					st.status = "No results · MusicBrainz has nothing for these words"
				case len(st.hits) == 0:
					st.status = "No results · edit the search above and try other words"
				}
				ui.refreshView(v)
				if len(st.hits) > 0 {
					v.Index = 1
					if st.status != "" {
						v.Index = 2
					}
					loadThumbs(gen, st.hits)
				}
			})
		}()
	}

	pick = func(h *coverHit) {
		if st.getting != nil {
			return
		}
		if h.state == "none" {
			st.status = "“" + h.g.Title + "” has no cover image"
			ui.refreshView(v)
			return
		}
		st.getting, st.status = h, ""
		ui.refreshView(v)
		go func() {
			data, err := caaFront(h.g.ID)
			if err == nil && data != nil {
				saveCoverFile(a, data)
			}
			ui.post(func() {
				st.getting = nil
				switch {
				case err != nil:
					st.status = "Cover not downloaded · " + iaErrText(err)
				case data == nil:
					h.state = "none"
					st.status = "“" + h.g.Title + "” has no cover image"
				default:
					ui.setAlbumCover(a, data)
					if err := ui.app.saveLibraryJSON(); err != nil {
						logMsg("Covers: saving the library: " + err.Error())
					}
					logMsg(fmt.Sprintf("Covers: %q: chose %q by %q (%s)", a.Name, h.g.Title, h.g.artist(), h.g.ID))
					if onSaved != nil {
						onSaved()
					}
					ui.hideView()
					return
				}
				ui.refreshView(v)
			})
		}()
	}

	st.busy = true
	v.Items = v.Build()
	run(false)
	return v
}
