package main

// datadir.go — where the database lives (library cache, artwork, settings,
// scan statistics, game saves). The default is MIYOOPOD_DATA_DIR (the port's
// conf folder); Settings → Database can move it to another folder, e.g. the
// SD card with the music. The choice is kept in a small pointer file in the
// default folder, which is always there. All paths are computed on use
// (dataPath), so switching folders never races with background saves.

import (
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

const dataDirPointer = "datadir.txt"

// the files and folders that make up the database (nothing else is ever
// copied or deleted: the data folder may be shared with other things, as
// /userdata or a card's root). Whatever the app keeps in the data folder
// must be listed here, or a move leaves it behind (unittest: datadir_test.go
// looks for dataPath / dataDir names in the code). Not listed: tmp/ (things
// under way), and the pointers datadir.txt and musicdir.txt, which stay in
// the default folder.
var dataItems = []string{
	".miyoopod_library.json",
	".miyoopod_settings.json",
	".miyoopod_playback.json",
	".miyoopod_update.json",
	".miyoopod_update_status",
	".update_check.json",
	"scan_stats.json",
	"ipodjs_settings.json",
	"album_merges.json",
	"recent_searches.json",
	"podcasts.json",
	"suno.json",
	"weather.json",
	"share_count.txt",
	"ipodSolitaireSaveState.json",
	"ipodSolitaireWins.txt",
	"fiveletters.json",
	"2048.json",
	"quiz_best.txt",
	"puzzles",
	"chosen_covers",
	".miyoopod_artwork",
	".ipod_art",
	"tools",
}

// folderItems: everything in a database folder of its own (the one in the
// music folder, which Move Music takes whole), but tmp/ and the pointers.
func folderItems(dir string) []string {
	var out []string
	es, _ := os.ReadDir(dir)
	for _, e := range es {
		switch n := e.Name(); n {
		case "tmp", dataDirPointer, musicDirPointer:
		default:
			out = append(out, n)
		}
	}
	return out
}

var (
	dataDirMu       sync.Mutex
	dataDirOverride = readDataDirPointer()
)

// defaultDataDir is the launcher's data folder (upstream: the music root).
func defaultDataDir() string {
	dir := os.Getenv("MIYOOPOD_DATA_DIR")
	if dir == "" {
		dir = musicRoot()
	}
	return filepath.Clean(dir)
}

func readDataDirPointer() string {
	b, err := os.ReadFile(filepath.Join(defaultDataDir(), dataDirPointer))
	if err != nil {
		return ""
	}
	d := filepath.Clean(strings.TrimSpace(string(b)))
	if d == "" || d == "." || !filepath.IsAbs(d) {
		return ""
	}
	d = renameLegacyDataDir(d)
	// the chosen folder must still be there (an SD card may be missing):
	// otherwise fall back to the default one for this run
	if st, err := os.Stat(d); err != nil || !st.IsDir() {
		dataDirMissing = d
		return ""
	}
	return d
}

// dataDirMissing is the chosen folder when it could not be found at start.
var dataDirMissing string

// dataDir is the folder that holds the database now.
func dataDir() string {
	dataDirMu.Lock()
	defer dataDirMu.Unlock()
	if dataDirOverride != "" {
		return dataDirOverride
	}
	return defaultDataDir()
}

// setDataDir makes dir the database folder and remembers the choice.
func setDataDir(dir string) error {
	dir = filepath.Clean(dir)
	def := defaultDataDir()
	ptr := filepath.Join(def, dataDirPointer)
	if dir == def {
		if err := os.Remove(ptr); err != nil && !os.IsNotExist(err) {
			return err
		}
		dir = ""
	} else {
		if err := os.MkdirAll(def, 0755); err != nil {
			return err
		}
		if err := writeFileAtomic(ptr, []byte(dir+"\n")); err != nil {
			return err
		}
	}
	dataDirMu.Lock()
	dataDirOverride = dir
	dataDirMu.Unlock()
	dataDirMissing = ""
	return nil
}

// checkWritable tells whether the database could be written in dir. If dir
// doesn't exist yet, its nearest existing parent is tested instead: nothing
// is created until the move is confirmed.
func checkWritable(dir string) error {
	d := dir
	for {
		st, err := os.Stat(d)
		if err == nil {
			if !st.IsDir() {
				return fmt.Errorf("%s is not a folder", d)
			}
			break
		}
		parent := filepath.Dir(d)
		if parent == d {
			return err
		}
		d = parent
	}
	f, err := os.CreateTemp(d, ".betterpod-test-*")
	if err != nil {
		return err
	}
	name := f.Name()
	f.Close()
	return os.Remove(name)
}

// hasDatabase reports whether dir already holds a betterPod library.
func hasDatabase(dir string) bool {
	st, err := os.Stat(filepath.Join(dir, ".miyoopod_library.json"))
	return err == nil && st.Size() > 0
}

// copyDatabase copies the database items from src to dst; progress is
// called with the number of files copied so far. With onlyNewer, files
// already in dst are copied again only if src's copy is newer (the final
// pass of a move, for what the app wrote while copying).
func copyDatabase(src, dst string, onlyNewer bool, progress func(int)) error {
	return copyItems(src, dst, dataItems, onlyNewer, progress)
}

// copyItems: copyDatabase for the items named.
func copyItems(src, dst string, items []string, onlyNewer bool, progress func(int)) error {
	if err := os.MkdirAll(dst, 0755); err != nil {
		return err
	}
	n := 0
	skip := func(from, to string, info os.FileInfo) bool {
		if !onlyNewer {
			return false
		}
		st, err := os.Stat(to)
		return err == nil && !info.ModTime().After(st.ModTime())
	}
	for _, name := range items {
		from := filepath.Join(src, name)
		st, err := os.Stat(from)
		if err != nil {
			continue
		}
		to := filepath.Join(dst, name)
		if st.IsDir() {
			err = filepath.Walk(from, func(p string, info os.FileInfo, err error) error {
				if err != nil {
					return err
				}
				rel, _ := filepath.Rel(from, p)
				target := filepath.Join(to, rel)
				if info.IsDir() {
					return os.MkdirAll(target, 0755)
				}
				if !info.Mode().IsRegular() || strings.HasSuffix(p, ".tmp") || skip(p, target, info) {
					return nil
				}
				if err := copyFile(p, target); err != nil {
					return err
				}
				n++
				if progress != nil && n%25 == 0 {
					progress(n)
				}
				return nil
			})
		} else if !skip(from, to, st) {
			err = copyFile(from, to)
			n++
		}
		if err != nil {
			return fmt.Errorf("%s: %v", name, err)
		}
	}
	if progress != nil {
		progress(n)
	}
	return nil
}

// removeDatabase deletes the database items from dir (Replace with
// current, before copying there).
func removeDatabase(dir string) {
	for _, name := range dataItems {
		_ = os.RemoveAll(filepath.Join(dir, name))
	}
}

// removeCopied deletes, after a move, the items' files in src that are in
// dst too, with the same size, then the folders left empty. Nothing that
// wasn't copied is deleted (a file written meanwhile, or that couldn't be
// read). It returns how many files were kept.
func removeCopied(src, dst string, items []string) int {
	kept := 0
	for _, name := range items {
		root := filepath.Join(src, name)
		if _, err := os.Lstat(root); err != nil {
			continue
		}
		var dirs []string
		filepath.Walk(root, func(p string, info os.FileInfo, err error) error {
			if err != nil {
				kept++
				return nil
			}
			if info.IsDir() {
				dirs = append(dirs, p)
				return nil
			}
			rel, _ := filepath.Rel(src, p)
			st, err := os.Stat(filepath.Join(dst, rel))
			if err != nil || !st.Mode().IsRegular() || st.Size() != info.Size() || st.ModTime().Before(info.ModTime()) {
				kept++
				return nil
			}
			if os.Remove(p) != nil {
				kept++
			}
			return nil
		})
		for i := len(dirs) - 1; i >= 0; i-- {
			os.Remove(dirs[i]) // (only if empty)
		}
	}
	return kept
}

// copyFile copies a file through a .tmp, synced, then renamed (its mode
// kept: the YouTube tools are programs).
func copyFile(from, to string) error {
	in, err := os.Open(from)
	if err != nil {
		return err
	}
	defer in.Close()
	mode := os.FileMode(0644)
	if st, err := in.Stat(); err == nil {
		mode = st.Mode().Perm()
	}
	tmp := to + ".tmp"
	out, err := os.OpenFile(tmp, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, mode)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, in); err == nil {
		err = out.Sync()
	}
	if err != nil {
		out.Close()
		os.Remove(tmp)
		return err
	}
	if err := out.Close(); err != nil {
		os.Remove(tmp)
		return err
	}
	return os.Rename(tmp, to)
}

