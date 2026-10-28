package main

// ij_archive.go — Music → Internet Archive: search archive.org, see an
// item's license and tracks, and download an album (FLAC or MP3) into the
// music folder, where the library picks it up.
//
// archive.org's public APIs need no account:
//
//	search    /advancedsearch.php?q=…&fl[]=…&output=json
//	metadata  /metadata/<identifier>          (files, formats, lengths)
//	download  /download/<identifier>/<file>   (redirects to a data server)
//
// Every result shows its license as archive.org records it (Creative
// Commons, public domain, or "No license stated"); the search is not
// filtered. Results come 50 at a time ("More results…" loads the next
// page); every request shows its state in the list and has a timeout, a
// download that stalls for 30 s stops, and answers to an older search are
// dropped. Downloads go to <music>/Internet Archive/<creator>/<title>/,
// one file at a time (a .part file renamed when complete), with the item's
// cover saved as cover.jpg; the library is rescanned at the end.

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"io"
	"math"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"time"

	"golang.org/x/image/font"
)

// iaBase is archive.org (MIYOOPOD_IA_BASE points elsewhere, for tests).
func iaBase() string {
	if b := os.Getenv("MIYOOPOD_IA_BASE"); b != "" {
		return strings.TrimRight(b, "/")
	}
	return "https://archive.org"
}

// ---- API ----

type iaResult struct {
	ID, Title, Creator, Year, License string
}

// jsonString reads a field that archive.org gives as a string, a number or
// a list of strings.
func jsonString(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	var s string
	if json.Unmarshal(raw, &s) == nil {
		return s
	}
	var l []string
	if json.Unmarshal(raw, &l) == nil {
		return strings.Join(l, ", ")
	}
	var n json.Number
	if json.Unmarshal(raw, &n) == nil {
		return n.String()
	}
	return ""
}

func iaGet(u string, timeout time.Duration, v any) error {
	req, _ := http.NewRequest("GET", u, nil)
	req.Header.Set("User-Agent", "KnulliPod/"+KNULLIPOD_VERSION)
	resp, err := updateClient(timeout).Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return fmt.Errorf("archive.org answered %d", resp.StatusCode)
	}
	return json.NewDecoder(io.LimitReader(resp.Body, 8<<20)).Decode(v)
}

// iaPageSize: results per page (the list asks for the next page on demand).
const iaPageSize = 50

// iaSearch returns one page (1-based) of audio items for the query, most
// downloaded first, and how many there are in all.
func iaSearch(q string, page int) ([]iaResult, int, error) {
	v := url.Values{}
	v.Set("q", "("+q+") AND mediatype:(audio OR etree)")
	for _, f := range []string{"identifier", "title", "creator", "year", "date", "licenseurl"} {
		v.Add("fl[]", f)
	}
	v.Set("sort[]", "downloads desc")
	v.Set("rows", strconv.Itoa(iaPageSize))
	v.Set("page", strconv.Itoa(page))
	v.Set("output", "json")
	var out struct {
		Response struct {
			NumFound int                          `json:"numFound"`
			Docs     []map[string]json.RawMessage `json:"docs"`
		} `json:"response"`
	}
	if err := iaGet(iaBase()+"/advancedsearch.php?"+v.Encode(), 20*time.Second, &out); err != nil {
		return nil, 0, err
	}
	var res []iaResult
	for _, d := range out.Response.Docs {
		r := iaResult{
			ID: jsonString(d["identifier"]), Title: jsonString(d["title"]),
			Creator: jsonString(d["creator"]), Year: jsonString(d["year"]),
			License: jsonString(d["licenseurl"]),
		}
		if r.Year == "" {
			if dt := jsonString(d["date"]); len(dt) >= 4 {
				r.Year = dt[:4]
			}
		}
		if r.ID == "" {
			continue
		}
		if r.Title == "" {
			r.Title = r.ID
		}
		res = append(res, r)
	}
	return res, out.Response.NumFound, nil
}

type iaFile struct {
	Name, Format, Title string
	Track               int
	Size                int64
	Length              float64
}

type iaItem struct {
	ID, Title, Creator, Date, License string
	Groups                            map[string][]iaFile // "FLAC" / "MP3": the audio files in that format
	Cover                             string              // an image file to save as cover.jpg
}

// iaAudioGroup maps archive.org's format names to the download choices:
// the lossless ones (FLAC, ALAC, WAV, AIFF) and MP3 (the best variant);
// other formats are left out.
func iaAudioGroup(format string) (string, int) {
	f := strings.ToLower(format)
	switch {
	case strings.Contains(f, "flac"):
		return "FLAC", 0
	case strings.Contains(f, "apple lossless"), strings.Contains(f, "alac"):
		return "ALAC", 0
	case strings.Contains(f, "wav"):
		return "WAV", 0
	case strings.Contains(f, "aiff"), f == "aif":
		return "AIFF", 0
	case f == "vbr mp3":
		return "MP3", 0
	case f == "320kbps mp3":
		return "MP3", 1
	case f == "256kbps mp3":
		return "MP3", 2
	case f == "mp3":
		return "MP3", 3
	case f == "128kbps mp3":
		return "MP3", 4
	case strings.Contains(f, "mp3"):
		return "MP3", 5
	}
	return "", 0
}

