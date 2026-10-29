package main

// ij_share.go — Share: send an album or a song to another betterPod, over
// the internet or the same Wi-Fi, with an 8-digit code.
//
//	sender    (an album's or a song's options) Share → a code, and the
//	          cover on a pedestal with a laser sweeping it while it goes
//	receiver  Music → Receive → the 8 digits (↑ ↓ a digit, ← → move, A)
//	          → the cover is built on the other pedestal, line by line,
//	          then the songs; they land in <music>/Shared/
//
// It is magic-wormhole (wormhole-william, MIT): the two consoles meet on its
// public server through the code, agree on a key with SPAKE2 (so the code
// can be short: a wrong guess just ends the share), and the files go
// straight from one to the other when the network allows it, else through
// its public relay. Either way they're encrypted end to end. The first 3
// digits are the "nameplate", the other 5 the password; betterPod uses its
// own app id, so its codes never meet those of other wormhole programs.
//
// What is sent is one directory (zipped by the library): first a small
// betterpod-share.json (what, from whom), then cover.jpg, then the songs,
// so the receiver can show the album and its cover while the rest comes.

import (
	"archive/zip"
	"bufio"
	"bytes"
	"compress/flate"
	"context"
	"crypto/rand"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/jpeg"
	"io"
	"math/big"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/psanford/wormhole-william/wormhole"
)

const (
	shareAppID    = "betterpod.xyz/share"
	shareManifest = "betterpod-share.json"
	shareCoverPx  = 150 // device px, on the pedestal
	shareBigPx    = 200 // device px, once received
)

// shareClient: the wormhole client (the servers can be changed for tests
// with BETTERPOD_WORMHOLE_URL and BETTERPOD_WORMHOLE_RELAY).
func shareClient() *wormhole.Client {
	c := &wormhole.Client{AppID: shareAppID}
	if u := os.Getenv("BETTERPOD_WORMHOLE_URL"); u != "" {
		c.RendezvousURL = u
	}
	if r := os.Getenv("BETTERPOD_WORMHOLE_RELAY"); r != "" {
		c.TransitRelayAddress = r
	}
	return c
}

type shareInfo struct {
	V      int    `json:"v"`
	From   string `json:"from"`
	Album  string `json:"album"`
	Artist string `json:"artist"`
	Title  string `json:"title,omitempty"` // one song: its title
	Songs  int    `json:"songs"`
	Bytes  int64  `json:"bytes"`
}

// share phases
const (
	shPreparing  = iota // sender: zipping, meeting the server
	shWaiting           // sender: the code is up, nobody yet
	shCode              // receiver: typing the code
	shConnecting        // receiver: looking for the sender
	shMoving            // the bytes are going
	shSaving            // receiver: unpacking into the music folder
	shDone
	shFailed
)

type ShareView struct {
	send  bool
	phase int
	info  shareInfo
	code  [8]int
	cur   int // receiver: the digit being set

	mu        sync.Mutex
	done      int64 // bytes moved
	total     int64
	rate      float64 // bytes / s, smoothed
	rateAt    time.Time
	rateBytes int64

	cover     *image.RGBA // shareCoverPx square
	coverBig  *image.RGBA // shareBigPx square (the received screen)
	btn       int         // received: 0 Listen Now, 1 Receive Another
	coverSent *image.RGBA // the "already sent" look of it
	coverAt   time.Time
	err       string
	egg       string // receiver: an easter egg's line
	eggAt     time.Time
	have      int // albums of the same artist already here
	dest      string
	tracks    []*Track // received, once in the library
	playable  bool
	phaseAt   time.Time
	count     int // shares done here, this one included
	cancel    context.CancelFunc
	bg        *image.RGBA
}

func (s *ShareView) setPhase(p int) {
	s.phase, s.phaseAt = p, time.Now()
}

