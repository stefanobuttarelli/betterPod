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
// copied or deleted: the data folder may be shared with other things)
var dataItems = []string{
	".miyoopod_library.json",
	".miyoopod_settings.json",
	".miyoopod_playback.json",
	"scan_stats.json",
	"ipodjs_settings.json",
	"ipodSolitaireSaveState.json",
	"ipodSolitaireWins.txt",
	".miyoopod_artwork",
	".ipod_art",
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
	for _, name := range dataItems {
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

// removeDatabase deletes the database items from dir (after a move).
func removeDatabase(dir string) {
	for _, name := range dataItems {
		_ = os.RemoveAll(filepath.Join(dir, name))
	}
}

func copyFile(from, to string) error {
	in, err := os.Open(from)
	if err != nil {
		return err
	}
	defer in.Close()
	tmp := to + ".tmp"
	out, err := os.Create(tmp)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, in); err != nil {
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

func writeFileAtomic(path string, data []byte) error {
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, data, 0644); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}

// rebaseArtPaths points the albums' artwork files at the current artwork
// folder (the library stores absolute paths).
func rebaseArtPaths(lib *Library) {
	if lib == nil {
		return
	}
	for _, al := range lib.Albums {
		if al.ArtPath != "" {
			al.ArtPath = ARTWORK_DIR() + filepath.Base(al.ArtPath)
		}
	}
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
	alt := ARTWORK_DIR() + filepath.Base(p)
	if _, err := os.Stat(alt); err == nil {
		return alt
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
