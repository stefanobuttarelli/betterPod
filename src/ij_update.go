package main

// ij_update.go — Settings → Software Update: KnulliPod updates itself over
// Wi-Fi from its GitHub repository.
//
// The repository carries a small manifest, release/latest.json:
//
//	{"version": "0.22", "url": ".../knullipod-v0.22-knulli.zip",
//	 "sha256": "…", "size": 6012345, "notes": "…"}
//
// "Check for updates" reads it; when the version is newer, "Install"
// downloads the zip (checking size and SHA-256), unpacks it next to the
// port, replaces the files of the installation one by one (each with an
// atomic rename, so even the running binary can be replaced) and restarts
// the app. The database (knullipod/conf and any folder chosen in Settings →
// Database) is never touched. The previous binary is kept as
// knullipod/MiyooPod.prev.
//
// HTTPS is verified against the system roots plus the Mozilla bundle in
// assets/cacert.pem; if the console's clock is behind (no network time
// yet), certificates are checked at the build date instead of failing.

import (
	"archive/zip"
	"crypto/sha256"
	"crypto/tls"
	"crypto/x509"
	"debug/elf"
	hexenc "encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

// KNULLIPOD_VERSION is the version shown in About and compared with the
// manifest; knulliPodBuildDate is the earliest date certificates are
// checked at.
const (
	KNULLIPOD_VERSION  = "0.23"
	knulliPodBuildDate = "2026-10-27"
	updateManifestURL  = "https://raw.githubusercontent.com/stefanobuttarelli/knullipod/main/release/latest.json"
)

type updateManifest struct {
	Version string `json:"version"`
	URL     string `json:"url"`
	SHA256  string `json:"sha256"`
	Size    int64  `json:"size"`
	Notes   string `json:"notes"`
}

type updState int

const (
	updIdle updState = iota
	updChecking
	updUpToDate
	updAvailable
	updDownloading
	updInstalling
	updDone
	updFailed
)

// upd is the update state, owned by the UI goroutine (the worker posts
// its changes through ui.post).
var upd struct {
	state    updState
	manifest *updateManifest
	progress float64 // download, 0..1
	err      string
	checked  time.Time
}

// restartAfterExit makes main exec the (new) binary once the app has shut
// down cleanly (playback state saved, SDL closed).
var restartAfterExit bool

func (ui *IpodUI) updateView() *View {
	v := &View{ID: "update", Title: "Update", Kind: KindSplit}
	v.Build = func() []Item {
		items := []Item{{Label: "Version", Value: KNULLIPOD_VERSION, NoArrow: true, Preview: PrevSettings}}
		act := func(label, sub string, f func()) Item {
			return Item{Label: label, Sublabel: sub, NoArrow: true, Preview: PrevSettings, OnSelect: f}
		}
		info := func(label, sub string) Item {
			return Item{Label: label, Sublabel: sub, NoArrow: true, Preview: PrevSettings}
		}
		switch upd.state {
		case updIdle:
			items = append(items, act("Check for updates", "Needs Wi-Fi", ui.checkForUpdate))
		case updChecking:
			items = append(items, info("Checking…", "Connecting to GitHub"))
		case updUpToDate:
			items = append(items, act("Check for updates", "KnulliPod is up to date", ui.checkForUpdate))
		case updAvailable:
			m := upd.manifest
			items = append(items, act("Install v"+m.Version, fmt.Sprintf("%.1f MB download", float64(m.Size)/1e6), ui.installUpdate))
			for _, l := range noteLines(m.Notes, 4) {
				items = append(items, Item{Label: "• " + l, NoArrow: true, Preview: PrevSettings})
			}
		case updDownloading:
			items = append(items, Item{Label: "Downloading…", Value: fmt.Sprintf("%d%%", int(upd.progress*100)),
				Sublabel: "v" + upd.manifest.Version, NoArrow: true, Preview: PrevSettings})
		case updInstalling:
			items = append(items, info("Installing…", "Don't turn off the console"))
		case updDone:
			items = append(items, info("Restarting…", "Updated to v"+upd.manifest.Version))
		case updFailed:
			items = append(items, act("Try again", upd.err, ui.checkForUpdate))
		}
		return items
	}
	v.Items = v.Build()
	return v
}

// noteLines splits the release notes into at most n lines for the list.
func noteLines(s string, n int) []string {
	var out []string
	for _, l := range strings.Split(strings.ReplaceAll(s, "\r", ""), "\n") {
		l = strings.TrimSpace(strings.TrimLeft(strings.TrimSpace(l), "-*•"))
		if l != "" && len(out) < n {
			out = append(out, l)
		}
	}
	return out
}

func (ui *IpodUI) setUpdate(f func()) {
	ui.post(func() {
		f()
		ui.refreshBuilt()
	})
}

func (ui *IpodUI) checkForUpdate() {
	if upd.state == updChecking || upd.state == updDownloading || upd.state == updInstalling {
		return
	}
	upd.state = updChecking
	ui.refreshBuilt()
	go func() {
		m, err := fetchManifest()
		ui.setUpdate(func() {
			upd.checked = time.Now()
			switch {
			case err != nil:
				upd.state, upd.err = updFailed, updateErrText(err)
				logMsg("Update check failed: " + err.Error())
			case isNewerVersion(KNULLIPOD_VERSION, m.Version):
				upd.state, upd.manifest = updAvailable, m
				logMsg("Update available: v" + m.Version)
			default:
				upd.state = updUpToDate
			}
		})
	}()
}

func (ui *IpodUI) installUpdate() {
	if upd.state != updAvailable || upd.manifest == nil {
		return
	}
	m := upd.manifest
	upd.state, upd.progress = updDownloading, 0
	ui.refreshBuilt()
	go func() {
		err := runUpdate(m, func(p float64) {
			ui.post(func() {
				if upd.state == updDownloading && int(p*100) != int(upd.progress*100) {
					upd.progress = p
					ui.refreshBuilt()
				}
			})
		}, func() {
			ui.setUpdate(func() { upd.state = updInstalling })
		})
		ui.setUpdate(func() {
			if err != nil {
				upd.state, upd.err = updFailed, updateErrText(err)
				logMsg("Update failed: " + err.Error())
				return
			}
			upd.state = updDone
			logMsg("Updated to v" + m.Version + ", restarting")
			time.AfterFunc(1500*time.Millisecond, func() {
				ui.post(func() {
					restartAfterExit = true
					ui.app.Running = false
				})
			})
		})
	}()
}

// updateErrText: a short message for the list row.
func updateErrText(err error) string {
	var dns *net.DNSError
	var op *net.OpError
	switch {
	case errors.As(err, &dns), errors.As(err, &op):
		return "No Wi-Fi connection"
	case errors.Is(err, os.ErrDeadlineExceeded) || strings.Contains(err.Error(), "Client.Timeout"):
		return "Connection timed out"
	}
	var cert x509.UnknownAuthorityError
	var inv x509.CertificateInvalidError
	if errors.As(err, &cert) || errors.As(err, &inv) || strings.Contains(err.Error(), "x509") {
		return "Secure connection failed"
	}
	s := err.Error()
	if len(s) > 60 {
		s = s[:60] + "…"
	}
	return s
}

// --- network ---------------------------------------------------------------

func updateRoots() *x509.CertPool {
	pool, err := x509.SystemCertPool()
	if err != nil || pool == nil {
		pool = x509.NewCertPool()
	}
	if exe, err := startExe(); err == nil {
		if pem, err := os.ReadFile(filepath.Join(filepath.Dir(exe), "assets", "cacert.pem")); err == nil {
			pool.AppendCertsFromPEM(pem)
		}
	}
	return pool
}

// updateClient verifies the server certificate itself, at the current
// time or at the build date if the clock is behind it.
func updateClient(timeout time.Duration) *http.Client {
	roots := updateRoots()
	build, _ := time.Parse("2006-01-02", knulliPodBuildDate)
	cfg := &tls.Config{
		InsecureSkipVerify: true, // replaced by VerifyConnection below
		VerifyConnection: func(cs tls.ConnectionState) error {
			if len(cs.PeerCertificates) == 0 {
				return errors.New("no server certificate")
			}
			at := time.Now()
			if at.Before(build) {
				at = build
			}
			opts := x509.VerifyOptions{Roots: roots, DNSName: cs.ServerName, CurrentTime: at, Intermediates: x509.NewCertPool()}
			for _, c := range cs.PeerCertificates[1:] {
				opts.Intermediates.AddCert(c)
			}
			_, err := cs.PeerCertificates[0].Verify(opts)
			return err
		},
	}
	tr := http.DefaultTransport.(*http.Transport).Clone()
	tr.TLSClientConfig = cfg
	return &http.Client{Timeout: timeout, Transport: tr}
}

func fetchManifest() (*updateManifest, error) {
	url := updateManifestURL
	if u := os.Getenv("MIYOOPOD_UPDATE_URL"); u != "" {
		url = u
	}
	req, _ := http.NewRequest("GET", url, nil)
	req.Header.Set("Cache-Control", "no-cache")
	resp, err := updateClient(15 * time.Second).Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return nil, fmt.Errorf("Server error %d", resp.StatusCode)
	}
	var m updateManifest
	if err := json.NewDecoder(io.LimitReader(resp.Body, 64<<10)).Decode(&m); err != nil {
		return nil, errors.New("Bad update info")
	}
	if m.Version == "" || m.URL == "" || len(m.SHA256) != 64 || m.Size <= 0 || m.Size > 200<<20 {
		return nil, errors.New("Bad update info")
	}
	return &m, nil
}

