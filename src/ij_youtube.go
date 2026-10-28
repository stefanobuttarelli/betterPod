package main

// ij_youtube.go — YouTube (off by default; Settings → YouTube): the tools
// and the settings page. Searching and downloading are in ij_ytdl.go.
//
// The work is done by yt-dlp (github.com/yt-dlp/yt-dlp), which needs a
// JavaScript runtime for YouTube: QuickJS-ng (small) or, failing that,
// Deno. Neither ships with KnulliPod: the settings page downloads them
// into the data folder (tools/) the first time YouTube is turned on, and
// can update or remove them. MIYOOPOD_YTDLP / MIYOOPOD_YTJS point at other
// programs (tests).

import (
	"archive/zip"
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"
)

// ytEnabled: YouTube is turned on in the settings (saved with the prefs).
var ytEnabled bool

const (
	ytdlpURL = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64"
	qjsURL   = "https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-linux-aarch64"
	denoURL  = "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-unknown-linux-gnu.zip"
)

func ytToolsDir() string { return dataPath("tools/") }

func ytdlpPath() string {
	if p := os.Getenv("MIYOOPOD_YTDLP"); p != "" {
		return p
	}
	return filepath.Join(ytToolsDir(), "yt-dlp")
}

// ytRuntime: the JavaScript runtime for yt-dlp, as its --js-runtimes
// value ("" if none is there).
func ytRuntime() string {
	if p := os.Getenv("MIYOOPOD_YTJS"); p != "" {
		return p
	}
	if p := filepath.Join(ytToolsDir(), "qjs"); fileExists(p) {
		return "quickjs:" + p
	}
	if p := filepath.Join(ytToolsDir(), "deno"); fileExists(p) {
		return "deno:" + p
	}
	return ""
}

func ytToolsReady() bool { return fileExists(ytdlpPath()) && ytRuntime() != "" }

// ytCommand runs yt-dlp with the common options (no config files, our
// runtime, cache and home in the data folder).
func ytCommand(ctx context.Context, args ...string) *exec.Cmd {
	base := []string{"--no-config", "--no-warnings", "--cache-dir", filepath.Join(ytToolsDir(), "cache")}
	if rt := ytRuntime(); rt != "" && rt != "none" {
		base = append(base, "--js-runtimes", rt)
	}
	cmd := exec.CommandContext(ctx, ytdlpPath(), append(base, args...)...)
	cmd.Env = append(os.Environ(), "HOME="+ytToolsDir(), "XDG_CACHE_HOME="+filepath.Join(ytToolsDir(), "cache"))
	return cmd
}

// ---- getting the tools ----

// ytTools: the state of the tools download, shown on the settings page.
var ytTools struct {
	mu      sync.Mutex
	busy    bool
	status  string // "Downloading yt-dlp… 45%", an error, ""
	version string // yt-dlp's, once known
}

func ytToolsStatus() (bool, string) {
	ytTools.mu.Lock()
	defer ytTools.mu.Unlock()
	return ytTools.busy, ytTools.status
}

func ytSetStatus(s string) {
	ytTools.mu.Lock()
	ytTools.status = s
	ytTools.mu.Unlock()
}

// fetchFile downloads url into path (via path.part), reporting progress.
func fetchFile(url, path string, progress func(done, total int64)) error {
	req, _ := http.NewRequest("GET", url, nil)
	req.Header.Set("User-Agent", "KnulliPod/"+KNULLIPOD_VERSION)
	resp, err := updateClient(10 * time.Minute).Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return fmt.Errorf("GitHub answered %d", resp.StatusCode)
	}
	tmp := path + ".part"
	out, err := os.Create(tmp)
	if err != nil {
		return err
	}
	buf := make([]byte, 64<<10)
	var done int64
	last := time.Now()
	for {
		n, rerr := resp.Body.Read(buf)
		if n > 0 {
			if _, err := out.Write(buf[:n]); err != nil {
				out.Close()
				os.Remove(tmp)
				return err
			}
			done += int64(n)
			if time.Since(last) > 300*time.Millisecond {
				progress(done, resp.ContentLength)
				last = time.Now()
			}
		}
		if rerr == io.EOF {
			break
		}
		if rerr != nil {
			out.Close()
			os.Remove(tmp)
			return rerr
		}
	}
	if err := out.Close(); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}

