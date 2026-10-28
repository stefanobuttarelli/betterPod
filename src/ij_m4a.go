package main

// ij_m4a.go — cutting and tagging M4A (AAC) files without ffmpeg, for the
// YouTube albums: the audio of a video is split into tracks at its chapters
// and each track is written as a plain M4A with its tags (title, artist,
// album, track number, year) and the cover inside.
//
// The source can be a plain MP4 (samples listed in moov/stbl) or a
// fragmented one (moof/trun, as YouTube's DASH audio comes). Samples are
// copied as they are (no re-encoding): a cut falls on an AAC frame
// (~23 ms). Each output is ftyp + moov (one track, all samples in one
// chunk, tags in udta/meta/ilst) + mdat.

import (
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"os"
)

type m4aSample struct {
	off  int64
	size uint32
	dur  uint32
}

// m4aSource: the audio track of an M4A file.
type m4aSource struct {
	f         *os.File
	timescale uint32
	stsd      []byte // the sample description box, as it is
	samples   []m4aSample
}

type mp4Box struct {
	typ        string
	start, end int64 // the whole box
	body       int64 // where its content starts
}

// mp4Boxes lists the boxes in [off, end) of r.
func mp4Boxes(r io.ReaderAt, off, end int64) ([]mp4Box, error) {
	var out []mp4Box
	var h [16]byte
	for off+8 <= end {
		if _, err := r.ReadAt(h[:8], off); err != nil {
			return out, err
		}
		size := int64(binary.BigEndian.Uint32(h[:4]))
		typ := string(h[4:8])
		hdr := int64(8)
		if size == 1 {
			if _, err := r.ReadAt(h[8:16], off+8); err != nil {
				return out, err
			}
			size = int64(binary.BigEndian.Uint64(h[8:16]))
			hdr = 16
		} else if size == 0 {
			size = end - off
		}
		if size < hdr || off+size > end {
			return out, fmt.Errorf("bad box %q", typ)
		}
		out = append(out, mp4Box{typ, off, off + size, off + hdr})
		off += size
	}
	return out, nil
}

// bytesAt is an io.ReaderAt over a box read into memory, keeping file
// offsets (base = where the bytes start in the file).
type bytesAt struct {
	b    []byte
	base int64
}

func (r bytesAt) ReadAt(p []byte, off int64) (int, error) {
	i := off - r.base
	if i < 0 || i >= int64(len(r.b)) {
		return 0, io.EOF
	}
	n := copy(p, r.b[i:])
	if n < len(p) {
		return n, io.EOF
	}
	return n, nil
}

func (r bytesAt) slice(b mp4Box) []byte { return r.b[b.body-r.base : b.end-r.base] }

func findBox(bs []mp4Box, typ string) (mp4Box, bool) {
	for _, b := range bs {
		if b.typ == typ {
			return b, true
		}
	}
	return mp4Box{}, false
}

func be32(b []byte, i int) uint32 { return binary.BigEndian.Uint32(b[i:]) }

// openM4A reads the audio track's samples of an M4A file.
func openM4A(path string) (*m4aSource, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	st, err := f.Stat()
	if err != nil {
		f.Close()
		return nil, err
	}
	src := &m4aSource{f: f}
	if err := src.parse(st.Size()); err != nil {
		f.Close()
		return nil, err
	}
	if len(src.samples) == 0 || src.timescale == 0 || src.stsd == nil {
		f.Close()
		return nil, errors.New("no audio found in the file")
	}
	return src, nil
}

func (s *m4aSource) Close() error { return s.f.Close() }

func (s *m4aSource) parse(size int64) error {
	top, err := mp4Boxes(s.f, 0, size)
	if err != nil && len(top) == 0 {
		return err
	}
	var trexDur, trexSize uint32
	for _, b := range top {
		switch b.typ {
		case "moov":
			buf := make([]byte, b.end-b.start)
			if _, err := s.f.ReadAt(buf, b.start); err != nil {
				return err
			}
			r := bytesAt{buf, b.start}
			kids, _ := mp4Boxes(r, b.body, b.end)
			for _, k := range kids {
				switch k.typ {
				case "trak":
					if s.stsd == nil {
						s.parseTrak(r, k)
					}
				case "mvex":
					ks, _ := mp4Boxes(r, k.body, k.end)
					if t, ok := findBox(ks, "trex"); ok {
						d := r.slice(t)
						if len(d) >= 24 {
							trexDur, trexSize = be32(d, 12), be32(d, 16)
						}
					}
				}
			}
		case "moof":
			buf := make([]byte, b.end-b.start)
			if _, err := s.f.ReadAt(buf, b.start); err != nil {
				return err
			}
			s.parseMoof(bytesAt{buf, b.start}, b, trexDur, trexSize)
		}
	}
	return nil
}

