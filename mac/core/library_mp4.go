package main

// Box helpers adapted from src/ij_m4a.go.
import (
	"encoding/binary"
	"fmt"
	"io"
)

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
		if size < hdr || size > end-off {
			return out, fmt.Errorf("bad box %q", typ)
		}
		out = append(out, mp4Box{typ, off, off + size, off + hdr})
		off += size
	}
	return out, nil
}

func findBox(bs []mp4Box, typ string) (mp4Box, bool) {
	for _, b := range bs {
		if b.typ == typ {
			return b, true
		}
	}
	return mp4Box{}, false
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