// works: the program runs (args: a harmless call).
func works(path string, args ...string) bool {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	return exec.CommandContext(ctx, path, args...).Run() == nil
}

// ytInstallTools downloads yt-dlp and a JavaScript runtime (QuickJS-ng,
// else Deno); update: download yt-dlp again even if it is there.
func (ui *IpodUI) ytInstallTools(update bool, done func()) {
	ytTools.mu.Lock()
	if ytTools.busy {
		ytTools.mu.Unlock()
		return
	}
	ytTools.busy = true
	ytTools.mu.Unlock()
	nudge := func() { ui.post(func() { ui.dirty = true; ui.refreshTop() }) }
	go func() {
		err := func() error {
			dir := ytToolsDir()
			if err := os.MkdirAll(dir, 0755); err != nil {
				return err
			}
			step := func(name, url, path string) error {
				ytSetStatus("Downloading " + name + "…")
				nudge()
				return fetchFile(url, path, func(d, t int64) {
					if t > 0 {
						ytSetStatus(fmt.Sprintf("Downloading %s… %d%% of %s", name, d*100/t, formatBytes(t)))
					} else {
						ytSetStatus(fmt.Sprintf("Downloading %s… %s", name, formatBytes(d)))
					}
					nudge()
				})
			}
			if p := ytdlpPath(); update || !fileExists(p) {
				if err := step("yt-dlp", ytdlpURL, p); err != nil {
					return fmt.Errorf("yt-dlp: %s", shortErr(err))
				}
				os.Chmod(p, 0755)
				if !works(p, "--version") {
					os.Remove(p)
					return errors.New("yt-dlp doesn't run on this console")
				}
			}
			if ytRuntime() == "" || update {
				qjs := filepath.Join(dir, "qjs")
				err := step("QuickJS", qjsURL, qjs)
				if err == nil {
					os.Chmod(qjs, 0755)
					if !works(qjs, "-e", "1") {
						os.Remove(qjs)
						err = errors.New("QuickJS doesn't run here")
					}
				}
				if err != nil && ytRuntime() == "" {
					// Deno: bigger, but the runtime yt-dlp prefers
					logMsg("YouTube: QuickJS: " + err.Error() + ", trying Deno")
					z := filepath.Join(dir, "deno.zip")
					if err := step("Deno", denoURL, z); err != nil {
						return fmt.Errorf("JavaScript runtime: %s", shortErr(err))
					}
					ytSetStatus("Unpacking Deno…")
					nudge()
					err := unzipOne(z, "deno", filepath.Join(dir, "deno"))
					os.Remove(z)
					if err != nil {
						return err
					}
					os.Chmod(filepath.Join(dir, "deno"), 0755)
				}
			}
			return nil
		}()
		v := ytVersion()
		ytTools.mu.Lock()
		ytTools.busy = false
		ytTools.version = v
		if err != nil {
			ytTools.status = "Couldn't get the tools · " + err.Error()
			logMsg("YouTube: tools: " + err.Error())
		} else {
			ytTools.status = ""
		}
		ytTools.mu.Unlock()
		ui.postSure(func() {
			ui.refreshTop()
			if done != nil && err == nil {
				done()
			}
		})
	}()
}

// unzipOne extracts the file called name from a zip archive.
func unzipOne(zipPath, name, dst string) error {
	zr, err := zip.OpenReader(zipPath)
	if err != nil {
		return err
	}
	defer zr.Close()
	for _, f := range zr.File {
		if filepath.Base(f.Name) != name {
			continue
		}
		rc, err := f.Open()
		if err != nil {
			return err
		}
		defer rc.Close()
		out, err := os.Create(dst)
		if err != nil {
			return err
		}
		if _, err := io.Copy(out, rc); err != nil {
			out.Close()
			return err
		}
		return out.Close()
	}
	return fmt.Errorf("%s not in the archive", name)
}

