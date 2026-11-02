package main

// ij_ytstream.go — Listen to a YouTube video in parts, instead of fetching
// all of its audio first (a 2-hour mix is 130 MB and minutes of waiting).
//
// YouTube's audio (format 140) is a fragmented MP4: ftyp and moov (the
// track, no samples), a sidx that lists every fragment's size and length,
// then the fragments (moof + mdat). The mixer can't play that, nor a file
// still growing, so the audio is fetched in parts with HTTP ranges: the
// first one short (it plays after a few seconds), the next ones a few
// minutes each, one ahead of the one playing. Each part is ftyp + moov +
// its fragments, written as a plain M4A as a download's tracks are
// (ij_m4a.go), and played after the one before; a part played is deleted.
//
// yt-dlp gives the address of the audio (-j: the chosen format's url and
// headers). When anything doesn't look like that (no sidx, fragments
// placed by absolute offsets), Listen fetches the whole audio as before.

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"time"
)

// ytPart: one part of a streamed video: its bytes in the file (whole
// fragments) and where it starts, in seconds.
type ytPart struct {
	off, size  int64
	start, dur float64
}

// ytStream: a video's audio, ready to be fetched in parts.
type ytStream struct {
	url     string
	headers map[string]string
	init    []byte // ftyp + moov
	parts   []ytPart
	dur     float64
}

var errNoStream = errors.New("this audio can't be streamed")

// ytStreamFirst / ytStreamNext: a part's length, in seconds (the first
// short, so it plays soon).
const (
	ytStreamFirst = 20.0
	ytStreamNext  = 180.0
)

// ytOpenStream asks yt-dlp for the audio's address (from the video's own
// info when it was read already, else from its link), then reads the
// beginning of the file and plans the parts.
func ytOpenStream(ctx context.Context, link string, infoJSON []byte) (*ytStream, error) {
	args := []string{"-j", "-f", "140/bestaudio[ext=m4a]", "--no-playlist"}
	if infoJSON != nil {
		work := filepath.Join(listenDir(), "work")
		os.MkdirAll(work, 0o755)
		p := filepath.Join(work, "stream-info.json")
		if err := os.WriteFile(p, infoJSON, 0o644); err != nil {
			return nil, err
		}
		args = append(args, "--load-info-json", p)
	} else {
		args = append(args, link)
	}
	out, err := ytCommand(ctx, args...).Output()
	if err != nil {
		return nil, fmt.Errorf("yt-dlp: %v", err)
	}
	var j struct {
		URL         string            `json:"url"`
		Headers     map[string]string `json:"http_headers"`
		Protocol    string            `json:"protocol"`
		AvailableAt float64           `json:"available_at"`
	}
	if err := json.Unmarshal(lastJSONLine(out), &j); err != nil || j.URL == "" {
		return nil, errNoStream
	}
	if j.Protocol != "" && j.Protocol != "https" && j.Protocol != "http" {
		return nil, errNoStream // (an HLS or DASH manifest, not a file)
	}
	if err := ytWaitLinks(ctx, j.AvailableAt); err != nil { // (403 before)
		return nil, err
	}
	return ytStreamFrom(ctx, j.URL, j.Headers)
}

// lastJSONLine: the last line of yt-dlp's output that is a JSON object.
func lastJSONLine(out []byte) []byte {
	lines := bytes.Split(bytes.TrimSpace(out), []byte("\n"))
	for i := len(lines) - 1; i >= 0; i-- {
		if l := bytes.TrimSpace(lines[i]); len(l) > 0 && l[0] == '{' {
			return l
		}
	}
	return nil
}

// ytStreamFrom reads the start of the audio file at url (ftyp, moov, sidx
// and the first fragment's header) and plans the parts.
func ytStreamFrom(ctx context.Context, url string, headers map[string]string) (*ytStream, error) {
	s := &ytStream{url: url, headers: headers}
	head, err := s.get(ctx, 0, 256<<10, nil)
	if err != nil {
		return nil, err
	}
	boxes, _ := mp4Boxes(bytesAt{head, 0}, 0, int64(len(head)))
	var moov, sidx mp4Box
	var haveMoov, haveSidx bool
	for _, b := range boxes {
		switch b.typ {
		case "moov":
			moov, haveMoov = b, true
		case "sidx":
			sidx, haveSidx = b, true
		}
		if haveSidx {
			break
		}
	}
	if !haveMoov {
		return nil, errNoStream
	}
	if !haveSidx {
		// a long sidx (a long video) may go past what was read: its header
		// says how long it is
		end := moov.end
		if end+8 > int64(len(head)) || string(head[end+4:end+8]) != "sidx" {
			return nil, errNoStream
		}
		n := int64(binary.BigEndian.Uint32(head[end:]))
		if n < 32 || n > 4<<20 {
			return nil, errNoStream
		}
		more, err := s.get(ctx, end, n, nil)
		if err != nil {
			return nil, err
		}
		head = append(head[:end], more...)
		sidx, haveSidx = mp4Box{"sidx", end, end + n, end + 8}, true
	}
	s.init = append([]byte(nil), head[:moov.end]...)
	if err := s.plan(head[sidx.body:sidx.end], sidx.end); err != nil {
		return nil, err
	}
	// the fragments must be placed from their moof (as YouTube's are): a
	// fragment that names absolute offsets can't be moved into a part
	first := s.parts[0].off
	if first+64 <= int64(len(head)) {
		if fb, _ := mp4Boxes(bytesAt{head, 0}, first, int64(len(head))); len(fb) > 0 && fb[0].typ == "moof" {
			if i := bytes.Index(head[first:min(int64(len(head)), fb[0].end)], []byte("tfhd")); i >= 0 {
				p := first + int64(i) + 4
				if p+4 <= int64(len(head)) && binary.BigEndian.Uint32(head[p:])&0x1 != 0 {
					return nil, errNoStream
				}
			}
		}
	}
	return s, nil
}

