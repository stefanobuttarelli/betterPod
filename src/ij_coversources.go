package main

// ij_coversources.go — where covers are looked for besides MusicBrainz and
// the Cover Art Archive. None of them needs a key or an account:
//
//   - iTunes (Apple's search API) and Deezer: the shops' catalogues, with
//     the soundtracks of many games and films that MusicBrainz lacks
//     (covers up to 1200 px from iTunes, 1000 px from Deezer);
//   - Wikipedia: the picture at the top of an article. For an album it is
//     its cover; for a game soundtrack ripped from the game, which has no
//     album cover of its own, the article of the game gives its box. These
//     pictures are small (about 300 px: Wikipedia keeps non-free images
//     small), so Wikipedia comes last.
//
// Every result is a coverCand, the same as MusicBrainz's release groups,
// and goes through the same checks (same title, same artist) before the
// automatic search may take it. The bases can point elsewhere for tests
// (MIYOOPOD_ITUNES_BASE, MIYOOPOD_DEEZER_BASE, MIYOOPOD_WIKI_BASE).

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"io"
	"net/url"
	"os"
	"regexp"
	"sort"
	"strings"
	"sync"
	"time"
)

// coverCand is one result of a cover search, from any source.
type coverCand struct {
	Source     string // "MusicBrainz", "iTunes", "Deezer", "Wikipedia"
	ID         string // MusicBrainz: the release group (its covers come from the Cover Art Archive)
	Title      string
	Artist     string
	Year       string
	Kind       string // "Soundtrack", "EP", "Single", "Compilation", "Video game"… ("" for an album)
	Thumb      string // a picture of about 250 px (not for MusicBrainz)
	Full       string // the cover itself
	soundtrack bool   // a soundtrack, or the game it comes from
}

// cand: a MusicBrainz release group as a result.
func (g mbGroup) cand() coverCand {
	c := coverCand{Source: "MusicBrainz", ID: g.ID, Title: g.Title, Artist: g.artist(), soundtrack: g.soundtrack()}
	if len(g.First) >= 4 {
		c.Year = g.First[:4]
	}
	if g.Primary != "" && g.Primary != "Album" {
		c.Kind = g.Primary
	}
	if c.soundtrack {
		c.Kind = "Soundtrack"
	}
	return c
}

// image downloads the result's thumbnail or full cover (nil, nil if there
// is none).
func (c coverCand) image(full bool) ([]byte, error) {
	if c.Source == "MusicBrainz" {
		if full {
			return caaFront(c.ID)
		}
		return caaImage(c.ID, "front-250")
	}
	u := c.Thumb
	if full || u == "" {
		u = c.Full
	}
	if u == "" {
		return nil, nil
	}
	return coverImage(c.Source, u)
}

// coverImage downloads a picture (nil, nil when the server has none, or
// when it is not an image that can be shown).
func coverImage(source, u string) ([]byte, error) {
	resp, err := coverGetAccept(u, "image/*", 30*time.Second)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode == 404 {
		return nil, nil
	}
	if resp.StatusCode != 200 {
		return nil, fmt.Errorf("%s answered %d", source, resp.StatusCode)
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, 12<<20))
	if err != nil {
		return nil, err
	}
	if _, _, err := image.DecodeConfig(bytes.NewReader(data)); err != nil {
		return nil, nil
	}
	return data, nil
}

// coverJSON gets a JSON answer from a source.
func coverJSON(source, u string, v any) error {
	resp, err := coverGetAccept(u, "application/json", 15*time.Second)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return fmt.Errorf("%s answered %d", source, resp.StatusCode)
	}
	return json.NewDecoder(io.LimitReader(resp.Body, 2<<20)).Decode(v)
}

func envBase(env, def string) string {
	if b := os.Getenv(env); b != "" {
		return strings.TrimRight(b, "/")
	}
	return def
}

// ---- iTunes ----

// Apple asks for no more than about 20 searches a minute.
var itunesLast time.Time
var itunesMu sync.Mutex

func itunesWait() {
	itunesMu.Lock()
	defer itunesMu.Unlock()
	if d := 3*time.Second - time.Since(itunesLast); d > 0 {
		time.Sleep(d)
	}
	itunesLast = time.Now()
}