// shareCode: 8 random digits (not one of the easter eggs).
func shareCode() [8]int {
	for {
		var c [8]int
		for i := range c {
			n, err := rand.Int(rand.Reader, big.NewInt(10))
			if err != nil {
				c[i] = int(time.Now().UnixNano()/int64(i+7)) % 10
			} else {
				c[i] = int(n.Int64())
			}
		}
		if s := codeString(c); s != "00000000" && s != "31415926" {
			return c
		}
	}
}

func codeString(c [8]int) string {
	var b strings.Builder
	for _, d := range c {
		b.WriteByte(byte('0' + d))
	}
	return b.String()
}

// wormholeCode: "NNN-PPPPP" (nameplate, password).
func wormholeCode(c [8]int) string {
	s := codeString(c)
	return s[:3] + "-" + s[3:]
}

func shareErrText(err error) string {
	if err == nil {
		return ""
	}
	m := err.Error()
	switch {
	case errors.Is(err, context.DeadlineExceeded) || strings.Contains(m, "no one"):
		return "No one is sharing with this code · check the digits"
	case strings.Contains(m, "decrypt") || strings.Contains(m, "verifier") || strings.Contains(m, "pake") || strings.Contains(m, "scary"):
		return "Wrong code, or the share has ended"
	case strings.Contains(m, "crowded"):
		return "This code is in use · try again"
	case strings.Contains(m, "dial") || strings.Contains(m, "lookup") || strings.Contains(m, "network") || strings.Contains(m, "websocket"):
		return "Couldn't reach the share service · check the Wi-Fi"
	case strings.Contains(m, "space"):
		return m
	}
	if len(m) > 60 {
		m = m[:60] + "…"
	}
	return "It didn't work · " + m
}

// shareName: this betterPod's name for Share (Settings → Sharing).
var shareName string

func shareNameLabel() string {
	if strings.TrimSpace(shareName) == "" {
		return "Not set"
	}
	return shareName
}

// sharingSettingsView: Settings → Sharing: the name friends see.
func (ui *IpodUI) sharingSettingsView() *View {
	v := &View{ID: "sharing", Title: "Sharing", Kind: KindSplit}
	edit := shareName
	v.Build = func() []Item {
		name := shareNameLabel()
		if ui.keyboardEditing(&edit) {
			name = ui.withCaret(&edit)
		}
		return []Item{
			{Label: "Device Name", Value: name, Sublabel: "Your friends see it when you share with them", NoArrow: true, OnSelect: func() {
				edit = shareName
				ui.showKeyboard(&edit, func() { ui.refreshView(v) }, func() {
					n := strings.TrimSpace(edit)
					if len([]rune(n)) > 32 {
						n = string([]rune(n)[:32])
					}
					shareName = n
					ui.savePrefs()
					ui.refreshBuilt()
				})
				ui.refreshView(v)
			}},
			{Label: "How it works", Sublabel: "[Y] on an album or a song → Share; Music → Receive", NoArrow: true},
		}
	}
	v.Items = v.Build()
	return v
}

func shareFrom() string {
	if n := strings.TrimSpace(shareName); n != "" {
		return n
	}
	h, _ := os.Hostname()
	h = strings.TrimSpace(h)
	if h == "" || strings.EqualFold(h, "localhost") || strings.EqualFold(h, "knulli") {
		return ""
	}
	return h
}

// ---- sending ----