func parseLength(s string) float64 {
	if s == "" {
		return 0
	}
	if strings.Contains(s, ":") {
		t := 0.0
		for _, p := range strings.Split(s, ":") {
			v, _ := strconv.ParseFloat(p, 64)
			t = t*60 + v
		}
		return t
	}
	v, _ := strconv.ParseFloat(s, 64)
	return v
}

func iaMetadata(id string) (*iaItem, error) {
	var out struct {
		Metadata map[string]json.RawMessage   `json:"metadata"`
		Files    []map[string]json.RawMessage `json:"files"`
	}
	if err := iaGet(iaBase()+"/metadata/"+url.PathEscape(id), 30*time.Second, &out); err != nil {
		return nil, err
	}
	if out.Metadata == nil {
		return nil, errors.New("Item not found")
	}
	it := &iaItem{ID: id, Groups: map[string][]iaFile{},
		Title: jsonString(out.Metadata["title"]), Creator: jsonString(out.Metadata["creator"]),
		Date: jsonString(out.Metadata["date"]), License: jsonString(out.Metadata["licenseurl"])}
	if it.Title == "" {
		it.Title = id
	}
	// for each group keep the best variant (VBR MP3 before 128 kbps…)
	best := map[string]int{}
	var all []struct {
		g    string
		rank int
		f    iaFile
	}
	for _, f := range out.Files {
		name := jsonString(f["name"])
		format := jsonString(f["format"])
		if name == "" {
			continue
		}
		low := strings.ToLower(format)
		if it.Cover == "" && (low == "jpeg" || low == "png") && jsonString(f["source"]) == "original" {
			it.Cover = name
		}
		g, rank := iaAudioGroup(format)
		if g == "" {
			continue
		}
		size, _ := strconv.ParseInt(jsonString(f["size"]), 10, 64)
		tn, _ := strconv.Atoi(strings.SplitN(jsonString(f["track"]), "/", 2)[0])
		file := iaFile{Name: name, Format: format, Title: jsonString(f["title"]), Track: tn,
			Size: size, Length: parseLength(jsonString(f["length"]))}
		if file.Title == "" {
			file.Title = strings.TrimSuffix(filepath.Base(name), filepath.Ext(name))
		}
		all = append(all, struct {
			g    string
			rank int
			f    iaFile
		}{g, rank, file})
		if r, ok := best[g]; !ok || rank < r {
			best[g] = rank
		}
	}
	for _, a := range all {
		if a.rank == best[a.g] {
			it.Groups[a.g] = append(it.Groups[a.g], a.f)
		}
	}
	for g := range it.Groups {
		fs := it.Groups[g]
		sort.SliceStable(fs, func(i, j int) bool {
			if fs[i].Track != fs[j].Track && fs[i].Track > 0 && fs[j].Track > 0 {
				return fs[i].Track < fs[j].Track
			}
			return fs[i].Name < fs[j].Name
		})
	}
	if it.Cover == "" {
		it.Cover = "__ia_thumb.jpg"
	}
	return it, nil
}

// licenseLabel turns a license URL into a short name.
func licenseLabel(u string) string {
	l := strings.ToLower(strings.TrimSpace(u))
	if l == "" {
		return "No license stated"
	}
	switch {
	case strings.Contains(l, "publicdomain/zero"):
		return "CC0 (public domain)"
	case strings.Contains(l, "publicdomain/mark"), strings.Contains(l, "public domain"):
		return "Public domain"
	case strings.Contains(l, "creativecommons.org/licenses/"):
		p := strings.Split(strings.Trim(l[strings.Index(l, "/licenses/")+len("/licenses/"):], "/"), "/")
		s := "CC " + strings.ToUpper(p[0])
		if len(p) > 1 && p[1] != "" {
			s += " " + p[1]
		}
		return s
	}
	if pu, err := url.Parse(u); err == nil && pu.Host != "" {
		return pu.Host
	}
	return u
}

// ---- views ----

