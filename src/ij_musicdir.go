package main

// ij_musicdir.go — Settings → Library → Music Folder: where the music is,
// and moving it to another memory card.
//
// The folder chosen is kept in musicdir.txt in the port's data folder
// (MIYOOPOD_DATA_DIR); without it, it's the launcher's (/userdata/music).
// If the chosen card is missing at start, the default one is used for that
// run and the screen says so.
//
// Moving the music, in the background, in steps that can't lose anything:
//
//  1. Counting: every file under the music folder (hidden ones too: the
//     database may live there), grouped by album folder for the screen.
//  2. Copying, folder by folder, into <card>/music: each file through
//     name.kpart, synced, then renamed, with its CRC-32 taken on the way
//     and its date kept. A file already there with the same size is
//     skipped (a move that stopped goes on). Playlists' lines with the old
//     folder are rewritten with the new one. A card taken out pauses it
//     until the card is back; a full card stops it.
//  3. Checking: each copied file is read back from the card (dropped from
//     the cache first, so it's really the card) and its CRC compared; a
//     file that doesn't match is copied again once.
//  4. Only then, when moving (not copying): the originals are deleted, and
//     the empty folders (not the music folder itself).
//  5. The music folder becomes the new one (and the database with it, if
//     it was inside), Now Playing is cleared (its files moved) and the
//     library is scanned again.
//
// The screen (ij_movemusic_view.go) shows it as the albums travelling
// from one card's pile to the other's.

import (
	"bufio"
	"bytes"
	"errors"
	"fmt"
	"hash/crc32"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"syscall"
	"time"
)

const musicDirPointer = "musicdir.txt"

var moveSlow, _ = strconv.Atoi(os.Getenv("MIYOOPOD_MOVE_SLOW"))

var (
	musicDirMu       sync.Mutex
	musicDirOverride = readMusicDirPointer()
	musicDirMissing  string // the chosen folder, when it wasn't there at start
)

// defaultMusicRoot: the launcher's music folder (no trailing slash).
func defaultMusicRoot() string {
	dir := os.Getenv("MIYOOPOD_MUSIC_DIR")
	if dir == "" {
		if isKnulli {
			dir = "/userdata/music"
		} else {
			dir = "/mnt/SDCARD/Media/Music"
		}
	}
	return filepath.Clean(dir)
}

func musicPointerPath() string {
	if d := os.Getenv("MIYOOPOD_DATA_DIR"); d != "" {
		return filepath.Join(d, musicDirPointer)
	}
	return ""
}

func readMusicDirPointer() string {
	p := musicPointerPath()
	if p == "" {
		return ""
	}
	b, err := os.ReadFile(p)
	if err != nil {
		return ""
	}
	d := filepath.Clean(strings.TrimSpace(string(b)))
	if d == "" || d == "." || !filepath.IsAbs(d) {
		return ""
	}
	if st, err := os.Stat(d); err != nil || !st.IsDir() {
		musicDirMissing = d
		return ""
	}
	return d
}

// setMusicRoot makes dir the music folder and remembers it.
func setMusicRoot(dir string) error {
	dir = filepath.Clean(dir)
	p := musicPointerPath()
	if p == "" {
		return errors.New("no data folder to remember it in")
	}
	if dir == defaultMusicRoot() {
		if err := os.Remove(p); err != nil && !os.IsNotExist(err) {
			return err
		}
	} else if err := writeFileAtomic(p, []byte(dir+"\n")); err != nil {
		return err
	}
	musicDirMu.Lock()
	musicDirOverride = dir
	if dir == defaultMusicRoot() {
		musicDirOverride = ""
	}
	musicDirMu.Unlock()
	musicDirMissing = ""
	MUSIC_ROOT = musicRoot()
	return nil
}

// ---- the cards ----

type musicCard struct {
	Name  string // "SD 1", "SD 2", or the mount's name
	Mount string
	Dev   string
	Free  int64
	Total int64
}

