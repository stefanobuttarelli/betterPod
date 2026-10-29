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
//  0. Before anything: the destination must be empty or this same move's
//     (a .betterpod-move file there says which folder it comes from), so
//     nothing of the card's own is overwritten; on a FAT/exFAT card, names
//     it can't hold (or two that differ only in upper/lower case) and files
//     over 4 GB (FAT32) stop it before it starts.
//  1. Counting: every file under the music folder (hidden ones too, but
//     not the database's folder when it lives there: it goes on changing),
//     grouped by album folder for the screen.
//  2. Copying, folder by folder, into <card>/music: each file through
//     name.kpart, synced, then renamed, with its CRC-32 taken on the way
//     and its date kept. A file already there with the same size (a move
//     that stopped) is checked against the original instead. Playlists'
//     lines starting with the old folder get the new one. A card taken out
//     pauses it until that card (its UUID) is back; a full card stops it.
//  3. Checking: each file is read back from the card (dropped from the
//     cache first, so it's really the card) and its CRC compared; a file
//     that doesn't match is copied again. If any was, every file is checked
//     once more (a repair can't undo another unseen).
//  4. The card is flushed, then the music folder becomes the new one (and
//     the database with it, copied, if it was inside), remembered on disk.
//  5. Only then, when moving (not copying): the originals are deleted, but
//     not one changed since it was copied, and the folders they leave empty.
//     Now Playing is cleared (its files moved) and the library scanned.
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
	have bool // already there (same size): not copied, checked against the original
	gone bool // gone from the music folder since it was counted: left out
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
	songs    int
	cardRoot string // the destination card's folder (its mount point)
	cardDev  string // its device: a card taken out leaves an empty folder there
	cardUUID string // its filesystem's UUID: another card in the same slot isn't it
	fsType   string // its filesystem (vfat, exfat…: limits on names and sizes)
	dataRel  string // the database's folder, when inside the music folder (relative)
	okAt     time.Time
	ok       bool
}

var moveCur *moveJob // the one running or just ended (UI goroutine)

func (j *moveJob) set(f func()) {
	j.mu.Lock()
	f()
	j.mu.Unlock()
}

