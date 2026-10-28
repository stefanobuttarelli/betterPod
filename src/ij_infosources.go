package main

// ij_infosources.go — where Album Info (ij_albuminfo.go) looks an album
// up. No keys and no accounts:
//
//   - MusicBrainz: releases with their songs (titles, artists, numbers,
//     discs, lengths) and genres;
//   - iTunes and Deezer: the shops' albums with their songs;
//   - Wikipedia: the article on the album, or on the game a soundtrack
//     comes from. No songs, but its infobox gives the artist (for a game:
//     the composer, else the developer), the year and the genre. This is
//     what a game rip (Colin McRae Rally 2.0) can get.
//
// A search gives albumInfo rows; detail() fetches the songs of the one
// chosen. The bases are the cover sources' (MIYOOPOD_MB_BASE,
// MIYOOPOD_ITUNES_BASE, MIYOOPOD_DEEZER_BASE, MIYOOPOD_WIKI_BASE).

import (
	"fmt"
	"math"
	"net/url"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

type infoTrack struct {
	Disc, Num     int
	Title, Artist string
	Secs          float64
}

type albumInfo struct {
	Source                    string
	ID                        string // the album at the source (Wikipedia: the article)
	Title, Artist, Year, Kind string
	Genre                     string
	ArtistNote                string // "composer", "developer" (Wikipedia)
	Count                     int    // songs, when the search says (0: unknown)
	Tracks                    []infoTrack
	soundtrack                bool
	fetched                   bool
}

// cand: the result as a cover search result, for the same checks.
func (a albumInfo) cand() coverCand {
	return coverCand{Source: a.Source, Title: a.Title, Artist: a.Artist, soundtrack: a.soundtrack}
}

type infoSource struct {
	name   string
	search func(term string) ([]albumInfo, error)
	auto   func(q albumQuery) ([]albumInfo, error) // the album's own search (nil: its title and artist as words)
}

var infoSources = []infoSource{
	{"MusicBrainz", mbReleaseSearch, mbReleaseAuto},
	{"iTunes", itunesAlbumSearch, nil},
	{"Deezer", deezerAlbumSearch, nil},
	{"Wikipedia", wikiAlbumSearch, nil},
}

// find: the album in a source (title and artist, then the title alone).
func (s infoSource) find(q albumQuery) ([]albumInfo, error) {
	if s.auto != nil {
		return s.auto(q)
	}
	var as []albumInfo
	var err error
	for _, t := range q.sourceTerms() {
		if as, err = s.search(t); len(as) > 0 {
			return as, nil
		}
	}
	return as, err
}

// ---- MusicBrainz ----

type mbCredit []struct {
	Name string `json:"name"`
	Join string `json:"joinphrase"`
}

func (c mbCredit) String() string {
	var b strings.Builder
	for _, x := range c {
		b.WriteString(x.Name + x.Join)
	}
	return strings.TrimSpace(b.String())
}

// mbReleaseSearch: plain words, matched against title, artist and the rest.
func mbReleaseSearch(term string) ([]albumInfo, error) { return mbReleaseQuery(term, true) }

// mbReleaseAuto: the title with the first artist, then the title alone, in
// MusicBrainz's query syntax (as the cover search does).
func mbReleaseAuto(q albumQuery) ([]albumInfo, error) {
	title := cleanTitle(q.title)
	var queries []string
	if len(q.artists) > 0 {
		queries = append(queries, "release:("+lucene(title)+") AND artist:("+lucene(cleanArtist(q.artists[0]))+")")
	}
	queries = append(queries, "release:("+lucene(title)+")")
	for _, qs := range queries {
		as, err := mbReleaseQuery(qs, false)
		if err != nil || len(as) > 0 {
			return as, err
		}
	}
	return nil, nil
}

func mbReleaseQuery(query string, dismax bool) ([]albumInfo, error) {
	mbWait()
	u := fmt.Sprintf("%s/ws/2/release/?fmt=json&limit=10&query=%s", mbBase(), url.QueryEscape(query))
	if dismax {
		u += "&dismax=true"
	}
	var out struct {
		Releases []struct {
			ID     string   `json:"id"`
			Title  string   `json:"title"`
			Date   string   `json:"date"`
			Count  int      `json:"track-count"`
			Credit mbCredit `json:"artist-credit"`
			Group  struct {
				Primary   string   `json:"primary-type"`
				Secondary []string `json:"secondary-types"`
			} `json:"release-group"`
		} `json:"releases"`
	}
	if err := coverJSON("MusicBrainz", u, &out); err != nil {
		return nil, err
	}
	var as []albumInfo
	seen := map[string]bool{}
	for _, r := range out.Releases {
		a := albumInfo{Source: "MusicBrainz", ID: r.ID, Title: r.Title, Artist: r.Credit.String(), Count: r.Count}
		if len(r.Date) >= 4 {
			a.Year = r.Date[:4]
		}
		g := mbGroup{Primary: r.Group.Primary, Secondary: r.Group.Secondary}
		a.Kind, a.soundtrack = g.cand().Kind, g.soundtrack()
		// the same album in several pressings: the first of each
		k := strings.ToLower(a.Title + "|" + a.Artist + "|" + strconv.Itoa(a.Count))
		if !seen[k] {
			seen[k] = true
			as = append(as, a)
		}
	}
	return as, nil
}

func mbReleaseDetail(a *albumInfo) error {
	mbWait()
	u := fmt.Sprintf("%s/ws/2/release/%s?fmt=json&inc=recordings+artist-credits+genres", mbBase(), url.PathEscape(a.ID))
	var out struct {
		Genres []struct {
			Name  string `json:"name"`
			Count int    `json:"count"`
		} `json:"genres"`
		Media []struct {
			Position int `json:"position"`
			Tracks   []struct {
				Position int      `json:"position"`
				Title    string   `json:"title"`
				Length   int      `json:"length"`
				Credit   mbCredit `json:"artist-credit"`
			} `json:"tracks"`
		} `json:"media"`
	}
	if err := coverJSON("MusicBrainz", u, &out); err != nil {
		return err
	}
	for _, m := range out.Media {
		for _, t := range m.Tracks {
			a.Tracks = append(a.Tracks, infoTrack{Disc: m.Position, Num: t.Position, Title: t.Title,
				Artist: t.Credit.String(), Secs: float64(t.Length) / 1000})
		}
	}
	best := 0
	for _, g := range out.Genres {
		if g.Count > best {
			best, a.Genre = g.Count, titleCase(g.Name)
		}
	}
	return nil
}

// titleCase: "video game music" → "Video Game Music".
func titleCase(s string) string {
	ws := strings.Fields(s)
	for i, w := range ws {
		if r := []rune(w); len(r) > 0 {
			ws[i] = strings.ToUpper(string(r[0])) + string(r[1:])
		}
	}
	return strings.Join(ws, " ")
}

// ---- iTunes ----

func itunesAlbumSearch(term string) ([]albumInfo, error) {
	itunesWait()
	u := fmt.Sprintf("%s/search?entity=album&limit=8&term=%s", envBase("MIYOOPOD_ITUNES_BASE", "https://itunes.apple.com"), url.QueryEscape(term))
	var out struct {
		Results []struct {
			ID         int64  `json:"collectionId"`
			Collection string `json:"collectionName"`
			Artist     string `json:"artistName"`
			Released   string `json:"releaseDate"`
			Genre      string `json:"primaryGenreName"`
			Count      int    `json:"trackCount"`
		} `json:"results"`
	}
	if err := coverJSON("iTunes", u, &out); err != nil {
		return nil, err
	}
	var as []albumInfo
	for _, r := range out.Results {
		a := albumInfo{Source: "iTunes", ID: strconv.FormatInt(r.ID, 10), Title: r.Collection, Artist: r.Artist,
			Genre: r.Genre, Count: r.Count}
		for _, k := range []string{"Single", "EP"} {
			if t, ok := strings.CutSuffix(a.Title, " - "+k); ok {
				a.Title, a.Kind = t, k
			}
		}
		if len(r.Released) >= 4 {
			a.Year = r.Released[:4]
		}
		if strings.Contains(strings.ToLower(r.Genre), "soundtrack") {
			a.Kind, a.soundtrack = "Soundtrack", true
		}
		as = append(as, a)
	}
	return as, nil
}

func itunesAlbumDetail(a *albumInfo) error {
	itunesWait()
	u := fmt.Sprintf("%s/lookup?entity=song&limit=200&id=%s", envBase("MIYOOPOD_ITUNES_BASE", "https://itunes.apple.com"), url.QueryEscape(a.ID))
	var out struct {
		Results []struct {
			Wrapper string  `json:"wrapperType"`
			Title   string  `json:"trackName"`
			Artist  string  `json:"artistName"`
			Num     int     `json:"trackNumber"`
			Disc    int     `json:"discNumber"`
			Millis  float64 `json:"trackTimeMillis"`
		} `json:"results"`
	}
	if err := coverJSON("iTunes", u, &out); err != nil {
		return err
	}
	for _, r := range out.Results {
		if r.Wrapper == "track" {
			a.Tracks = append(a.Tracks, infoTrack{Disc: r.Disc, Num: r.Num, Title: r.Title, Artist: r.Artist, Secs: r.Millis / 1000})
		}
	}
	return nil
}

// ---- Deezer ----

func deezerAlbumSearch(term string) ([]albumInfo, error) {
	base := envBase("MIYOOPOD_DEEZER_BASE", "https://api.deezer.com")
	var out struct {
		Data []struct {
			ID     int64  `json:"id"`
			Title  string `json:"title"`
			Count  int    `json:"nb_tracks"`
			Type   string `json:"record_type"`
			Artist struct {
				Name string `json:"name"`
			} `json:"artist"`
		} `json:"data"`
		Error *struct {
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := coverJSON("Deezer", fmt.Sprintf("%s/search/album?limit=8&q=%s", base, url.QueryEscape(term)), &out); err != nil {
		return nil, err
	}
	if out.Error != nil {
		return nil, fmt.Errorf("Deezer: %s", out.Error.Message)
	}
	var as []albumInfo
	for _, r := range out.Data {
		a := albumInfo{Source: "Deezer", ID: strconv.FormatInt(r.ID, 10), Title: r.Title, Artist: r.Artist.Name, Count: r.Count}
		switch r.Type {
		case "ep":
			a.Kind = "EP"
		case "single":
			a.Kind = "Single"
		case "compile":
			a.Kind = "Compilation"
		}
		as = append(as, a)
	}
	return as, nil
}

func deezerAlbumDetail(a *albumInfo) error {
	base := envBase("MIYOOPOD_DEEZER_BASE", "https://api.deezer.com")
	type dzErr *struct {
		Message string `json:"message"`
	}
	var album struct {
		Error    dzErr  `json:"error"` // Deezer's errors (its quota) come with 200
		Released string `json:"release_date"`
		Genres   struct {
			Data []struct {
				Name string `json:"name"`
			} `json:"data"`
		} `json:"genres"`
	}
	if err := coverJSON("Deezer", base+"/album/"+url.PathEscape(a.ID), &album); err != nil {
		return err
	}
	if album.Error != nil {
		return fmt.Errorf("Deezer: %s", album.Error.Message)
	}
	if len(album.Released) >= 4 && album.Released[:4] != "0000" {
		a.Year = album.Released[:4]
	}
	if len(album.Genres.Data) > 0 {
		a.Genre = album.Genres.Data[0].Name
	}
	var tracks struct {
		Error dzErr `json:"error"`
		Data  []struct {
			Title  string  `json:"title"`
			Secs   float64 `json:"duration"`
			Num    int     `json:"track_position"`
			Disc   int     `json:"disk_number"`
			Artist struct {
				Name string `json:"name"`
			} `json:"artist"`
		} `json:"data"`
	}
	if err := coverJSON("Deezer", base+"/album/"+url.PathEscape(a.ID)+"/tracks?limit=300", &tracks); err != nil {
		return err
	}
	if tracks.Error != nil {
		return fmt.Errorf("Deezer: %s", tracks.Error.Message)
	}
	for i, t := range tracks.Data {
		n := t.Num
		if n == 0 {
			n = i + 1
		}
		a.Tracks = append(a.Tracks, infoTrack{Disc: t.Disc, Num: n, Title: t.Title, Artist: t.Artist.Name, Secs: t.Secs})
	}
	return nil
}

// ---- Wikipedia ----

func wikiAlbumSearch(term string) ([]albumInfo, error) {
	cs, err := wikiSearch(term, 8)
	if err != nil {
		return nil, err
	}
	var as []albumInfo
	for _, c := range cs {
		if c.Kind == "Single" {
			continue
		}
		as = append(as, albumInfo{Source: "Wikipedia", ID: c.ID, Title: c.Title, Artist: c.Artist, Year: c.Year,
			Kind: c.Kind, soundtrack: c.soundtrack})
	}
	return as, nil
}

var (
	reWikiRef      = regexp.MustCompile(`(?s)<ref[^>/]*/>|<ref[^>]*>.*?</ref>`)
	reWikiTag      = regexp.MustCompile(`<[^>]+>`)
	reWikiLink     = regexp.MustCompile(`\[\[(?:[^|\]]*\|)?([^\]]*)\]\]`)
	reWikiNote     = regexp.MustCompile(`\{\{(?i:efn|refn|sfn|nbsp|citation needed|cn)[^{}]*\}\}`)
	reWikiListTmpl = regexp.MustCompile(`\{\{(?i:hlist|flatlist|plainlist|ubl|unbulleted list|plain list)\s*\|`)
	reWikiField    = regexp.MustCompile(`(?m)^[ \t]*\|[ \t]*([a-z_ ]+?)[ \t]*=[ \t]*(.*)$`)
)

// wikiClean: a field of an infobox as plain text, the first item of a
// list ("[[Codemasters]]{{efn|…}}" → "Codemasters").
var reWikiBr = regexp.MustCompile(`(?i)<br\s*/?>`)

func wikiClean(s string) string {
	s = reWikiRef.ReplaceAllString(s, "")
	s = reWikiNote.ReplaceAllString(s, "")
	s = reWikiListTmpl.ReplaceAllString(s, "")
	s = reWikiLink.ReplaceAllString(s, "$1")
	s = strings.NewReplacer("'''", "", "''", "", "&nbsp;", " ", "}}", "").Replace(s)
	s = reWikiBr.ReplaceAllString(s, "\n")
	s = reWikiTag.ReplaceAllString(s, "")
	for _, sep := range []string{"\n", "|", "*", ","} {
		for _, part := range strings.Split(s, sep) {
			if p := strings.TrimSpace(part); p != "" && !strings.Contains(p, "{{") {
				s = p
				break
			}
		}
	}
	return strings.TrimSpace(strings.Trim(s, "*{}| "))
}

func wikiAlbumDetail(a *albumInfo) error {
	u := envBase("MIYOOPOD_WIKI_BASE", "https://en.wikipedia.org") +
		"/w/api.php?action=parse&format=json&formatversion=2&prop=wikitext&section=0&page=" + url.QueryEscape(a.ID)
	var out struct {
		Parse struct {
			Wikitext string `json:"wikitext"`
		} `json:"parse"`
	}
	if err := coverJSON("Wikipedia", u, &out); err != nil {
		return err
	}
	fields := map[string]string{}
	for _, m := range reWikiField.FindAllStringSubmatch(out.Parse.Wikitext, -1) {
		k := strings.ToLower(strings.TrimSpace(m[1]))
		if _, ok := fields[k]; !ok {
			fields[k] = m[2]
		}
	}
	for _, k := range []string{"artist", "composer", "developer"} {
		if v := wikiClean(fields[k]); v != "" {
			a.Artist = v
			if k != "artist" {
				a.ArtistNote = k
			}
			break
		}
	}
	if y := reWikiYear.FindString(wikiClean(fields["released"])); y != "" {
		a.Year = y
	} else if y := reWikiYear.FindString(fields["released"]); y != "" {
		a.Year = y
	}
	if a.Kind == "Video game" || a.Kind == "Soundtrack" {
		a.Genre = "Soundtrack"
	} else if g := wikiClean(fields["genre"]); g != "" {
		a.Genre = g
	}
	return nil
}

// detail fetches the songs (and what else the search didn't give).
func (a *albumInfo) detail() error {
	if a.fetched {
		return nil
	}
	var err error
	switch a.Source {
	case "MusicBrainz":
		err = mbReleaseDetail(a)
	case "iTunes":
		err = itunesAlbumDetail(a)
	case "Deezer":
		err = deezerAlbumDetail(a)
	case "Wikipedia":
		err = wikiAlbumDetail(a)
	}
	if err == nil {
		a.fetched = true
		sort.SliceStable(a.Tracks, func(i, j int) bool {
			x, y := a.Tracks[i], a.Tracks[j]
			if max(x.Disc, 1) != max(y.Disc, 1) {
				return max(x.Disc, 1) < max(y.Disc, 1)
			}
			return x.Num < y.Num
		})
	}
	return err
}

// ---- matching the songs ----

// matchTracks pairs the album's songs with the result's, only where it's
// sure, since the result's titles and numbers go into the files:
//
//  1. the same title (an edition note aside);
//  2. the same disc and number, when the lengths agree (within 5 s), or
//     when a length is unknown and both have the same number of songs (a
//     different edition with an extra intro would shift every title);
//  3. the same length (within 3 s, the nearest), for songs without a
//     number or a title in common.
//
// Never by position: songs without numbers are in alphabetical order. It
// returns, for each of ours, the index of its match or -1 (left as it is).
func matchTracks(ours []songTags, secs []float64, theirs []infoTrack) []int {
	m := make([]int, len(ours))
	for i := range m {
		m[i] = -1
	}
	if len(theirs) == 0 {
		return m
	}
	used := make([]bool, len(theirs))
	sec := func(i int) float64 {
		if i < len(secs) {
			return secs[i]
		}
		return 0
	}
	// 1. titles
	for i, o := range ours {
		if strings.TrimSpace(o.Title) == "" {
			continue
		}
		for j, t := range theirs {
			if !used[j] && sameTitle(o.Title, t.Title) {
				m[i], used[j] = j, true
				break
			}
		}
	}
	// 2. disc and number, checked by length
	for i, o := range ours {
		if m[i] >= 0 || o.Track <= 0 {
			continue
		}
		for j, t := range theirs {
			if used[j] || t.Num != o.Track || max(t.Disc, 1) != max(o.Disc, 1) {
				continue
			}
			a, b := sec(i), t.Secs
			ok := a > 0 && b > 0 && math.Abs(a-b) <= 5
			if a <= 0 || b <= 0 {
				ok = len(ours) == len(theirs)
			}
			if ok {
				m[i], used[j] = j, true
			}
			break
		}
	}
	// 3. length
	for i := range ours {
		if m[i] >= 0 || sec(i) <= 0 {
			continue
		}
		best, bd := -1, 3.0
		for j, t := range theirs {
			if d := math.Abs(t.Secs - sec(i)); !used[j] && t.Secs > 0 && d <= bd {
				best, bd = j, d
			}
		}
		if best >= 0 {
			m[i], used[best] = best, true
		}
	}
	return m
}
