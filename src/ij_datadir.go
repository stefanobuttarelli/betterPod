package main

// ij_datadir.go — Settings → Database: choose the folder that holds the
// library database (see datadir.go), with a simple folder browser.

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

var (
	dbBusy   bool   // a move / switch is running
	dbStatus string // last result, shown on the Database screen
)

// dbLabel is the short name of a database folder for the Settings row.
func dbLabel(dir string) string {
	switch filepath.Clean(dir) {
	case defaultDataDir():
		return "Port folder"
	case musicDataDir():
		return "Music folder"
	}
	return filepath.Base(dir)
}

// musicDataDir is the suggested folder inside the music folder (hidden, so
// the library scan skips it).
func musicDataDir() string { return filepath.Join(MUSIC_ROOT, ".knullipod") }

// externalRoots lists mounted cards and drives under /media.
func externalRoots() []string {
	var out []string
	entries, _ := os.ReadDir("/media")
	for _, e := range entries {
		if e.IsDir() && !strings.HasPrefix(e.Name(), ".") {
			out = append(out, filepath.Join("/media", e.Name()))
		}
	}
	return out
}

func (ui *IpodUI) databaseView() *View {
	v := &View{ID: "database", Title: "Database", Kind: KindSplit}
	v.Build = func() []Item {
		cur := dataDir()
		pick := func(label, sub, dir string) Item {
			return Item{Label: label, Sublabel: sub, Checked: filepath.Clean(dir) == cur, NoArrow: true,
				Preview: PrevSettings, OnSelect: func() { ui.chooseDataDir(dir) }}
		}
		var items []Item
		if dbBusy {
			items = append(items, Item{Label: dbStatus, NoArrow: true, Preview: PrevSettings})
		} else if dbStatus != "" {
			items = append(items, Item{Label: "Status", Sublabel: dbStatus, NoArrow: true, Preview: PrevSettings})
		} else if dataDirMissing != "" {
			items = append(items, Item{Label: "Folder not found", Sublabel: dataDirMissing, NoArrow: true, Preview: PrevSettings})
		}
		items = append(items, Item{Label: "Current folder", Sublabel: cur, NoArrow: true, Preview: PrevSettings})
		items = append(items,
			pick("Port folder", defaultDataDir(), defaultDataDir()),
			pick("Music folder", musicDataDir(), musicDataDir()),
		)
		for _, r := range externalRoots() {
			d := filepath.Join(r, "knullipod")
			items = append(items, pick(filepath.Base(r), d, d))
		}
		// a folder chosen with the browser
		known := cur == defaultDataDir() || cur == musicDataDir()
		for _, r := range externalRoots() {
			known = known || cur == filepath.Join(r, "knullipod")
		}
		if !known {
			items = append(items, pick(filepath.Base(cur), cur, cur))
		}
		items = append(items, Item{Label: "Choose a folder…", Preview: PrevSettings,
			OnSelect: func() { ui.showView(ui.folderBrowserView("/userdata"), "") }})
		if r := externalRoots(); len(r) > 0 {
			items = append(items, Item{Label: "Browse cards and drives…", Preview: PrevSettings,
				OnSelect: func() { ui.showView(ui.folderBrowserView("/media"), "") }})
		}
		return items
	}
	v.Items = v.Build()
	return v
}

// folderBrowserView lists the subfolders of dir.
func (ui *IpodUI) folderBrowserView(dir string) *View {
	v := &View{ID: "dbBrowse", Title: filepath.Base(dir), Kind: KindSplit}
	if dir == "/" {
		v.Title = "/"
	}
	v.Build = func() []Item {
		items := []Item{{Label: "Use this folder", Sublabel: dir, NoArrow: true, Preview: PrevSettings,
			OnSelect: func() { ui.chooseDataDir(dir) }}}
		if _, err := os.Stat(filepath.Join(dir, "knullipod")); err != nil && dir != "/" {
			items = append(items, Item{Label: "New folder “knullipod”", NoArrow: true, Preview: PrevSettings,
				OnSelect: func() {
					nd := filepath.Join(dir, "knullipod")
					if err := os.MkdirAll(nd, 0755); err != nil {
						dbStatus = "Can't create the folder: " + err.Error()
						ui.refreshBuilt()
						return
					}
					ui.refreshBuilt()
					ui.showView(ui.folderBrowserView(nd), "")
				}})
		}
		entries, _ := os.ReadDir(dir)
		var names []string
		for _, e := range entries {
			if e.IsDir() && (!strings.HasPrefix(e.Name(), ".") || e.Name() == ".knullipod") {
				names = append(names, e.Name())
			}
		}
		sort.Slice(names, func(i, j int) bool { return strings.ToLower(names[i]) < strings.ToLower(names[j]) })
		for _, n := range names {
			sub := filepath.Join(dir, n)
			it := Item{Label: n, Preview: PrevSettings, OnSelect: func() { ui.showView(ui.folderBrowserView(sub), "") }}
			if hasDatabase(sub) {
				it.Sublabel = "contains a KnulliPod database"
			}
			items = append(items, it)
		}
		return items
	}
	v.Items = v.Build()
	return v
}

