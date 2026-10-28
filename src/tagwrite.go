package main

// tagwrite.go — writes a song's tags into its file: ID3v2 for MP3, Vorbis
// comments for FLAC and Ogg Vorbis (the formats the library reads tags
// from with dhowden/tag). Used by Podcasts (ij_podcasts.go) for the
// episodes it downloads, and by Album Info (ij_albuminfo.go).
//
// Only the fields of songTags are written; every other tag and the cover
// art are kept as they are. When the new tags fit in the room the file
// already has (ID3 and FLAC padding) they are written in place, so the
// audio is not copied; otherwise the file is written again beside the old
// one (name.tagtmp) and renamed over it, so a failure never leaves half a
// file. WAV, AIFF and anything else return errNoTags.

import (
	"bufio"
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"unicode/utf16"
)

// songTags: what Album Info writes. Empty strings and zero numbers remove
// the field.
type songTags struct {
	Title, Artist, Album, AlbumArtist, Genre string
	Track, TrackTotal, Disc, Year            int
}

var errNoTags = errors.New("this kind of file has no tags KnulliPod can write")

// canWriteTags: the formats writeTags handles.
func canWriteTags(path string) bool {
	switch strings.ToLower(filepath.Ext(path)) {
	case ".mp3", ".flac", ".ogg", ".oga":
		return true
	}
	return false
}

// writeTags writes t into the file at path.
func writeTags(path string, t songTags) error {
	switch strings.ToLower(filepath.Ext(path)) {
	case ".mp3":
		return writeID3(path, t)
	case ".flac":
		return writeFLACTags(path, t)
	case ".ogg", ".oga":
		return writeOggTags(path, t)
	}
	return errNoTags
}

func (t songTags) trackText() string {
	if t.Track <= 0 {
		return ""
	}
	if t.TrackTotal > 0 {
		return fmt.Sprintf("%d/%d", t.Track, t.TrackTotal)
	}
	return strconv.Itoa(t.Track)
}

func itoaPos(n int) string {
	if n <= 0 {
		return ""
	}
	return strconv.Itoa(n)
}

// replaceFile writes a new version of path: head, then the old file from
// offset from on, into path.tagtmp, renamed over path at the end.
func replaceFile(path string, head []byte, from int64) error {
	in, err := os.Open(path)
	if err != nil {
		return err
	}
	defer in.Close()
	if _, err := in.Seek(from, io.SeekStart); err != nil {
		return err
	}
	tmp := path + ".tagtmp"
	out, err := os.Create(tmp)
	if err != nil {
		return err
	}
	_, err = out.Write(head)
	if err == nil {
		_, err = io.Copy(out, in)
	}
	if err == nil {
		err = out.Sync()
	}
	if cerr := out.Close(); err == nil {
		err = cerr
	}
	if err != nil {
		os.Remove(tmp)
		return err
	}
	in.Close()
	return os.Rename(tmp, path)
}

// writeAt overwrites the start of path with b (the new tags fit in the old
// room).
func writeAt(path string, b []byte, off int64) error {
	f, err := os.OpenFile(path, os.O_WRONLY, 0)
	if err != nil {
		return err
	}
	_, err = f.WriteAt(b, off)
	if err == nil {
		err = f.Sync()
	}
	if cerr := f.Close(); err == nil {
		err = cerr
	}
	return err
}

// ---- ID3v2 (MP3) ----

// The frames songTags owns; the others are kept.
var id3Owned = map[string]bool{"TIT2": true, "TPE1": true, "TPE2": true, "TALB": true, "TCON": true,
	"TRCK": true, "TPOS": true, "TYER": true, "TDRC": true}

const id3Padding = 2048

func syncsafe(b []byte) int {
	return int(b[0]&0x7f)<<21 | int(b[1]&0x7f)<<14 | int(b[2]&0x7f)<<7 | int(b[3]&0x7f)
}

func putSyncsafe(b []byte, n int) {
	b[0], b[1], b[2], b[3] = byte(n>>21)&0x7f, byte(n>>14)&0x7f, byte(n>>7)&0x7f, byte(n)&0x7f
}

// deunsync undoes ID3 unsynchronisation (0xFF 0x00 → 0xFF).
func deunsync(b []byte) []byte {
	out := make([]byte, 0, len(b))
	for i := 0; i < len(b); i++ {
		out = append(out, b[i])
		if b[i] == 0xff && i+1 < len(b) && b[i+1] == 0 {
			i++
		}
	}
	return out
}

