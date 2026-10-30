// Package main is betterPod Share's engine on the Mac: it reads an album
// (its songs, tags and cover) and sends it to a betterPod with an 8-digit
// code, exactly as a betterPod's own Share does (src/ij_share.go), so every
// betterPod from 0.69 on receives it with Music → Receive.
//
// It is built as a C archive (go build -buildmode=c-archive) and called by
// the Swift app through the functions in export.go. Run as a program, it is
// a small command line sender, for tests (main.go).
//
// What goes, in this order (the receiver shows the album while the rest
// comes): betterpod-share.json (what, from whom), cover.jpg, then the songs,
// in one folder. The offer carries the songs' formats, so a betterPod that
// can't play them (a Miyoo and AAC) says no before anything moves.
//
// Keep in step with src/ij_share.go: shareAppID, shareInfo, shareOffer, the
// code's form and the rejection's wording.
package main

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/big"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/psanford/wormhole-william/wormhole"
)

const (
	shareAppID    = "betterpod.xyz/share"
	shareManifest = "betterpod-share.json"
)

type shareInfo struct {
	V      int    `json:"v"`
	From   string `json:"from"`
	Album  string `json:"album"`
	Artist string `json:"artist"`
	Title  string `json:"title,omitempty"` // one song: its title
	Songs  int    `json:"songs"`
	Bytes  int64  `json:"bytes"`
}

type shareOffer struct {
	Exts []string `json:"exts"`
}

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

// newCode: 8 random digits, not one of the receiver's easter eggs.
func newCode() string {
	for {
		var b strings.Builder
		for i := 0; i < 8; i++ {
			n, err := rand.Int(rand.Reader, big.NewInt(10))
			if err != nil {
				n = big.NewInt(time.Now().UnixNano() % 10)
			}
			b.WriteByte(byte('0' + n.Int64()))
		}
		if s := b.String(); s != "00000000" && s != "31415926" {
			return s
		}
	}
}

// Share phases, as the app shows them.
const (
	phPreparing = "preparing" // zipping, meeting the server
	phWaiting   = "waiting"   // the code is up, nobody yet
	phMoving    = "moving"    // the bytes are going
	phDone      = "done"
	phFailed    = "failed"
)

// Share: one album being sent.
type Share struct {
	Code string

	mu         sync.Mutex
	phase      string
	done       int64
	total      int64
	rate       float64 // bytes / s, smoothed
	rateAt     time.Time
	rateBytes  int64
	err        string
	unplayable []string
	unplayOn   string
	to         string // the receiver's name, once it accepts
	cancel     context.CancelFunc
}

// Status: what the app polls.
type Status struct {
	Phase      string   `json:"phase"`
	Code       string   `json:"code"`
	Done       int64    `json:"done"`
	Total      int64    `json:"total"`
	Rate       float64  `json:"rate"`
	Error      string   `json:"error,omitempty"`
	Unplayable []string `json:"unplayable,omitempty"`
	To         string   `json:"to,omitempty"` // the receiving betterPod's name (from 0.81.3)
	On         string   `json:"on,omitempty"`
}

func (s *Share) Status() Status {
	s.mu.Lock()
	defer s.mu.Unlock()
	return Status{Phase: s.phase, Code: s.Code, Done: s.done, Total: s.total, Rate: s.rate,
		Error: s.err, Unplayable: s.unplayable, On: s.unplayOn, To: s.to}
}

func (s *Share) set(phase string) {
	s.mu.Lock()
	s.phase = phase
	s.mu.Unlock()
}

func (s *Share) Cancel() {
	if s.cancel != nil {
		s.cancel()
	}
}

// StartShare sends sel, from the name from (shown on the betterPod).
func StartShare(sel *Selection, from string) *Share {
	ctx, cancel := context.WithCancel(context.Background())
	s := &Share{Code: newCode(), phase: phPreparing, cancel: cancel}
	if c := os.Getenv("BETTERPOD_SHARE_CODE"); len(c) == 8 { // (tests)
		s.Code = c
	}
	go s.run(ctx, sel, from)
	return s
}