// iaSearchView: the search row (the keyboard opens at once; the search
// runs on ✓) and the results.
func (ui *IpodUI) iaSearchView() *View {
	st := &struct {
		term, ran, status string
		busy              bool
		res               []iaResult
		total, page       int
		gen               int               // the current search; older answers are dropped
		thumbs            map[string]*Album // the items' pictures, by identifier
		live              atomic.Int64      // st.gen, for the thumbnail workers
	}{thumbs: map[string]*Album{}}
	v := &View{ID: "iaSearch", Title: "Internet Archive", Kind: KindFull}
	v.onClose = func() { st.gen++; st.live.Store(int64(st.gen)) } // the thumbnails stop
	var run func()
	var more func()
	v.Build = func() []Item {
		// the search row: while the keyboard is up it shows the text being
		// typed (with a caret); afterwards the search and its results, and
		// pressing it opens the keyboard again on the same text to change
		// it (⌫ deletes, hold the centre button for Clear search)
		editing := ui.keyboardEditing(&st.term)
		note := ""
		if st.total > 0 && !editing {
			note = fmt.Sprintf("%d %s", st.total, pluralize(st.total, "result", "results"))
		}
		edit := func() {
			ui.showKeyboard(&st.term, func() { ui.refreshView(v) }, run)
			ui.refreshView(v)
		}
		items := []Item{ui.searchField(&st.term, "Search archive.org", note, edit, []SheetOption{
			{Label: "Clear search", OnSelect: func() {
				st.term = ""
				edit()
			}},
		})}
		// a term without spaces may be an item's identifier (the part of
		// the link after /details/): open it directly, which also works for
		// items left out of archive.org's search index
		if id := strings.TrimSpace(st.term); !editing && id != "" && !strings.ContainsAny(id, " \t") && iaLooksLikeID(id) {
			found := false
			for _, r := range st.res {
				found = found || r.ID == id
			}
			if !found {
				items = append(items, Item{Label: "Open item “" + id + "”", Sublabel: "By its identifier (archive.org/details/" + id + ")",
					OnSelect: func() { ui.showView(ui.iaItemView(iaResult{ID: id, Title: id}), "") }})
			}
		}
		for _, r := range st.res {
			r := r
			sub := r.Creator
			if r.Year != "" {
				sub = strings.Trim(sub+" · "+r.Year, " ·")
			}
			sub = strings.Trim(sub+" · "+licenseLabel(r.License), " ·")
			th := st.thumbs[r.ID]
			if th == nil {
				th = &Album{Name: r.Title, Artist: r.Creator} // the default art until the picture comes
				st.thumbs[r.ID] = th
			}
			items = append(items, Item{Label: r.Title, Sublabel: sub, Album: th,
				OnSelect: func() { ui.showView(ui.iaItemView(r), "") }})
		}
		if st.ran == "" && !st.busy {
			items = append(items, ui.recentItems("ia", func(term string) {
				st.term = term
				run()
			}, func() { ui.refreshView(v) })...)
		}
		switch {
		case st.busy:
			items = append(items, Item{Label: "Searching…", NoArrow: true})
		case st.status != "":
			items = append(items, Item{Label: st.status, NoArrow: true, OnSelect: func() {
				if len(st.res) > 0 {
					more() // an error on a later page: try it again
				} else {
					run()
				}
			}})
		case len(st.res) < st.total:
			items = append(items, Item{Label: "More results…",
				Sublabel: fmt.Sprintf("%d of %d shown", len(st.res), st.total), OnSelect: more})
		}
		return items
	}
	fetch := func(q string, page int, gen int) {
		go func() {
			res, total, err := iaSearch(q, page)
			ui.postSure(func() {
				if gen != st.gen {
					return // a newer search has started
				}
				st.busy = false
				if err != nil {
					st.status = iaErrText(err) + " · press A to retry"
				} else {
					st.status = ""
					st.res = append(st.res, res...)
					ui.iaLoadThumbs(v, res, st.thumbs, func() bool { return st.live.Load() == int64(gen) })
					st.total, st.page = total, page
					if len(res) == 0 {
						st.total = len(st.res) // no more pages
					}
					if len(st.res) == 0 {
						st.status = "No results"
					}
				}
				first := v.Index
				ui.refreshView(v)
				if page == 1 && len(st.res) > 0 {
					v.Index = 1
				} else {
					v.Index = first
				}
			})
		}()
	}
	run = func() {
		q := strings.TrimSpace(st.term)
		if q == "" {
			return
		}
		st.gen++
		st.live.Store(int64(st.gen))
		addRecentSearch("ia", q)
		st.busy, st.status, st.ran = true, "", q
		st.res, st.total, st.page = nil, 0, 0
		v.Index = 0
		ui.refreshView(v)
		fetch(q, 1, st.gen)
	}
	more = func() {
		if st.busy || st.ran == "" {
			return
		}
		st.busy, st.status = true, ""
		ui.refreshView(v)
		fetch(st.ran, st.page+1, st.gen)
	}
	v.Items = v.Build()
	if len(recentSearches("ia")) == 0 {
		ui.showKeyboard(&st.term, func() { ui.refreshView(v) }, run)
	} else {
		v.Index = 1 // the recent searches (the field above opens the keyboard)
	}
	return v
}

// iaLoadThumbs fetches the items' pictures (archive.org's own, about
// 180 px: the cover when the item has one), three at a time, while current
// (safe off the UI goroutine) says the search is still the one shown. On
// the UI goroutine.
func (ui *IpodUI) iaLoadThumbs(v *View, res []iaResult, thumbs map[string]*Album, current func() bool) {
	type job struct {
		th *Album
		id string
	}
	work := make(chan job)
	for w := 0; w < 3; w++ {
		go func() {
			for j := range work {
				if !current() {
					continue
				}
				data, err := coverImage("Internet Archive", iaBase()+"/services/img/"+url.PathEscape(j.id))
				if err != nil || data == nil {
					continue
				}
				img, _, err := image.Decode(bytes.NewReader(data))
				if err != nil {
					continue
				}
				th := j.th
				ui.postSure(func() {
					if !current() {
						return
					}
					th.ArtImg = img
					ui.artc.dropAlbum(th)
					v.dirty = true
					ui.dirty = true
				})
			}
		}()
	}
	var jobs []job
	for _, r := range res {
		if r.ID == "" {
			continue
		}
		th := thumbs[r.ID]
		if th == nil {
			th = &Album{Name: r.Title, Artist: r.Creator}
			thumbs[r.ID] = th
		}
		if th.ArtImg == nil {
			jobs = append(jobs, job{th, r.ID})
		}
	}
	go func() {
		defer close(work)
		for _, j := range jobs {
			if !ui.app.Running {
				return
			}
			work <- j
		}
	}()
}