type id3Frame struct {
	id    string
	flags [2]byte
	data  []byte
}

// readID3 returns the tag's version (3 or 4; 0 without a tag), its frames,
// and where the audio starts.
func readID3(f *os.File) (int, []id3Frame, int64, error) {
	var h [10]byte
	if _, err := f.ReadAt(h[:], 0); err != nil {
		if errors.Is(err, io.EOF) {
			return 0, nil, 0, nil
		}
		return 0, nil, 0, err
	}
	if string(h[:3]) != "ID3" {
		return 0, nil, 0, nil
	}
	ver, flags, size := int(h[3]), h[5], syncsafe(h[6:10])
	end := int64(10 + size)
	if ver == 4 && flags&0x10 != 0 {
		end += 10 // footer
	}
	if ver != 3 && ver != 4 {
		return ver, nil, end, fmt.Errorf("ID3v2.%d tags can't be edited", ver)
	}
	body := make([]byte, size)
	if _, err := f.ReadAt(body, 10); err != nil {
		return ver, nil, end, err
	}
	if ver == 3 && flags&0x80 != 0 {
		body = deunsync(body)
	}
	pos := 0
	if flags&0x40 != 0 && len(body) >= 4 { // extended header: dropped
		if ver == 3 {
			pos = 4 + int(binary.BigEndian.Uint32(body))
		} else {
			pos = syncsafe(body)
		}
	}
	var frames []id3Frame
	for pos+10 <= len(body) && body[pos] != 0 {
		id := string(body[pos : pos+4])
		n := int(binary.BigEndian.Uint32(body[pos+4:]))
		if ver == 4 {
			n = syncsafe(body[pos+4:])
		}
		fl := [2]byte{body[pos+8], body[pos+9]}
		if n < 0 || pos+10+n > len(body) {
			break // a damaged frame: the rest is dropped
		}
		data := append([]byte(nil), body[pos+10:pos+10+n]...)
		if ver == 4 && fl[1]&0x02 != 0 {
			data, fl[1] = deunsync(data), fl[1]&^0x02
		}
		frames = append(frames, id3Frame{id, fl, data})
		pos += 10 + n
	}
	return ver, frames, end, nil
}

// id3Text: a text frame's body, UTF-8 in v2.4; Latin-1 or UTF-16 in v2.3.
func id3Text(ver int, s string) []byte {
	if ver == 4 {
		return append([]byte{3}, s...)
	}
	latin := true
	for _, r := range s {
		if r > 0xff {
			latin = false
			break
		}
	}
	if latin {
		b := []byte{0}
		for _, r := range s {
			b = append(b, byte(r))
		}
		return b
	}
	b := []byte{1, 0xff, 0xfe}
	for _, u := range utf16.Encode([]rune(s)) {
		b = append(b, byte(u), byte(u>>8))
	}
	return b
}

func writeID3(path string, t songTags) error {
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	ver, frames, audio, err := readID3(f)
	f.Close()
	if err != nil {
		return err
	}
	if ver == 0 {
		ver = 4
	}
	year := "TDRC"
	if ver == 3 {
		year = "TYER"
	}
	var body bytes.Buffer
	frame := func(id string, fl [2]byte, data []byte) {
		var h [10]byte
		copy(h[:4], id)
		if ver == 4 {
			putSyncsafe(h[4:8], len(data))
		} else {
			binary.BigEndian.PutUint32(h[4:8], uint32(len(data)))
		}
		h[8], h[9] = fl[0], fl[1]
		body.Write(h[:])
		body.Write(data)
	}
	for _, kv := range [][2]string{{"TIT2", t.Title}, {"TPE1", t.Artist}, {"TPE2", t.AlbumArtist}, {"TALB", t.Album},
		{"TCON", t.Genre}, {"TRCK", t.trackText()}, {"TPOS", itoaPos(t.Disc)}, {year, itoaPos(t.Year)}} {
		if kv[1] != "" {
			frame(kv[0], [2]byte{}, id3Text(ver, kv[1]))
		}
	}
	for _, fr := range frames {
		if !id3Owned[fr.id] {
			frame(fr.id, fr.flags, fr.data)
		}
	}
	room := int(audio) - 10 // the old tag, its padding included
	pad := id3Padding
	inPlace := audio > 0 && body.Len() <= room
	if inPlace {
		pad = room - body.Len()
	}
	head := make([]byte, 10, 10+body.Len()+pad)
	copy(head, "ID3")
	head[3] = byte(ver)
	putSyncsafe(head[6:10], body.Len()+pad)
	head = append(head, body.Bytes()...)
	head = append(head, make([]byte, pad)...)
	if inPlace {
		return writeAt(path, head, 0)
	}
	return replaceFile(path, head, audio)
}