// plan reads the sidx's fragments and groups them into parts.
func (s *ytStream) plan(b []byte, sidxEnd int64) error {
	if len(b) < 24 {
		return errNoStream
	}
	ver := b[0]
	scale := float64(binary.BigEndian.Uint32(b[8:]))
	i := 12
	var first int64
	if ver == 0 {
		first = int64(binary.BigEndian.Uint32(b[i+4:]))
		i += 8
	} else {
		if len(b) < 32 {
			return errNoStream
		}
		first = int64(binary.BigEndian.Uint64(b[i+8:]))
		i += 16
	}
	n := int(binary.BigEndian.Uint16(b[i+2:]))
	i += 4
	if scale <= 0 || n == 0 || i+n*12 > len(b) {
		return errNoStream
	}
	off := sidxEnd + first
	var cur ytPart
	cur.off = off
	t := 0.0
	target := ytStreamFirst
	for k := 0; k < n; k++ {
		ref := binary.BigEndian.Uint32(b[i:])
		if ref>>31 != 0 {
			return errNoStream // (a sidx of sidxes)
		}
		size := int64(ref & 0x7fffffff)
		d := float64(binary.BigEndian.Uint32(b[i+4:])) / scale
		i += 12
		cur.size += size
		cur.dur += d
		off += size
		t += d
		if cur.dur >= target || k == n-1 {
			s.parts = append(s.parts, cur)
			cur = ytPart{off: off, start: t}
			target = ytStreamNext
		}
	}
	s.dur = t
	return nil
}

// get reads n bytes of the audio from off (a range), counting them.
func (s *ytStream) get(ctx context.Context, off, n int64, progress func(got, total int64)) ([]byte, error) {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	// a connection that stops sending is dropped
	stall := time.AfterFunc(iaStall, cancel)
	defer stall.Stop()
	req, err := http.NewRequestWithContext(ctx, "GET", s.url, nil)
	if err != nil {
		return nil, err
	}
	for k, v := range s.headers {
		req.Header.Set(k, v)
	}
	req.Header.Set("Range", "bytes="+strconv.FormatInt(off, 10)+"-"+strconv.FormatInt(off+n-1, 10))
	resp, err := updateClient(0).Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode != 206 && !(resp.StatusCode == 200 && off == 0) {
		return nil, fmt.Errorf("YouTube answered %d", resp.StatusCode)
	}
	buf := bytes.NewBuffer(make([]byte, 0, n))
	chunk := make([]byte, 64<<10)
	for int64(buf.Len()) < n {
		k, rerr := resp.Body.Read(chunk)
		if k > 0 {
			stall.Reset(iaStall)
			buf.Write(chunk[:min(k, int(n)-buf.Len())])
			if progress != nil {
				progress(int64(buf.Len()), n)
			}
		}
		if rerr == io.EOF {
			break
		}
		if rerr != nil {
			return nil, rerr
		}
	}
	if off > 0 && int64(buf.Len()) < n {
		return nil, io.ErrUnexpectedEOF
	}
	return buf.Bytes(), nil
}

// fetchPart writes part k as a plain M4A at dst.
func (s *ytStream) fetchPart(ctx context.Context, k int, dst, title string, progress func(got, total int64)) error {
	p := s.parts[k]
	body, err := s.get(ctx, p.off, p.size, progress)
	if err != nil {
		return err
	}
	piece := dst + ".frag"
	defer os.Remove(piece)
	if err := os.WriteFile(piece, append(append([]byte(nil), s.init...), body...), 0o644); err != nil {
		return err
	}
	m, err := openM4A(piece)
	if err != nil {
		return err
	}
	defer m.Close()
	return m.WriteRange(dst, 0, 0, m4aTags{Title: title})
}