// iaGroupOrder: the download buttons, lossless formats first.
var iaGroupOrder = []string{"FLAC", "ALAC", "WAV", "AIFF", "MP3"}

// iaLooksLikeID: letters, digits, '-', '_' and '.' only (archive.org
// identifiers).
func iaLooksLikeID(s string) bool {
	if len(s) < 3 || len(s) > 100 {
		return false
	}
	for _, r := range s {
		if !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || r == '-' || r == '_' || r == '.') {
			return false
		}
	}
	return true
}

func iaErrText(err error) string {
	s := updateErrText(err)
	if s == "No Wi-Fi connection" {
		return s
	}
	if r := []rune(s); len(r) > 48 {
		s = string(r[:48]) + "…"
	}
	return s
}

// iaItemView: the item's license and size, the download buttons and its
// tracks (selecting one downloads just that track).
func (ui *IpodUI) iaItemView(r iaResult) *View {
	st := &struct {
		item *iaItem
		err  string
	}{}
	v := &View{ID: "iaItem", Title: r.Title, Kind: KindFull}
	v.Build = func() []Item {
		lic := licenseLabel(r.License)
		if st.item != nil && st.item.License != "" {
			lic = licenseLabel(st.item.License)
		}
		items := []Item{{Label: "License", Value: lic, NoArrow: true}}
		if st.item == nil {
			if st.err != "" {
				items = append(items, Item{Label: st.err, NoArrow: true})
			} else {
				items = append(items, Item{Label: "Loading…", NoArrow: true})
			}
			return items
		}
		it := st.item
		if len(it.Groups) == 0 {
			return append(items, Item{Label: "No audio files KnulliPod can play", NoArrow: true})
		}
		for _, g := range iaGroupOrder {
			fs := it.Groups[g]
			if len(fs) == 0 {
				continue
			}
			var size int64
			for _, f := range fs {
				size += f.Size
			}
			label := "Download " + g
			if g != "MP3" {
				label += " (lossless)"
			}
			sub := fmt.Sprintf("%d %s · %s", len(fs), pluralize(len(fs), "track", "tracks"), formatBytes(size))
			// some already downloaded (an earlier download stopped): resume
			switch have, left := iaHave(it, fs); {
			case have == len(fs):
				label, sub = g+" downloaded", fmt.Sprintf("All %d %s are in your library · press A to check again", len(fs), pluralize(len(fs), "track", "tracks"))
			case have > 0:
				label = "Resume " + g + " download"
				sub = fmt.Sprintf("%d of %d %s left · %s", len(fs)-have, len(fs), pluralize(len(fs), "track", "tracks"), formatBytes(left))
			}
			g := g
			items = append(items, Item{Label: label,
				Sublabel: sub,
				OnSelect: func() { ui.startIADownload(it, g, it.Groups[g]) }})
		}
		// the track list shows the first format there is (lossless first)
		g := ""
		for _, o := range iaGroupOrder {
			if len(it.Groups[o]) > 0 {
				g = o
				break
			}
		}
		for _, f := range it.Groups[g] {
			f := f
			d := ""
			if f.Length > 0 {
				d = formatTrackLength(f.Length)
			}
			items = append(items, Item{Label: f.Title, Value: d, NoArrow: true,
				OnSelect: func() { ui.startIADownload(it, g, []iaFile{f}) }})
		}
		return items
	}
	v.Items = v.Build()
	go func() {
		it, err := iaMetadata(r.ID)
		ui.postSure(func() {
			st.item = it
			if err != nil {
				st.err = iaErrText(err)
			} else if it.Title != "" {
				// opened by identifier: the real title for the header
				v.Title = it.Title
				if ui.topScreen() == v {
					ui.headerTitle = it.Title
				}
			}
			ui.refreshView(v)
			if err == nil && len(v.Items) > 1 && v.Index == 0 {
				v.Index = 1 // on the first download button
			}
		})
	}()
	return v
}

func formatBytes(n int64) string {
	switch {
	case n >= 1e9:
		return fmt.Sprintf("%.1f GB", float64(n)/1e9)
	case n >= 1e6:
		return fmt.Sprintf("%.0f MB", float64(n)/1e6)
	case n > 0:
		return fmt.Sprintf("%.0f KB", float64(n)/1e3)
	}
	return "size unknown"
}

// ---- download ----

type iaDownload struct {
	mu       sync.Mutex
	whole    bool // the step is for the whole download (its audio, the cover), not a track
	item     *iaItem
	files    []iaFile
	format   string
	dir      string
	done     int64 // bytes so far
	total    int64
	index    int // file being downloaded
	state    string
	err      string
	finished bool
	cancel   chan struct{}
	retry    string   // while waiting to try again: why, and which try
	failed   []string // tracks skipped after every try failed
	lastErr  string
	again    func() // Try Again
	source   string // the header ("YouTube"; empty: Internet Archive)
}

var iaCur *iaDownload