// mountOf: the mount point and device of the longest mount holding path.
func mountOf(path string) (mnt, dev string) {
	f, err := os.Open("/proc/mounts")
	if err != nil {
		return "", ""
	}
	defer f.Close()
	path = filepath.Clean(path)
	if r, err := filepath.EvalSymlinks(path); err == nil {
		path = r
	}
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		fs := strings.Fields(sc.Text())
		if len(fs) < 2 {
			continue
		}
		m := strings.ReplaceAll(fs[1], `\040`, " ")
		if (path == m || strings.HasPrefix(path, strings.TrimRight(m, "/")+"/")) && len(m) > len(mnt) {
			mnt, dev = m, fs[0]
		}
	}
	return mnt, dev
}

// cardName: "SD 1" for the H700's first slot (mmcblk0), "SD 2" for the
// second (mmcblk1), else the mount's own name.
func cardName(dev, mnt string) string {
	switch {
	case strings.HasPrefix(dev, "/dev/mmcblk0"):
		return "SD 1"
	case strings.HasPrefix(dev, "/dev/mmcblk1"):
		return "SD 2"
	}
	if os.Getenv("MIYOOPOD_FAKE_CARDS") != "" {
		return filepath.Base(mnt) // (tests)
	}
	return filepath.Base(mnt)
}

// musicCards: the cards the music could go to: the default folder's and
// those under /media (MIYOOPOD_FAKE_CARDS: other folders, for tests),
// writable and not the boot partition.
func musicCards() []musicCard {
	var roots []string
	if f := os.Getenv("MIYOOPOD_FAKE_CARDS"); f != "" {
		roots = strings.Split(f, ":")
	} else {
		roots = append(roots, filepath.Dir(defaultMusicRoot()))
		roots = append(roots, externalRoots()...)
	}
	var out []musicCard
	seen := map[string]bool{}
	for _, r := range roots {
		if strings.EqualFold(filepath.Base(r), "BATOCERA") || strings.EqualFold(filepath.Base(r), "boot") {
			continue
		}
		mnt, dev := mountOf(r)
		if os.Getenv("MIYOOPOD_FAKE_CARDS") != "" {
			mnt, dev = r, r
		}
		if mnt == "" || seen[dev] {
			continue
		}
		if checkWritable(r) != nil {
			continue
		}
		seen[dev] = true
		free, total, _ := diskSpace(r)
		out = append(out, musicCard{Name: cardName(dev, r), Mount: r, Dev: dev, Free: free, Total: total})
	}
	return out
}

// musicDest: the music folder on a card (the default one on its card).
func musicDest(c musicCard) string {
	if d := defaultMusicRoot(); filepath.Dir(d) == c.Mount {
		return d
	}
	return filepath.Join(c.Mount, "music")
}

// ---- the move ----

type moveFile struct {
	rel  string
	size int64
	mod  time.Time
	crc  uint32
	have bool // already there (same size): not copied, checked
}

type moveUnit struct {
	dir   string // relative
	album *Album // nil: "Other files"
	files []*moveFile
	bytes int64
}

type moveJob struct {
	mu       sync.Mutex
	from, to string // music folders
	fromName string
	toName   string
	move     bool // delete the originals once checked
	units    []*moveUnit
	phase    string // "count", "copy", "paused", "verify", "delete", "done", "failed", "stopped"
	cur      int    // unit being copied / checked
	unitDone int64  // bytes of it copied
	doneB    int64
	all      int64
	files    int
	checked  int // files checked
	file     string
	err      string
	speed    float64 // bytes/s (smoothed)
	stop     chan struct{}
	started  time.Time
	songs    int
	cardRoot string // the destination card's folder (its mount point)
	cardDev  string // its device: a card taken out leaves an empty folder there
}

var moveCur *moveJob // the one running or just ended (UI goroutine)

func (j *moveJob) set(f func()) {
	j.mu.Lock()
	f()
	j.mu.Unlock()
}

func (j *moveJob) stopped() bool {
	select {
	case <-j.stop:
		return true
	default:
		return false
	}
}