// itunesSearch: albums matching the words, with their artwork at 250 and
// 1200 px (the artwork URL names its size).
func itunesSearch(term string, limit int) ([]coverCand, error) {
	itunesWait()
	u := fmt.Sprintf("%s/search?entity=album&limit=%d&term=%s",
		envBase("MIYOOPOD_ITUNES_BASE", "https://itunes.apple.com"), limit, url.QueryEscape(term))
	var out struct {
		Results []struct {
			Collection string `json:"collectionName"`
			Artist     string `json:"artistName"`
			Released   string `json:"releaseDate"`
			Art        string `json:"artworkUrl100"`
			Genre      string `json:"primaryGenreName"`
		} `json:"results"`
	}
	if err := coverJSON("iTunes", u, &out); err != nil {
		return nil, err
	}
	var cs []coverCand
	for _, r := range out.Results {
		if r.Art == "" || !strings.Contains(r.Art, "100x100") {
			continue
		}
		c := coverCand{Source: "iTunes", Title: r.Collection, Artist: r.Artist,
			Thumb: strings.Replace(r.Art, "100x100", "250x250", 1),
			Full:  strings.Replace(r.Art, "100x100", "1200x1200", 1)}
		for _, k := range []string{"Single", "EP"} {
			if t, ok := strings.CutSuffix(c.Title, " - "+k); ok {
				c.Title, c.Kind = t, k
			}
		}
		if len(r.Released) >= 4 {
			c.Year = r.Released[:4]
		}
		if strings.Contains(strings.ToLower(r.Genre), "soundtrack") {
			c.Kind, c.soundtrack = "Soundtrack", true
		}
		cs = append(cs, c)
	}
	return cs, nil
}

// ---- Deezer ----