// snapPhase: the phase (read under the lock: the worker writes it).
func (j *moveJob) snapPhase() string {
	j.mu.Lock()
	defer j.mu.Unlock()
	return j.phase
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
	if r, err := filepath.EvalSymlinks(from); err == nil {
		from = r // (a music folder that is a link: its real place)
	}
	j := &moveJob{from: from, to: to, fromName: cardName(fromDev, from), toName: c.Name, move: move,
		phase: "count", stop: make(chan struct{})}
	if os.Getenv("MIYOOPOD_FAKE_CARDS") != "" {
		j.fromName = filepath.Base(filepath.Dir(from))
	}
	if j.fromName == c.Name {
		j.fromName = "Here"
	}
	j.cardRoot = filepath.Dir(to)
	if os.Getenv("MIYOOPOD_FAKE_CARDS") == "" {
		_, j.cardDev = mountOf(j.cardRoot)
		j.cardUUID = uuidOf(j.cardDev)
		j.fsType = fsTypeOf(j.cardRoot)
	}
	if d := filepath.Clean(dataDir()); strings.HasPrefix(d+"/", from+"/") && d != from {
		j.dataRel, _ = filepath.Rel(from, d)
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
				// (the music folder was switched by run, before deleting)
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
	// 0. the destination: empty, or this same move's
	if err := j.checkDest(); err != nil {
		return err
	}
	// 1. counting
	units := map[string]*moveUnit{}
	other := &moveUnit{dir: ""}
	var all int64
	nfiles := 0
	var limits []string
	lower := map[string]string{}
	err := filepath.Walk(j.from, func(p string, info os.FileInfo, err error) error {
		if err != nil {
			return nil // an unreadable folder: left out
		}
		if j.stopped() {
			return errIACancelled
		}
		rel, _ := filepath.Rel(j.from, p)
		if info.IsDir() {
			if j.dataRel != "" && rel == j.dataRel {
				return filepath.SkipDir // the database: copied when switching
			}
			return nil
		}
		if !info.Mode().IsRegular() || strings.HasSuffix(p, ".kpart") {
			return nil
		}
		if why := j.fsLimit(rel, info.Size(), lower); why != "" {
			limits = append(limits, why)
			return nil
		}
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
	if len(limits) > 0 {
		more := ""
		if len(limits) > 1 {
			more = fmt.Sprintf(" (and %d more)", len(limits)-1)
		}
		return fmt.Errorf("%s%s: nothing was copied", limits[0], more)
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
	need := all - j.alreadyThere(list)
	if free, _, ok := diskSpace(filepath.Dir(j.to)); ok && free < need+64<<20 {
		return fmt.Errorf("Not enough space on %s: %s free, %s needed", j.toName, humanBytes(free), humanBytes(need))
	}
	// this move's: a stopped one goes on, and nothing else is ever overwritten
	if err := os.MkdirAll(j.to, 0755); err != nil {
		return err
	}
	if err := writeFileAtomic(filepath.Join(j.to, moveMarker), []byte(j.from+"\n")); err != nil {
		return err
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
				err := j.copyFile(f, buf, false, func(n int64) {
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
					if f.have || f.gone {
						// not copied: not part of the speed
						j.set(func() { lastB += f.size })
					}
					break
				}
				if errors.Is(err, errIACancelled) {
					return err
				}
				if errors.Is(err, syscall.ENOSPC) {
					return fmt.Errorf("%s is full", j.toName)
				}
				if errors.Is(err, errCardGone) || !j.cardOK(true) {
					if err := j.waitCard(); err != nil {
						return err
					}
					continue // the file's bytes were counted: count them again
				}
				return fmt.Errorf("%s: %v", f.rel, err)
			}
		}
	}

	// 3. checking (all again if any had to be copied again)
	for pass := 0; ; pass++ {
		repaired, err := j.checkAll(list, buf, pass == 0)
		if err != nil {
			return err
		}
		if repaired == 0 {
			break
		}
		if pass == 1 {
			return errors.New("The copies keep changing: is the card all right?")
		}
	}

	// 4. the card flushed, then the music folder switched (and the
	// database, if it was inside), before anything is deleted
	syscall.Sync()
	if err := j.switchTo(ui); err != nil {
		return fmt.Errorf("Couldn't switch to %s: %v", j.toName, err)
	}

	// 5. deleting the originals (not one changed since it was copied)
	if j.move {
		j.set(func() { j.phase, j.checked = "delete", 0 })
		n, kept := 0, 0
		dirs := map[string]bool{}
		for _, u := range list {
			for _, f := range u.files {
				n++
				j.set(func() { j.checked = n })
				if f.gone {
					continue
				}
				src := filepath.Join(j.from, f.rel)
				if st, err := os.Stat(src); err != nil || st.Size() != f.size || !st.ModTime().Equal(f.mod) {
					if err == nil {
						kept++
						logMsg("Move Music: kept (changed since copied): " + f.rel)
					}
					continue
				}
				if err := os.Remove(src); err != nil && !os.IsNotExist(err) {
					logMsg("Move Music: " + err.Error())
				}
				dirs[filepath.Dir(src)] = true
			}
		}
		if j.dataRel != "" {
			os.RemoveAll(filepath.Join(j.from, j.dataRel)) // (copied when switching)
			dirs[filepath.Dir(filepath.Join(j.from, j.dataRel))] = true
		}
		removeEmptyDirs(j.from, dirs)
		if kept > 0 {
			logMsg(fmt.Sprintf("Move Music: %d changed files kept in %s", kept, j.from))
		}
	}
	os.Remove(filepath.Join(j.to, moveMarker))
	os.Remove(filepath.Join(j.to, legacyMoveMarker))
	j.set(func() { j.phase = "done" })
	return nil
}

// moveMarker: in the destination while a move goes into it: the folder it
// comes from.
const moveMarker = ".betterpod-move"

// legacyMoveMarker: the same, written before the app was called betterPod.
const legacyMoveMarker = ".knullipod-move"

// checkDest: the destination is empty (but for hidden files), or holds
// this same move's files (its marker names this music folder).
func (j *moveJob) checkDest() error {
	for _, mk := range []string{moveMarker, legacyMoveMarker} {
		if b, err := os.ReadFile(filepath.Join(j.to, mk)); err == nil && strings.TrimSpace(string(b)) == j.from {
			return nil
		}
	}
	busy := false
	filepath.Walk(j.to, func(p string, info os.FileInfo, err error) error {
		if err != nil || busy {
			return nil
		}
		if info.Mode().IsRegular() && !strings.HasPrefix(info.Name(), ".") && !strings.HasSuffix(p, ".kpart") {
			busy = true
			return filepath.SkipAll
		}
		return nil
	})
	if busy {
		return fmt.Errorf("%s already has music in %s: use the music already there, or empty that folder first", j.toName, j.to)
	}
	return nil
}

// fsLimit: why the card can't hold this file (FAT/exFAT names, FAT32's
// 4 GB), or "". lower keeps the names seen, by folder and lower case.
func (j *moveJob) fsLimit(rel string, size int64, lower map[string]string) string {
	fat := j.fsType == "vfat" || j.fsType == "msdos" || j.fsType == "fat" || j.fsType == "exfat"
	if !fat {
		return ""
	}
	if j.fsType != "exfat" && size >= 4<<30 {
		return fmt.Sprintf("%s is over 4 GB, too big for %s", rel, j.toName)
	}
	for _, part := range strings.Split(rel, "/") {
		if strings.ContainsAny(part, `:*?"<>|\`) || strings.HasSuffix(part, ".") || strings.HasSuffix(part, " ") {
			return fmt.Sprintf("%s has a name %s can't hold", rel, j.toName)
		}
	}
	k := strings.ToLower(rel)
	if o, ok := lower[k]; ok && o != rel {
		return fmt.Sprintf("%s and %s differ only in upper/lower case, which %s can't tell apart", o, rel, j.toName)
	}
	lower[k] = rel
	return ""
}

// waitCard: paused until the card (the same one) is back.
func (j *moveJob) waitCard() error {
	phase := ""
	j.set(func() { phase, j.phase = j.phase, "paused" })
	for {
		if j.stopped() {
			return errIACancelled
		}
		time.Sleep(time.Second)
		if j.cardOK(true) {
			break
		}
	}
	j.set(func() { j.phase = phase })
	return nil
}

// checkAll reads every copy back and copies again those that don't match
// (repair: false on the second pass, which only reports them).
func (j *moveJob) checkAll(list []*moveUnit, buf []byte, repair bool) (int, error) {
	j.set(func() { j.phase, j.cur, j.checked = "verify", 0, 0 })
	checked, repaired := 0, 0
	for k, u := range list {
		j.set(func() { j.cur = k })
		for _, f := range u.files {
			if j.stopped() {
				return 0, errIACancelled
			}
			if !j.cardOK(false) {
				if err := j.waitCard(); err != nil {
					return 0, err
				}
			}
			if !f.gone {
				ok, err := j.check(f, buf)
				if errors.Is(err, os.ErrNotExist) && !fileExists(filepath.Join(j.from, f.rel)) {
					f.gone, ok, err = true, true, nil // gone from the music folder meanwhile
				}
				if err != nil || !ok {
					repaired++
					if !repair {
						return repaired, nil
					}
					// once more from the start (the old copy removed first,
					// or it would be taken as already there)
					os.Remove(filepath.Join(j.to, f.rel))
					f.have = false
					if st, err := os.Stat(filepath.Join(j.from, f.rel)); err == nil {
						f.size, f.mod = st.Size(), st.ModTime()
					}
					if cerr := j.copyFile(f, buf, true, func(int64) {}); cerr != nil {
						return 0, fmt.Errorf("%s: %v", f.rel, cerr)
					}
				}
			}
			checked++
			j.set(func() { j.checked = checked })
		}
	}
	return repaired, nil
}

// switchTo makes the copy the music folder, on the UI goroutine: the
// database first (copied, when it was inside the music folder), then the
// folder, both remembered on disk.
func (j *moveJob) switchTo(ui *IpodUI) error {
	res := make(chan error, 1)
	ui.postSure(func() {
		if j.dataRel != "" {
			ui.savePrefs()
			from, to := filepath.Join(j.from, j.dataRel), filepath.Join(j.to, j.dataRel)
			if err := copyDatabase(from, to, false, nil); err != nil {
				res <- err
				return
			}
			if err := setDataDir(to); err != nil {
				res <- err
				return
			}
		}
		res <- setMusicRoot(j.to)
	})
	return <-res
}

// cardOK: the destination card is in, and it's the same card (not the
// empty folder a card taken out leaves, on the system card, nor another
// card in the same slot). Looked at again at most every 2 s unless full.
func (j *moveJob) cardOK(full bool) bool {
	if !full && time.Since(j.okAt) < 2*time.Second {
		return j.ok
	}
	ok := j.cardCheck()
	j.ok, j.okAt = ok, time.Now()
	return ok
}

func (j *moveJob) cardCheck() bool {
	if _, err := os.Stat(j.cardRoot); err != nil {
		return false
	}
	if j.cardDev != "" {
		_, dev := mountOf(j.cardRoot)
		if dev != j.cardDev {
			return false
		}
		if j.cardUUID != "" && uuidOf(dev) != j.cardUUID {
			return false
		}
	}
	return checkWritable(j.cardRoot) == nil
}

// uuidOf: the UUID of the filesystem on dev ("" if not known).
func uuidOf(dev string) string {
	if dev == "" {
		return ""
	}
	want, err := filepath.EvalSymlinks(dev)
	if err != nil {
		want = dev
	}
	es, err := os.ReadDir("/dev/disk/by-uuid")
	if err != nil {
		return ""
	}
	for _, e := range es {
		if t, err := filepath.EvalSymlinks(filepath.Join("/dev/disk/by-uuid", e.Name())); err == nil && t == want {
			return e.Name()
		}
	}
	return ""
}

// fsTypeOf: the filesystem type of the mount holding path ("" if unknown).
func fsTypeOf(path string) string {
	mnt, _ := mountOf(path)
	f, err := os.Open("/proc/mounts")
	if err != nil {
		return ""
	}
	defer f.Close()
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		fs := strings.Fields(sc.Text())
		if len(fs) >= 3 && strings.ReplaceAll(fs[1], `\040`, " ") == mnt {
			return fs[2]
		}
	}
	return ""
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
func (j *moveJob) copyFile(f *moveFile, buf []byte, force bool, progress func(int64)) error {
	src := filepath.Join(j.from, f.rel)
	dst := filepath.Join(j.to, f.rel)
	if !j.cardOK(false) {
		return errCardGone
	}
	isList := isPlaylistFile(f.rel)
	if st, err := os.Stat(dst); !force && err == nil && st.Size() == f.size && !isList {
		f.have = true // (checked against the original, not taken on trust)
		progress(f.size)
		return nil
	}
	in, err := os.Open(src)
	if os.IsNotExist(err) {
		f.gone = true // gone from the music folder since it was counted
		progress(f.size)
		return nil
	}
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(dst), 0755); err != nil {
		in.Close()
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
		// playlists: lines starting with the old folder get the new one
		if f.size > 16<<20 {
			out.Close()
			os.Remove(part)
			return errors.New("a playlist over 16 MB")
		}
		b, err := io.ReadAll(in)
		if err != nil {
			out.Close()
			os.Remove(part)
			return err
		}
		r = bytes.NewReader(rewritePlaylist(b, j.from, j.to))
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

func isPlaylistFile(rel string) bool {
	e := strings.ToLower(filepath.Ext(rel))
	return e == ".m3u" || e == ".m3u8"
}

// rewritePlaylist: the lines starting with from/ start with to/ instead.
func rewritePlaylist(b []byte, from, to string) []byte {
	lines := bytes.SplitAfter(b, []byte("\n"))
	old, neu := []byte(from+"/"), []byte(to+"/")
	for i, l := range lines {
		if bytes.HasPrefix(l, old) {
			lines[i] = append(append([]byte{}, neu...), l[len(old):]...)
		}
	}
	return bytes.Join(lines, nil)
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
	isList := isPlaylistFile(f.rel)
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
	syscall.Syscall6(syscall.SYS_FADVISE64, f.Fd(), 0, 0, 4 /* POSIX_FADV_DONTNEED */, 0, 0)
}

// removeEmptyDirs removes the folders the moved files left empty, and
// their parents that are empty then, up to root (not root).
func removeEmptyDirs(root string, dirs map[string]bool) {
	var list []string
	for d := range dirs {
		for ; strings.HasPrefix(d, root+"/"); d = filepath.Dir(d) {
			list = append(list, d)
		}
	}
	sort.Slice(list, func(a, b int) bool { return len(list[a]) > len(list[b]) })
	for _, d := range list {
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
		if j := moveCur; j != nil && moveRunning(j.snapPhase()) {
			items = append(items, Item{Label: "Moving the music…", Sublabel: "Press [A] to see it", OnSelect: func() {
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