// iaStall: a download that receives nothing for this long is stopped
// (MIYOOPOD_IA_STALL, in seconds, changes it for tests).
var iaStall = func() time.Duration {
	if v, err := strconv.Atoi(os.Getenv("MIYOOPOD_IA_STALL")); err == nil && v > 0 {
		return time.Duration(v) * time.Second
	}
	return 30 * time.Second
}()

func safeName(s string) string {
	s = strings.Map(func(r rune) rune {
		if strings.ContainsRune(`/\:*?"<>|`, r) || r < 32 {
			return '_'
		}
		return r
	}, strings.TrimSpace(s))
	s = strings.Trim(s, ". ")
	if len([]rune(s)) > 80 {
		s = string([]rune(s)[:80])
	}
	// and short enough in bytes for the card's file system (255)
	for len(s) > 200 {
		r := []rune(s)
		s = string(r[:len(r)-1])
	}
	if s == "" {
		s = "Unknown"
	}
	return s
}

func freeBytes(dir string) int64 {
	var st syscall.Statfs_t
	if syscall.Statfs(dir, &st) != nil {
		return -1
	}
	return int64(st.Bavail) * int64(st.Bsize)
}

// iaDir: the folder an item is downloaded to.
func iaDir(it *iaItem) string {
	creator := it.Creator
	if creator == "" {
		creator = "Unknown Artist"
	}
	return filepath.Join(musicRoot(), "Internet Archive", safeName(creator), safeName(it.Title))
}

// iaHave: how many of files are already downloaded, and the bytes left.
func iaHave(it *iaItem, files []iaFile) (have int, left int64) {
	dir := iaDir(it)
	for _, f := range files {
		dst := filepath.Join(dir, safeName(filepath.Base(f.Name)))
		if st, err := os.Stat(dst); err == nil && (f.Size == 0 || st.Size() == f.Size) {
			have++
		} else {
			left += f.Size
		}
	}
	return have, left
}

func (ui *IpodUI) startIADownload(it *iaItem, format string, files []iaFile) {
	if iaCur != nil && !iaCur.finished {
		ui.showView(ui.iaDownloadView(), "")
		return
	}
	d := &iaDownload{item: it, files: files, format: format, dir: iaDir(it), cancel: make(chan struct{}), state: "Starting…"}
	for _, f := range files {
		d.total += f.Size
	}
	ui.startDownloadJob(d, func() error { return d.run(ui) }, func() { ui.startIADownload(it, format, files) })
}

// startDownloadJob shows the download screen for d and runs run in the
// background (Internet Archive, YouTube); again restarts it (Try Again).
func (ui *IpodUI) startDownloadJob(d *iaDownload, run func() error, again func()) {
	d.again = again
	iaCur = d
	ui.showView(ui.iaDownloadView(), "")
	go func() {
		err := run()
		ui.postSure(func() {
			d.mu.Lock()
			d.finished = true
			switch {
			case errors.Is(err, errIACancelled):
				d.err, d.state = "Cancelled", "Download stopped"
			case err != nil:
				d.err, d.state = err.Error(), "Download stopped"
			default:
				d.state = "Added to your library"
			}
			d.mu.Unlock()
			if err == nil || d.index > 0 {
				ui.rescanLibrary()
			}
			ui.dirty = true
		})
	}()
}