// deezerSearch: albums matching the words (Deezer answers its errors, such
// as too many requests, as JSON with status 200).
func deezerSearch(term string, limit int) ([]coverCand, error) {
	u := fmt.Sprintf("%s/search/album?limit=%d&q=%s",
		envBase("MIYOOPOD_DEEZER_BASE", "https://api.deezer.com"), limit, url.QueryEscape(term))
	var out struct {
		Data []struct {
			Title  string `json:"title"`
			Medium string `json:"cover_medium"`
			XL     string `json:"cover_xl"`
			MD5    string `json:"md5_image"`
			Type   string `json:"record_type"`
			Artist struct {
				Name string `json:"name"`
			} `json:"artist"`
		} `json:"data"`
		Error *struct {
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := coverJSON("Deezer", u, &out); err != nil {
		return nil, err
	}
	if out.Error != nil {
		return nil, errors.New("Deezer: " + out.Error.Message)
	}
	var cs []coverCand
	for _, r := range out.Data {
		if r.XL == "" || r.MD5 == "" { // no cover (Deezer's grey placeholder)
			continue
		}
		c := coverCand{Source: "Deezer", Title: r.Title, Artist: r.Artist.Name, Thumb: r.Medium, Full: r.XL}
		switch r.Type {
		case "ep":
			c.Kind = "EP"
		case "single":
			c.Kind = "Single"
		case "compile":
			c.Kind = "Compilation"
		}
		cs = append(cs, c)
	}
	return cs, nil
}

// ---- Wikipedia ----

var (
	reWikiDisamb = regexp.MustCompile(`\s*\([^)]*\)$`)
	reWikiYear   = regexp.MustCompile(`\b(1[89]|20)\d\d\b`)
	reWikiSong   = regexp.MustCompile(`\bsong\b`)
)

// wikiKind: what an article is about, from its short description ("1977
// studio album by Fleetwood Mac", "2000 video game"); "" for anything that
// has no cover (people, bands, songs, series).
func wikiKind(desc string) string {
	d := strings.ToLower(desc)
	switch {
	case strings.Contains(d, "video game series"), reWikiSong.MatchString(d):
		return ""
	case strings.Contains(d, "video game"):
		return "Video game"
	case strings.Contains(d, "soundtrack"), strings.Contains(d, "film score"):
		return "Soundtrack"
	case strings.Contains(d, "extended play"), strings.HasPrefix(d, "ep "), strings.Contains(d, " ep "), strings.Contains(d, " ep by"):
		return "EP"
	case strings.Contains(d, "compilation album"):
		return "Compilation"
	case strings.Contains(d, "album"), strings.Contains(d, "mixtape"):
		return "Album"
	case strings.Contains(d, "single by"):
		return "Single"
	}
	return ""
}

// wikiSearch: articles about albums, soundtracks and games matching the
// words, with the picture at their top (the free ones and the others:
// album covers and game boxes are not free).
func wikiSearch(term string, limit int) ([]coverCand, error) {
	u := envBase("MIYOOPOD_WIKI_BASE", "https://en.wikipedia.org") +
		"/w/api.php?action=query&format=json&formatversion=2&generator=search&gsrnamespace=0" +
		fmt.Sprintf("&gsrlimit=%d&gsrsearch=%s", limit, url.QueryEscape(term)) +
		"&prop=pageimages%7Cdescription&piprop=thumbnail%7Coriginal&pithumbsize=250&pilicense=any"
	var out struct {
		Query struct {
			Pages []struct {
				Title string `json:"title"`
				Index int    `json:"index"`
				Desc  string `json:"description"`
				Thumb *struct {
					Source string `json:"source"`
				} `json:"thumbnail"`
				Original *struct {
					Source string `json:"source"`
				} `json:"original"`
			} `json:"pages"`
		} `json:"query"`
	}
	if err := coverJSON("Wikipedia", u, &out); err != nil {
		return nil, err
	}
	pages := out.Query.Pages
	sort.SliceStable(pages, func(i, j int) bool { return pages[i].Index < pages[j].Index })
	var cs []coverCand
	for _, p := range pages {
		kind := wikiKind(p.Desc)
		if kind == "" || p.Original == nil || p.Original.Source == "" {
			continue
		}
		full := p.Original.Source
		if low := strings.ToLower(full); strings.Contains(low, ".svg") {
			if p.Thumb == nil {
				continue
			}
			full = p.Thumb.Source // a drawing: its rendering
		}
		c := coverCand{Source: "Wikipedia", Title: reWikiDisamb.ReplaceAllString(p.Title, ""), Kind: kind, Full: full}
		if p.Thumb != nil {
			c.Thumb = p.Thumb.Source
		}
		if i := strings.LastIndex(p.Desc, " by "); i >= 0 {
			c.Artist = strings.TrimSpace(p.Desc[i+4:])
		}
		c.Year = reWikiYear.FindString(p.Desc)
		c.soundtrack = kind == "Soundtrack" || kind == "Video game"
		if kind == "Album" {
			c.Kind = ""
		}
		cs = append(cs, c)
	}
	return cs, nil
}

// ---- all of them ----

// coverSource: a search by words in one source.
type coverSource struct {
	name   string
	search func(term string, limit int) ([]coverCand, error)
}

// otherCoverSources: after MusicBrainz, in this order.
var otherCoverSources = []coverSource{
	{"iTunes", itunesSearch},
	{"Deezer", deezerSearch},
	{"Wikipedia", wikiSearch},
}

// sourceTerms: what an album is searched for in a source other than
// MusicBrainz: its title with its first artist, then the title alone.
func (q albumQuery) sourceTerms() []string {
	title := cleanTitle(q.title)
	var terms []string
	if len(q.artists) > 0 {
		terms = append(terms, title+" "+cleanArtist(q.artists[0]))
	}
	return append(terms, title)
}

// searchSource: the album in one source, every term until one gives
// results.
func (q albumQuery) searchSource(s coverSource, limit int) ([]coverCand, error) {
	var lastErr error
	for _, t := range q.sourceTerms() {
		cs, err := s.search(t, limit)
		if err != nil {
			lastErr = err
			continue
		}
		if len(cs) > 0 {
			return cs, nil
		}
	}
	return nil, lastErr
}

// matches: a result the automatic search may take (the checks of
// mbReleaseGroups: the same title, and the same artist unless the album
// is a soundtrack or a compilation).
func (q albumQuery) matches(c coverCand) bool {
	if !q.titleOK(c) {
		return false
	}
	return q.artistOK(c) || q.soundtrack || q.various
}

// findOtherCover: the automatic search in the other sources, for an album
// MusicBrainz doesn't have (nil data: not found). A result with the very
// same title is taken before one that only shares the main title, in any
// source: "Hollow Knight: Silksong" also passes for "Hollow Knight
// (Original Soundtrack)", but not if "Hollow Knight" itself is found.
func findOtherCover(a *Album) ([]byte, string, error) {
	q := newAlbumQuery(a)
	var lastErr error
	found := make([][]coverCand, len(otherCoverSources))
	asked := make([]bool, len(otherCoverSources))
	for pass := 0; pass < 2; pass++ {
		for i, s := range otherCoverSources {
			if !asked[i] {
				asked[i] = true
				cs, err := q.searchSource(s, 8)
				if err != nil {
					lastErr = err
				}
				found[i] = cs
			}
			for _, c := range found[i] {
				if !q.matches(c) || sameTitle(q.title, c.Title) != (pass == 0) {
					continue
				}
				data, err := c.image(true)
				if err != nil {
					lastErr = err
					continue
				}
				if data != nil {
					return data, fmt.Sprintf("%s (%q by %q)", s.name, c.Title, c.Artist), nil
				}
			}
		}
	}
	return nil, "", lastErr
}