// atomicLocks: one lock per file written by writeFileAtomic, so two
// writers of the same file don't share its .tmp.
var atomicLocks sync.Map

// writeFileAtomic writes the file whole or not at all: into a .tmp, synced
// (a power cut after the rename must not leave an empty file: the rename
// can reach the card before the data), then renamed over the old one.
func writeFileAtomic(path string, data []byte) error {
	mu, _ := atomicLocks.LoadOrStore(path, &sync.Mutex{})
	mu.(*sync.Mutex).Lock()
	defer mu.(*sync.Mutex).Unlock()
	tmp := path + ".tmp"
	f, err := os.OpenFile(tmp, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0644)
	if err != nil {
		return err
	}
	if _, err = f.Write(data); err == nil {
		err = f.Sync()
	}
	if cerr := f.Close(); err == nil {
		err = cerr
	}
	if err != nil {
		os.Remove(tmp)
		return err
	}
	return os.Rename(tmp, path)
}

// rebaseArtPaths points the albums' artwork files at the current database
// folder (the library stores absolute paths): the artwork cache's, and the
// covers chosen by hand. Other covers (a cover.jpg by the songs) stay.
func rebaseArtPaths(lib *Library) {
	if lib == nil {
		return
	}
	for _, al := range lib.Albums {
		if p := dataArtPath(al.ArtPath); p != "" {
			al.ArtPath = p
		}
	}
}