func (d *iaDownload) run(ui *IpodUI) error {
	if err := os.MkdirAll(d.dir, 0755); err != nil {
		return err
	}
	// room for what is still to come (an album resumed needs only the rest)
	_, left := iaHave(d.item, d.files)
	for _, f := range d.files {
		if st, err := os.Stat(filepath.Join(d.dir, safeName(filepath.Base(f.Name))) + ".part"); err == nil {
			left -= st.Size()
		}
	}
	if fb := freeBytes(d.dir); fb >= 0 && left > 0 && fb < left+50<<20 {
		return fmt.Errorf("Not enough space (%s free)", formatBytes(fb))
	}
	client := updateClient(0)
	// get downloads one file into dst.part and renames it at the end. A
	// .part left by an earlier attempt is continued (Range) when the server
	// allows it. base is d.done before this file, for the progress.
	get := func(name string, dst string, count bool, base, size int64) error {
		u := iaBase() + "/download/" + url.PathEscape(d.item.ID) + "/" + escapePath(name)
		part := dst + ".part"
		var have int64
		if st, err := os.Stat(part); err == nil {
			have = st.Size()
		}
		setDone := func(n int64) {
			if count {
				d.mu.Lock()
				d.done = base + n
				d.mu.Unlock()
				// (no redraw nudge: the download view polls the progress)
			}
		}
		setDone(have)
		// a connection that stops sending for iaStall is dropped (the
		// request is cancelled, the read fails)
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		var stalled atomic.Bool
		watchdog := time.AfterFunc(iaStall, func() { stalled.Store(true); cancel() })
		defer watchdog.Stop()
		req, _ := http.NewRequestWithContext(ctx, "GET", u, nil)
		req.Header.Set("User-Agent", "KnulliPod/"+KNULLIPOD_VERSION)
		if have > 0 {
			req.Header.Set("Range", fmt.Sprintf("bytes=%d-", have))
		}
		resp, err := client.Do(req)
		if err != nil {
			if stalled.Load() {
				return &iaRetry{msg: "Connection stalled"}
			}
			return &iaRetry{msg: iaErrText(err)}
		}
		defer resp.Body.Close()
		flags := os.O_WRONLY | os.O_CREATE
		switch {
		case resp.StatusCode == 206 && have > 0 && strings.HasPrefix(resp.Header.Get("Content-Range"), fmt.Sprintf("bytes %d-", have)):
			flags |= os.O_APPEND // continue the .part
		case resp.StatusCode == 200:
			flags |= os.O_TRUNC // from the start
			have = 0
			setDone(0)
		case resp.StatusCode == 416 && have > 0:
			// nothing after the .part's end: whole if it has the right size,
			// else it is stale and the file starts over
			if size == 0 || have == size {
				return os.Rename(part, dst)
			}
			os.Remove(part)
			return &iaRetry{msg: "The partial file didn't match", after: time.Second}
		case resp.StatusCode == 429 || resp.StatusCode >= 500:
			r := &iaRetry{msg: fmt.Sprintf("archive.org answered %d", resp.StatusCode)}
			if sec, err := strconv.Atoi(resp.Header.Get("Retry-After")); err == nil && sec > 0 {
				r.after = time.Duration(min(sec, 120)) * time.Second
			}
			return r
		default:
			return fmt.Errorf("archive.org answered %d (%s)", resp.StatusCode, http.StatusText(resp.StatusCode))
		}
		out, err := os.OpenFile(part, flags, 0644)
		if err != nil {
			return err
		}
		buf := make([]byte, 64<<10)
		got := have
		for {
			select {
			case <-d.cancel:
				out.Close()
				os.Remove(part)
				return errIACancelled
			default:
			}
			n, rerr := resp.Body.Read(buf)
			if n > 0 {
				watchdog.Reset(iaStall)
				if _, err := out.Write(buf[:n]); err != nil {
					out.Close()
					return err // the card is full or failing: not retried
				}
				got += int64(n)
				setDone(got)
			}
			if rerr == io.EOF {
				break
			}
			if rerr != nil {
				out.Close() // the .part stays, the next attempt continues it
				if stalled.Load() {
					return &iaRetry{msg: "Connection stalled"}
				}
				return &iaRetry{msg: iaErrText(rerr)}
			}
		}
		if err := out.Close(); err != nil {
			return err
		}
		return os.Rename(part, dst)
	}
	// getRetry: get, tried again after a pause when archive.org is busy or
	// the connection drops (a .part is continued, not started over)
	getRetry := func(name, dst string, count bool, base, size int64) error {
		for attempt := 1; ; attempt++ {
			err := get(name, dst, count, base, size)
			var r *iaRetry
			if !errors.As(err, &r) {
				return err
			}
			if attempt >= iaAttempts {
				return &iaGaveUp{r.msg}
			}
			wait := r.after
			if wait == 0 {
				wait = time.Duration(min(3<<(attempt-1), 60)) * time.Second // 3, 6, 12, 24, 48, 60 s
			}
			logMsg(fmt.Sprintf("Internet Archive: %s: %s, trying again in %v (%d/%d)", name, r.msg, wait, attempt, iaAttempts))
			for end := time.Now().Add(wait); time.Now().Before(end); {
				left := int(time.Until(end).Seconds() + 0.99)
				d.mu.Lock()
				d.retry = fmt.Sprintf("%s · retrying in %d s (%d/%d)", r.msg, left, attempt+1, iaAttempts)
				d.mu.Unlock()
				ui.post(func() { ui.dirty = true })
				select {
				case <-d.cancel:
					return errIACancelled
				case <-time.After(250 * time.Millisecond):
				}
			}
			d.mu.Lock()
			d.retry = ""
			d.mu.Unlock()
		}
	}
	// the cover first (best effort), so the album shows with its art
	ext := strings.ToLower(filepath.Ext(d.item.Cover))
	if ext == ".jpeg" || ext == "" {
		ext = ".jpg"
	}
	if c := filepath.Join(d.dir, "cover"+ext); !fileExists(c) && d.item.Cover != "" {
		_ = get(d.item.Cover, c, false, 0, 0)
	}
	netFails := 0 // tracks given up in a row because of the connection
	for i, f := range d.files {
		d.mu.Lock()
		d.index, d.state = i, f.Title
		base := d.done
		d.mu.Unlock()
		dst := filepath.Join(d.dir, safeName(filepath.Base(f.Name)))
		if st, err := os.Stat(dst); err == nil && (f.Size == 0 || st.Size() == f.Size) {
			d.mu.Lock()
			d.done += f.Size
			d.mu.Unlock()
			continue // already there
		}
		err := getRetry(f.Name, dst, true, base, f.Size)
		d.mu.Lock()
		d.retry = ""
		d.mu.Unlock()
		var gaveUp *iaGaveUp
		if errors.As(err, &gaveUp) {
			netFails++
		} else {
			netFails = 0
		}
		switch {
		case err == nil:
		case errors.Is(err, errIACancelled):
			return err
		case isDiskErr(err):
			return err // no point going on
		case netFails >= 2:
			// two tracks in a row couldn't be fetched: the connection is
			// gone (Try Again resumes when it's back)
			return fmt.Errorf("%s (stopped: two tracks in a row failed)", gaveUp.msg)
		default:
			// this track failed: note it and go on with the others
			logMsg("Internet Archive: " + f.Name + ": " + err.Error())
			d.mu.Lock()
			d.failed = append(d.failed, f.Title)
			d.lastErr = err.Error()
			d.done = base + f.Size
			d.mu.Unlock()
		}
	}
	d.mu.Lock()
	d.index = len(d.files)
	d.mu.Unlock()
	return nil
}