// ytVersion: yt-dlp's version ("" if it doesn't run).
func ytVersion() string {
	if !fileExists(ytdlpPath()) {
		return ""
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	out, err := ytCommand(ctx, "--version").Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

// ytRemoveTools deletes the tools (and their cache).
func ytRemoveTools() {
	os.RemoveAll(filepath.Join(ytToolsDir(), "cache"))
	for _, n := range []string{"yt-dlp", "qjs", "deno"} {
		os.Remove(filepath.Join(ytToolsDir(), n))
	}
	ytTools.mu.Lock()
	ytTools.version = ""
	ytTools.mu.Unlock()
}

// refreshTop rebuilds the view on top (its Build reads shared state).
func (ui *IpodUI) refreshTop() {
	if v := ui.topScreen(); v != nil && v.Build != nil {
		ui.refreshView(v)
	}
	ui.dirty = true
}

// ---- Settings → YouTube ----

func onOff(b bool) string {
	if b {
		return "On"
	}
	return "Off"
}

func (ui *IpodUI) youtubeSettingsView() *View {
	v := &View{ID: "ytSettings", Title: "YouTube", Kind: KindFull}
	if ytTools.version == "" && fileExists(ytdlpPath()) {
		go func() {
			ver := ytVersion()
			ytTools.mu.Lock()
			ytTools.version = ver
			ytTools.mu.Unlock()
			ui.post(func() { ui.refreshView(v) })
		}()
	}
	v.Build = func() []Item {
		busy, status := ytToolsStatus()
		items := []Item{{Label: "YouTube", Value: onOff(ytEnabled), Sublabel: "Search and download in Music → Search", NoArrow: true,
			OnSelect: func() {
				ytEnabled = !ytEnabled
				ui.savePrefs()
				if ytEnabled && !ytToolsReady() {
					ui.ytInstallTools(false, nil)
				}
				ui.refreshBuilt()
			}}}
		switch {
		case busy:
			items = append(items, Item{Label: status, Sublabel: "The tools for YouTube, from GitHub", NoArrow: true})
		case status != "":
			l, sub, _ := strings.Cut(status, " · ")
			items = append(items, Item{Label: l, Sublabel: sub + " · press to try again", NoArrow: true,
				OnSelect: func() { ui.ytInstallTools(false, nil) }})
		case ytToolsReady():
			ytTools.mu.Lock()
			ver := ytTools.version
			ytTools.mu.Unlock()
			rt := "QuickJS"
			if strings.HasPrefix(ytRuntime(), "deno") {
				rt = "Deno"
			}
			sub := "yt-dlp " + ver + " · " + rt
			if ver == "" {
				sub = "yt-dlp · " + rt
			}
			items = append(items,
				Item{Label: "Tools Ready", Sublabel: sub, NoArrow: true},
				Item{Label: "Update Tools", Sublabel: "YouTube changes often: update when downloads fail", OnSelect: func() { ui.ytInstallTools(true, nil) }},
				Item{Label: "Remove Tools", Sublabel: "Frees their space; YouTube is turned off", NoArrow: true, OnSelect: func() {
					ui.showSheetTitled("Remove the YouTube tools?", []SheetOption{{Label: "Remove Tools", Danger: true, OnSelect: func() {
						ytRemoveTools()
						ytEnabled = false
						ui.savePrefs()
						ui.refreshBuilt()
					}}})
				}})
		default:
			items = append(items, Item{Label: "Get the Tools", Sublabel: "yt-dlp and a JavaScript runtime, about 40 MB, from GitHub",
				OnSelect: func() { ui.ytInstallTools(false, nil) }})
		}
		items = append(items, Item{Label: "Only download what you may", Sublabel: "Your own or freely licensed videos", NoArrow: true})
		return items
	}
	v.Items = v.Build()
	return v
}
