package main

import (
	"bytes"
	"encoding/base64"
	"encoding/binary"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// Validate length fields before calling the general metadata reader. Some
// readers allocate from a tag's declared length before discovering truncation.
func validateLibraryContainer(f *os.File, path string) error {
	switch strings.ToLower(filepath.Ext(path)) {
	case ".mp3":
		_, frames, _, err := readID3(f)
		if err != nil {
			return err
		}
		for _, fr := range frames {
			// The writer preserves opaque frames, but the reader may inflate
			// compressed frames from a size stored inside the frame itself.
			if fr.flags[1] != 0 {
				return errors.New("Compressed or encrypted ID3 frames are read-only")
			}
		}
	case ".flac":
		var h [10]byte
		if _, err := f.ReadAt(h[:], 0); err != nil {
			return err
		}
		start := int64(0)
		if string(h[:3]) == "ID3" {
			return errors.New("FLAC files with a second ID3 metadata header are read-only")
		}
		var magic [4]byte
		if _, err := f.ReadAt(magic[:], start); err != nil {
			return err
		}
		if string(magic[:]) != "fLaC" {
			return errors.New("Invalid FLAC header")
		}
		total := 0
		for off := start + 4; ; {
			var b [4]byte
			if _, err := f.ReadAt(b[:], off); err != nil {
				return err
			}
			n := int(b[1])<<16 | int(b[2])<<8 | int(b[3])
			total += n + 4
			if total > libraryTagLimit {
				return errors.New("FLAC metadata exceeds 16 MB")
			}
			data := make([]byte, n)
			if _, err := f.ReadAt(data, off+4); err != nil {
				return err
			}
			switch b[0] & 0x7f {
			case 4:
				if err := validateLibraryComments(data); err != nil {
					return err
				}
			case 6:
				if err := validateLibraryPicture(data); err != nil {
					return err
				}
			}
			off += int64(n + 4)
			if b[0]&0x80 != 0 {
				break
			}
		}
	case ".ogg":
		if _, err := f.Seek(0, io.SeekStart); err != nil {
			return err
		}
		total := 0
		var packet []byte
		for {
			page, err := readOggPage(f)
			if err != nil {
				return err
			}
			total += len(page.body) + len(page.lacing) + 27
			if total > libraryTagLimit {
				return errors.New("Ogg metadata exceeds 16 MB")
			}
			off := 0
			for _, n := range page.lacing {
				packet = append(packet, page.body[off:off+int(n)]...)
				off += int(n)
				if n < 255 {
					if bytes.HasPrefix(packet, []byte("\x03vorbis")) {
						if err := validateLibraryComments(packet[7:]); err != nil {
							return err
						}
						_, err := f.Seek(0, io.SeekStart)
						return err
					}
					if bytes.HasPrefix(packet, []byte("OpusTags")) {
						return errors.New("Ogg Opus files are read-only")
					}
					packet = nil
				}
			}
		}
	case ".m4a":
		st, err := f.Stat()
		if err != nil {
			return err
		}
		boxes, err := m4aTagBoxes(f, 0, st.Size())
		if err != nil {
			return err
		}
		for _, b := range boxes {
			if b.typ != "mdat" && b.typ != "free" && b.typ != "skip" && b.end-b.body > libraryTagLimit {
				return errors.New("M4A metadata exceeds 16 MB")
			}
			var header [4]byte
			if _, err := f.ReadAt(header[:], b.start); err != nil {
				return err
			}
			if binary.BigEndian.Uint32(header[:]) < 8 {
				return errors.New("Extended or size-zero M4A headers are read-only")
			}
			if b.typ == "moov" {
				if b.end-b.body > libraryTagLimit {
					return errors.New("M4A metadata exceeds 16 MB")
				}
				data := make([]byte, b.end-b.body)
				if _, err := f.ReadAt(data, b.body); err != nil {
					return err
				}
				if err := validateLibraryMP4(data, 0); err != nil {
					return err
				}
			}
		}
	}
	_, err := f.Seek(0, io.SeekStart)
	return err
}

func validateLibraryComments(data []byte) error {
	vc, err := parseVorbisComment(data)
	if err != nil {
		return err
	}
	for _, field := range vc.fields {
		key, value, _ := strings.Cut(field, "=")
		if strings.EqualFold(key, "METADATA_BLOCK_PICTURE") {
			pic, err := base64.StdEncoding.DecodeString(value)
			if err != nil {
				return err
			}
			if err := validateLibraryPicture(pic); err != nil {
				return err
			}
		}
	}
	return nil
}

func validateLibraryPicture(data []byte) error {
	bad := errors.New("Invalid embedded cover lengths")
	// Skip type, MIME and description, then dimensions and data length.
	pos := 4
	for i := 0; i < 2; i++ {
		if pos+4 > len(data) {
			return bad
		}
		n := int64(binary.BigEndian.Uint32(data[pos : pos+4]))
		pos += 4
		if n > int64(len(data)-pos) {
			return bad
		}
		pos += int(n)
	}
	if pos+20 > len(data) {
		return bad
	}
	pos += 16
	n := int64(binary.BigEndian.Uint32(data[pos : pos+4]))
	pos += 4
	if n > int64(len(data)-pos) {
		return bad
	}
	return nil
}

func validateLibraryMP4(data []byte, depth int) error {
	if depth > 16 {
		return errors.New("M4A metadata is nested too deeply")
	}
	boxes, err := m4aTagBoxes(bytes.NewReader(data), 0, int64(len(data)))
	if err != nil {
		return err
	}
	for _, b := range boxes {
		if binary.BigEndian.Uint32(data[b.start:b.start+4]) < 8 {
			return errors.New("Extended or size-zero M4A metadata is read-only")
		}
		body := data[b.body:b.end]
		switch b.typ {
		case "meta":
			if len(body) < 4 {
				return errors.New("Invalid M4A metadata header")
			}
			if err := validateLibraryMP4(body[4:], depth+1); err != nil {
				return err
			}
		case "moov", "udta", "ilst", "----":
			if err := validateLibraryMP4(body, depth+1); err != nil {
				return err
			}
		}
	}
	return nil
}
