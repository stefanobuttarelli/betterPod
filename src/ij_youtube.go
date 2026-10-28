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
	"crypto/sha256"
	"debug/elf"
	hexenc "encoding/hex"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"syscall"
	"time"
)

// ytEnabled: YouTube is turned on in the settings (saved with the prefs).
var ytEnabled bool

const (
	ytRel      = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/"
	ytZipName  = "yt-dlp_linux_aarch64.zip" // unpacked: starts faster than the one-file build
	ytOneName  = "yt-dlp_linux_aarch64"
	ytSumsName = "SHA2-256SUMS"
	qjsURL     = "https://github.com/quickjs-ng/quickjs/releases/latest/download/qjs-linux-aarch64"
	denoURL    = "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-unknown-linux-gnu.zip"
)

func ytToolsDir() string { return dataPath("tools/") }

// ytdlpPath: the unpacked build if it is there, else the one-file build.
func ytdlpPath() string {
	if p := os.Getenv("MIYOOPOD_YTDLP"); p != "" {
		return p
	}
	if p := filepath.Join(ytToolsDir(), "ytdlp", ytOneName); fileExists(p) {
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
// runtime, cache and home in the data folder). It runs in its own process
// group, so cancelling stops yt-dlp and everything it started (the
// one-file build is a launcher plus Python, and the runtime): SIGTERM
// first (it cleans up), SIGKILL 5 s later.
func ytCommand(ctx context.Context, args ...string) *exec.Cmd {
	base := []string{"--no-config", "--no-warnings", "--cache-dir", filepath.Join(ytToolsDir(), "cache")}
	if rt := ytRuntime(); rt != "" && rt != "none" {
		base = append(base, "--js-runtimes", rt)
	}
	cmd := exec.CommandContext(ctx, ytdlpPath(), append(base, args...)...)
	cmd.Env = append(os.Environ(), "HOME="+ytToolsDir(), "XDG_CACHE_HOME="+filepath.Join(ytToolsDir(), "cache"),
		"TMPDIR="+filepath.Join(ytToolsDir(), "tmp"))
	os.MkdirAll(filepath.Join(ytToolsDir(), "tmp"), 0755)
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
	cmd.Cancel = func() error {
		pid := cmd.Process.Pid
		err := syscall.Kill(-pid, syscall.SIGTERM)
		go func() {
			time.Sleep(5 * time.Second)
			syscall.Kill(-pid, syscall.SIGKILL)
		}()
		return err
	}
	cmd.WaitDelay = 6 * time.Second // then the pipes are closed even if a child holds them
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

func ytGetVersion() string {
	ytTools.mu.Lock()
	defer ytTools.mu.Unlock()
	return ytTools.version
}

// fetchFile downloads url into path (via path.part), reporting progress;
// it returns the SHA-256 of what it wrote, and checks the size against
// the server's.
func fetchFile(url, path string, progress func(done, total int64)) (string, error) {
	req, _ := http.NewRequest("GET", url, nil)
	req.Header.Set("User-Agent", "KnulliPod/"+KNULLIPOD_VERSION)
	resp, err := updateClient(10 * time.Minute).Do(req)
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return "", fmt.Errorf("GitHub answered %d", resp.StatusCode)
	}
	tmp := path + ".part"
	out, err := os.Create(tmp)
	if err != nil {
		return "", err
	}
	h := sha256.New()
	w := io.MultiWriter(out, h)
	buf := make([]byte, 64<<10)
	var done int64
	last := time.Now()
	fail := func(err error) (string, error) {
		out.Close()
		os.Remove(tmp)
		return "", err
	}
	for {
		n, rerr := resp.Body.Read(buf)
		if n > 0 {
			if _, err := w.Write(buf[:n]); err != nil {
				return fail(err)
			}
			done += int64(n)
			if progress != nil && time.Since(last) > 300*time.Millisecond {
				progress(done, resp.ContentLength)
				last = time.Now()
			}
		}
		if rerr == io.EOF {
			break
		}
		if rerr != nil {
			return fail(rerr)
		}
	}
	if resp.ContentLength > 0 && done != resp.ContentLength {
		return fail(errors.New("the download was cut short"))
	}
	if err := out.Close(); err != nil {
		os.Remove(tmp)
		return "", err
	}
	return hexenc.EncodeToString(h.Sum(nil)), os.Rename(tmp, path)
}

// fetchSum: the SHA-256 listed for name in a checksum file ("hash  name"
// lines; a file with one hash matches any name).
func fetchSum(url, name string) (string, error) {
	tmp := filepath.Join(ytToolsDir(), "sums.tmp")
	defer os.Remove(tmp)
	if _, err := fetchFile(url, tmp, nil); err != nil {
		return "", err
	}
	data, err := os.ReadFile(tmp)
	if err != nil {
		return "", err
	}
	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	for _, l := range lines {
		f := strings.Fields(l)
		if len(f) >= 2 && strings.TrimPrefix(f[len(f)-1], "*") == name {
			return strings.ToLower(f[0]), nil
		}
		if len(lines) == 1 && len(f) >= 1 && len(f[0]) == 64 {
			return strings.ToLower(f[0]), nil
		}
	}
	return "", fmt.Errorf("no checksum for %s", name)
}

// checkTool: an executable for this console's CPU that runs (args: a
// harmless call).
func checkTool(path string, args ...string) error {
	nf, err := elf.Open(path)
	if err != nil {
		return errors.New("not a program for this console")
	}
	machine := nf.Machine
	nf.Close()
	if exe, err := startExe(); err == nil {
		if cf, err := elf.Open(exe); err == nil {
			mine := cf.Machine
			cf.Close()
			if mine != machine {
				return fmt.Errorf("built for another CPU (%v)", machine)
			}
		}
	}
	os.Chmod(path, 0755)
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	if err := exec.CommandContext(ctx, path, args...).Run(); err != nil {
		return errors.New("doesn't run on this console")
	}
	return nil
}

// ytInstallTools downloads yt-dlp and a JavaScript runtime (QuickJS-ng,
// else Deno). update: get them again even if they are there. Each tool is
// downloaded beside the old one, checked (checksum where the project
// publishes one, CPU, a test run) and only then put in its place, so a
// failed update leaves the old tools working.
func (ui *IpodUI) ytInstallTools(update bool, done func()) {
	ytTools.mu.Lock()
	if ytTools.busy {
		ytTools.mu.Unlock()
		return
	}
	ytTools.busy = true
	ytTools.mu.Unlock()
	nudge := func() {
		ui.post(func() {
			if v := ui.topScreen(); v != nil && v.ID == "ytSettings" {
				ui.refreshView(v)
			}
			ui.dirty = true
		})
	}
	go func() {
		err := func() error {
			dir := ytToolsDir()
			if err := os.MkdirAll(dir, 0755); err != nil {
				return err
			}
			step := func(name, url, path string) (string, error) {
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
			if update || !fileExists(ytdlpPath()) {
				if err := ytInstallYtdlp(step); err != nil {
					return fmt.Errorf("yt-dlp: %s", shortErr(err))
				}
			}
			if ytRuntime() == "" || update {
				qjs := filepath.Join(dir, "qjs")
				_, err := step("QuickJS", qjsURL, qjs+".new")
				if err == nil {
					if err = checkTool(qjs+".new", "-e", "1"); err == nil {
						err = os.Rename(qjs+".new", qjs)
					}
				}
				os.Remove(qjs + ".new")
				if err != nil {
					logMsg("YouTube: QuickJS: " + err.Error())
				}
				if err != nil && ytRuntime() == "" {
					// Deno: bigger, but the runtime yt-dlp prefers
					if err := ytInstallDeno(step); err != nil {
						return fmt.Errorf("JavaScript runtime: %s", shortErr(err))
					}
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

type fetchStep func(name, url, path string) (string, error)

// ytInstallYtdlp: the unpacked build (zip), checked against the release's
// SHA2-256SUMS; the one-file build if the zip can't be used.
func ytInstallYtdlp(step fetchStep) error {
	dir := ytToolsDir()
	sums := ytRel + ytSumsName
	try := func(name string, install func(file string) error) error {
		want, err := fetchSum(sums, name)
		if err != nil {
			return err
		}
		file := filepath.Join(dir, name+".download")
		defer os.Remove(file)
		got, err := step("yt-dlp", ytRel+name, file)
		if err != nil {
			return err
		}
		if got != want {
			return errors.New("the file doesn't match its published checksum")
		}
		return install(file)
	}
	err := try(ytZipName, func(file string) error {
		tmp := filepath.Join(dir, "ytdlp.new")
		os.RemoveAll(tmp)
		if err := unzipAll(file, tmp); err != nil {
			os.RemoveAll(tmp)
			return err
		}
		exe := filepath.Join(tmp, ytOneName)
		if !fileExists(exe) {
			os.RemoveAll(tmp)
			return errors.New("unexpected archive")
		}
		if err := checkTool(exe, "--version"); err != nil {
			os.RemoveAll(tmp)
			return err
		}
		os.RemoveAll(filepath.Join(dir, "ytdlp"))
		if err := os.Rename(tmp, filepath.Join(dir, "ytdlp")); err != nil {
			return err
		}
		os.Remove(filepath.Join(dir, "yt-dlp")) // the one-file build isn't needed any more
		return nil
	})
	if err == nil {
		return nil
	}
	logMsg("YouTube: yt-dlp zip: " + err.Error() + ", trying the one-file build")
	return try(ytOneName, func(file string) error {
		if err := checkTool(file, "--version"); err != nil {
			return err
		}
		return os.Rename(file, filepath.Join(dir, "yt-dlp"))
	})
}

// ytInstallDeno: Deno from its zip, checked against its .sha256sum.
func ytInstallDeno(step fetchStep) error {
	dir := ytToolsDir()
	want, err := fetchSum(denoURL+".sha256sum", filepath.Base(denoURL))
	if err != nil {
		return err
	}
	z := filepath.Join(dir, "deno.zip")
	defer os.Remove(z)
	got, err := step("Deno", denoURL, z)
	if err != nil {
		return err
	}
	if got != want {
		return errors.New("Deno doesn't match its published checksum")
	}
	ytSetStatus("Unpacking Deno…")
	tmp := filepath.Join(dir, "deno.new")
	defer os.Remove(tmp)
	if err := unzipOne(z, "deno", tmp); err != nil {
		return err
	}
	if err := checkTool(tmp, "--version"); err != nil {
		return err
	}
	return os.Rename(tmp, filepath.Join(dir, "deno"))
}

// unzipOne extracts the file called name from a zip archive.
func unzipOne(zipPath, name, dst string) error {
	zr, err := zip.OpenReader(zipPath)
	if err != nil {
		return err
	}
	defer zr.Close()
	for _, f := range zr.File {
		if filepath.Base(f.Name) == name && !f.FileInfo().IsDir() {
			return unzipFile(f, dst)
		}
	}
	return fmt.Errorf("%s not in the archive", name)
}

// unzipAll extracts an archive into dir (nothing outside it).
func unzipAll(zipPath, dir string) error {
	zr, err := zip.OpenReader(zipPath)
	if err != nil {
		return err
	}
	defer zr.Close()
	root := filepath.Clean(dir) + string(filepath.Separator)
	for _, f := range zr.File {
		dst := filepath.Join(dir, f.Name)
		if !strings.HasPrefix(dst, root) {
			return fmt.Errorf("bad path in the archive: %s", f.Name)
		}
		if f.FileInfo().IsDir() {
			if err := os.MkdirAll(dst, 0755); err != nil {
				return err
			}
			continue
		}
		if err := os.MkdirAll(filepath.Dir(dst), 0755); err != nil {
			return err
		}
		if err := unzipFile(f, dst); err != nil {
			return err
		}
		if f.Mode()&0111 != 0 {
			os.Chmod(dst, 0755)
		}
	}
	return nil
}

func unzipFile(f *zip.File, dst string) error {
	rc, err := f.Open()
	if err != nil {
		return err
	}
	defer rc.Close()
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	if _, err := io.CopyN(out, rc, int64(f.UncompressedSize64)); err != nil {
		out.Close()
		os.Remove(dst)
		return err
	}
	return out.Close()
}

// ytVersion: yt-dlp's version ("" if it doesn't run).
func ytVersion() string {
	if !fileExists(ytdlpPath()) {
		return ""
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	out, err := ytCommand(ctx, "--version").Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

// ytRemoveTools deletes the tools (and their cache and work files).
func ytRemoveTools() {
	for _, n := range []string{"cache", "tmp", "work", "ytdlp", "yt-dlp", "qjs", "deno"} {
		os.RemoveAll(filepath.Join(ytToolsDir(), n))
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
	if ytGetVersion() == "" && fileExists(ytdlpPath()) {
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
			items = append(items, Item{Label: l, Sublabel: sub + " · press A to try again", NoArrow: true,
				OnSelect: func() { ui.ytInstallTools(false, nil) }})
		case ytToolsReady():
			ver := ytGetVersion()
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
			items = append(items, Item{Label: "Get the Tools", Sublabel: "yt-dlp and QuickJS from GitHub: 45 MB to get, 100 MB on the card",
				OnSelect: func() { ui.ytInstallTools(false, nil) }})
		}
		items = append(items, Item{Label: "Only download what you may", Sublabel: "Your own or freely licensed videos", NoArrow: true})
		return items
	}
	v.Items = v.Build()
	return v
}