// ---- Vorbis comments (FLAC, Ogg Vorbis) ----

// The comment fields songTags owns (upper case); the others are kept.
var vorbisOwned = map[string]bool{"TITLE": true, "ARTIST": true, "ALBUM": true, "ALBUMARTIST": true,
	"ALBUM ARTIST": true, "TRACKNUMBER": true, "TRACKTOTAL": true, "TOTALTRACKS": true, "DISCNUMBER": true,
	"DATE": true, "YEAR": true, "GENRE": true}

type vorbisComment struct {
	vendor string
	fields []string // "KEY=value"
}

func parseVorbisComment(b []byte) (vorbisComment, error) {
	var vc vorbisComment
	bad := errors.New("damaged comment block")
	if len(b) < 8 {
		return vc, bad
	}
	n := int(binary.LittleEndian.Uint32(b))
	if 4+n+4 > len(b) {
		return vc, bad
	}
	vc.vendor = string(b[4 : 4+n])
	pos := 4 + n
	count := int(binary.LittleEndian.Uint32(b[pos:]))
	pos += 4
	for i := 0; i < count; i++ {
		if pos+4 > len(b) {
			return vc, bad
		}
		l := int(binary.LittleEndian.Uint32(b[pos:]))
		if l < 0 || pos+4+l > len(b) {
			return vc, bad
		}
		vc.fields = append(vc.fields, string(b[pos+4:pos+4+l]))
		pos += 4 + l
	}
	return vc, nil
}

// set replaces the owned fields with t's.
func (vc *vorbisComment) set(t songTags) {
	keep := vc.fields[:0]
	for _, f := range vc.fields {
		k, _, _ := strings.Cut(f, "=")
		if !vorbisOwned[strings.ToUpper(k)] {
			keep = append(keep, f)
		}
	}
	vc.fields = keep
	for _, kv := range [][2]string{{"TITLE", t.Title}, {"ARTIST", t.Artist}, {"ALBUM", t.Album},
		{"ALBUMARTIST", t.AlbumArtist}, {"TRACKNUMBER", itoaPos(t.Track)}, {"TRACKTOTAL", itoaPos(t.TrackTotal)},
		{"DISCNUMBER", itoaPos(t.Disc)}, {"DATE", itoaPos(t.Year)}, {"GENRE", t.Genre}} {
		if kv[1] != "" {
			vc.fields = append(vc.fields, kv[0]+"="+kv[1])
		}
	}
	if vc.vendor == "" {
		vc.vendor = "KnulliPod"
	}
}

func (vc vorbisComment) bytes() []byte {
	var b bytes.Buffer
	var n [4]byte
	put := func(s string) {
		binary.LittleEndian.PutUint32(n[:], uint32(len(s)))
		b.Write(n[:])
		b.WriteString(s)
	}
	put(vc.vendor)
	binary.LittleEndian.PutUint32(n[:], uint32(len(vc.fields)))
	b.Write(n[:])
	for _, f := range vc.fields {
		put(f)
	}
	return b.Bytes()
}

// ---- FLAC ----

const flacPadding = 8192

type flacBlock struct {
	typ  byte
	data []byte
}

