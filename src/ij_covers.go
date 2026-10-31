package main

// ij_covers.go — Settings → Library → Missing Covers → Search All Automatically: covers for the
// albums that have none, from the Cover Art Archive (coverartarchive.org),
// found through MusicBrainz, or else from iTunes, Deezer or Wikipedia
// (ij_coversources.go).
//
// For each album without artwork: a MusicBrainz search for the release
// group (album title + artist, best matches first), then the release
// group's front cover from the Cover Art Archive at 1200 px (no huge
// originals). MusicBrainz allows about one request per second per address
// with a User-Agent that says who is asking, so requests are spaced by
// 1.1 s. An album MusicBrainz doesn't have, or has without a cover, is
// looked for in the other sources, with the same checks. A cover found is
// saved as cover.jpg in the album's folder (when the folder holds only
// that album and has no cover yet) and in the artwork cache; the library
// is saved at the end.

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	_ "image/jpeg"
	_ "image/png"
	"io"
	"math"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode"

	"golang.org/x/image/font"
	"golang.org/x/text/unicode/norm"
)

const coverUA = "betterPod/" + BETTERPOD_VERSION + " ( https://github.com/stefanobuttarelli/betterPod )"

// mbBase / caaBase can point elsewhere for tests (MIYOOPOD_MB_BASE,
// MIYOOPOD_CAA_BASE).
func mbBase() string {
	if b := os.Getenv("MIYOOPOD_MB_BASE"); b != "" {
		return strings.TrimRight(b, "/")
	}
	return "https://musicbrainz.org"
}

func caaBase() string {
	if b := os.Getenv("MIYOOPOD_CAA_BASE"); b != "" {
		return strings.TrimRight(b, "/")
	}
	return "https://coverartarchive.org"
}

var mbLast time.Time
var mbMu sync.Mutex

// mbWait spaces MusicBrainz requests by 1.1 s.
func mbWait() {
	mbMu.Lock()
	defer mbMu.Unlock()
	if d := 1100*time.Millisecond - time.Since(mbLast); d > 0 {
		time.Sleep(d)
	}
	mbLast = time.Now()
}

func coverGet(u string, timeout time.Duration) (*http.Response, error) {
	return coverGetAccept(u, "application/json", timeout)
}