// startMusicMove counts, then copies, checks and (moving) deletes, then
// switches the music folder. On the UI goroutine.
func (ui *IpodUI) startMusicMove(c musicCard, move bool) {
	from := filepath.Clean(MUSIC_ROOT)
	to := musicDest(c)
	_, fromDev := mountOf(from)
	j := &moveJob{from: from, to: to, fromName: cardName(fromDev, from), toName: c.Name, move: move,
		phase: "count", stop: make(chan struct{}), started: time.Now()}
	if os.Getenv("MIYOOPOD_FAKE_CARDS") != "" {
		j.fromName = filepath.Base(filepath.Dir(from))
	}
	if j.fromName == c.Name {
		j.fromName = "Here"
	}
	j.cardRoot = filepath.Dir(to)
	if os.Getenv("MIYOOPOD_FAKE_CARDS") == "" {
		_, j.cardDev = mountOf(j.cardRoot)
	}
	moveCur = j
	dbBusy = true // no library scans meanwhile
	// the songs playing are about to move
	if p := ui.app.Playing; p != nil && p.State == StatePlaying {
		ui.app.togglePlayPause()
	}
	lib := ui.library()
	byDir := map[string]*Album{}
	for _, t := range lib.Tracks {
		if t != nil {
			if d := filepath.Dir(t.Path); byDir[d] == nil {
				byDir[d] = ui.trackAlbum(t)
			}
		}
	}
	songs := len(lib.Tracks)
	ui.showView(&View{ID: "moveMusic", Title: "Moving Music", Kind: KindFull, Custom: &moveView{job: j}}, "")
	logMsg(fmt.Sprintf("Move Music: %s → %s (move=%v)", from, to, move))
	go func() {
		err := j.run(ui, byDir, songs)
		ui.postSure(func() {
			dbBusy = false
			switch {
			case err == nil:
				oldData := dataDir()
				if err2 := setMusicRoot(j.to); err2 != nil {
					j.set(func() { j.phase, j.err = "failed", "Couldn't switch: "+err2.Error() })
					return
				}
				if strings.HasPrefix(oldData+"/", j.from+"/") {
					// the database was in the music folder: it moved with it
					_ = setDataDir(filepath.Join(j.to, strings.TrimPrefix(oldData, j.from)))
				}
				ui.clearNowPlaying()
				ui.rescanLibrary()
				logMsg("Move Music: done, the music is in " + j.to)
			case errors.Is(err, errIACancelled):
				j.set(func() { j.phase = "stopped" })
				logMsg("Move Music: stopped")
			default:
				j.set(func() { j.phase, j.err = "failed", err.Error() })
				logMsg("Move Music: " + err.Error())
			}
			ui.dirty = true
		})
	}()
}

