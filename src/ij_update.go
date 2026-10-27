package main

// ij_update.go — Settings → Software Update: KnulliPod updates itself over
// Wi-Fi from its GitHub repository.
//
// The repository carries a small manifest, release/latest.json:
//
//	{"version": "0.22", "path": "release/knullipod-v0.22-knulli.zip",
//	 "url": "https://raw.githubusercontent.com/…/knullipod-v0.22-knulli.zip",
//	 "sha256": "…", "size": 6012345, "notes": "…"}
//
// Both are read through GitHub's API (current right after a push), with
// raw.githubusercontent.com as the fallback.
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
	"image"
	"io"
	"math"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"golang.org/x/image/font"
)

// KNULLIPOD_VERSION is the version shown in About and compared with the
// manifest; knulliPodBuildDate is the earliest date certificates are
// checked at.
const (
	KNULLIPOD_VERSION  = "0.32.2"
	knulliPodBuildDate = "2026-10-27"
	updateManifestURL  = "https://raw.githubusercontent.com/stefanobuttarelli/knullipod/main/release/latest.json"
	// GitHub's API serves the current files at once; raw.githubusercontent
	// keeps "main" pointing at the previous commit for up to 5 minutes
	// after a push (it is the fallback, e.g. over the API's rate limit)
	updateAPIBase     = "https://api.github.com/repos/stefanobuttarelli/knullipod/contents/"
	updateManifestRel = "release/latest.json"
)

type updateManifest struct {
	Version string `json:"version"`
	URL     string `json:"url"`
	Path    string `json:"path,omitempty"` // the zip in the repository, read through the API
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
	return &View{ID: "update", Title: "Software Update", Kind: KindFull, Custom: &UpdateView{}}
}

// UpdateView is the Software Update screen: white, with the KnulliPod name
// and the state at the top, the download bar (blue, glossy, with its
// reflection, like Now Playing's) in the middle, the release notes when a
// new version is found, and the buttons at the bottom (the wheel moves
// between them, the centre button presses).
type UpdateView struct {
	sel   int
	shown float64 // the bar's fill as drawn, easing towards the download
	last  time.Time
}

type updButton struct {
	label string
	do    func(ui *IpodUI)
}

func (u *UpdateView) buttons() []updButton {
	check := updButton{"Check for Updates", func(ui *IpodUI) { ui.checkForUpdate() }}
	switch upd.state {
	case updIdle:
		return []updButton{check}
	case updUpToDate:
		return []updButton{{"Check Again", check.do}}
	case updFailed:
		return []updButton{{"Try Again", check.do}}
	case updAvailable:
		return []updButton{
			{"Download and Install", func(ui *IpodUI) { ui.installUpdate() }},
			{"Not Now", func(ui *IpodUI) { ui.hideView() }},
		}
	}
	return nil
}

func (u *UpdateView) Animating(now time.Time) bool {
	return math.Abs(u.target()-u.shown) > 0.002
}

func (u *UpdateView) target() float64 {
	switch upd.state {
	case updDownloading:
		return upd.progress
	case updInstalling, updDone:
		return 1
	}
	return 0
}

func (u *UpdateView) OnScroll(ui *IpodUI, dir int, velocity float64) {
	n := len(u.buttons())
	if n == 0 {
		return
	}
	u.sel = (u.sel + dir + n) % n
	if u.sel < 0 {
		u.sel = 0
	}
	ui.dirty = true
}

func (u *UpdateView) OnCenter(ui *IpodUI) {
	b := u.buttons()
	if len(b) == 0 {
		return
	}
	if u.sel >= len(b) {
		u.sel = 0
	}
	f := b[u.sel].do
	u.sel = 0
	f(ui)
	ui.dirty = true
}

