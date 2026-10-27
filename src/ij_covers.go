package main

// ij_covers.go — Settings → Library → Find Missing Covers: covers for the
// albums that have none, from the Cover Art Archive (coverartarchive.org),
// found through MusicBrainz.
//
// For each album without artwork: a MusicBrainz search for the release
// group (album title + artist, best matches first), then the release
// group's front cover from the Cover Art Archive at 1200 px (no huge
// originals). MusicBrainz allows about one request per second per address
// with a User-Agent that says who is asking, so requests are spaced by
// 1.1 s. A cover found is saved as cover.jpg in the album's folder (when
// the folder holds only that album and has no cover yet) and in the
// artwork cache; the library is saved at the end.

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
	"strings"
	"sync"
	"time"
	"unicode"

	"golang.org/x/image/font"
	"golang.org/x/text/unicode/norm"
)

const coverUA = "KnulliPod/" + KNULLIPOD_VERSION + " ( https://github.com/stefanobuttarelli/knullipod )"

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
	req, err := http.NewRequest("GET", u, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("User-Agent", coverUA)
	req.Header.Set("Accept", "application/json")
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
		out = append(out, w)
	}
	return out
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
}

func (g mbGroup) artist() string {
	var n []string
	for _, c := range g.Credit {
		n = append(n, c.Name)
	}
	return strings.Join(n, " ")
}

func mbSearchGroups(q string) ([]mbGroup, error) {
	mbWait()
	u := mbBase() + "/ws/2/release-group/?fmt=json&limit=8&query=" + url.QueryEscape(q)
	resp, err := coverGet(u, 15*time.Second)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode == 503 {
		return nil, errors.New("MusicBrainz is busy")
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

// mbReleaseGroups returns release group IDs for the album, best first (at
// most 3), trying the steps above in turn.
func mbReleaseGroups(album, artist string) ([]string, error) {
	title := cleanTitle(album)
	art := cleanArtist(artist)
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
	if usableArtist(art) {
		// 1. phrases
		gs, err := mbSearchGroups("releasegroup:" + mbEscape(title) + " AND artist:" + mbEscape(art))
		if err != nil {
			return nil, err
		}
		for _, g := range gs {
			if sameTitle(album, g.Title) && sameArtist(artist, g.artist()) {
				add(g.ID)
			}
		}
		if len(ids) > 0 {
			return ids, nil
		}
		// 2. words, checked one by one
		gs, err = mbSearchGroups("releasegroup:(" + lucene(title) + ") AND artist:(" + lucene(art) + ")")
		if err != nil {
			return nil, err
		}
		for _, g := range gs {
			if sameTitle(album, g.Title) && sameArtist(artist, g.artist()) {
				add(g.ID)
			}
		}
		return ids, nil
	}
	// 3. no artist to go by: the title alone, exact
	gs, err := mbSearchGroups("releasegroup:" + mbEscape(title))
	if err != nil {
		return nil, err
	}
	for _, g := range gs {
		if sameTitle(album, g.Title) {
			add(g.ID)
		}
	}
	return ids, nil
}

// caaFront downloads a release group's front cover at 1200 px (nil, nil if
// it has none).
func caaFront(rgid string) ([]byte, error) {
	resp, err := coverGet(caaBase()+"/release-group/"+rgid+"/front-1200", 30*time.Second)
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
	ids, err := mbReleaseGroups(a.Name, a.Artist)
	if err != nil {
		return nil, err
	}
	if len(ids) == 0 {
		logMsg(fmt.Sprintf("Covers: %q by %q: not on MusicBrainz (searched %q by %q)", a.Name, a.Artist, cleanTitle(a.Name), cleanArtist(a.Artist)))
		return nil, nil
	}
	for _, id := range ids {
		data, err := caaFront(id)
		if err != nil {
			return nil, err
		}
		if data != nil {
			return data, nil
		}
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

// missingCovers: albums without artwork that can be searched for.
func (ui *IpodUI) missingCovers() []*Album {
	var out []*Album
	for _, a := range ui.library().Albums {
		if a.ArtPath == "" && a.ArtData == nil && a.ArtImg == nil && strings.TrimSpace(a.Name) != "" {
			out = append(out, a)
		}
	}
	return out
}

// ---- the job ----

type coverJob struct {
	mu                   sync.Mutex
	albums               []*Album
	missed               []string // albums without a cover found
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
			// save in the album folder (best effort) and in the artwork
			// cache, on the UI goroutine (the album is shared with it)
			if dir := albumFolder(a); dir != "" {
				if p := filepath.Join(dir, "cover.jpg"); !fileExists(p) && sniffImageExt(data) == "jpg" {
					_ = writeFileAtomic(p, data)
				}
			}
			done := make(chan struct{})
			ui.post(func() {
				a.ArtData, a.ArtExt = data, sniffImageExt(data)
				if err := ui.app.saveAlbumArtwork(a); err != nil {
					logMsg("Covers: saving " + a.Name + ": " + err.Error())
				}
				ui.artc.dropAlbum(a)
				ui.kb.initialized = false
				ui.onArtLoaded()
				close(done)
			})
			<-done
			j.mu.Lock()
			j.found++
			j.mu.Unlock()
		}
	done:
		if j.found > 0 {
			ui.post(func() {
				if err := ui.app.saveLibraryJSON(); err != nil {
					logMsg("Covers: saving the library: " + err.Error())
				}
			})
		}
		ui.post(func() {
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
		line2 = "From the Cover Art Archive · needs Wi-Fi"
		if n == 0 {
			line1, button = "Every album has a cover", "Done"
		}
	} else {
		j.mu.Lock()
		n, i, found, failed, cur, fin, canc, errs := len(j.albums), j.index, j.found, j.failed, j.current, j.finished, j.cancelled, j.err
		missed := append([]string(nil), j.missed...)
		j.mu.Unlock()
		switch {
		case fin && n == 0:
			line1, button = "Every album has a cover", "Done"
		case fin:
			title = fmt.Sprintf("Found %d of %d", found, n)
			line1 = fmt.Sprintf("%d not found", n-found)
			if canc {
				line1 = "Stopped · " + line1
			}
			if errs != "" && found == 0 && len(missed) == 0 {
				line2 = errs
			} else if len(missed) > 0 {
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
	f := face(W700, 14)
	bh := 28.0
	by := 220 - 8 - bh
	bx0, bx1 := 40.0, wCSS-40
	fillRoundedGradient(dst, bx0*DPR, by*DPR, bx1*DPR, (by+bh)*DPR, 9*DPR, selGrad, 1)
	strokeRounded(dst, bx0*DPR, by*DPR, bx1*DPR, (by+bh)*DPR, 9*DPR, 2, hex("#3a78ad"), 1)
	tw := textWidth(f, button)
	drawText(dst, f, button, (float64(W)-tw)/2, (by+bh/2+lineCapHeight(14)/2)*DPR, colWhite)
}