func coverGetAccept(u, accept string, timeout time.Duration) (*http.Response, error) {
	req, err := http.NewRequest("GET", u, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", coverUA)
	req.Header.Set("Accept", accept)
	req.Header.Set("Accept-Language", "en") // Deezer answers in the language of the address
	return updateClient(timeout).Do(req)
}

// mbEscape quotes a value for a MusicBrainz (Lucene) query.
func mbEscape(s string) string {
	s = strings.NewReplacer(`\`, `\\`, `"`, `\"`).Replace(strings.TrimSpace(s))
	return `"` + s + `"`
}

// ---- matching ----
//
// Titles in files are often longer or written differently than in
// MusicBrainz: "(Remastered 2011)", "[Deluxe Edition]", "CD1", "feat. …",
// punctuation, accents. The search goes from strict to tolerant:
//
//  1. the cleaned title and artist as phrases;
//  2. the same words unquoted (finds other spellings);
//  3. with no usable artist (untagged, "Various Artists"), the title alone
//     with an exact match.
//
// A wrong cover is worse than none, so every result is checked: the same
// title words (edition notes and years left out) and the same artist, see
// sameTitle / sameArtist; the score alone is not trusted.

var (
	reBrackets = regexp.MustCompile(`\s*[\(\[\{][^\)\]\}]*[\)\]\}]`)
	reEdition  = regexp.MustCompile(`(?i)\s*[-–:]\s*(\d{4}\s+)?(remaster(ed)?|deluxe|expanded|anniversary|edition|bonus|special|collector'?s?|mono|stereo|live)\b.*$`)
	reDisc     = regexp.MustCompile(`(?i)\s*[-–,]?\s*\b(cd|disc|disk)\s*\d+\b.*$`)
	reFeat     = regexp.MustCompile(`(?i)\s+(feat\.?|ft\.?|featuring|with)\s+.*$`)
)

// cleanTitle strips edition notes, disc numbers and brackets.
func cleanTitle(s string) string {
	t := reBrackets.ReplaceAllString(s, "")
	t = reEdition.ReplaceAllString(t, "")
	t = reDisc.ReplaceAllString(t, "")
	t = strings.TrimSpace(t)
	if t == "" {
		return strings.TrimSpace(s)
	}
	return t
}

// cleanArtist drops "feat. …".
func cleanArtist(s string) string {
	t := strings.TrimSpace(reFeat.ReplaceAllString(s, ""))
	if t == "" {
		return strings.TrimSpace(s)
	}
	return t
}

// words: lower case, accents and punctuation removed, "&" read as "and".
func words(s string) []string {
	s = strings.ToLower(norm.NFD.String(s))
	var b strings.Builder
	for _, r := range s {
		switch {
		case unicode.Is(unicode.Mn, r):
			// accent
		case r == '&':
			b.WriteString(" and ")
		case r == '\'' || r == '’' || r == '‘' || r == '`' || r == '.':
			// apostrophes and dots join words: Pepper’s is Peppers, Sgt. is Sgt
		case unicode.IsLetter(r) || unicode.IsDigit(r):
			b.WriteRune(r)
		default:
			b.WriteRune(' ')
		}
	}
	var out []string
	for _, w := range strings.Fields(b.String()) {
		if w != "the" {
			out = append(out, w)
		}
	}
	return out
}

// editionWords are left out when comparing titles (Rumours 2004 Remaster
// is Rumours), and so are years.
var editionWords = map[string]bool{
	"remaster": true, "remastered": true, "deluxe": true, "edition": true, "expanded": true,
	"anniversary": true, "bonus": true, "special": true, "version": true, "mono": true,
	"stereo": true, "explicit": true, "digital": true, "collector": true, "collectors": true,
}

func titleWords(s string) []string {
	var out []string
	for _, w := range words(cleanTitle(s)) {
		if editionWords[w] || len(w) == 4 && w >= "1900" && w <= "2099" {
			continue
		}
		out = append(out, singular(w))
	}
	return out
}

// singular: "records" and "record" compare the same.
func singular(w string) string {
	if len(w) > 3 && strings.HasSuffix(w, "s") && !strings.HasSuffix(w, "ss") {
		return w[:len(w)-1]
	}
	return w
}

// mainTitle: the part before a subtitle ("Hercules: An Original Walt
// Disney Records Soundtrack" → "Hercules").
func mainTitle(s string) string {
	for _, sep := range []string{": ", " - ", " – ", " — "} {
		if i := strings.Index(s, sep); i > 0 {
			return s[:i]
		}
	}
	return s
}

func sameWords(a, b []string) bool {
	if len(a) == 0 || len(b) == 0 {
		return false
	}
	return strings.Join(a, " ") == strings.Join(b, " ")
}

// sameTitle: the same words, leaving out edition notes and years. A
// title that only starts the same (Rumours / Rumours Live) is another album.
func sameTitle(ours, theirs string) bool {
	return sameWords(titleWords(ours), titleWords(theirs))
}

// sameArtist: the same words, or one name contains the other ("Beatles" /
// "The Beatles & Friends").
func sameArtist(ours, theirs string) bool {
	a, b := words(cleanArtist(ours)), words(cleanArtist(theirs))
	if sameWords(a, b) {
		return true
	}
	ja, jb := " "+strings.Join(a, " ")+" ", " "+strings.Join(b, " ")+" "
	return len(a) > 0 && len(b) > 0 && (strings.Contains(ja, jb) || strings.Contains(jb, ja))
}

// lucene escapes the query syntax characters of an unquoted term.
func lucene(s string) string {
	var b strings.Builder
	for _, r := range s {
		if strings.ContainsRune(`+-&|!(){}[]^"~*?:\/`, r) {
			b.WriteRune(' ')
			continue
		}
		b.WriteRune(r)
	}
	return strings.Join(strings.Fields(b.String()), " ")
}

type mbGroup struct {
	ID     string `json:"id"`
	Score  int    `json:"score"`
	Title  string `json:"title"`
	Credit []struct {
		Name string `json:"name"`
	} `json:"artist-credit"`
	Secondary []string `json:"secondary-types"`
	Primary   string   `json:"primary-type"`
	First     string   `json:"first-release-date"`
}

func (g mbGroup) soundtrack() bool {
	for _, t := range g.Secondary {
		if strings.EqualFold(t, "Soundtrack") {
			return true
		}
	}
	return false
}

func (g mbGroup) artist() string {
	var n []string
	for _, c := range g.Credit {
		n = append(n, c.Name)
	}
	return strings.Join(n, " ")
}

func mbSearchGroups(q string) ([]mbGroup, error) {
	return mbSearch(q, false, 8)
}

// mbSearch runs a release-group search: q in Lucene syntax, or (dismax) as
// plain words matched against title, artist and so on.
func mbSearch(q string, dismax bool, limit int) ([]mbGroup, error) {
	// MusicBrainz answers 503 when asked too fast: wait and try again
	for try := 0; ; try++ {
		gs, err := mbSearchOnce(q, dismax, limit)
		if errors.Is(err, errMBBusy) && try < 2 {
			time.Sleep(time.Duration(2+try*3) * time.Second)
			continue
		}
		return gs, err
	}
}

var errMBBusy = errors.New("MusicBrainz is busy")

func mbSearchOnce(q string, dismax bool, limit int) ([]mbGroup, error) {
	mbWait()
	u := fmt.Sprintf("%s/ws/2/release-group/?fmt=json&limit=%d&query=%s", mbBase(), limit, url.QueryEscape(q))
	if dismax {
		u += "&dismax=true"
	}
	resp, err := coverGet(u, 15*time.Second)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode == 503 {
		return nil, errMBBusy
	}
	if resp.StatusCode != 200 {
		return nil, fmt.Errorf("MusicBrainz answered %d", resp.StatusCode)
	}
	var out struct {
		Groups []mbGroup `json:"release-groups"`
	}
	if err := json.NewDecoder(io.LimitReader(resp.Body, 2<<20)).Decode(&out); err != nil {
		return nil, err
	}
	return out.Groups, nil
}

func usableArtist(a string) bool {
	l := strings.ToLower(strings.TrimSpace(a))
	return l != "" && l != "unknown artist" && l != "various artists" && l != "various" && l != "va"
}

// albumQuery is what is known of an album to look it up.
type albumQuery struct {
	title      string
	artists    []string // album artist and track artists, most common first
	various    bool     // several different track artists (a compilation)
	soundtrack bool     // the title says so, or most songs' genre
}

func newAlbumQuery(a *Album) albumQuery {
	q := albumQuery{title: a.Name}
	count := map[string]int{}
	var order []string
	addArt := func(n string, w int) {
		n = strings.TrimSpace(n)
		if !usableArtist(n) {
			return
		}
		if count[n] == 0 {
			order = append(order, n)
		}
		count[n] += w
	}
	addArt(a.Artist, 100)
	for _, t := range a.Tracks {
		if t != nil {
			addArt(t.AlbumArtist, 10)
			addArt(t.Artist, 1)
		}
	}
	sort.SliceStable(order, func(i, j int) bool { return count[order[i]] > count[order[j]] })
	q.artists = order
	trackArtists := map[string]bool{}
	for _, t := range a.Tracks {
		if t != nil && usableArtist(t.Artist) {
			trackArtists[strings.ToLower(cleanArtist(t.Artist))] = true
		}
	}
	q.various = len(trackArtists) > 2 || !usableArtist(a.Artist)
	low := strings.ToLower(a.Name)
	q.soundtrack = strings.Contains(low, "soundtrack") || strings.Contains(low, "o.s.t") || strings.Contains(low, " ost")
	// game rips and film scores are often tagged by genre only
	sg := 0
	for _, t := range a.Tracks {
		if t != nil && soundtrackGenre(t.Genre) {
			sg++
		}
	}
	if sg > 0 && sg*2 >= len(a.Tracks) {
		q.soundtrack = true
	}
	return q
}

// soundtrackGenre: "Soundtrack", "Game", "Video Game Music", "OST", "Score"…
func soundtrackGenre(g string) bool {
	g = strings.ToLower(strings.TrimSpace(g))
	if g == "ost" || g == "vgm" {
		return true
	}
	return strings.Contains(g, "soundtrack") || strings.Contains(g, "game") || strings.Contains(g, "score")
}

// artistOK: the result's artist is one of ours, or "Various Artists" for
// a compilation.
func (q albumQuery) artistOK(c coverCand) bool {
	ga := c.Artist
	for _, a := range q.artists {
		if sameArtist(a, ga) {
			return true
		}
	}
	return q.various && (strings.EqualFold(strings.TrimSpace(ga), "Various Artists") || ga == "")
}

// titleOK: the same title; or the same main title (before ":") when the
// artist matches too, or both are soundtracks.
func (q albumQuery) titleOK(c coverCand) bool {
	if sameTitle(q.title, c.Title) {
		return true
	}
	mainSame := sameTitle(mainTitle(q.title), mainTitle(c.Title)) || sameTitle(mainTitle(q.title), c.Title)
	return mainSame && (q.artistOK(c) || q.soundtrack && c.soundtrack)
}

// mbReleaseGroups returns release group IDs for the album, best first (at
// most 3): by title and each of its artists, then by the main title alone
// (soundtracks and compilations), every result checked.
func mbReleaseGroups(a *Album) ([]string, error) {
	q := newAlbumQuery(a)
	var ids []string
	add := func(id string) {
		for _, x := range ids {
			if x == id {
				return
			}
		}
		if len(ids) < 3 {
			ids = append(ids, id)
		}
	}
	check := func(gs []mbGroup, needArtist bool) {
		for _, g := range gs {
			c := g.cand()
			if !q.titleOK(c) {
				continue
			}
			if needArtist && !q.artistOK(c) {
				continue
			}
			add(g.ID)
		}
	}
	title := cleanTitle(q.title)
	// 1–2. with the artists (at most two), as phrases then as words
	for n, art := range q.artists {
		if n == 2 || len(ids) > 0 {
			break
		}
		art = cleanArtist(art)
		gs, err := mbSearchGroups("releasegroup:" + mbEscape(title) + " AND artist:" + mbEscape(art))
		if err != nil {
			return nil, err
		}
		check(gs, true)
		if len(ids) > 0 {
			break
		}
		gs, err = mbSearchGroups("releasegroup:(" + lucene(title) + ") AND artist:(" + lucene(art) + ")")
		if err != nil {
			return nil, err
		}
		check(gs, true)
	}
	if len(ids) > 0 {
		return ids, nil
	}
	// 3. the title alone (also its main part): only results whose title
	// matches as above, and whose artist is ours, or for soundtracks and
	// compilations any credit
	short := cleanTitle(mainTitle(q.title))
	for _, t := range []string{title, short} {
		gs, err := mbSearchGroups("releasegroup:" + mbEscape(t))
		if err != nil {
			return nil, err
		}
		check(gs, !(q.soundtrack || q.various))
		if len(ids) > 0 || short == title {
			break
		}
	}
	return ids, nil
}

// caaFront downloads a release group's front cover at 1200 px (nil, nil if
// it has none).
func caaFront(rgid string) ([]byte, error) { return caaImage(rgid, "front-1200") }

// caaImage downloads a release group's front cover at a size ("front-250",
// "front-1200"); nil, nil if it has none.
func caaImage(rgid, size string) ([]byte, error) {
	resp, err := coverGet(caaBase()+"/release-group/"+rgid+"/"+size, 30*time.Second)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode == 404 {
		return nil, nil
	}
	if resp.StatusCode != 200 {
		return nil, fmt.Errorf("Cover Art Archive answered %d", resp.StatusCode)
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, 12<<20))
	if err != nil {
		return nil, err
	}
	if _, _, err := image.DecodeConfig(bytes.NewReader(data)); err != nil {
		return nil, nil // not an image we can show
	}
	return data, nil
}

// findCover looks up one album; nil data means no cover was found.
func findCover(a *Album) ([]byte, error) {
	data, err := findMBCover(a)
	if data != nil || err != nil {
		return data, err
	}
	data, from, err := findOtherCover(a)
	switch {
	case data != nil:
		logMsg(fmt.Sprintf("Covers: %q by %q: from %s", a.Name, a.Artist, from))
	case err == nil:
		logMsg(fmt.Sprintf("Covers: %q by %q: not in iTunes, Deezer or Wikipedia either", a.Name, a.Artist))
	}
	return data, err
}

// findMBCover: the cover from MusicBrainz and the Cover Art Archive.
func findMBCover(a *Album) ([]byte, error) {
	ids, err := mbReleaseGroups(a)
	if err != nil {
		return nil, err
	}
	if len(ids) == 0 {
		logMsg(fmt.Sprintf("Covers: %q by %q: not on MusicBrainz (searched %q by %q)", a.Name, a.Artist, cleanTitle(a.Name), cleanArtist(a.Artist)))
		return nil, nil
	}
	var lastErr error
	for _, id := range ids {
		data, err := caaFront(id)
		if err != nil {
			lastErr = err // try the other candidates
			continue
		}
		if data != nil {
			return data, nil
		}
	}
	if lastErr != nil {
		return nil, lastErr
	}
	logMsg(fmt.Sprintf("Covers: %q by %q: on MusicBrainz but without a cover", a.Name, a.Artist))
	return nil, nil
}

// albumFolder: the folder that holds only this album's tracks ("" if the
// tracks are spread out or loose in the music folder).
func albumFolder(a *Album) string {
	if len(a.Tracks) == 0 || a.Tracks[0] == nil {
		return ""
	}
	dir := filepath.Dir(a.Tracks[0].Path)
	for _, t := range a.Tracks {
		if t != nil && filepath.Dir(t.Path) != dir {
			return ""
		}
	}
	if filepath.Clean(dir) == filepath.Clean(musicRoot()) {
		return ""
	}
	return dir
}

// saveCoverFile saves a found cover as cover.jpg in the album's folder
// (best effort: only a folder of its own, without a cover yet).
func saveCoverFile(a *Album, data []byte) {
	if dir := albumFolder(a); dir != "" {
		if p := filepath.Join(dir, "cover.jpg"); !fileExists(p) && sniffImageExt(data) == "jpg" {
			_ = writeFileAtomic(p, data)
		}
	}
}

// setAlbumCover gives the album the cover, chosen (Missing Covers, Album
// Info → Cover): it is kept apart from the artwork cache, so it stays
// through rescans, over the files' own and a cover.jpg (chosenCoverPath,
// finishAlbums). On the UI goroutine (the album is shared with it).
func (ui *IpodUI) setAlbumCover(a *Album, data []byte) {
	ext := sniffImageExt(data)
	p := chosenCoverPath(a, ext)
	os.MkdirAll(filepath.Dir(p), 0o755)
	for _, e := range []string{"jpg", "png", "webp", "gif"} { // (one per album)
		if e != ext {
			os.Remove(chosenCoverPath(a, e))
		}
	}
	if err := os.WriteFile(p, data, 0o644); err != nil {
		logMsg("Covers: saving " + a.Name + ": " + err.Error())
		a.ArtData, a.ArtExt = data, ext
		ui.app.saveAlbumArtwork(a)
	} else {
		a.ArtPath, a.ArtData, a.ArtExt = p, nil, ext
		missingLib = nil // (Missing Covers counts again)
	}
	a.ArtImg = nil
	ui.artc.dropAlbum(a)
	ui.kb.initialized = false
	ui.onArtLoaded()
	go ui.app.saveLibraryJSON()
}

// chosenCoverPath: where an album's chosen cover is kept (by its key, its
// folder relative to the music folder).
func chosenCoverPath(a *Album, ext string) string {
	return chosenCoverFor(a.Key, ext)
}

func chosenCoverFor(key, ext string) string {
	return filepath.Join(dataDir(), "chosen_covers", generateAlbumCacheKey("", keyRel(key))+"."+ext)
}

// moveChosenCover: an album's chosen cover goes with it to its new key (a
// name changed by Album Info). An album already there with a chosen cover
// keeps its own.
func moveChosenCover(oldKey, newKey string) {
	exts := []string{"jpg", "png", "webp", "gif"}
	for _, e := range exts {
		if fileExists(chosenCoverFor(newKey, e)) {
			return
		}
	}
	for _, e := range exts {
		if p := chosenCoverFor(oldKey, e); fileExists(p) {
			if err := os.Rename(p, chosenCoverFor(newKey, e)); err != nil {
				logMsg("Covers: " + err.Error())
			}
		}
	}
}

// applyChosenCovers: the albums with a chosen cover get it (after a scan).
func applyChosenCovers(lib *Library) {
	dir := filepath.Join(dataDir(), "chosen_covers")
	entries, err := os.ReadDir(dir)
	if err != nil || len(entries) == 0 {
		return
	}
	have := map[string]string{}
	for _, e := range entries {
		n := e.Name()
		have[strings.TrimSuffix(n, filepath.Ext(n))] = filepath.Join(dir, n)
	}
	for _, al := range lib.Albums {
		if p, ok := have[generateAlbumCacheKey("", keyRel(al.Key))]; ok {
			al.ArtPath, al.ArtData = p, nil
		}
	}
}

// forgetChosenCover: the album goes back to its own cover (the files', or
// a cover.jpg), read again.
func (ui *IpodUI) forgetChosenCover(a *Album) {
	for _, e := range []string{"jpg", "png", "webp", "gif"} {
		os.Remove(chosenCoverPath(a, e))
	}
	var paths []string
	for _, t := range albumTracks(a) {
		paths = append(paths, t.Path)
	}
	a.ArtPath, a.ArtData, a.ArtImg = "", nil, nil
	ui.artc.dropAlbum(a)
	go ui.rereadTracks(paths)
}

// hasChosenCover: the album's cover was chosen by hand.
func hasChosenCover(a *Album) bool {
	return a.ArtPath != "" && strings.HasPrefix(a.ArtPath, filepath.Join(dataDir(), "chosen_covers")+string(filepath.Separator))
}

// missingCovers: albums without artwork that can be searched for.
func (ui *IpodUI) missingCovers() []*Album {
	// (asked at every frame by Find Missing Covers: worked out at most
	// every 2 s, as it looks at the cover files)
	if lib := ui.library(); lib == missingLib && time.Since(missingAt) < 2*time.Second {
		return missingOut
	}
	out := ui.missingCoversNow()
	missingLib, missingAt, missingOut = ui.library(), time.Now(), out
	return out
}

var (
	missingLib *Library
	missingAt  time.Time
	missingOut []*Album
)

func (ui *IpodUI) missingCoversNow() []*Album {
	var out []*Album
	for _, a := range ui.library().Albums {
		if len(a.Tracks) == 0 || strings.TrimSpace(a.Name) == "" || a.ArtData != nil || a.ArtImg != nil {
			continue // (an album left with no songs isn't listed anywhere)
		}
		// a cover file that is gone (deleted, moved) is a missing cover too:
		// the default one shows
		if a.ArtPath != "" && fileExists(a.ArtPath) {
			continue
		}
		out = append(out, a)
	}
	return out
}

// ---- the job ----

type coverJob struct {
	mu                   sync.Mutex
	albums               []*Album
	missed               []string // albums without a cover found
	errors               int      // albums that could not be looked up
	index, found, failed int
	current              *Album
	err                  string
	finished, cancelled  bool
	cancel               chan struct{}
}

var coverCur *coverJob

func (ui *IpodUI) startCoverSearch() {
	if coverCur != nil && !coverCur.finished {
		return
	}
	j := &coverJob{albums: ui.missingCovers(), cancel: make(chan struct{})}
	coverCur = j
	if len(j.albums) == 0 {
		j.finished = true
		ui.dirty = true
		return
	}
	go func() {
		netErrs := 0
		for i, a := range j.albums {
			select {
			case <-j.cancel:
				j.mu.Lock()
				j.cancelled = true
				j.mu.Unlock()
				goto done
			default:
			}
			j.mu.Lock()
			j.index, j.current = i, a
			j.mu.Unlock()
			ui.post(func() { ui.dirty = true })
			data, err := findCover(a)
			if err != nil {
				logMsg("Covers: " + a.Name + ": " + err.Error())
				j.mu.Lock()
				j.failed++
				j.errors++
				j.err = iaErrText(err)
				j.mu.Unlock()
				if netErrs++; netErrs >= 3 {
					break // no connection: stop instead of trying every album
				}
				continue
			}
			netErrs = 0
			if data == nil {
				j.mu.Lock()
				j.failed++
				j.missed = append(j.missed, a.Name)
				j.mu.Unlock()
				continue
			}
			saveCoverFile(a, data)
			done := make(chan struct{})
			ui.postSure(func() {
				ui.setAlbumCover(a, data)
				close(done)
			})
			<-done
			j.mu.Lock()
			j.found++
			j.mu.Unlock()
		}
	done:
		if j.found > 0 {
			ui.postSure(func() {
				if err := ui.app.saveLibraryJSON(); err != nil {
					logMsg("Covers: saving the library: " + err.Error())
				}
			})
		}
		ui.postSure(func() {
			j.mu.Lock()
			j.finished, j.current = true, nil
			j.index = len(j.albums)
			j.mu.Unlock()
			logMsg(fmt.Sprintf("Covers: %d found, %d not found", j.found, j.failed))
			ui.refreshBuilt()
		})
	}()
	ui.dirty = true
}

// ---- the screen ----

// CoverView: white like Software Update: the album being looked up, the
// bar, found / not found, and Find Covers / Cancel / Done.
type CoverView struct {
	shown float64
	last  time.Time
}

func (ui *IpodUI) coverView() *View {
	if coverCur != nil && coverCur.finished {
		coverCur = nil // (a search that ended: the count of now, not its results)
	}
	return &View{ID: "covers", Title: "Find Missing Covers", Kind: KindFull, Custom: &CoverView{}}
}

func (cv *CoverView) progress() float64 {
	j := coverCur
	if j == nil {
		return 0
	}
	j.mu.Lock()
	defer j.mu.Unlock()
	if j.finished {
		return 1
	}
	if len(j.albums) == 0 {
		return 0
	}
	return float64(j.index) / float64(len(j.albums))
}

func (cv *CoverView) Animating(now time.Time) bool                   { return math.Abs(cv.progress()-cv.shown) > 0.002 }
func (cv *CoverView) OnScroll(ui *IpodUI, dir int, velocity float64) {}

func (cv *CoverView) OnCenter(ui *IpodUI) {
	j := coverCur
	switch {
	case j == nil && len(ui.missingCovers()) == 0:
		ui.hideView()
	case j == nil:
		ui.startCoverSearch()
	case !j.finished:
		select {
		case <-j.cancel:
		default:
			close(j.cancel)
		}
	default:
		coverCur = nil
		ui.hideView()
	}
	ui.dirty = true
}

func (cv *CoverView) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	W := dst.Bounds().Dx()
	fillRect(dst, 0, 0, W, dst.Bounds().Dy(), th.bg)
	wCSS := float64(W) / DPR
	center := func(f font.Face, s string, y float64, c Col) {
		s = ellipsize(f, s, (wCSS-40)*DPR)
		drawText(dst, f, s, (float64(W)-textWidth(f, s))/2, y*DPR, c)
	}
	dt := now.Sub(cv.last).Seconds()
	if cv.last.IsZero() || dt > 0.5 {
		dt = 0.5
	}
	cv.last = now
	if t := cv.progress(); t < cv.shown {
		cv.shown = t
	} else {
		cv.shown += (t - cv.shown) * math.Min(1, dt*8)
	}
	_, a16 := lineBox(16)
	_, a13 := lineBox(13)
	_, a12 := lineBox(12)
	j := coverCur
	title, line1, line2, button := "Find Missing Covers", "", "", "Find Covers"
	if j == nil {
		n := len(ui.missingCovers())
		line1 = fmt.Sprintf("%d %s without a cover", n, pluralize(n, "album", "albums"))
		line2 = "From MusicBrainz, iTunes, Deezer, Wikipedia · needs Wi-Fi"
		if n == 0 {
			line1, button = "Every album has a cover", "Done"
		}
	} else {
		j.mu.Lock()
		n, i, found, failed, cur, fin, canc, errs := len(j.albums), j.index, j.found, j.failed, j.current, j.finished, j.cancelled, j.err
		missed := append([]string(nil), j.missed...)
		nerr := j.errors
		j.mu.Unlock()
		switch {
		case fin && n == 0:
			line1, button = "Every album has a cover", "Done"
		case fin:
			title = fmt.Sprintf("Found %d of %d", found, n)
			line1 = fmt.Sprintf("%d not found", len(missed))
			if canc {
				line1 = "Stopped · " + line1
			}
			switch {
			case nerr > 0 && len(missed) == 0:
				line1 = fmt.Sprintf("%d could not be looked up", nerr)
				line2 = errs
			case nerr > 0:
				line1 = fmt.Sprintf("%d not found · %d could not be looked up (%s)", len(missed), nerr, errs)
				line2 = "Not found: " + strings.Join(missed, ", ")
			case len(missed) > 0:
				line2 = "Not found: " + strings.Join(missed, ", ")
			}
			button = "Done"
		default:
			if cur != nil {
				title = cur.Name
				line1 = cur.Artist
			}
			line2 = fmt.Sprintf("Album %d of %d · %d found · %d not found", min(i+1, n), n, found, failed)
			button = "Cancel"
		}
	}
	center(face(W700, 16), title, 30+a16, th.text)
	if line1 != "" {
		center(face(W500, 13), line1, 52+a13, th.sub)
	}
	if line2 != "" {
		center(face(W500, 12), line2, 72+a12, th.sub)
	}
	bw := math.Min(240, wCSS-80)
	mid := 124.0
	ui.drawProgressBar(dst, (wCSS-bw)/2, mid-8, bw, cv.shown*100, false)
	if j != nil {
		center(face(W700, 12), fmt.Sprintf("%d%%", int(cv.shown*100+0.5)), mid+22+a12, th.sub)
	}
	drawJobButton(dst, th, button, button != "Cancel")
}