// dataArtPath: where a cover kept in a database folder (the artwork cache,
// or chosen_covers) is in the current one; "" for a cover kept elsewhere.
func dataArtPath(p string) string {
	if p == "" {
		return ""
	}
	switch filepath.Base(filepath.Dir(p)) {
	case ".miyoopod_artwork":
		return ARTWORK_DIR() + filepath.Base(p)
	case "chosen_covers":
		return filepath.Join(dataDir(), "chosen_covers", filepath.Base(p))
	}
	return ""
}

// artPathFor finds an album's artwork file, also when the library was
// written in another data folder.
func artPathFor(p string) string {
	if p == "" {
		return p
	}
	if _, err := os.Stat(p); err == nil {
		return p
	}
	if alt := dataArtPath(p); alt != "" {
		if _, err := os.Stat(alt); err == nil {
			return alt
		}
	}
	return p
}

// databaseSize is the space taken by the database items in dir (not by the
// rest of the folder, which may be big: /userdata, a card's root…).
func databaseSize(dir string) int64 {
	var n int64
	for _, name := range dataItems {
		p := filepath.Join(dir, name)
		if st, err := os.Stat(p); err == nil && !st.IsDir() {
			n += st.Size()
		} else if err == nil {
			n += dirSize(p)
		}
	}
	return n
}

// renameLegacyDataDir: a database folder made before the app was called
// betterPod (".knullipod" in the music folder, "knullipod" on a card or
// anywhere chosen) takes the new name once, if nothing has that name yet;
// the pointer follows. Anything that fails leaves it where it is.
func renameLegacyDataDir(d string) string {
	base := filepath.Base(d)
	var nb string
	switch base {
	case ".knullipod":
		nb = ".betterpod"
	case "knullipod":
		nb = "betterpod"
	default:
		return d
	}
	nd := filepath.Join(filepath.Dir(d), nb)
	if _, err := os.Lstat(nd); err == nil {
		return d
	}
	if st, err := os.Stat(d); err != nil || !st.IsDir() {
		return d
	}
	if err := os.Rename(d, nd); err != nil {
		return d
	}
	if err := writeFileAtomic(filepath.Join(defaultDataDir(), dataDirPointer), []byte(nd+"\n")); err != nil {
		os.Rename(nd, d) // (the pointer could not follow: back as it was)
		return d
	}
	return nd
}