func (j *moveJob) run(ui *IpodUI, byDir map[string]*Album, songs int) error {
	// 1. counting
	units := map[string]*moveUnit{}
	other := &moveUnit{dir: ""}
	var all int64
	nfiles := 0
	err := filepath.Walk(j.from, func(p string, info os.FileInfo, err error) error {
		if err != nil {
			return nil // an unreadable folder: left out
		}
		if j.stopped() {
			return errIACancelled
		}
		if info.IsDir() || !info.Mode().IsRegular() {
			return nil
		}
		if strings.HasSuffix(p, ".kpart") {
			return nil
		}
		rel, _ := filepath.Rel(j.from, p)
		f := &moveFile{rel: rel, size: info.Size(), mod: info.ModTime()}
		d := filepath.Dir(p)
		u := other
		if a := byDir[d]; a != nil {
			if u = units[d]; u == nil {
				u = &moveUnit{dir: filepath.Dir(rel), album: a}
				units[d] = u
			}
		}
		u.files = append(u.files, f)
		u.bytes += f.size
		all += f.size
		nfiles++
		return nil
	})
	if err != nil {
		return err
	}
	var list []*moveUnit
	for _, u := range units {
		list = append(list, u)
	}
	sort.Slice(list, func(a, b int) bool { return list[a].dir < list[b].dir })
	if len(other.files) > 0 {
		list = append(list, other)
	}
	if nfiles == 0 {
		return errors.New("There's no music to move")
	}
	if free, _, ok := diskSpace(filepath.Dir(j.to)); ok && free < all-j.alreadyThere(list)+64<<20 {
		return fmt.Errorf("Not enough space on %s: %s free, %s needed", j.toName, humanBytes(free), humanBytes(all))
	}
	j.set(func() { j.units, j.all, j.files, j.songs, j.phase = list, all, nfiles, songs, "copy" })
	// the covers load now, so they show as the albums go
	ui.post(func() {
		for _, u := range list {
			if u.album != nil {
				ui.thumb(u.album, 56)
			}
		}
	})

	// 2. copying
	buf := make([]byte, 1<<20)
	var done int64
	lastT, lastB := time.Now(), int64(0)
	for k, u := range list {
		j.set(func() { j.cur, j.unitDone = k, 0 })
		var udone int64
		for _, f := range u.files {
			if j.stopped() {
				return errIACancelled
			}
			j.set(func() { j.file = filepath.Base(f.rel) })
			fileDone, fileU := done, udone // a try that fails counts again from here
			for {
				done, udone = fileDone, fileU
				err := j.copyFile(f, buf, func(n int64) {
					udone += n
					done += n
					now := time.Now()
					j.set(func() {
						j.unitDone, j.doneB = udone, done
						if dt := now.Sub(lastT).Seconds(); dt > 0.5 {
							inst := float64(done-lastB) / dt
							if j.speed == 0 {
								j.speed = inst
							} else {
								j.speed = 0.7*j.speed + 0.3*inst
							}
							lastT, lastB = now, done
						}
					})
				})
				if err == nil {
					break
				}
				if errors.Is(err, errIACancelled) {
					return err
				}
				if errors.Is(err, syscall.ENOSPC) {
					return fmt.Errorf("%s is full", j.toName)
				}
				if errors.Is(err, errCardGone) || !j.cardOK() {
					// the card was taken out: wait for it
					j.set(func() { j.phase = "paused" })
					for {
						if j.stopped() {
							return errIACancelled
						}
						time.Sleep(time.Second)
						if j.cardOK() {
							break
						}
					}
					j.set(func() { j.phase = "copy" })
					// the file's bytes were counted: count them again
					continue
				}
				return fmt.Errorf("%s: %v", f.rel, err)
			}
		}
	}
	// 3. checking
	j.set(func() { j.phase, j.cur, j.checked = "verify", 0, 0 })
	checked := 0
	for k, u := range list {
		j.set(func() { j.cur = k })
		for _, f := range u.files {
			if j.stopped() {
				return errIACancelled
			}
			for !j.cardOK() { // taken out while checking: wait for it
				j.set(func() { j.phase = "paused" })
				if j.stopped() {
					return errIACancelled
				}
				time.Sleep(time.Second)
			}
			j.set(func() { j.phase = "verify" })
			ok, err := j.check(f, buf)
			if err != nil || !ok {
				// once more from the start
				if cerr := j.copyFile(&moveFile{rel: f.rel, size: f.size, mod: f.mod}, buf, func(int64) {}); cerr != nil {
					return fmt.Errorf("%s: %v", f.rel, cerr)
				}
				f.have = false
				if ok, err = j.check(f, buf); err != nil || !ok {
					return fmt.Errorf("%s doesn't match its copy", f.rel)
				}
			}
			checked++
			j.set(func() { j.checked = checked })
		}
	}
	// 4. deleting the originals
	if j.move {
		j.set(func() { j.phase, j.checked = "delete", 0 })
		n := 0
		for _, u := range list {
			for _, f := range u.files {
				if err := os.Remove(filepath.Join(j.from, f.rel)); err != nil && !os.IsNotExist(err) {
					logMsg("Move Music: " + err.Error())
				}
				n++
				j.set(func() { j.checked = n })
			}
		}
		removeEmptyDirs(j.from)
	}
	j.set(func() { j.phase = "done" })
	return nil
}

// cardOK: the destination card is in, and it's the same card (not the
// empty folder a card taken out leaves, on the system card).
func (j *moveJob) cardOK() bool {
	if _, err := os.Stat(j.cardRoot); err != nil {
		return false
	}
	if j.cardDev != "" {
		if _, dev := mountOf(j.cardRoot); dev != j.cardDev {
			return false
		}
	}
	return checkWritable(j.cardRoot) == nil
}