// --- install ---------------------------------------------------------------

// installDirs: the port folder (…/ports/knullipod, where the binary is) and
// the ports folder above it (where KnulliPod.sh is).
func installDirs() (string, string, error) {
	exe, err := startExe()
	if err != nil {
		return "", "", err
	}
	port := filepath.Dir(exe)
	return port, filepath.Dir(port), nil
}

type countWriter struct {
	n, total int64
	report   func(float64)
	last     time.Time
}

func (c *countWriter) Write(p []byte) (int, error) {
	c.n += int64(len(p))
	if now := time.Now(); now.Sub(c.last) > 150*time.Millisecond || c.n == c.total {
		c.last = now
		c.report(float64(c.n) / float64(c.total))
	}
	return len(p), nil
}

// runUpdate downloads, verifies and installs the update described by m.
func runUpdate(m *updateManifest, progress func(float64), installing func()) error {
	port, ports, err := installDirs()
	if err != nil {
		return err
	}
	stage := filepath.Join(ports, ".knullipod-update")
	os.RemoveAll(stage)
	if err := os.MkdirAll(stage, 0755); err != nil {
		return err
	}
	defer os.RemoveAll(stage)

	// download, hashing as it arrives
	zipPath := filepath.Join(stage, "update.zip")
	resp, err := updateClient(10 * time.Minute).Get(m.URL)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 200 {
		return fmt.Errorf("Download failed (%d)", resp.StatusCode)
	}
	out, err := os.Create(zipPath)
	if err != nil {
		return err
	}
	h := sha256.New()
	cw := &countWriter{total: m.Size, report: progress}
	n, err := io.Copy(io.MultiWriter(out, h, cw), io.LimitReader(resp.Body, m.Size+1))
	if cerr := out.Close(); err == nil {
		err = cerr
	}
	if err != nil {
		return err
	}
	if n != m.Size {
		return errors.New("Download incomplete")
	}
	if !strings.EqualFold(hexenc.EncodeToString(h.Sum(nil)), m.SHA256) {
		return errors.New("Download corrupted")
	}
	installing()

	// unpack into the staging folder
	files, err := unpackUpdate(zipPath, filepath.Join(stage, "new"))
	if err != nil {
		return err
	}
	newBin := filepath.Join(stage, "new", "knullipod", "MiyooPod")
	if err := checkBinary(newBin); err != nil {
		return err
	}
	// keep the running binary as MiyooPod.prev
	if cur := filepath.Join(port, "MiyooPod"); fileExists(cur) {
		copyFileMode(cur, cur+".prev", 0755)
	}
	// replace the files, each with an atomic rename
	for _, rel := range files {
		src := filepath.Join(stage, "new", rel)
		dst := filepath.Join(ports, rel)
		st, err := os.Stat(src)
		if err != nil {
			return err
		}
		if err := os.MkdirAll(filepath.Dir(dst), 0755); err != nil {
			return err
		}
		tmp := dst + ".kpnew"
		if err := copyFileMode(src, tmp, st.Mode().Perm()); err != nil {
			os.Remove(tmp)
			return err
		}
		if err := os.Rename(tmp, dst); err != nil {
			os.Remove(tmp)
			return err
		}
	}
	syscall.Sync()
	return nil
}

