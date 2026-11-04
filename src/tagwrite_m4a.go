package main

// M4A tag edits keep the media at its original offsets. The updated moov
// goes at the end; an old moov before the audio becomes free space. Later
// edits replace that final moov, so the file doesn't grow on each save.
// Everything is written beside the original, synced, then renamed.

import (
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"os"
)

const m4aTagLimit = 16 << 20 // movie tables and cover art, not the audio

// Tag writes need a complete box list: a damaged tail must not be dropped.
func m4aTagBoxes(r io.ReaderAt, off, end int64) ([]mp4Box, error) {
	bs, err := mp4Boxes(r, off, end)
	if err != nil {
		return nil, err
	}
	last := off
	for _, b := range bs {
		last = b.end
	}
	if last != end {
		return nil, errors.New("the M4A box list is incomplete")
	}
	return bs, nil
}

// m4aEditBox replaces one child, keeping every other child byte for byte.
// Missing containers are created; duplicate ones are refused as ambiguous.
func m4aEditBox(body []byte, typ string, edit func([]byte) ([]byte, error)) ([]byte, error) {
	bs, err := m4aTagBoxes(bytes.NewReader(body), 0, int64(len(body)))
	if err != nil {
		return nil, err
	}
	var out bytes.Buffer
	found := false
	for _, b := range bs {
		if binary.BigEndian.Uint32(body[b.start:b.start+4]) == 0 {
			return nil, errors.New("a nested size-zero M4A box can't be edited")
		}
		if b.typ != typ {
			out.Write(body[b.start:b.end])
			continue
		}
		if found {
			return nil, fmt.Errorf("the M4A has more than one %s box", typ)
		}
		found = true
		x, err := edit(body[b.body:b.end])
		if err != nil {
			return nil, err
		}
		out.Write(mp4Make(typ, x))
	}
	if !found {
		x, err := edit(nil)
		if err != nil {
			return nil, err
		}
		out.Write(mp4Make(typ, x))
	}
	return out.Bytes(), nil
}

func m4aEditMeta(body []byte, t songTags, mask tagMask) ([]byte, error) {
	if body == nil {
		hdlr := mp4Make("hdlr", u32(0), u32(0), []byte("mdir"), []byte("appl"), make([]byte, 8), []byte{0})
		body = append(u32(0), hdlr...)
	}
	if len(body) < 4 || binary.BigEndian.Uint32(body[:4]) != 0 {
		return nil, errors.New("this M4A metadata version can't be edited")
	}
	bs, err := m4aTagBoxes(bytes.NewReader(body), 4, int64(len(body)))
	if err != nil {
		return nil, err
	}
	handler := false
	for _, b := range bs {
		if b.typ == "keys" {
			return nil, errors.New("keyed M4A metadata can't be edited")
		}
		if b.typ == "hdlr" {
			if handler || b.end-b.body < 12 || string(body[b.body+8:b.body+12]) != "mdir" {
				return nil, errors.New("this M4A metadata handler can't be edited")
			}
			handler = true
		}
	}
	x, err := m4aEditBox(body[4:], "ilst", func(items []byte) ([]byte, error) {
		return m4aEditItems(items, t, mask)
	})
	if err != nil {
		return nil, err
	}
	if !handler {
		hdlr := mp4Make("hdlr", u32(0), u32(0), []byte("mdir"), []byte("appl"), make([]byte, 8), []byte{0})
		x = append(hdlr, x...)
	}
	return append(u32(0), x...), nil
}

func m4aEditItems(body []byte, t songTags, mask tagMask) ([]byte, error) {
	bs, err := m4aTagBoxes(bytes.NewReader(body), 0, int64(len(body)))
	if err != nil {
		return nil, err
	}
	for _, b := range bs {
		if binary.BigEndian.Uint32(body[b.start:b.start+4]) == 0 {
			return nil, errors.New("a size-zero M4A tag can't be edited")
		}
	}
	drop := map[string]bool{}
	var added [][]byte
	for _, f := range []struct {
		bit tagMask
		key string
		val string
	}{{tagTitle, "\xa9nam", t.Title}, {tagArtist, "\xa9ART", t.Artist}, {tagAlbumArtist, "aART", t.AlbumArtist},
		{tagAlbum, "\xa9alb", t.Album}, {tagGenre, "\xa9gen", t.Genre}, {tagYear, "\xa9day", itoaPos(t.Year)}} {
		if mask&f.bit == 0 {
			continue
		}
		drop[f.key] = true
		if f.bit == tagArtist {
			drop["\xa9art"] = true
		}
		if f.bit == tagGenre {
			drop["gnre"] = true // the numeric genre is another spelling of this field
		}
		if f.val != "" {
			added = append(added, mp4Make(f.key, mp4Make("data", u32(1), u32(0), []byte(f.val))))
		}
	}
	for _, f := range []struct {
		bit tagMask
		key string
		num int
	}{{tagTrack | tagTrackTotal, "trkn", t.Track}, {tagDisc, "disk", t.Disc}} {
		if mask&f.bit == 0 {
			continue
		}
		drop[f.key] = true
		// Keep the total (and locale/reserved bytes) when only the number
		// changes. The library doesn't hold the disc total at all.
		data := make([]byte, 16)
		found := false
		for _, b := range bs {
			if b.typ != f.key {
				continue
			}
			ks, err := m4aTagBoxes(bytes.NewReader(body), b.body, b.end)
			if err != nil || found || len(ks) != 1 || ks[0].typ != "data" || ks[0].end-ks[0].body < 14 {
				return nil, fmt.Errorf("the M4A %s tag can't be read safely", f.key)
			}
			found = true
			data = append([]byte(nil), body[ks[0].body:ks[0].end]...)
			if binary.BigEndian.Uint32(data[:4]) != 0 {
				return nil, fmt.Errorf("the M4A %s number format can't be edited", f.key)
			}
		}
		put := func(off, n int) error {
			if n < 0 || n > 65535 {
				return errors.New("M4A track and disc numbers must be between 0 and 65535")
			}
			binary.BigEndian.PutUint16(data[off:off+2], uint16(n))
			return nil
		}
		if f.key == "disk" || mask&tagTrack != 0 {
			if err := put(10, f.num); err != nil {
				return nil, err
			}
		}
		if f.key == "trkn" && mask&tagTrackTotal != 0 {
			if err := put(12, t.TrackTotal); err != nil {
				return nil, err
			}
		}
		if binary.BigEndian.Uint16(data[10:12]) != 0 || binary.BigEndian.Uint16(data[12:14]) != 0 {
			added = append(added, mp4Make(f.key, mp4Make("data", data)))
		}
	}
	var out bytes.Buffer
	for _, b := range bs {
		if !drop[b.typ] {
			out.Write(body[b.start:b.end])
		}
	}
	for _, b := range added {
		out.Write(b)
	}
	return out.Bytes(), nil
}