func (u *UpdateView) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	W := dst.Bounds().Dx()
	fillRect(dst, 0, 0, W, dst.Bounds().Dy(), th.bg)
	wCSS := float64(W) / DPR
	center := func(f font.Face, s string, y float64, c Col) {
		s = ellipsize(f, s, (wCSS-40)*DPR)
		drawText(dst, f, s, (float64(W)-textWidth(f, s))/2, y*DPR, c)
	}
	// the bar eases towards the download progress
	dt := now.Sub(u.last).Seconds()
	if u.last.IsZero() || dt > 0.5 {
		dt = 0.5
	}
	u.last = now
	t := u.target()
	if t < u.shown {
		u.shown = t
	} else {
		u.shown += (t - u.shown) * math.Min(1, dt*10)
	}

	// name and state
	logo := ui.asset("ipod_logo")
	blit(dst, logo, (W-logo.Bounds().Dx())/2, px(14), 1)
	_, asc16 := lineBox(16)
	_, asc13 := lineBox(13)
	center(face(W700, 16), "KnulliPod", 14+32+8+asc16, th.text)
	status, sub := "Version "+KNULLIPOD_VERSION, ""
	m := upd.manifest
	switch upd.state {
	case updIdle:
		sub = "Connect to Wi-Fi to check for a new version"
	case updChecking:
		status = "Checking for updates…"
	case updUpToDate:
		sub = "KnulliPod is up to date"
		if !upd.checked.IsZero() {
			sub += " · checked " + checkedWhen(upd.checked, now)
		}
	case updAvailable:
		status = fmt.Sprintf("Version %s is available · %.1f MB", m.Version, float64(m.Size)/1e6)
	case updDownloading:
		status = "Downloading version " + m.Version + "…"
		sub = fmt.Sprintf("%.1f of %.1f MB", upd.progress*float64(m.Size)/1e6, float64(m.Size)/1e6)
	case updInstalling:
		status = "Installing…"
		sub = "Don't turn off the console"
	case updDone:
		status = "Updated to version " + m.Version
		sub = "Restarting…"
	case updFailed:
		status = "Update failed"
		sub = upd.err
	}
	y := 14 + 32 + 8 + 20.0
	center(face(W600, 13), status, y+asc13, th.text)
	if sub != "" {
		center(face(W500, 12), sub, y+18+asc13, th.sub)
	}

	// middle: the release notes when a version is found, else the bar
	mid := 122.0
	if upd.state == updAvailable {
		// what's new: up to three lines between the state and the
		// buttons, aligned with the buttons
		notes := noteLines(m.Notes, 3)
		f := face(W500, 12)
		lh, asc := lineBox(12)
		lh += 1
		ny := y + 22
		for _, l := range notes {
			s := ellipsize(f, "• "+l, (wCSS-84)*DPR)
			drawText(dst, f, s, 42*DPR, (ny+asc)*DPR, th.sub)
			ny += lh
		}
	} else {
		bw := math.Min(240, wCSS-80)
		ui.drawProgressBar(dst, (wCSS-bw)/2, mid-8, bw, u.shown*100, false)
		if upd.state == updDownloading || upd.state == updInstalling || upd.state == updDone {
			_, a := lineBox(12)
			center(face(W700, 12), fmt.Sprintf("%d%%", int(u.shown*100+0.5)), mid+22+a, th.sub)
		}
	}

	// buttons, at the bottom, in the action sheet's style
	b := u.buttons()
	if u.sel >= len(b) {
		u.sel = 0
	}
	f := face(W700, 14)
	bh := 28.0
	by := 220 - 8 - float64(len(b))*(bh+6) + 6
	bx0, bx1 := 40.0, wCSS-40
	for i, bt := range b {
		grad, border, tc := th.sheetOpt, th.sheetBorder, th.sheetText
		if i == u.sel {
			grad, border, tc = selGrad, hex("#3a78ad"), colWhite
		}
		fillRoundedGradient(dst, bx0*DPR, by*DPR, bx1*DPR, (by+bh)*DPR, 9*DPR, grad, 1)
		strokeRounded(dst, bx0*DPR, by*DPR, bx1*DPR, (by+bh)*DPR, 9*DPR, 2, border, 1)
		// centred on the button: the label's cap height sits in the middle
		tw := textWidth(f, bt.label)
		base := by + bh/2 + lineCapHeight(14)/2
		drawText(dst, f, bt.label, (float64(W)-tw)/2, base*DPR, tc)
		by += bh + 6
	}
}