// iaAttempts: how many times a file is tried before it is skipped.
const iaAttempts = 6

// iaRetry: a failure worth trying again (busy server, dropped connection).
type iaRetry struct {
	msg   string
	after time.Duration // Retry-After
}

func (r *iaRetry) Error() string { return r.msg }

var errIACancelled = errors.New("Cancelled")

// iaGaveUp: every try of a file failed with a busy server or the connection.
type iaGaveUp struct{ msg string }

func (g *iaGaveUp) Error() string { return g.msg }

// isDiskErr: writing to the card failed (full, read-only, damaged).
func isDiskErr(err error) bool {
	var pe *os.PathError
	if errors.As(err, &pe) {
		return true
	}
	var le *os.LinkError
	return errors.As(err, &le)
}

// escapePath escapes each segment of a file path inside an item.
func escapePath(p string) string {
	seg := strings.Split(p, "/")
	for i := range seg {
		seg[i] = url.PathEscape(seg[i])
	}
	return strings.Join(seg, "/")
}

// IADownloadView: white, like Software Update: the album, the track being
// downloaded, the glossy bar, and Cancel / Done.
type IADownloadView struct {
	sel   int
	shown float64
	last  time.Time

	// time left: the speed, smoothed over the last seconds
	sampleAt   time.Time
	sampleDone int64
	speed      float64   // bytes per second
	since      time.Time // first sample
	movedAt    time.Time // last time bytes arrived
	etaText    string
	etaAt      time.Time
	tickAt     time.Time // last draw (a redraw a second while downloading)
}

// eta updates the speed and returns the time left ("2:13 left"), "" while
// it can't be told yet.
func (dv *IADownloadView) eta(now time.Time, done, total int64) string {
	if total <= 0 || done >= total {
		return ""
	}
	if dv.sampleAt.IsZero() || done < dv.sampleDone {
		dv.sampleAt, dv.sampleDone, dv.since, dv.movedAt, dv.speed = now, done, now, now, 0
		return ""
	}
	if dt := now.Sub(dv.sampleAt).Seconds(); dt >= 0.5 {
		inst := float64(done-dv.sampleDone) / dt
		if done > dv.sampleDone {
			dv.movedAt = now
		}
		if dv.speed == 0 {
			dv.speed = inst
		} else {
			k := math.Min(1, dt/5) // about the last 5 seconds
			dv.speed += (inst - dv.speed) * k
		}
		dv.sampleAt, dv.sampleDone = now, done
	}
	switch {
	case now.Sub(dv.since) < 3*time.Second || dv.speed <= 0:
		return ""
	case now.Sub(dv.movedAt) > 4*time.Second:
		return "waiting for data…"
	}
	// the text changes at most once a second, so it doesn't flicker
	if now.Sub(dv.etaAt) >= time.Second || dv.etaText == "" {
		left := int(float64(total-done)/dv.speed + 0.5)
		switch {
		case left >= 3600:
			dv.etaText = fmt.Sprintf("%d:%02d:%02d left", left/3600, left/60%60, left%60)
		default:
			dv.etaText = fmt.Sprintf("%d:%02d left", left/60, left%60)
		}
		dv.etaAt = now
	}
	return dv.etaText
}

func formatSpeed(bps float64) string {
	return formatBytes(int64(bps)) + "/s"
}

func (ui *IpodUI) iaDownloadView() *View {
	title := "Internet Archive"
	if iaCur != nil && iaCur.source != "" {
		title = iaCur.source
	}
	return &View{ID: "iaDownload", Title: title, Kind: KindFull, Custom: &IADownloadView{}}
}

