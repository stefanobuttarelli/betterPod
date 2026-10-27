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
// filtered. Downloads go to <music>/Internet Archive/<creator>/<title>/,
// one file at a time (a .part file renamed when complete), with the item's
// cover saved as cover.jpg; the library is rescanned at the end.

import (
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

// iaSearch returns up to 50 audio items for the query, most downloaded first.
func iaSearch(q string) ([]iaResult, error) {
	v := url.Values{}
	v.Set("q", "("+q+") AND mediatype:(audio OR etree)")
	for _, f := range []string{"identifier", "title", "creator", "year", "date", "licenseurl"} {
		v.Add("fl[]", f)
	}
	v.Set("sort[]", "downloads desc")
	v.Set("rows", "50")
	v.Set("output", "json")
	var out struct {
		Response struct {
			Docs []map[string]json.RawMessage `json:"docs"`
		} `json:"response"`
	}
	if err := iaGet(iaBase()+"/advancedsearch.php?"+v.Encode(), 20*time.Second, &out); err != nil {
		return nil, err
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
	return res, nil
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

// iaAudioGroup maps archive.org's format names to the two download
// choices (other formats are left out).
func iaAudioGroup(format string) (string, int) {
	f := strings.ToLower(format)
	switch {
	case strings.Contains(f, "flac"):
		return "FLAC", 0
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
		return nil, errors.New("item not found")
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
	}{}
	v := &View{ID: "iaSearch", Title: "Internet Archive", Kind: KindFull}
	var run func()
	v.Build = func() []Item {
		sub := "Enter text, then ✓ to search"
		if st.term != "" {
			sub = st.term
		}
		items := []Item{{Label: "Search archive.org", Sublabel: sub, Icon: "search_icon", OnSelect: func() {
			ui.showKeyboard(func(k string) { iaKey(ui, v, &st.term, k, run) })
		}}}
		switch {
		case st.busy:
			items = append(items, Item{Label: "Searching…", NoArrow: true})
		case st.status != "":
			items = append(items, Item{Label: st.status, NoArrow: true})
		}
		for _, r := range st.res {
			r := r
			sub := r.Creator
			if r.Year != "" {
				sub = strings.Trim(sub+" · "+r.Year, " ·")
			}
			sub = strings.Trim(sub+" · "+licenseLabel(r.License), " ·")
			items = append(items, Item{Label: r.Title, Sublabel: sub,
				OnSelect: func() { ui.showView(ui.iaItemView(r), "") }})
		}
		return items
	}
	run = func() {
		q := strings.TrimSpace(st.term)
		if q == "" || st.busy {
			return
		}
		st.busy, st.status, st.ran = true, "", q
		ui.refreshView(v)
		go func() {
			res, err := iaSearch(q)
			ui.post(func() {
				st.busy, st.res = false, res
				if err != nil {
					st.status = iaErrText(err)
				} else if len(res) == 0 {
					st.status = "No results"
				}
				v.Index = 0
				if len(res) > 0 {
					v.Index = 1
				}
				ui.refreshView(v)
			})
		}()
	}
	v.Items = v.Build()
	ui.showKeyboard(func(k string) { iaKey(ui, v, &st.term, k, run) })
	return v
}

func iaKey(ui *IpodUI, v *View, term *string, k string, run func()) {
	switch k {
	case "Enter":
		run()
	case "delete":
		if r := []rune(*term); len(r) > 0 {
			*term = string(r[:len(r)-1])
		}
	default:
		*term += k
	}
	ui.refreshView(v)
}

func iaErrText(err error) string {
	s := updateErrText(err)
	if s == "No Wi-Fi connection" {
		return s
	}
	if len(s) > 48 {
		s = s[:48] + "…"
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
			return append(items, Item{Label: "No FLAC or MP3 files in this item", NoArrow: true})
		}
		for _, g := range []string{"FLAC", "MP3"} {
			fs := it.Groups[g]
			if len(fs) == 0 {
				continue
			}
			var size int64
			for _, f := range fs {
				size += f.Size
			}
			label := "Download " + g
			if g == "FLAC" {
				label += " (lossless)"
			}
			g := g
			items = append(items, Item{Label: label,
				Sublabel: fmt.Sprintf("%d %s · %s", len(fs), pluralize(len(fs), "track", "tracks"), formatBytes(size)),
				OnSelect: func() { ui.startIADownload(it, g, it.Groups[g]) }})
		}
		g := "FLAC"
		if len(it.Groups[g]) == 0 {
			g = "MP3"
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
		ui.post(func() {
			st.item = it
			if err != nil {
				st.err = iaErrText(err)
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
}

var iaCur *iaDownload

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

func (ui *IpodUI) startIADownload(it *iaItem, format string, files []iaFile) {
	if iaCur != nil && !iaCur.finished {
		ui.showView(ui.iaDownloadView(), "")
		return
	}
	creator := it.Creator
	if creator == "" {
		creator = "Unknown Artist"
	}
	dir := filepath.Join(musicRoot(), "Internet Archive", safeName(creator), safeName(it.Title))
	d := &iaDownload{item: it, files: files, format: format, dir: dir, cancel: make(chan struct{}), state: "Starting…"}
	for _, f := range files {
		d.total += f.Size
	}
	iaCur = d
	ui.showView(ui.iaDownloadView(), "")
	go func() {
		err := d.run(ui)
		ui.post(func() {
			d.mu.Lock()
			d.finished = true
			if err != nil {
				d.err = iaErrText(err)
				d.state = "Download stopped"
			} else {
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
	if fb := freeBytes(d.dir); fb >= 0 && d.total > 0 && fb < d.total+50<<20 {
		return fmt.Errorf("Not enough space (%s free)", formatBytes(fb))
	}
	client := updateClient(0)
	get := func(name string, dst string, count bool) error {
		u := iaBase() + "/download/" + url.PathEscape(d.item.ID) + "/" + escapePath(name)
		req, _ := http.NewRequest("GET", u, nil)
		req.Header.Set("User-Agent", "KnulliPod/"+KNULLIPOD_VERSION)
		resp, err := client.Do(req)
		if err != nil {
			return err
		}
		defer resp.Body.Close()
		if resp.StatusCode != 200 {
			return fmt.Errorf("archive.org answered %d", resp.StatusCode)
		}
		part := dst + ".part"
		out, err := os.Create(part)
		if err != nil {
			return err
		}
		buf := make([]byte, 64<<10)
		for {
			select {
			case <-d.cancel:
				out.Close()
				os.Remove(part)
				return errors.New("Cancelled")
			default:
			}
			n, rerr := resp.Body.Read(buf)
			if n > 0 {
				if _, err := out.Write(buf[:n]); err != nil {
					out.Close()
					os.Remove(part)
					return err
				}
				if count {
					d.mu.Lock()
					d.done += int64(n)
					d.mu.Unlock()
					ui.post(func() { ui.dirty = true })
				}
			}
			if rerr == io.EOF {
				break
			}
			if rerr != nil {
				out.Close()
				os.Remove(part)
				return rerr
			}
		}
		if err := out.Close(); err != nil {
			return err
		}
		return os.Rename(part, dst)
	}
	// the cover first (best effort), so the album shows with its art
	if c := filepath.Join(d.dir, "cover.jpg"); !fileExists(c) && d.item.Cover != "" {
		_ = get(d.item.Cover, c, false)
	}
	for i, f := range d.files {
		d.mu.Lock()
		d.index, d.state = i, f.Title
		d.mu.Unlock()
		dst := filepath.Join(d.dir, safeName(filepath.Base(f.Name)))
		if st, err := os.Stat(dst); err == nil && (f.Size == 0 || st.Size() == f.Size) {
			d.mu.Lock()
			d.done += f.Size
			d.mu.Unlock()
			continue // already there
		}
		if err := get(f.Name, dst, true); err != nil {
			return err
		}
	}
	d.mu.Lock()
	d.index = len(d.files)
	d.mu.Unlock()
	return nil
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
}

func (ui *IpodUI) iaDownloadView() *View {
	return &View{ID: "iaDownload", Title: "Internet Archive", Kind: KindFull, Custom: &IADownloadView{}}
}

func (dv *IADownloadView) progress() float64 {
	d := iaCur
	if d == nil {
		return 0
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	if d.finished && d.err == "" {
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
	return math.Abs(dv.progress()-dv.shown) > 0.002
}

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
	W := dst.Bounds().Dx()
	fillRect(dst, 0, 0, W, dst.Bounds().Dy(), th.bg)
	wCSS := float64(W) / DPR
	d := iaCur
	if d == nil {
		return
	}
	d.mu.Lock()
	title, creator := d.item.Title, d.item.Creator
	state, errs, fin := d.state, d.err, d.finished
	idx, n, done, total, format := d.index, len(d.files), d.done, d.total, d.format
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
	status := ""
	switch {
	case fin && errs == "":
		status = fmt.Sprintf("%s · %d %s · %s", state, n, pluralize(n, "track", "tracks"), format)
	case fin:
		status = state + ": " + errs
	default:
		status = fmt.Sprintf("Track %d of %d · %s", min(idx+1, n), n, state)
	}
	center(face(W600, 13), status, 76+a13, th.text)
	if total > 0 && !fin {
		center(face(W500, 12), fmt.Sprintf("%s of %s · %s", formatBytes(done), formatBytes(total), format), 94+a12, th.sub)
	}
	bw := math.Min(240, wCSS-80)
	mid := 124.0
	ui.drawProgressBar(dst, (wCSS-bw)/2, mid-8, bw, dv.shown*100, false)
	center(face(W700, 12), fmt.Sprintf("%d%%", int(dv.shown*100+0.5)), mid+22+a12, th.sub)
	// one button: Cancel while downloading, Done at the end
	label := "Cancel"
	if fin {
		label = "Done"
	}
	f := face(W700, 14)
	bh := 28.0
	by := 220 - 8 - bh
	bx0, bx1 := 40.0, wCSS-40
	fillRoundedGradient(dst, bx0*DPR, by*DPR, bx1*DPR, (by+bh)*DPR, 9*DPR, selGrad, 1)
	strokeRounded(dst, bx0*DPR, by*DPR, bx1*DPR, (by+bh)*DPR, 9*DPR, 2, hex("#3a78ad"), 1)
	tw := textWidth(f, label)
	drawText(dst, f, label, (float64(W)-tw)/2, (by+bh/2+lineCapHeight(14)/2)*DPR, colWhite)
}