// checkedWhen: "at 14:32" today, else "on 27 Oct".
func checkedWhen(t, now time.Time) string {
	if t.YearDay() == now.YearDay() && t.Year() == now.Year() {
		return "at " + t.Format("15:04")
	}
	return "on " + t.Format("2 Jan")
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

// ---- automatic check, once a day ----
//
// A quiet check runs 30 s after start-up and then whenever a day has passed
// since the last successful one (retried every hour while there is no
// connection). It only changes what the screens show: a new version adds
// "Update Available" to the main menu and fills Software Update; nothing is
// downloaded without asking. The time of the last check (and a version
// found then) is kept in the database folder, so a version found yesterday
// is looked for again right after start-up.

type updCheckFile struct {
	Checked   int64  `json:"checked"`
	Available string `json:"available,omitempty"`
}

func updCheckPath() string { return dataPath(".update_check.json") }

func readUpdCheck() updCheckFile {
	var f updCheckFile
	if b, err := os.ReadFile(updCheckPath()); err == nil {
		json.Unmarshal(b, &f)
	}
	return f
}

func writeUpdCheck(f updCheckFile) {
	if b, err := json.Marshal(f); err == nil {
		writeFileAtomic(updCheckPath(), b)
	}
}

var autoUpd struct {
	next    time.Time // when the next check is due (zero: not planned yet)
	running bool
}

// autoCheckTick is called by the UI loop; it is cheap until a check is due.
func (ui *IpodUI) autoCheckTick(now time.Time) {
	if autoUpd.running || os.Getenv("MIYOOPOD_AUTO_UPDATE_CHECK") == "0" {
		return
	}
	if autoUpd.next.IsZero() {
		// first plan: 30 s after start-up if a day has passed since the
		// last check or a version was found then, else a day after it
		f := readUpdCheck()
		last := time.Unix(f.Checked, 0)
		autoUpd.next = now.Add(30 * time.Second)
		if f.Checked > 0 && now.Sub(last) < 24*time.Hour && !(f.Available != "" && isNewerVersion(KNULLIPOD_VERSION, f.Available)) {
			autoUpd.next = last.Add(24 * time.Hour)
		}
		if f.Checked > 0 {
			upd.checked = last
		}
	}
	if now.Before(autoUpd.next) {
		return
	}
	switch upd.state {
	case updChecking, updDownloading, updInstalling, updDone:
		autoUpd.next = now.Add(time.Hour)
		return
	}
	autoUpd.running = true
	go func() {
		m, err := fetchManifest()
		ui.setUpdate(func() {
			autoUpd.running = false
			if err != nil {
				// no Wi-Fi yet: try again in an hour
				autoUpd.next = time.Now().Add(time.Hour)
				logMsg("Automatic update check failed: " + err.Error())
				return
			}
			autoUpd.next = time.Now().Add(24 * time.Hour)
			rec := updCheckFile{Checked: time.Now().Unix()}
			upd.checked = time.Now()
			if isNewerVersion(KNULLIPOD_VERSION, m.Version) {
				rec.Available = m.Version
				if upd.state != updDownloading && upd.state != updInstalling {
					upd.state, upd.manifest = updAvailable, m
				}
				logMsg("Automatic update check: v" + m.Version + " available")
			} else if upd.state == updIdle || upd.state == updFailed || upd.state == updAvailable {
				upd.state = updUpToDate
			}
			writeUpdCheck(rec)
		})
	}()
}

// updateAvailable: a newer version was found (the main menu shows it).
func updateAvailable() bool {
	return upd.manifest != nil && isNewerVersion(KNULLIPOD_VERSION, upd.manifest.Version) &&
		upd.state != updUpToDate && upd.state != updIdle
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
				writeUpdCheck(updCheckFile{Checked: time.Now().Unix(), Available: m.Version})
			default:
				upd.state = updUpToDate
				writeUpdCheck(updCheckFile{Checked: time.Now().Unix()})
			}
			if err == nil {
				autoUpd.next = time.Now().Add(24 * time.Hour)
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
	if u := os.Getenv("MIYOOPOD_UPDATE_URL"); u != "" {
		return fetchManifestFrom(u, false)
	}
	m, err := fetchManifestFrom(updateAPIBase+updateManifestRel, true)
	if err == nil {
		return m, nil
	}
	logMsg("Update check through the API failed (" + err.Error() + "), trying raw")
	return fetchManifestFrom(updateManifestURL, false)
}

// githubGet requests url; through the API it asks for the raw file.
func githubGet(url string, api bool, timeout time.Duration) (*http.Response, error) {
	req, err := http.NewRequest("GET", url, nil)
	if err != nil {
		return nil, err
	}
	req.Header.Set("Cache-Control", "no-cache")
	req.Header.Set("User-Agent", "KnulliPod/"+KNULLIPOD_VERSION)
	if api {
		req.Header.Set("Accept", "application/vnd.github.raw")
	}
	return updateClient(timeout).Do(req)
}

func fetchManifestFrom(url string, api bool) (*updateManifest, error) {
	resp, err := githubGet(url, api, 15*time.Second)
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

// openDownload starts the download of the zip: through the API when the
// manifest names its path, else (or if that fails) from its URL.
func openDownload(m *updateManifest) (*http.Response, error) {
	if m.Path != "" && os.Getenv("MIYOOPOD_UPDATE_URL") == "" {
		resp, err := githubGet(updateAPIBase+m.Path, true, 10*time.Minute)
		if err == nil && resp.StatusCode == 200 {
			return resp, nil
		}
		if err == nil {
			resp.Body.Close()
			err = fmt.Errorf("status %d", resp.StatusCode)
		}
		logMsg("Download through the API failed (" + err.Error() + "), trying raw")
	}
	resp, err := githubGet(m.URL, false, 10*time.Minute)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode != 200 {
		resp.Body.Close()
		return nil, fmt.Errorf("Download failed (%d)", resp.StatusCode)
	}
	return resp, nil
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
	resp, err := openDownload(m)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
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