func (s *Share) run(ctx context.Context, sel *Selection, from string) {
	from = strings.TrimSpace(from)
	info := shareInfo{V: 1, From: from, Album: sel.Title(), Artist: sel.Artist(), Songs: sel.Songs, Bytes: sel.Bytes}
	dir := safeName(sel.Title())
	switch sel.Kind {
	case "song":
		info.Album, info.Title = sel.Groups[0].Album, sel.Groups[0].Songs[0].Title
		dir = safeName(info.Artist + " - " + info.Title)
	case "mix":
		dir = "Mix"
		if from != "" {
			dir = safeName("From " + from)
		}
	}
	file := func(path string, data []byte, disk string) wormhole.DirectoryEntry {
		return wormhole.DirectoryEntry{Path: path, Mode: 0o644, Reader: func() (io.ReadCloser, error) {
			if data != nil {
				return io.NopCloser(bytes.NewReader(data)), nil
			}
			return os.Open(disk)
		}}
	}
	// the manifest and the share's cover first (the betterPod shows them
	// while the rest comes), then the songs; in a mix each album's own
	// cover follows its songs (the betterPod reads only the first cover.jpg
	// as the share's)
	man, _ := json.Marshal(info)
	entries := []wormhole.DirectoryEntry{file(dir+"/"+shareManifest, man, "")}
	if sel.CoverJPG != nil {
		entries = append(entries, file(dir+"/cover.jpg", sel.CoverJPG, ""))
	}
	var offer shareOffer
	subUsed := map[string]bool{}
	for _, g := range sel.Groups {
		sub := dir
		if sel.Kind == "mix" {
			name := safeName(g.Album)
			if subUsed[strings.ToLower(name)] && g.Artist != "" {
				name = safeName(g.Artist + " - " + g.Album)
			}
			for i := 2; subUsed[strings.ToLower(name)]; i++ {
				name = safeName(fmt.Sprintf("%s (%d)", g.Album, i))
			}
			subUsed[strings.ToLower(name)] = true
			sub = dir + "/" + name
		}
		used := map[string]bool{}
		for _, t := range g.Songs {
			if e := strings.ToLower(filepath.Ext(t.Path)); !slices.Contains(offer.Exts, e) {
				offer.Exts = append(offer.Exts, e)
			}
			name := filepath.Base(t.Path)
			for i := 2; used[strings.ToLower(name)]; i++ {
				ext := filepath.Ext(t.Path)
				name = fmt.Sprintf("%s (%d)%s", strings.TrimSuffix(filepath.Base(t.Path), ext), i, ext)
			}
			used[strings.ToLower(name)] = true
			entries = append(entries, file(sub+"/"+name, nil, t.Path))
		}
		if sel.Kind == "mix" && g.CoverJPG != nil {
			entries = append(entries, file(sub+"/cover.jpg", g.CoverJPG, ""))
		}
	}
	progress := wormhole.WithProgress(func(sent, total int64) {
		s.mu.Lock()
		defer s.mu.Unlock()
		if s.phase == phWaiting || s.phase == phPreparing {
			s.phase = phMoving
			s.rateAt, s.rateBytes = time.Now(), sent
		}
		s.done, s.total = sent, total
		if dt := time.Since(s.rateAt).Seconds(); dt >= 0.5 {
			r := float64(sent-s.rateBytes) / dt
			if s.rate == 0 {
				s.rate = r
			} else {
				s.rate = s.rate*0.6 + r*0.4
			}
			s.rateAt, s.rateBytes = time.Now(), sent
		}
	})
	offerJSON, _ := json.Marshal(offer)
	_, res, err := shareClient().SendDirectory(ctx, dir, entries, wormhole.WithCode(s.Code[:3]+"-"+s.Code[3:]),
		progress, wormhole.WithOfferInfo(offerJSON), wormhole.WithAnswerInfo(func(b []byte) {
			// the receiver says its name when it accepts (betterPod 0.81.3 on)
			var a struct {
				Name string `json:"name"`
			}
			if json.Unmarshal(b, &a) == nil {
				n := strings.TrimSpace(a.Name)
				if len([]rune(n)) > 32 {
					n = string([]rune(n)[:32])
				}
				s.mu.Lock()
				s.to = n
				s.mu.Unlock()
			}
		}))
	if err != nil {
		s.fail(ctx, err)
		return
	}
	s.mu.Lock()
	if s.phase == phPreparing {
		s.phase = phWaiting
	}
	s.mu.Unlock()
	select {
	case r := <-res:
		if r.OK {
			s.mu.Lock()
			s.done = s.total
			s.mu.Unlock()
			s.set(phDone)
		} else {
			s.fail(ctx, r.Error)
		}
	case <-ctx.Done():
	}
}

func (s *Share) fail(ctx context.Context, err error) {
	if ctx.Err() != nil {
		return // cancelled by hand
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if names, on, ok := parseUnplayable(err); ok {
		s.unplayable, s.unplayOn = names, on
		s.err = "The betterPod on " + on + " can't play " + strings.Join(names, ", ")
	} else {
		s.err = errText(err)
	}
	s.phase = phFailed
}

// parseUnplayable: a betterPod's "Can't play AAC / ALAC (.m4a) on Onion OS".
func parseUnplayable(err error) (names []string, on string, ok bool) {
	if err == nil {
		return nil, "", false
	}
	m := strings.TrimPrefix(err.Error(), "TransferError: ")
	if !strings.HasPrefix(m, "Can't play ") {
		return nil, "", false
	}
	m = strings.TrimPrefix(m, "Can't play ")
	i := strings.LastIndex(m, " on ")
	if i <= 0 {
		return nil, "", false
	}
	return strings.Split(m[:i], ", "), m[i+len(" on "):], true
}

func errText(err error) string {
	if err == nil {
		return ""
	}
	m := strings.TrimPrefix(err.Error(), "TransferError: ")
	switch {
	case strings.Contains(m, "decrypt") || strings.Contains(m, "verifier") || strings.Contains(m, "pake") || strings.Contains(m, "scary"):
		return "The code was typed wrong on the betterPod. Share again for a new code."
	case strings.Contains(m, "crowded"):
		return "This code is in use. Share again for a new one."
	case strings.Contains(m, "rejected") || strings.Contains(m, "transfer rejected"):
		return "The betterPod said no (not enough space, or it was cancelled)."
	case errors.Is(err, context.DeadlineExceeded) || strings.Contains(m, "dial") || strings.Contains(m, "lookup") ||
		strings.Contains(m, "network") || strings.Contains(m, "websocket"):
		return "Couldn't reach the share service. Check the internet connection."
	}
	if len(m) > 90 {
		m = m[:90] + "…"
	}
	return "It didn't work: " + m
}

// safeName: as src/ij_archive.go's (the folder name on the betterPod).
func safeName(s string) string {
	s = strings.Map(func(r rune) rune {
		if strings.ContainsRune(`/\:*?"<>|`, r) || r < 32 {
			return '_'
		}
		return r
	}, strings.TrimSpace(s))
	s = strings.Trim(s, ". ")
	if len([]rune(s)) > 80 {
		s = string([]rune(s)[:80])
	}
	for len(s) > 200 {
		r := []rune(s)
		s = string(r[:len(r)-1])
	}
	if s == "" {
		s = "Unknown"
	}
	return s
}