// alreadyThere: the bytes of files already on the card (same size).
func (j *moveJob) alreadyThere(units []*moveUnit) int64 {
	var n int64
	for _, u := range units {
		for _, f := range u.files {
			if st, err := os.Stat(filepath.Join(j.to, f.rel)); err == nil && st.Size() == f.size {
				n += f.size
			}
		}
	}
	return n
}

// copyFile copies one file through .kpart (progress gets the bytes as they
// go); a file already there with its size is left (f.have).
func (j *moveJob) copyFile(f *moveFile, buf []byte, progress func(int64)) error {
	src := filepath.Join(j.from, f.rel)
	dst := filepath.Join(j.to, f.rel)
	if !j.cardOK() {
		return errCardGone
	}
	isList := strings.EqualFold(filepath.Ext(f.rel), ".m3u") || strings.EqualFold(filepath.Ext(f.rel), ".m3u8")
	if st, err := os.Stat(dst); err == nil && st.Size() == f.size && !isList {
		f.have = true
		progress(f.size)
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(dst), 0755); err != nil {
		return err
	}
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	part := dst + ".kpart"
	out, err := os.Create(part)
	if err != nil {
		return err
	}
	h := crc32.NewIEEE()
	var r io.Reader = in
	if isList {
		// playlists: lines with the old folder get the new one
		b, err := io.ReadAll(io.LimitReader(in, 16<<20))
		if err != nil {
			out.Close()
			os.Remove(part)
			return err
		}
		b = bytes.ReplaceAll(b, []byte(j.from+"/"), []byte(j.to+"/"))
		r = bytes.NewReader(b)
	}
	for {
		if j.stopped() {
			out.Close()
			os.Remove(part)
			return errIACancelled
		}
		n, rerr := r.Read(buf)
		if moveSlow > 0 && n > 0 {
			time.Sleep(time.Duration(moveSlow) * time.Millisecond) // (tests: MIYOOPOD_MOVE_SLOW, ms per MB)
		}
		if n > 0 {
			if _, werr := out.Write(buf[:n]); werr != nil {
				out.Close()
				os.Remove(part)
				return werr
			}
			h.Write(buf[:n])
			if !isList {
				progress(int64(n))
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
	if isList {
		progress(f.size)
	}
	if err := out.Sync(); err != nil {
		out.Close()
		os.Remove(part)
		return err
	}
	if err := out.Close(); err != nil {
		os.Remove(part)
		return err
	}
	if err := os.Rename(part, dst); err != nil {
		return err
	}
	_ = os.Chtimes(dst, f.mod, f.mod)
	f.crc = h.Sum32()
	return nil
}

// check reads the copy back from the card and compares it (with the
// original, for a file that was already there).
func (j *moveJob) check(f *moveFile, buf []byte) (bool, error) {
	sum := func(p string, drop bool) (uint32, int64, error) {
		fh, err := os.Open(p)
		if err != nil {
			return 0, 0, err
		}
		defer fh.Close()
		if drop {
			fadviseDontNeed(fh) // from the card, not the cache
		}
		h := crc32.NewIEEE()
		n, err := io.CopyBuffer(h, fh, buf)
		return h.Sum32(), n, err
	}
	want := f.crc
	isList := strings.EqualFold(filepath.Ext(f.rel), ".m3u") || strings.EqualFold(filepath.Ext(f.rel), ".m3u8")
	if f.have {
		c, n, err := sum(filepath.Join(j.from, f.rel), false)
		if err != nil {
			return false, err
		}
		if n != f.size {
			return false, nil
		}
		want = c
	}
	got, n, err := sum(filepath.Join(j.to, f.rel), true)
	if err != nil {
		return false, err
	}
	if !isList && n != f.size {
		return false, nil
	}
	return got == want, nil
}

var errCardGone = errors.New("the card isn't there")

// fadviseDontNeed drops a file's pages from the cache.
func fadviseDontNeed(f *os.File) {
	_ = f.Sync()
	syscall.Syscall6(syscall.SYS_FADVISE64, f.Fd(), 0, 0, 4 /* POSIX_FADV_DONTNEED */, 0, 0)
}

// removeEmptyDirs removes the empty folders under root (not root).
func removeEmptyDirs(root string) {
	var dirs []string
	filepath.Walk(root, func(p string, info os.FileInfo, err error) error {
		if err == nil && info.IsDir() && p != root {
			dirs = append(dirs, p)
		}
		return nil
	})
	sort.Slice(dirs, func(a, b int) bool { return len(dirs[a]) > len(dirs[b]) })
	for _, d := range dirs {
		os.Remove(d) // only when empty
	}
}

// ---- Settings → Library → Music Folder ----

func (ui *IpodUI) musicFolderView() *View {
	v := &View{ID: "musicFolder", Title: "Music Folder", Kind: KindFull}
	var cards []musicCard
	loaded := false
	go func() {
		cs := musicCards()
		ui.postSure(func() { cards, loaded = cs, true; ui.refreshView(v) })
	}()
	v.Build = func() []Item {
		cur := filepath.Clean(MUSIC_ROOT)
		_, curDev := mountOf(cur)
		items := []Item{{Label: "Now", Sublabel: cur, NoArrow: true}}
		if musicDirMissing != "" {
			items = append(items, Item{Label: "Chosen folder not found", Sublabel: musicDirMissing + " · is the card in?", NoArrow: true})
		}
		if j := moveCur; j != nil && j.phase != "done" && j.phase != "failed" && j.phase != "stopped" {
			items = append(items, Item{Label: "Moving the music…", Sublabel: "Press A to see it", OnSelect: func() {
				ui.showView(&View{ID: "moveMusic", Title: "Moving Music", Kind: KindFull, Custom: &moveView{job: j}}, "")
			}})
			return items
		}
		if !loaded {
			return append(items, Item{Label: "Looking for the cards…", NoArrow: true})
		}
		for _, c := range cards {
			c := c
			dest := musicDest(c)
			here := c.Dev == curDev || filepath.Clean(dest) == cur
			sub := fmt.Sprintf("%s free of %s · %s", humanBytes(c.Free), humanBytes(c.Total), dest)
			it := Item{Label: c.Name, Sublabel: sub}
			if here {
				it.Value, it.NoArrow = "Here", true
			} else {
				it.OnSelect = func() { ui.askMusicMove(c) }
			}
			items = append(items, it)
		}
		if len(cards) <= 1 {
			items = append(items, Item{Label: "No other card found", Sublabel: "Put a second card in (formatted with Knulli's Format A Disk)", NoArrow: true})
		}
		return items
	}
	v.Items = v.Build()
	return v
}

func (ui *IpodUI) askMusicMove(c musicCard) {
	if iaCur != nil && !iaCur.finished {
		ui.showToast("Wait for the download to end")
		return
	}
	if ui.app.LibScanRunning {
		ui.showToast("Wait for the library scan to end")
		return
	}
	size := ui.musicSize()
	title := fmt.Sprintf("Move %s of music to %s? Keep the console charging.", humanBytes(size), c.Name)
	opts := []SheetOption{
		{Label: "Move Music", OnSelect: func() { ui.startMusicMove(c, true) }},
		{Label: "Copy Music (keep the originals)", OnSelect: func() { ui.startMusicMove(c, false) }},
	}
	// the music is there already (an earlier copy): just use it
	if dest := musicDest(c); fileExists(dest) && dest != filepath.Clean(MUSIC_ROOT) {
		if es, _ := os.ReadDir(dest); len(es) > 0 {
			opts = append(opts, SheetOption{Label: "Use the Music Already There", OnSelect: func() {
				if err := setMusicRoot(dest); err != nil {
					ui.showToast("Couldn't: " + shortErr(err))
					return
				}
				ui.clearNowPlaying()
				ui.rescanLibrary()
				ui.showToast("Music folder: " + c.Name)
				ui.refreshBuilt()
			}})
		}
	}
	ui.showSheetTitled(title, opts)
}

// musicSize: the music's size, from the last scan.
func (ui *IpodUI) musicSize() int64 {
	scanStatsMu.Lock()
	defer scanStatsMu.Unlock()
	if scanStats != nil {
		return scanStats.AudioBytes
	}
	return 0
}