func (dv *IADownloadView) progress() float64 {
	d := iaCur
	if d == nil {
		return 0
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.finished && d.err == "" {
		if n := len(d.files); n > 0 && len(d.failed) > 0 {
			return float64(n-len(d.failed)) / float64(n) // the tracks that made it
		}
		return 1
	}
	if d.total > 0 {
		return math.Min(1, float64(d.done)/float64(d.total))
	}
	if len(d.files) > 0 {
		return float64(d.index) / float64(len(d.files))
	}
	return 0
}

func (dv *IADownloadView) Animating(now time.Time) bool {
	if math.Abs(dv.progress()-dv.shown) > 0.002 {
		return true
	}
	// while downloading, redraw each second for the time left
	if d := iaCur; d != nil {
		d.mu.Lock()
		running := !d.finished
		d.mu.Unlock()
		return running && now.Sub(dv.tickAt) >= time.Second
	}
	return false
}

// canRetry: the download ended with tracks missing (not cancelled).
func (dv *IADownloadView) canRetry() bool {
	d := iaCur
	if d == nil {
		return false
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	return d.finished && (len(d.failed) > 0 || d.err != "" && d.err != "Cancelled")
}

// iaHintY: the line above the button (CSS px).
const iaHintY = 160.0

func (dv *IADownloadView) OnScroll(ui *IpodUI, dir int, velocity float64) {}

func (dv *IADownloadView) OnCenter(ui *IpodUI) {
	d := iaCur
	if d == nil {
		ui.hideView()
		return
	}
	d.mu.Lock()
	fin := d.finished
	d.mu.Unlock()
	if fin {
		if dv.canRetry() {
			// again: the tracks already there are skipped, a .part is
			// continued
			iaCur = nil
			ui.hideView()
			if d.again != nil {
				d.again() // a fresh download view
			}
			return
		}
		ui.hideView()
		return
	}
	select {
	case <-d.cancel:
	default:
		close(d.cancel)
	}
}

func (dv *IADownloadView) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	dv.tickAt = now
	W := dst.Bounds().Dx()
	fillRect(dst, 0, 0, W, dst.Bounds().Dy(), th.bg)
	wCSS := float64(W) / DPR
	d := iaCur
	if d == nil {
		return
	}
	d.mu.Lock()
	title, creator := d.item.Title, d.item.Creator
	state, errs, fin, whole := d.state, d.err, d.finished, d.whole
	idx, n, done, total, format := d.index, len(d.files), d.done, d.total, d.format
	retry, failed, lastErr := d.retry, len(d.failed), d.lastErr
	d.mu.Unlock()
	center := func(f font.Face, s string, y float64, c Col) {
		s = ellipsize(f, s, (wCSS-40)*DPR)
		drawText(dst, f, s, (float64(W)-textWidth(f, s))/2, y*DPR, c)
	}
	// the bar eases towards the progress
	dt := now.Sub(dv.last).Seconds()
	if dv.last.IsZero() || dt > 0.5 {
		dt = 0.5
	}
	dv.last = now
	t := dv.progress()
	if t < dv.shown {
		dv.shown = t
	} else {
		dv.shown += (t - dv.shown) * math.Min(1, dt*10)
	}
	_, a16 := lineBox(16)
	_, a13 := lineBox(13)
	_, a12 := lineBox(12)
	center(face(W700, 16), title, 22+a16, th.text)
	if creator != "" {
		center(face(W500, 13), creator, 42+a13, th.sub)
	}
	status, detail := "", ""
	switch {
	case fin && errs == "" && failed > 0:
		status = fmt.Sprintf("%d of %d %s added · %d failed", n-failed, n, pluralize(n, "track", "tracks"), failed)
		detail = lastErr
	case fin && errs == "":
		status = fmt.Sprintf("%s · %d %s · %s", state, n, pluralize(n, "track", "tracks"), format)
	case fin:
		status, detail = state, errs
	case whole:
		status, detail = state, retry // (no track yet: the tracks come after)
	default:
		status = fmt.Sprintf("Track %d of %d · %s", min(idx+1, n), n, state)
		detail = retry
	}
	center(face(W600, 13), status, 76+a13, th.text)
	eta := ""
	if !fin && retry == "" {
		eta = dv.eta(now, done, total)
	}
	if detail != "" {
		center(face(W500, 12), detail, 94+a12, th.sub)
	} else if total > 0 && !fin {
		line := fmt.Sprintf("%s of %s · %s", formatBytes(done), formatBytes(total), format)
		if eta != "" && dv.speed > 0 && now.Sub(dv.movedAt) <= 4*time.Second {
			line += " · " + formatSpeed(dv.speed)
		}
		center(face(W500, 12), line, 94+a12, th.sub)
	}
	bw := math.Min(240, wCSS-80)
	mid := 124.0
	ui.drawProgressBar(dst, (wCSS-bw)/2, mid-8, bw, dv.shown*100, false)
	pct := fmt.Sprintf("%d%%", int(dv.shown*100+0.5))
	if eta != "" {
		pct += " · " + eta
	}
	center(face(W700, 12), pct, mid+22+a12, th.sub)
	// one button: Cancel while downloading, Done at the end
	label := "Cancel"
	if fin {
		label = "Done"
		if dv.canRetry() {
			label = "Try Again"
			center(face(W500, 12), "Tracks already downloaded are kept · B to close", iaHintY+a12, th.sub)
		}
	}
	drawJobButton(dst, th, label, fin)
}

// drawJobButton: the one button at the bottom of a job's screen (a
// download, the cover search). Blue when it ends something that finished
// (Done, Try Again); grey, as the action sheet's Cancel, while it would
// stop work still going (Cancel), so blue always means "it's over".
func drawJobButton(dst *image.RGBA, th *UITheme, label string, primary bool) {
	W := dst.Bounds().Dx()
	wCSS := float64(W) / DPR
	f := face(W700, 14)
	bh := 28.0
	by := 220 - 8 - bh
	bx0, bx1 := 40.0, wCSS-40
	grad, border, tc := th.sheetOpt, th.sheetBorder, th.sheetText
	if primary {
		grad, border, tc = selGrad, hex("#3a78ad"), colWhite
	}
	fillRoundedGradient(dst, bx0*DPR, by*DPR, bx1*DPR, (by+bh)*DPR, 9*DPR, grad, 1)
	strokeRounded(dst, bx0*DPR, by*DPR, bx1*DPR, (by+bh)*DPR, 9*DPR, 2, border, 1)
	tw := textWidth(f, label)
	drawText(dst, f, label, (float64(W)-tw)/2, (by+bh/2+lineCapHeight(14)/2)*DPR, tc)
}