// backToDatabase closes the folder browser screens.
func (ui *IpodUI) backToDatabase() {
	for len(ui.split) > 1 && ui.topScreen().ID == "dbBrowse" {
		ui.hideView()
	}
}

// chooseDataDir asks what to do with the database for the folder dir.
func (ui *IpodUI) chooseDataDir(dir string) {
	dir = filepath.Clean(dir)
	cur := dataDir()
	if dir == cur || dbBusy {
		return
	}
	if ui.app.LibScanRunning {
		dbStatus = "Wait for the library scan to finish"
		ui.refreshBuilt()
		return
	}
	// not inside the database itself (the artwork would copy into itself)
	if rel, err := filepath.Rel(cur, dir); err == nil && !strings.HasPrefix(rel, "..") {
		for _, it := range dataItems {
			if rel == it || strings.HasPrefix(rel, it+string(filepath.Separator)) {
				dbStatus = "That folder is part of the database"
				ui.refreshBuilt()
				return
			}
		}
	}
	if err := checkWritable(dir); err != nil {
		dbStatus = "Can't write there: " + err.Error()
		ui.refreshBuilt()
		return
	}
	if hasDatabase(dir) {
		ui.showSheet([]SheetOption{
			{Label: "Use database there", OnSelect: func() { ui.useDataDir(dir) }},
			{Label: "Replace with current", OnSelect: func() { ui.moveDataDir(dir, true) }},
		})
		return
	}
	ui.showSheet([]SheetOption{
		{Label: "Move database here", OnSelect: func() { ui.moveDataDir(dir, false) }},
	})
}

// moveDataDir copies the database to dir, switches to it and removes the
// old copy. replace first clears the database already in dir.
func (ui *IpodUI) moveDataDir(dir string, replace bool) {
	app := ui.app
	old := dataDir()
	dbBusy, dbStatus = true, "Moving the database…"
	ui.backToDatabase()
	ui.refreshBuilt()
	// make sure what's in memory is on disk first
	_ = app.saveSettings()
	ui.savePrefs()
	app.savePlaybackState()
	logMsg(fmt.Sprintf("DATABASE: moving %s -> %s", old, dir))
	go func() {
		if replace {
			removeDatabase(dir)
		}
		err := copyDatabase(old, dir, func(n int) {
			ui.post(func() {
				dbStatus = fmt.Sprintf("Copying… %d files", n)
				ui.refreshBuilt()
			})
		})
		ui.post(func() {
			if err == nil {
				err = setDataDir(dir)
			}
			if err != nil {
				logMsg("DATABASE: move failed: " + err.Error())
				dbBusy, dbStatus = false, "Move failed: "+err.Error()
				ui.refreshBuilt()
				return
			}
			rebaseArtPaths(app.Library)
			dbBusy, dbStatus = false, "Database moved to "+dir
			ui.refreshBuilt()
			go func() {
				if app.Library != nil {
					if err := app.saveLibraryJSON(); err != nil {
						logMsg("DATABASE: " + err.Error())
					}
				}
				removeDatabase(old)
				logMsg("DATABASE: now in " + dir)
			}()
		})
	}()
}

// useDataDir switches to the database already in dir (e.g. one shared on
// the SD card with the music) and loads its library.
func (ui *IpodUI) useDataDir(dir string) {
	app := ui.app
	if err := setDataDir(dir); err != nil {
		dbStatus = "Can't switch: " + err.Error()
		ui.refreshBuilt()
		return
	}
	logMsg("DATABASE: using " + dir)
	dbBusy, dbStatus = true, "Loading the library…"
	ui.backToDatabase()
	ui.refreshBuilt()
	// keep this console's settings if the folder has none
	if _, err := os.Stat(SETTINGS_PATH); err != nil {
		_ = app.saveSettings()
	}
	if _, err := os.Stat(ipodPrefsPath()); err != nil {
		ui.savePrefs()
	}
	go func() {
		scanStatsMu.Lock()
		scanStats = nil
		scanStatsMu.Unlock()
		loadScanStats()
		err := app.loadLibraryJSON()
		ui.post(func() {
			dbBusy = false
			if err != nil {
				logMsg("DATABASE: " + err.Error() + ", rescanning")
				dbStatus = "No usable library there: rescanning"
				ui.rescanLibrary()
			} else {
				dbStatus = "Using the database in " + dir
			}
			ui.kb.initialized = false
			ui.refreshBuilt()
		})
	}()
}