func writeM4ATags(path string, t songTags, mask tagMask) error {
	in, err := os.Open(path)
	if err != nil {
		return err
	}
	defer in.Close()
	st, err := in.Stat()
	if err != nil {
		return err
	}
	top, err := m4aTagBoxes(in, 0, st.Size())
	if err != nil {
		return err
	}
	moov, ok := findBox(top, "moov")
	_, ftyp := findBox(top, "ftyp")
	_, mdat := findBox(top, "mdat")
	if !ok || !ftyp || !mdat || moov.end-moov.start > m4aTagLimit {
		return errors.New("the M4A movie metadata is missing or too large")
	}
	if len(top) == 0 || top[0].typ != "ftyp" || top[0].body != 8 || top[0].end-top[0].body < 8 {
		return errors.New("the M4A file type header can't be read")
	}
	for _, b := range top {
		if b.typ == "moof" || (b.typ == "moov" && b.start != moov.start) {
			return errors.New("fragmented M4A or multiple movies can't be edited")
		}
		if b.body-b.start == 16 && b.end-b.start-8 > 0xffffffff {
			return errors.New("an extended M4A box over 4 GB can't be edited")
		}
	}
	body := make([]byte, moov.end-moov.body)
	if _, err := in.ReadAt(body, moov.body); err != nil {
		return err
	}
	kids, err := m4aTagBoxes(bytes.NewReader(body), 0, int64(len(body)))
	if err != nil {
		return err
	}
	for _, b := range kids {
		if b.typ == "cmov" || b.typ == "mvex" || b.typ == "meta" {
			return errors.New("this M4A movie layout can't be edited")
		}
	}
	body, err = m4aEditBox(body, "udta", func(udta []byte) ([]byte, error) {
		return m4aEditBox(udta, "meta", func(meta []byte) ([]byte, error) {
			return m4aEditMeta(meta, t, mask)
		})
	})
	if err != nil {
		return err
	}
	if len(body) > m4aTagLimit-8 {
		return errors.New("the edited M4A metadata is too large")
	}
	updated := mp4Make("moov", body)
	copyEnd := st.Size()
	if moov.end == st.Size() {
		copyEnd = moov.start
	}
	// A final size-zero mdat extends to EOF. Give it its actual size
	// before appending moov, without shifting its header or the audio.
	last := top[len(top)-1]
	var lastSize [4]byte
	if _, err := in.ReadAt(lastSize[:], last.start); err != nil {
		return err
	}
	zeroLast := binary.BigEndian.Uint32(lastSize[:]) == 0 && copyEnd == st.Size()
	if zeroLast && last.end-last.start > 0xffffffff {
		return errors.New("a size-zero M4A box over 4 GB can't be edited")
	}
	tmp := path + ".tagtmp"
	out, err := os.OpenFile(tmp, os.O_WRONLY|os.O_CREATE|os.O_EXCL, st.Mode().Perm())
	if err != nil {
		return err
	}
	defer os.Remove(tmp)
	defer out.Close()
	if _, err := io.CopyN(out, io.NewSectionReader(in, 0, copyEnd), copyEnd); err != nil {
		return err
	}
	if copyEnd == st.Size() {
		if _, err := out.WriteAt([]byte("free"), moov.start+4); err != nil {
			return err
		}
	}
	// The library's tag reader skips 32-bit box sizes only. An extended
	// header becomes an 8-byte free box followed by a normal header:
	// the payload stays at exactly the same offset. This also lets it
	// reach the new metadata when the source used an extended mdat.
	for _, b := range top {
		if b.body-b.start != 16 || b.end > copyEnd {
			continue
		}
		typ := b.typ
		if b.start == moov.start {
			typ = "free"
		}
		h := append(mp4Make("free"), u32(uint32(b.end-b.start-8))...)
		h = append(h, typ...)
		if _, err := out.WriteAt(h, b.start); err != nil {
			return err
		}
	}
	if zeroLast {
		if _, err := out.WriteAt(u32(uint32(last.end-last.start)), last.start); err != nil {
			return err
		}
	}
	if _, err := out.Write(updated); err != nil {
		return err
	}
	if err := out.Sync(); err != nil {
		return err
	}
	if err := out.Close(); err != nil {
		return err
	}
	if err := in.Close(); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}