// unpackUpdate extracts KnulliPod.sh and knullipod/… (never knullipod/conf)
// into dir and returns their relative paths.
func unpackUpdate(zipPath, dir string) ([]string, error) {
	zr, err := zip.OpenReader(zipPath)
	if err != nil {
		return nil, errors.New("the update is not a valid zip")
	}
	defer zr.Close()
	var files []string
	for _, f := range zr.File {
		name := filepath.Clean(filepath.FromSlash(f.Name))
		if f.FileInfo().IsDir() {
			continue
		}
		if strings.HasPrefix(name, "..") || filepath.IsAbs(name) {
			return nil, errors.New("bad path in the update")
		}
		if name != "KnulliPod.sh" && !strings.HasPrefix(name, "knullipod"+string(filepath.Separator)) {
			continue
		}
		if strings.HasPrefix(name, filepath.Join("knullipod", "conf")+string(filepath.Separator)) {
			continue
		}
		mode := f.Mode().Perm()
		if mode == 0 {
			mode = 0644
		}
		if name == "KnulliPod.sh" || name == filepath.Join("knullipod", "MiyooPod") {
			mode = 0755
		}
		dst := filepath.Join(dir, name)
		if err := os.MkdirAll(filepath.Dir(dst), 0755); err != nil {
			return nil, err
		}
		rc, err := f.Open()
		if err != nil {
			return nil, err
		}
		w, err := os.OpenFile(dst, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, mode)
		if err == nil {
			_, err = io.Copy(w, io.LimitReader(rc, 256<<20))
			if cerr := w.Close(); err == nil {
				err = cerr
			}
		}
		rc.Close()
		if err != nil {
			return nil, err
		}
		files = append(files, name)
	}
	if len(files) == 0 {
		return nil, errors.New("the update is empty")
	}
	return files, nil
}