// shareTracks opens Share for an album's or a song's tracks.
func (ui *IpodUI) shareTracks(tracks []*Track) {
	if len(tracks) == 0 {
		return
	}
	s := &ShareView{send: true, code: shareCode()}
	s.setPhase(shPreparing)
	t0 := tracks[0]
	al := ui.trackAlbum(t0)
	s.info = shareInfo{V: 1, From: shareFrom(), Songs: len(tracks)}
	if al != nil {
		s.info.Album, s.info.Artist = al.Name, al.Artist
	}
	if s.info.Album == "" {
		s.info.Album = t0.Album
	}
	if s.info.Artist == "" {
		s.info.Artist = t0.Artist
	}
	if len(tracks) == 1 {
		s.info.Title = t0.Title
	}
	for _, t := range tracks {
		if st, err := os.Stat(t.Path); err == nil {
			s.info.Bytes += st.Size()
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.cancel = cancel
	v := &View{ID: "share", Title: "Share", Kind: KindFull, Custom: s}
	v.onClose = cancel
	ui.showView(v, "")

	// the cover (decoded here, not on the UI goroutine)
	var src image.Image
	if al != nil {
		src = decodeAlbumSource(al)
	}
	go func() {
		var coverJPG []byte
		if src != nil {
			big := scaleImage(src, 500, 500)
			var b bytes.Buffer
			if jpeg.Encode(&b, big, &jpeg.Options{Quality: 85}) == nil {
				coverJPG = b.Bytes()
			}
			c := scaleImage(src, shareCoverPx, shareCoverPx)
			sent := shareSentLook(c)
			ui.post(func() { s.cover, s.coverSent, s.coverAt = c, sent, time.Now(); ui.dirty = true })
		}
		ui.shareSend(ctx, s, tracks, coverJPG)
	}()
}

func (ui *IpodUI) shareSend(ctx context.Context, s *ShareView, tracks []*Track, coverJPG []byte) {
	dir := safeName(s.info.Album)
	if s.info.Title != "" {
		dir = safeName(s.info.Artist + " - " + s.info.Title)
	}
	man, _ := json.Marshal(s.info)
	entries := []wormhole.DirectoryEntry{{Path: dir + "/" + shareManifest, Mode: 0o644,
		Reader: func() (io.ReadCloser, error) { return io.NopCloser(bytes.NewReader(man)), nil }}}
	if coverJPG != nil {
		entries = append(entries, wormhole.DirectoryEntry{Path: dir + "/cover.jpg", Mode: 0o644,
			Reader: func() (io.ReadCloser, error) { return io.NopCloser(bytes.NewReader(coverJPG)), nil }})
	}
	used := map[string]bool{}
	for _, t := range tracks {
		p := t.Path
		name := filepath.Base(p)
		for i := 2; used[strings.ToLower(name)]; i++ {
			ext := filepath.Ext(p)
			name = fmt.Sprintf("%s (%d)%s", strings.TrimSuffix(filepath.Base(p), ext), i, ext)
		}
		used[strings.ToLower(name)] = true
		entries = append(entries, wormhole.DirectoryEntry{Path: dir + "/" + name, Mode: 0o644,
			Reader: func() (io.ReadCloser, error) { return os.Open(p) }})
	}
	// the zip is made on the card, not in memory (/tmp)
	tmp := dataPath("tmp")
	os.MkdirAll(tmp, 0o755)
	os.Setenv("TMPDIR", tmp)
	progress := wormhole.WithProgress(func(sent, total int64) {
		s.mu.Lock()
		s.done, s.total = sent, total
		s.mu.Unlock()
		ui.post(func() {
			if s.phase == shWaiting {
				s.setPhase(shMoving)
			}
			ui.dirty = true
		})
	})
	_, res, err := shareClient().SendDirectory(ctx, dir, entries, wormhole.WithCode(wormholeCode(s.code)), progress)
	if err != nil {
		ui.postSure(func() { s.fail(ctx, err) })
		return
	}
	ui.post(func() {
		if s.phase == shPreparing {
			s.setPhase(shWaiting)
		}
	})
	select {
	case r := <-res:
		ui.postSure(func() {
			if r.OK {
				s.setPhase(shDone)
				s.count = shareCountUp()
			} else {
				s.fail(ctx, r.Error)
			}
			ui.dirty = true
		})
	case <-ctx.Done():
	}
}

func (s *ShareView) fail(ctx context.Context, err error) {
	if ctx.Err() != nil && !errors.Is(err, context.DeadlineExceeded) {
		return // closed by hand
	}
	s.err = shareErrText(err)
	s.setPhase(shFailed)
}

// shareSentLook: the cover once the laser has passed: grey, a little
// lighter, tinted blue, with scan lines.
func shareSentLook(c *image.RGBA) *image.RGBA {
	b := c.Bounds()
	out := image.NewRGBA(b)
	for y := b.Min.Y; y < b.Max.Y; y++ {
		line := y%3 == 0
		for x := b.Min.X; x < b.Max.X; x++ {
			i := c.PixOffset(x, y)
			r, g, bl := float64(c.Pix[i]), float64(c.Pix[i+1]), float64(c.Pix[i+2])
			l := 0.3*r + 0.59*g + 0.11*bl
			l = 128 + (l-128)*0.5 // less contrast
			l = l*1.2 + 10
			R, G, B := l*0.78+80*0.22, l*0.82+160*0.18, l*0.72+255*0.28
			if line {
				R, G, B = R+(255-R)*0.24, G+(255-G)*0.24, B+(255-B)*0.24
			}
			out.Pix[i], out.Pix[i+1], out.Pix[i+2], out.Pix[i+3] = clamp8(R), clamp8(G), clamp8(B), 255
		}
	}
	return out
}

func clamp8(v float64) uint8 {
	switch {
	case v < 0:
		return 0
	case v > 255:
		return 255
	}
	return uint8(v)
}

// shared count (the 10th share has a little surprise)
func shareCountUp() int {
	p := dataPath("share_count.txt")
	n := 0
	if b, err := os.ReadFile(p); err == nil {
		fmt.Sscan(string(b), &n)
	}
	n++
	writeFileAtomic(p, []byte(fmt.Sprint(n)))
	return n
}

// ---- receiving ----

func (ui *IpodUI) openReceive() {
	s := &ShareView{}
	s.setPhase(shCode)
	v := &View{ID: "receive", Title: "Receive", Kind: KindFull, Custom: s}
	v.onClose = func() {
		if s.cancel != nil {
			s.cancel()
		}
	}
	ui.showView(v, "")
}

func (ui *IpodUI) shareReceive(s *ShareView) {
	code := s.code
	switch codeString(code) {
	case "00000000":
		s.egg, s.eggAt = "Nothing comes from nowhere.", time.Now()
		return
	case "31415926":
		s.egg, s.eggAt = "3.14159265358979323846… and so on, forever.", time.Now()
		return
	}
	ctx, cancel := context.WithCancel(context.Background())
	s.cancel = cancel
	s.egg = ""
	s.setPhase(shConnecting)
	go func() {
		// nobody there: the server would wait forever
		met := make(chan struct{})
		go func() {
			select {
			case <-met:
			case <-time.After(30 * time.Second):
				cancel()
			case <-ctx.Done():
			}
		}()
		msg, err := shareClient().Receive(ctx, wormholeCode(code))
		close(met)
		if err != nil {
			if ctx.Err() != nil {
				err = errors.New("no one")
			}
			ui.postSure(func() {
				if s.phase == shConnecting {
					s.err, s.phase = shareErrText(err), shFailed
				}
			})
			return
		}
		if msg.Type != wormhole.TransferDirectory {
			msg.Reject()
			ui.postSure(func() { s.err = "That wasn't music"; s.setPhase(shFailed) })
			return
		}
		if free := freeBytes(musicRoot()); free > 0 && msg.UncompressedBytes64 > free-64<<20 {
			msg.Reject()
			ui.postSure(func() {
				s.err = fmt.Sprintf("Not enough space · %s needed", humanBytes(msg.UncompressedBytes64))
				s.setPhase(shFailed)
			})
			return
		}
		ui.shareReceiveData(ctx, s, msg)
	}()
}

// shareReceiveData writes the zip as it comes (reading the manifest and the
// cover from its start meanwhile), then unpacks it into <music>/Shared.
func (ui *IpodUI) shareReceiveData(ctx context.Context, s *ShareView, msg *wormhole.IncomingMessage) {
	tmp := dataPath("tmp")
	os.MkdirAll(tmp, 0o755)
	zpath := filepath.Join(tmp, "receive.zip")
	f, err := os.Create(zpath)
	if err != nil {
		ui.postSure(func() { s.err = shareErrText(err); s.setPhase(shFailed) })
		return
	}
	defer os.Remove(zpath)
	total := msg.TransferBytes64
	s.mu.Lock()
	s.total = total
	s.mu.Unlock()
	ui.post(func() { s.setPhase(shMoving); s.info.Album = msg.Name })
	grow := &growingFile{path: zpath}
	go ui.sharePeek(s, grow)
	buf := make([]byte, 256<<10)
	slow := os.Getenv("BETTERPOD_SHARE_SLOW") != "" // (for tests: to watch it)
	if slow {
		buf = buf[:16<<10]
	}
	var n int64
	for {
		k, rerr := msg.Read(buf)
		if k > 0 {
			if _, werr := f.Write(buf[:k]); werr != nil {
				rerr = werr
			}
			n += int64(k)
			grow.set(n, false)
			s.mu.Lock()
			s.done = n
			s.mu.Unlock()
			ui.post(func() { ui.dirty = true })
		}
		if rerr == io.EOF {
			break
		}
		if slow {
			time.Sleep(40 * time.Millisecond)
		}
		if rerr != nil {
			f.Close()
			grow.set(n, true)
			ui.postSure(func() { s.fail(ctx, rerr) })
			return
		}
	}
	f.Close()
	grow.set(n, true)
	ui.post(func() { s.setPhase(shSaving) })
	dest, err := shareUnpack(zpath, msg.Name)
	if err != nil {
		ui.postSure(func() { s.err = shareErrText(err); s.setPhase(shFailed) })
		return
	}
	// albums of this artist already here (counted before the rescan)
	have := 0
	if a := strings.ToLower(s.info.Artist); a != "" {
		for _, al := range ui.library().Albums {
			if strings.ToLower(al.Artist) == a {
				have++
			}
		}
	}
	logMsg("Share: received into " + dest)
	ui.postSure(func() {
		s.dest, s.have = dest, have
		s.setPhase(shDone)
		s.count = shareCountUp()
		ui.rescanLibrary()
		go ui.shareFindTracks(s)
	})
}

// shareFindTracks waits for the rescan, then finds the received songs.
func (ui *IpodUI) shareFindTracks(s *ShareView) {
	time.Sleep(300 * time.Millisecond)
	for i := 0; i < 600 && (ui.app.LibScanRunning || rescanPending.Load()); i++ {
		time.Sleep(200 * time.Millisecond)
	}
	ui.postSure(func() {
		var out []*Track
		pre := filepath.Clean(s.dest) + string(os.PathSeparator)
		for _, t := range ui.library().Tracks {
			if strings.HasPrefix(t.Path, pre) {
				out = append(out, t)
			}
		}
		sort.SliceStable(out, func(i, j int) bool {
			a, b := out[i], out[j]
			if a.DiscNum != b.DiscNum {
				return a.DiscNum < b.DiscNum
			}
			if a.TrackNum != b.TrackNum {
				return a.TrackNum < b.TrackNum
			}
			return a.Path < b.Path
		})
		s.tracks, s.playable = out, len(out) > 0
		logMsg(fmt.Sprintf("Share: %d received songs in the library", len(out)))
		ui.dirty = true
	})
}

// sharePeek reads the zip's first entries while it is still arriving: the
// manifest (what it is) and the cover.
func (ui *IpodUI) sharePeek(s *ShareView, g *growingFile) {
	// (a ByteReader, so flate reads no further than its own stream)
	r := bufio.NewReader(&growingReader{g: g})
	for i := 0; i < 2; i++ {
		var h [30]byte
		if _, err := io.ReadFull(r, h[:]); err != nil || binary.LittleEndian.Uint32(h[:4]) != 0x04034b50 {
			return
		}
		method := binary.LittleEndian.Uint16(h[8:])
		csize := binary.LittleEndian.Uint32(h[18:])
		nlen := binary.LittleEndian.Uint16(h[26:])
		xlen := binary.LittleEndian.Uint16(h[28:])
		name := make([]byte, nlen)
		if _, err := io.ReadFull(r, name); err != nil {
			return
		}
		if _, err := io.CopyN(io.Discard, r, int64(xlen)); err != nil {
			return
		}
		var data []byte
		switch method {
		case 8: // deflate: it ends by itself
			fr := flate.NewReader(r)
			d, err := io.ReadAll(io.LimitReader(fr, 8<<20))
			if err != nil {
				return
			}
			data = d
		case 0:
			d := make([]byte, csize)
			if _, err := io.ReadFull(r, d); err != nil {
				return
			}
			data = d
		default:
			return
		}
		// the data descriptor (when the sizes weren't in the header)
		if binary.LittleEndian.Uint16(h[6:])&8 != 0 {
			var dd [16]byte
			if _, err := io.ReadFull(r, dd[:4]); err != nil {
				return
			}
			n := 12
			if binary.LittleEndian.Uint32(dd[:4]) != 0x08074b50 {
				n = 8
			}
			if _, err := io.ReadFull(r, dd[4:4+n]); err != nil {
				return
			}
		}
		switch strings.ToLower(filepath.Base(string(name))) {
		case shareManifest:
			var info shareInfo
			if json.Unmarshal(data, &info) == nil {
				ui.post(func() {
					s.info = info
					ui.dirty = true
				})
			}
		case "cover.jpg":
			if img, _, err := image.Decode(bytes.NewReader(data)); err == nil {
				c := scaleImage(img, shareCoverPx, shareCoverPx)
				big := scaleImage(img, shareBigPx, shareBigPx)
				ui.post(func() { s.cover, s.coverBig, s.coverAt = c, big, time.Now(); ui.dirty = true })
			}
			return
		}
	}
}

// growingFile: a file being written, read while it grows.
type growingFile struct {
	path string
	mu   sync.Mutex
	size int64
	end  bool
}

func (g *growingFile) set(n int64, end bool) {
	g.mu.Lock()
	g.size, g.end = n, end
	g.mu.Unlock()
}

type growingReader struct {
	g   *growingFile
	f   *os.File
	pos int64
}

func (r *growingReader) Read(p []byte) (int, error) {
	for {
		r.g.mu.Lock()
		size, end := r.g.size, r.g.end
		r.g.mu.Unlock()
		if r.pos < size {
			if r.f == nil {
				f, err := os.Open(r.g.path)
				if err != nil {
					return 0, err
				}
				r.f = f
			}
			if int64(len(p)) > size-r.pos {
				p = p[:size-r.pos]
			}
			n, err := r.f.ReadAt(p, r.pos)
			r.pos += int64(n)
			if n > 0 {
				return n, nil
			}
			if err != nil && err != io.EOF {
				return 0, err
			}
		} else if end {
			if r.f != nil {
				r.f.Close()
			}
			return 0, io.EOF
		}
		time.Sleep(30 * time.Millisecond)
	}
}

// shareUnpack: the zip's music and images into <music>/Shared/<name>.
func shareUnpack(zpath, name string) (string, error) {
	zr, err := zip.OpenReader(zpath)
	if err != nil {
		return "", err
	}
	defer zr.Close()
	base := filepath.Join(musicRoot(), "Shared")
	dest := filepath.Join(base, safeName(name))
	for i := 2; ; i++ {
		if _, err := os.Stat(dest); os.IsNotExist(err) {
			break
		}
		dest = filepath.Join(base, fmt.Sprintf("%s (%d)", safeName(name), i))
	}
	if err := os.MkdirAll(dest, 0o755); err != nil {
		return "", err
	}
	for _, f := range zr.File {
		// only plain files, only music and pictures, never outside dest
		if f.FileInfo().IsDir() {
			continue
		}
		rel := filepath.Clean(filepath.FromSlash(f.Name))
		if rel == "." || filepath.IsAbs(rel) || strings.HasPrefix(rel, "..") || strings.Contains(rel, string(os.PathSeparator)+"..") {
			continue
		}
		if strings.EqualFold(filepath.Base(rel), shareManifest) || !shareKeep(rel) {
			continue
		}
		out := filepath.Join(dest, rel)
		os.MkdirAll(filepath.Dir(out), 0o755)
		rc, err := f.Open()
		if err != nil {
			return dest, err
		}
		w, err := os.Create(out)
		if err != nil {
			rc.Close()
			return dest, err
		}
		_, err = io.Copy(w, rc)
		rc.Close()
		if cerr := w.Close(); err == nil {
			err = cerr
		}
		if err != nil {
			return dest, err
		}
	}
	return dest, nil
}

func shareKeep(name string) bool {
	switch strings.ToLower(filepath.Ext(name)) {
	case ".mp3", ".flac", ".ogg", ".oga", ".opus", ".m4a", ".aac", ".wav", ".wma", ".ape", ".wv", ".mpc",
		".jpg", ".jpeg", ".png", ".webp", ".lrc", ".cue":
		return true
	}
	return false
}

// ---- keys ----

func (s *ShareView) OnKey(ui *IpodUI, k Key) bool {
	if !s.send && s.phase == shDone && (k == LEFT || k == RIGHT) {
		s.btn = 1 - s.btn
		ui.dirty = true
		return true
	}
	if !s.send && s.phase == shDone && s.playable && k == Y {
		// the album, where it is in the library
		if al := ui.trackAlbum(s.tracks[0]); al != nil {
			ui.showView(ui.albumView(al), al.Name)
		}
		return true
	}
	if s.send || s.phase != shCode {
		if !s.send && s.phase == shFailed && (k == UP || k == DOWN || k == LEFT || k == RIGHT) {
			s.setPhase(shCode) // back to the digits to fix them
			s.err = ""
			ui.dirty = true
			return true
		}
		return false
	}
	switch k {
	case UP:
		s.code[s.cur] = (s.code[s.cur] + 1) % 10
	case DOWN:
		s.code[s.cur] = (s.code[s.cur] + 9) % 10
	case LEFT:
		if s.cur > 0 {
			s.cur--
		}
	case RIGHT:
		if s.cur < 7 {
			s.cur++
		}
	case START:
		ui.shareReceive(s)
	default:
		return false
	}
	s.egg = ""
	ui.dirty = true
	return true
}

func (s *ShareView) OnCenter(ui *IpodUI) {
	switch {
	case !s.send && (s.phase == shCode || s.phase == shFailed):
		if s.phase == shFailed {
			s.err = ""
		}
		ui.shareReceive(s)
	case !s.send && s.phase == shDone && s.btn == 1:
		// another one: the digits again
		*s = ShareView{bg: s.bg}
		s.setPhase(shCode)
	case !s.send && s.phase == shDone && s.playable:
		ui.playSong(s.tracks, 0, true)
	}
	ui.dirty = true
}

func (s *ShareView) OnScroll(ui *IpodUI, dir int, velocity float64) {}

func (s *ShareView) Animating(now time.Time) bool {
	switch s.phase {
	case shCode:
		return s.egg != "" && now.Sub(s.eggAt) < 4*time.Second
	case shFailed:
		return now.Sub(s.phaseAt) < time.Second
	case shDone:
		return now.Sub(s.phaseAt) < 2500*time.Millisecond
	}
	return true
}