func writeFLACTags(path string, t songTags) error {
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer f.Close()
	// a FLAC can start with an ID3v2 tag: kept as it is
	start := int64(0)
	var h [10]byte
	if _, err := f.ReadAt(h[:], 0); err != nil {
		return err
	}
	if string(h[:3]) == "ID3" {
		start = int64(10 + syncsafe(h[6:10]))
	}
	var magic [4]byte
	if _, err := f.ReadAt(magic[:], start); err != nil || string(magic[:]) != "fLaC" {
		return errors.New("not a FLAC file")
	}
	var blocks []flacBlock
	pos := start + 4
	for {
		var bh [4]byte
		if _, err := f.ReadAt(bh[:], pos); err != nil {
			return err
		}
		n := int(bh[1])<<16 | int(bh[2])<<8 | int(bh[3])
		data := make([]byte, n)
		if _, err := f.ReadAt(data, pos+4); err != nil {
			return err
		}
		blocks = append(blocks, flacBlock{bh[0] & 0x7f, data})
		pos += 4 + int64(n)
		if bh[0]&0x80 != 0 {
			break
		}
		if len(blocks) > 1000 {
			return errors.New("damaged FLAC metadata")
		}
	}
	audio := pos
	if len(blocks) == 0 || blocks[0].typ != 0 {
		return errors.New("FLAC without STREAMINFO")
	}
	// the comments: changed, or added after STREAMINFO
	var vc vorbisComment
	found := false
	var out []flacBlock
	for _, b := range blocks {
		switch b.typ {
		case 1: // padding: made again below
			continue
		case 4:
			if found {
				continue // a second comment block: dropped
			}
			if vc, err = parseVorbisComment(b.data); err != nil {
				return err
			}
			found = true
		}
		out = append(out, b)
	}
	vc.set(t)
	if !found {
		out = append(out[:1], append([]flacBlock{{typ: 4}}, out[1:]...)...)
	}
	for i := range out {
		if out[i].typ == 4 {
			out[i].data = vc.bytes()
		}
	}
	size := 0
	for _, b := range out {
		size += 4 + len(b.data)
	}
	room := int(audio - start - 4)
	pad := flacPadding
	inPlace := size+4 <= room
	if inPlace {
		pad = room - size - 4
	}
	out = append(out, flacBlock{typ: 1, data: make([]byte, pad)})
	var meta bytes.Buffer
	meta.WriteString("fLaC")
	for i, b := range out {
		hb := b.typ
		if i == len(out)-1 {
			hb |= 0x80
		}
		n := len(b.data)
		meta.Write([]byte{hb, byte(n >> 16), byte(n >> 8), byte(n)})
		meta.Write(b.data)
	}
	f.Close()
	if inPlace {
		return writeAt(path, meta.Bytes(), start)
	}
	head := meta.Bytes()
	if start > 0 {
		pre := make([]byte, start)
		g, err := os.Open(path)
		if err != nil {
			return err
		}
		_, err = g.ReadAt(pre, 0)
		g.Close()
		if err != nil {
			return err
		}
		head = append(pre, head...)
	}
	return replaceFile(path, head, audio)
}

// ---- Ogg Vorbis ----

var oggCRCTable = func() [256]uint32 {
	var t [256]uint32
	for i := range t {
		r := uint32(i) << 24
		for j := 0; j < 8; j++ {
			if r&0x80000000 != 0 {
				r = r<<1 ^ 0x04c11db7
			} else {
				r <<= 1
			}
		}
		t[i] = r
	}
	return t
}()

func oggCRC(b []byte) uint32 {
	var c uint32
	for _, x := range b {
		c = c<<8 ^ oggCRCTable[byte(c>>24)^x]
	}
	return c
}

type oggPage struct {
	flags   byte
	granule uint64
	serial  uint32
	seq     uint32
	lacing  []byte
	body    []byte
}

func readOggPage(r io.Reader) (*oggPage, error) {
	var h [27]byte
	if _, err := io.ReadFull(r, h[:]); err != nil {
		return nil, err
	}
	if string(h[:4]) != "OggS" {
		return nil, errors.New("not an Ogg page")
	}
	p := &oggPage{flags: h[5], granule: binary.LittleEndian.Uint64(h[6:]),
		serial: binary.LittleEndian.Uint32(h[14:]), seq: binary.LittleEndian.Uint32(h[18:])}
	p.lacing = make([]byte, h[26])
	if _, err := io.ReadFull(r, p.lacing); err != nil {
		return nil, err
	}
	n := 0
	for _, l := range p.lacing {
		n += int(l)
	}
	p.body = make([]byte, n)
	if _, err := io.ReadFull(r, p.body); err != nil {
		return nil, err
	}
	return p, nil
}

func (p *oggPage) bytes() []byte {
	b := make([]byte, 27, 27+len(p.lacing)+len(p.body))
	copy(b, "OggS")
	b[5] = p.flags
	binary.LittleEndian.PutUint64(b[6:], p.granule)
	binary.LittleEndian.PutUint32(b[14:], p.serial)
	binary.LittleEndian.PutUint32(b[18:], p.seq)
	b[26] = byte(len(p.lacing))
	b = append(b, p.lacing...)
	b = append(b, p.body...)
	binary.LittleEndian.PutUint32(b[22:], oggCRC(b))
	return b
}