// parseTrak: the sound track's timescale, sample description and (plain
// MP4) its samples.
func (s *m4aSource) parseTrak(r bytesAt, trak mp4Box) {
	kids, _ := mp4Boxes(r, trak.body, trak.end)
	mdia, ok := findBox(kids, "mdia")
	if !ok {
		return
	}
	mk, _ := mp4Boxes(r, mdia.body, mdia.end)
	if h, ok := findBox(mk, "hdlr"); !ok || len(r.slice(h)) < 12 || string(r.slice(h)[8:12]) != "soun" {
		return
	}
	if m, ok := findBox(mk, "mdhd"); ok {
		d := r.slice(m)
		if d[0] == 1 && len(d) >= 24 {
			s.timescale = be32(d, 20)
		} else if len(d) >= 16 {
			s.timescale = be32(d, 12)
		}
	}
	minf, ok := findBox(mk, "minf")
	if !ok {
		return
	}
	nk, _ := mp4Boxes(r, minf.body, minf.end)
	stbl, ok := findBox(nk, "stbl")
	if !ok {
		return
	}
	sk, _ := mp4Boxes(r, stbl.body, stbl.end)
	if b, ok := findBox(sk, "stsd"); ok {
		s.stsd = append([]byte(nil), r.b[b.start-r.base:b.end-r.base]...)
	}
	// plain MP4: sizes, durations and chunk offsets
	stsz, ok1 := findBox(sk, "stsz")
	stts, ok2 := findBox(sk, "stts")
	stsc, ok3 := findBox(sk, "stsc")
	stco, ok4 := findBox(sk, "stco")
	co64, ok5 := findBox(sk, "co64")
	if !ok1 || !ok2 || !ok3 || !(ok4 || ok5) {
		return
	}
	z := r.slice(stsz)
	n := int(be32(z, 8))
	if n == 0 {
		return
	}
	fixed := be32(z, 4)
	sizes := make([]uint32, n)
	for i := range sizes {
		if fixed != 0 {
			sizes[i] = fixed
		} else if 12+4*i+4 <= len(z) {
			sizes[i] = be32(z, 12+4*i)
		}
	}
	durs := make([]uint32, 0, n)
	t := r.slice(stts)
	for i, e := 0, int(be32(t, 4)); i < e && 8+8*i+8 <= len(t); i++ {
		cnt, d := be32(t, 8+8*i), be32(t, 12+8*i)
		for j := uint32(0); j < cnt && len(durs) < n; j++ {
			durs = append(durs, d)
		}
	}
	var chunks []int64
	if ok4 {
		c := r.slice(stco)
		for i, e := 0, int(be32(c, 4)); i < e && 8+4*i+4 <= len(c); i++ {
			chunks = append(chunks, int64(be32(c, 8+4*i)))
		}
	} else {
		c := r.slice(co64)
		for i, e := 0, int(be32(c, 4)); i < e && 8+8*i+8 <= len(c); i++ {
			chunks = append(chunks, int64(binary.BigEndian.Uint64(c[8+8*i:])))
		}
	}
	sc := r.slice(stsc)
	type run struct{ first, per uint32 }
	var runs []run
	for i, e := 0, int(be32(sc, 4)); i < e && 8+12*i+12 <= len(sc); i++ {
		runs = append(runs, run{be32(sc, 8+12*i), be32(sc, 12+12*i)})
	}
	si := 0
	for ci, off := range chunks {
		per := uint32(0)
		for _, rn := range runs {
			if uint32(ci+1) >= rn.first {
				per = rn.per
			}
		}
		for j := uint32(0); j < per && si < n; j++ {
			d := uint32(1024)
			if si < len(durs) {
				d = durs[si]
			}
			s.samples = append(s.samples, m4aSample{off, sizes[si], d})
			off += int64(sizes[si])
			si++
		}
	}
}