// checkBinary: the new MiyooPod is an executable for this machine.
func checkBinary(path string) error {
	nf, err := elf.Open(path)
	if err != nil {
		return errors.New("the update has no valid MiyooPod")
	}
	defer nf.Close()
	if exe, err := startExe(); err == nil {
		if cf, err := elf.Open(exe); err == nil {
			defer cf.Close()
			if cf.Machine != nf.Machine {
				return fmt.Errorf("the update is for another CPU (%v)", nf.Machine)
			}
		}
	}
	return nil
}

func fileExists(p string) bool {
	_, err := os.Stat(p)
	return err == nil
}

func copyFileMode(src, dst string, mode os.FileMode) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.OpenFile(dst, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, mode)
	if err != nil {
		return err
	}
	if _, err := io.Copy(out, in); err != nil {
		out.Close()
		return err
	}
	if err := out.Sync(); err != nil {
		out.Close()
		return err
	}
	if err := out.Close(); err != nil {
		return err
	}
	return os.Chmod(dst, mode)
}

// restartIfUpdated runs the new binary in place of this process, with the
// same arguments and environment (called by main after the clean shutdown).
func restartIfUpdated() {
	if !restartAfterExit {
		return
	}
	exe, err := startExe()
	if err != nil {
		return
	}
	logMsg("Restarting " + exe)
	if err := syscall.Exec(exe, os.Args, os.Environ()); err != nil {
		logMsg("Restart failed: " + err.Error())
	}
}

// the binary's path, resolved at start-up: once the update has renamed the
// new binary over it, /proc/self/exe names a deleted file
var exePath, exeErr = func() (string, error) {
	exe, err := os.Executable()
	if err != nil {
		return "", err
	}
	if r, err := filepath.EvalSymlinks(exe); err == nil {
		exe = r
	}
	return exe, nil
}()

func startExe() (string, error) { return exePath, exeErr }