// oggPaginate lays packets out on pages from sequence seq (header pages:
// granule 0 where a packet ends, else -1).
func oggPaginate(packets [][]byte, serial, seq uint32) []*oggPage {
	var pages []*oggPage
	p := &oggPage{serial: serial, seq: seq}
	cont := false
	for _, pk := range packets {
		for rest := len(pk); ; rest -= 255 {
			if len(p.lacing) == 255 {
				p.granule = ^uint64(0)
				if pagePacketEnds(p) {
					p.granule = 0
				}
				pages = append(pages, p)
				seq++
				p = &oggPage{serial: serial, seq: seq}
				if cont {
					p.flags = 1
				}
			}
			l := min(rest, 255)
			p.lacing = append(p.lacing, byte(l))
			off := len(pk) - rest
			p.body = append(p.body, pk[off:off+l]...)
			cont = l == 255
			if l < 255 {
				break
			}
		}
	}
	if len(p.lacing) > 0 {
		pages = append(pages, p)
	}
	return pages
}

func pagePacketEnds(p *oggPage) bool {
	for _, l := range p.lacing {
		if l < 255 {
			return true
		}
	}
	return false
}

func writeOggTags(path string, t songTags) error {
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer f.Close()
	r := &countReader{r: f}
	first, err := readOggPage(r)
	if err != nil {
		return err
	}
	if first.flags&2 == 0 || len(first.body) < 7 || string(first.body[:7]) != "\x01vorbis" {
		return errors.New("not an Ogg Vorbis file")
	}
	// the comment and setup packets, on the pages after the first
	var packets [][]byte
	var cur []byte
	used := 0
	for len(packets) < 2 {
		p, err := readOggPage(r)
		if err != nil {
			return err
		}
		if p.serial != first.serial {
			return errors.New("Ogg files with several streams can't be edited")
		}
		used++
		off := 0
		for i, l := range p.lacing {
			cur = append(cur, p.body[off:off+int(l)]...)
			off += int(l)
			if l < 255 {
				packets = append(packets, cur)
				cur = nil
				if len(packets) == 2 && i != len(p.lacing)-1 {
					return errors.New("unusual Ogg layout")
				}
			}
		}
	}
	audio := r.n
	if len(packets[0]) < 7 || string(packets[0][:7]) != "\x03vorbis" {
		return errors.New("Ogg Vorbis without comments")
	}
	body := packets[0][7:]
	vc, err := parseVorbisComment(body)
	if err != nil {
		return err
	}
	vc.set(t)
	comment := append([]byte("\x03vorbis"), vc.bytes()...)
	comment = append(comment, 1) // framing bit
	pages := oggPaginate([][]byte{comment, packets[1]}, first.serial, 1)
	shift := uint32(len(pages) - used)

	tmp := path + ".tagtmp"
	out, err := os.Create(tmp)
	if err != nil {
		return err
	}
	fail := func(err error) error {
		out.Close()
		os.Remove(tmp)
		return err
	}
	if _, err := out.Write(first.bytes()); err != nil {
		return fail(err)
	}
	for _, p := range pages {
		if _, err := out.Write(p.bytes()); err != nil {
			return fail(err)
		}
	}
	if _, err := f.Seek(audio, io.SeekStart); err != nil {
		return fail(err)
	}
	if shift == 0 {
		_, err = io.Copy(out, f) // same page numbers: the audio as it is
	} else {
		br := bufio.NewReaderSize(f, 64<<10)
		for {
			p, perr := readOggPage(br)
			if perr == io.EOF {
				break
			}
			if perr != nil {
				err = perr
				break
			}
			p.seq += shift
			if _, err = out.Write(p.bytes()); err != nil {
				break
			}
		}
	}
	if err == nil {
		err = out.Sync()
	}
	if err != nil {
		return fail(err)
	}
	if err := out.Close(); err != nil {
		os.Remove(tmp)
		return err
	}
	f.Close()
	return os.Rename(tmp, path)
}

type countReader struct {
	r io.Reader
	n int64
}

func (c *countReader) Read(b []byte) (int, error) {
	n, err := c.r.Read(b)
	c.n += int64(n)
	return n, err
}