// parseMoof adds a fragment's samples (fragmented MP4).
func (s *m4aSource) parseMoof(r bytesAt, moof mp4Box, defDur, defSize uint32) {
	kids, _ := mp4Boxes(r, moof.body, moof.end)
	for _, traf := range kids {
		if traf.typ != "traf" {
			continue
		}
		tk, _ := mp4Boxes(r, traf.body, traf.end)
		base := moof.start
		dDur, dSize := defDur, defSize
		if h, ok := findBox(tk, "tfhd"); ok {
			d := r.slice(h)
			flags := be32(d, 0) & 0xffffff
			i := 8
			if flags&0x1 != 0 {
				base = int64(binary.BigEndian.Uint64(d[i:]))
				i += 8
			}
			if flags&0x2 != 0 {
				i += 4
			}
			if flags&0x8 != 0 {
				dDur = be32(d, i)
				i += 4
			}
			if flags&0x10 != 0 {
				dSize = be32(d, i)
			}
		}
		for _, b := range tk {
			if b.typ != "trun" {
				continue
			}
			d := r.slice(b)
			flags := be32(d, 0) & 0xffffff
			n := int(be32(d, 4))
			i := 8
			off := base
			if flags&0x1 != 0 {
				off = base + int64(int32(be32(d, i)))
				i += 4
			}
			if flags&0x4 != 0 {
				i += 4
			}
			for k := 0; k < n; k++ {
				dur, size := dDur, dSize
				if flags&0x100 != 0 {
					dur = be32(d, i)
					i += 4
				}
				if flags&0x200 != 0 {
					size = be32(d, i)
					i += 4
				}
				if flags&0x400 != 0 {
					i += 4
				}
				if flags&0x800 != 0 {
					i += 4
				}
				if i > len(d) {
					return
				}
				s.samples = append(s.samples, m4aSample{off, size, dur})
				off += int64(size)
			}
		}
	}
}

// Duration of the whole track, in seconds.
func (s *m4aSource) Duration() float64 {
	var t uint64
	for _, x := range s.samples {
		t += uint64(x.dur)
	}
	return float64(t) / float64(s.timescale)
}

// sampleRange: the samples from start to end seconds (end <= 0: to the
// end of the track).
func (s *m4aSource) sampleRange(start, end float64) (int, int) {
	ts := float64(s.timescale)
	var t uint64
	from, to := len(s.samples), len(s.samples)
	for i, x := range s.samples {
		sec := float64(t) / ts
		if from == len(s.samples) && sec >= start-0.0001 {
			from = i
		}
		if end > 0 && sec >= end-0.0001 {
			to = i
			break
		}
		t += uint64(x.dur)
	}
	if from > to {
		from = to
	}
	return from, to
}

// m4aTags: what an output track says about itself.
type m4aTags struct {
	Title, Artist, AlbumArtist, Album, Year string
	Track, Tracks                           int
	Cover                                   []byte // JPEG or PNG
}

// WriteRange writes the samples from start to end seconds (end <= 0: to
// the end) as a tagged M4A at path.
func (s *m4aSource) WriteRange(path string, start, end float64, tags m4aTags) error {
	from, to := s.sampleRange(start, end)
	if to <= from {
		return errors.New("nothing in that part of the audio")
	}
	smp := s.samples[from:to]
	var dur uint64
	var data uint64
	for _, x := range smp {
		dur += uint64(x.dur)
		data += uint64(x.size)
	}
	if data > 0xffffffff-1<<20 {
		return errors.New("track too big")
	}
	ftyp := mp4Make("ftyp", []byte("M4A "), u32(0), []byte("M4A mp42isom"))
	moov := s.moov(smp, dur, 0, tags)
	// the chunk starts right after the mdat header
	off := uint32(len(ftyp) + len(moov) + 8)
	moov = s.moov(smp, dur, off, tags)

	tmp := path + ".part"
	out, err := os.Create(tmp)
	if err != nil {
		return err
	}
	w := &errWriter{w: out}
	w.write(ftyp)
	w.write(moov)
	w.write(u32(uint32(8 + data)))
	w.write([]byte("mdat"))
	buf := make([]byte, 0, 64<<10)
	for _, x := range smp {
		if w.err != nil {
			break
		}
		if cap(buf) < int(x.size) {
			buf = make([]byte, 0, x.size)
		}
		b := buf[:x.size]
		if _, err := s.f.ReadAt(b, x.off); err != nil && !errors.Is(err, io.EOF) {
			w.err = err
			break
		}
		w.write(b)
	}
	if err := out.Close(); w.err == nil {
		w.err = err
	}
	if w.err != nil {
		os.Remove(tmp)
		return w.err
	}
	return os.Rename(tmp, path)
}

type errWriter struct {
	w   io.Writer
	err error
}

func (e *errWriter) write(b []byte) {
	if e.err == nil {
		_, e.err = e.w.Write(b)
	}
}

func u32(v uint32) []byte { b := make([]byte, 4); binary.BigEndian.PutUint32(b, v); return b }
func u16(v uint16) []byte { b := make([]byte, 2); binary.BigEndian.PutUint16(b, v); return b }

// mp4Make builds a box from its parts.
func mp4Make(typ string, parts ...[]byte) []byte {
	n := 8
	for _, p := range parts {
		n += len(p)
	}
	b := make([]byte, 0, n)
	b = append(b, u32(uint32(n))...)
	b = append(b, typ...)
	for _, p := range parts {
		b = append(b, p...)
	}
	return b
}

var mp4Matrix = func() []byte {
	var b bytes.Buffer
	for _, v := range []uint32{0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000} {
		b.Write(u32(v))
	}
	return b.Bytes()
}()

// moov: one sound track with smp in one chunk at chunkOff, and the tags.
func (s *m4aSource) moov(smp []m4aSample, dur uint64, chunkOff uint32, tags m4aTags) []byte {
	d32 := uint32(min(dur, 0xffffffff))
	mvhd := mp4Make("mvhd", u32(0), u32(0), u32(0), u32(s.timescale), u32(d32),
		u32(0x10000), u16(0x100), make([]byte, 10), mp4Matrix, make([]byte, 24), u32(2))
	tkhd := mp4Make("tkhd", u32(7), u32(0), u32(0), u32(1), u32(0), u32(d32),
		make([]byte, 8), u16(0), u16(1), u16(0x100), u16(0), mp4Matrix, u32(0), u32(0))
	mdhd := mp4Make("mdhd", u32(0), u32(0), u32(0), u32(s.timescale), u32(d32), u16(0x55c4), u16(0))
	hdlr := mp4Make("hdlr", u32(0), u32(0), []byte("soun"), make([]byte, 12), []byte("SoundHandler\x00"))
	smhd := mp4Make("smhd", u32(0), u16(0), u16(0))
	dinf := mp4Make("dinf", mp4Make("dref", u32(0), u32(1), mp4Make("url ", u32(1))))
	// durations, run-length coded
	var stts bytes.Buffer
	entries := 0
	for i := 0; i < len(smp); {
		j := i
		for j < len(smp) && smp[j].dur == smp[i].dur {
			j++
		}
		stts.Write(u32(uint32(j - i)))
		stts.Write(u32(smp[i].dur))
		entries++
		i = j
	}
	sttsB := mp4Make("stts", u32(0), u32(uint32(entries)), stts.Bytes())
	stsc := mp4Make("stsc", u32(0), u32(1), u32(1), u32(uint32(len(smp))), u32(1))
	var sz bytes.Buffer
	for _, x := range smp {
		sz.Write(u32(x.size))
	}
	stsz := mp4Make("stsz", u32(0), u32(0), u32(uint32(len(smp))), sz.Bytes())
	stco := mp4Make("stco", u32(0), u32(1), u32(chunkOff))
	stbl := mp4Make("stbl", s.stsd, sttsB, stsc, stsz, stco)
	minf := mp4Make("minf", smhd, dinf, stbl)
	mdia := mp4Make("mdia", mdhd, hdlr, minf)
	trak := mp4Make("trak", tkhd, mdia)
	return mp4Make("moov", mvhd, trak, m4aUdta(tags))
}

// m4aUdta: the iTunes-style tags (udta/meta/ilst).
func m4aUdta(t m4aTags) []byte {
	var items [][]byte
	text := func(key, v string) {
		if v != "" {
			items = append(items, mp4Make(key, mp4Make("data", u32(1), u32(0), []byte(v))))
		}
	}
	text("\xa9nam", t.Title)
	text("\xa9ART", t.Artist)
	text("aART", t.AlbumArtist)
	text("\xa9alb", t.Album)
	text("\xa9day", t.Year)
	text("\xa9too", "KnulliPod")
	if t.Track > 0 {
		items = append(items, mp4Make("trkn", mp4Make("data", u32(0), u32(0),
			u16(0), u16(uint16(t.Track)), u16(uint16(t.Tracks)), u16(0))))
	}
	if len(t.Cover) > 0 {
		kind := uint32(13) // JPEG
		if bytes.HasPrefix(t.Cover, []byte("\x89PNG")) {
			kind = 14
		}
		items = append(items, mp4Make("covr", mp4Make("data", u32(kind), u32(0), t.Cover)))
	}
	ilst := mp4Make("ilst", items...)
	hdlr := mp4Make("hdlr", u32(0), u32(0), []byte("mdir"), []byte("appl"), make([]byte, 8), []byte{0})
	meta := mp4Make("meta", u32(0), hdlr, ilst)
	return mp4Make("udta", meta)
}
