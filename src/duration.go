package main

// duration.go — track length from the file headers, without decoding:
// FLAC STREAMINFO, MP3 Xing/Info/VBRI (or the CBR bitrate), MP4 mvhd,
// WAV fmt/data, AIFF COMM and the last Ogg page. The lists (Cover Flow's
// track list) show it before a track has been played; playback still
// takes the decoder's own duration once the track is loaded.

import (
	"bytes"
	"encoding/binary"
	"io"
	"math"
	"os"
	"path/filepath"
	"strings"
)

// probeDuration returns the length of the audio file in seconds, or 0.
func probeDuration(path string) float64 {
	f, err := os.Open(path)
	if err != nil {
		return 0
	}
	defer f.Close()
	st, err := f.Stat()
	if err != nil {
		return 0
	}
	size := st.Size()
	var d float64
	switch strings.ToLower(filepath.Ext(path)) {
	case ".flac":
		d = flacDuration(f)
	case ".mp3":
		d = mp3Duration(f, size)
	case ".m4a", ".mp4":
		d = mp4Duration(f, size)
	case ".wav":
		d = wavDuration(f)
	case ".aif", ".aiff":
		d = aiffDuration(f)
	case ".ogg":
		d = oggDuration(f, size)
	}
	if math.IsNaN(d) || math.IsInf(d, 0) || d < 0 || d > 48*3600 {
		return 0
	}
	return d
}

// id3Skip returns the offset after an ID3v2 tag at the start of f (0 if none).
func id3Skip(f io.ReaderAt) int64 {
	var h [10]byte
	if _, err := f.ReadAt(h[:], 0); err != nil || string(h[:3]) != "ID3" {
		return 0
	}
	n := int64(h[6]&0x7f)<<21 | int64(h[7]&0x7f)<<14 | int64(h[8]&0x7f)<<7 | int64(h[9]&0x7f)
	n += 10
	if h[5]&0x10 != 0 {
		n += 10 // footer
	}
	return n
}

func flacDuration(f io.ReaderAt) float64 {
	off := id3Skip(f)
	var b [42]byte
	if _, err := f.ReadAt(b[:], off); err != nil || string(b[:4]) != "fLaC" || b[4]&0x7f != 0 {
		return 0
	}
	si := b[8:]
	rate := uint32(si[10])<<12 | uint32(si[11])<<4 | uint32(si[12])>>4
	total := uint64(si[13]&0x0f)<<32 | uint64(binary.BigEndian.Uint32(si[14:18]))
	if rate == 0 || total == 0 {
		return 0
	}
	return float64(total) / float64(rate)
}

var mp3Bitrates = [2][3][16]int{
	{ // MPEG 1: layer I, II, III
		{0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 0},
		{0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384, 0},
		{0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0},
	},
	{ // MPEG 2 / 2.5
		{0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256, 0},
		{0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0},
		{0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0},
	},
}

var mp3Rates = [4][3]int{{11025, 12000, 8000}, {0, 0, 0}, {22050, 24000, 16000}, {44100, 48000, 32000}}

func mp3Duration(f io.ReaderAt, size int64) float64 {
	start := id3Skip(f)
	buf := make([]byte, 64*1024)
	n, _ := f.ReadAt(buf, start)
	buf = buf[:n]
	for i := 0; i+4 <= len(buf); i++ {
		if buf[i] != 0xff || buf[i+1]&0xe0 != 0xe0 {
			continue
		}
		ver := (buf[i+1] >> 3) & 3 // 0: 2.5, 2: 2, 3: 1
		layer := (buf[i+1] >> 1) & 3
		bri := buf[i+2] >> 4
		sri := (buf[i+2] >> 2) & 3
		if ver == 1 || layer == 0 || bri == 0 || bri == 15 || sri == 3 {
			continue
		}
		li := 3 - int(layer) // 0: I, 1: II, 2: III
		vi := 1
		if ver == 3 {
			vi = 0
		}
		kbps := mp3Bitrates[vi][li][bri]
		rate := mp3Rates[ver][sri]
		if kbps == 0 || rate == 0 {
			continue
		}
		spf := 1152.0
		if li == 0 {
			spf = 384
		} else if li == 2 && vi == 1 {
			spf = 576
		}
		mono := buf[i+3]>>6 == 3
		side := 32
		if vi == 0 && mono || vi == 1 && !mono {
			side = 17
		} else if vi == 1 && mono {
			side = 9
		}
		// Xing / Info header: frame count
		if x := i + 4 + side; x+12 <= len(buf) {
			if tag := string(buf[x : x+4]); tag == "Xing" || tag == "Info" {
				if binary.BigEndian.Uint32(buf[x+4:])&1 != 0 {
					if frames := binary.BigEndian.Uint32(buf[x+8:]); frames > 0 {
						return float64(frames) * spf / float64(rate)
					}
				}
			}
		}
		// VBRI header (Fraunhofer), 32 bytes after the frame header
		if x := i + 4 + 32; x+18 <= len(buf) && string(buf[x:x+4]) == "VBRI" {
			if frames := binary.BigEndian.Uint32(buf[x+14:]); frames > 0 {
				return float64(frames) * spf / float64(rate)
			}
		}
		// constant bitrate: the audio bytes over the bitrate
		audio := size - start - int64(i)
		var tag [3]byte
		if _, err := f.ReadAt(tag[:], size-128); err == nil && string(tag[:]) == "TAG" {
			audio -= 128
		}
		return float64(audio) * 8 / float64(kbps*1000)
	}
	return 0
}

// mp4Duration reads moov/mvhd (timescale and duration), walking the boxes.
func mp4Duration(f io.ReaderAt, size int64) float64 {
	moov, moovEnd, ok := mp4Find(f, 0, size, "moov")
	if !ok {
		return 0
	}
	mv, _, ok := mp4Find(f, moov, moovEnd, "mvhd")
	if !ok {
		return 0
	}
	var b [32]byte
	if _, err := f.ReadAt(b[:], mv); err != nil {
		return 0
	}
	if b[0] == 1 {
		ts := binary.BigEndian.Uint32(b[20:24])
		dur := binary.BigEndian.Uint64(b[24:32])
		if ts == 0 {
			return 0
		}
		return float64(dur) / float64(ts)
	}
	ts := binary.BigEndian.Uint32(b[12:16])
	dur := binary.BigEndian.Uint32(b[16:20])
	if ts == 0 || dur == 0xffffffff {
		return 0
	}
	return float64(dur) / float64(ts)
}

// mp4Find returns the payload range of the first box named name in
// [from, to).
func mp4Find(f io.ReaderAt, from, to int64, name string) (int64, int64, bool) {
	var h [16]byte
	for off := from; off+8 <= to; {
		if _, err := f.ReadAt(h[:8], off); err != nil {
			return 0, 0, false
		}
		sz := int64(binary.BigEndian.Uint32(h[:4]))
		hdr := int64(8)
		if sz == 1 {
			if _, err := f.ReadAt(h[8:16], off+8); err != nil {
				return 0, 0, false
			}
			sz = int64(binary.BigEndian.Uint64(h[8:16]))
			hdr = 16
		} else if sz == 0 {
			sz = to - off
		}
		if sz < hdr {
			return 0, 0, false
		}
		if string(h[4:8]) == name {
			return off + hdr, off + sz, true
		}
		off += sz
	}
	return 0, 0, false
}

func wavDuration(f io.ReaderAt) float64 {
	var h [12]byte
	if _, err := f.ReadAt(h[:], 0); err != nil || string(h[:4]) != "RIFF" || string(h[8:12]) != "WAVE" {
		return 0
	}
	var byteRate uint32
	var c [16]byte
	for off := int64(12); ; {
		if _, err := f.ReadAt(c[:8], off); err != nil {
			return 0
		}
		sz := int64(binary.LittleEndian.Uint32(c[4:8]))
		switch string(c[:4]) {
		case "fmt ":
			if _, err := f.ReadAt(c[:12], off+8); err != nil {
				return 0
			}
			byteRate = binary.LittleEndian.Uint32(c[8:12])
		case "data":
			if byteRate == 0 {
				return 0
			}
			return float64(sz) / float64(byteRate)
		}
		off += 8 + sz + sz&1
	}
}

func aiffDuration(f io.ReaderAt) float64 {
	var h [12]byte
	if _, err := f.ReadAt(h[:], 0); err != nil || string(h[:4]) != "FORM" || (string(h[8:12]) != "AIFF" && string(h[8:12]) != "AIFC") {
		return 0
	}
	var c [26]byte
	for off := int64(12); ; {
		if _, err := f.ReadAt(c[:8], off); err != nil {
			return 0
		}
		sz := int64(binary.BigEndian.Uint32(c[4:8]))
		if string(c[:4]) == "COMM" {
			if _, err := f.ReadAt(c[8:26], off+8); err != nil {
				return 0
			}
			frames := binary.BigEndian.Uint32(c[10:14])
			rate := ieeeExtended(c[16:26])
			if rate <= 0 {
				return 0
			}
			return float64(frames) / rate
		}
		off += 8 + sz + sz&1
	}
}

// ieeeExtended decodes an 80-bit IEEE 754 extended float (AIFF sample rate).
func ieeeExtended(b []byte) float64 {
	exp := int(binary.BigEndian.Uint16(b[0:2]) & 0x7fff)
	mant := binary.BigEndian.Uint64(b[2:10])
	if exp == 0 && mant == 0 {
		return 0
	}
	return float64(mant) * math.Pow(2, float64(exp-16383-63))
}

// oggDuration: the granule position of the last page over the sample rate
// of the first stream (Vorbis, Opus or FLAC in Ogg).
func oggDuration(f io.ReaderAt, size int64) float64 {
	head := make([]byte, 512)
	n, _ := f.ReadAt(head, 0)
	head = head[:n]
	if len(head) < 64 || string(head[:4]) != "OggS" {
		return 0
	}
	segs := int(head[26])
	p := 27 + segs
	if p+20 > len(head) {
		return 0
	}
	pkt := head[p:]
	var rate float64
	preSkip := 0.0
	switch {
	case len(pkt) >= 16 && string(pkt[1:7]) == "vorbis":
		rate = float64(binary.LittleEndian.Uint32(pkt[12:16]))
	case len(pkt) >= 16 && string(pkt[:8]) == "OpusHead":
		rate = 48000
		preSkip = float64(binary.LittleEndian.Uint16(pkt[10:12]))
	case len(pkt) >= 30 && string(pkt[1:5]) == "FLAC":
		si := pkt[17:]
		rate = float64(uint32(si[10])<<12 | uint32(si[11])<<4 | uint32(si[12])>>4)
	}
	if rate <= 0 {
		return 0
	}
	tail := int64(64 * 1024)
	if tail > size {
		tail = size
	}
	buf := make([]byte, tail)
	if _, err := f.ReadAt(buf, size-tail); err != nil && err != io.EOF {
		return 0
	}
	i := bytes.LastIndex(buf, []byte("OggS"))
	if i < 0 || i+14 > len(buf) {
		return 0
	}
	g := int64(binary.LittleEndian.Uint64(buf[i+6 : i+14]))
	if g <= 0 {
		return 0
	}
	return (float64(g) - preSkip) / rate
}

// audioFormat is what the Now Playing badge shows: whether the file is
// lossless (FLAC, ALAC, WAV, AIFF) and, when the headers say, its bit depth
// and sample rate.
type audioFormat struct {
	lossless bool
	bits     int
	rate     int
}

// probeFormat reads the format from the headers (no decoding).
func probeFormat(path string) audioFormat {
	f, err := os.Open(path)
	if err != nil {
		return audioFormat{}
	}
	defer f.Close()
	st, err := f.Stat()
	if err != nil {
		return audioFormat{}
	}
	switch strings.ToLower(filepath.Ext(path)) {
	case ".flac":
		off := id3Skip(f)
		var b [42]byte
		if _, err := f.ReadAt(b[:], off); err != nil || string(b[:4]) != "fLaC" {
			return audioFormat{lossless: true}
		}
		si := b[8:]
		rate := int(si[10])<<12 | int(si[11])<<4 | int(si[12])>>4
		bits := (int(si[12]&1)<<4 | int(si[13])>>4) + 1
		return audioFormat{true, bits, rate}
	case ".wav":
		return wavFormat(f)
	case ".aif", ".aiff":
		return aiffFormat(f)
	case ".m4a", ".mp4":
		return mp4Format(f, st.Size())
	}
	return audioFormat{}
}

func wavFormat(f io.ReaderAt) audioFormat {
	var h [12]byte
	if _, err := f.ReadAt(h[:], 0); err != nil || string(h[:4]) != "RIFF" || string(h[8:12]) != "WAVE" {
		return audioFormat{}
	}
	var c [24]byte
	for off := int64(12); off < 1<<20; {
		if _, err := f.ReadAt(c[:8], off); err != nil {
			return audioFormat{}
		}
		sz := int64(binary.LittleEndian.Uint32(c[4:8]))
		if string(c[:4]) == "fmt " {
			if _, err := f.ReadAt(c[:16], off+8); err != nil {
				return audioFormat{}
			}
			tag := binary.LittleEndian.Uint16(c[0:2])
			// PCM, IEEE float, extensible: uncompressed
			lossless := tag == 1 || tag == 3 || tag == 0xfffe
			return audioFormat{lossless, int(binary.LittleEndian.Uint16(c[14:16])), int(binary.LittleEndian.Uint32(c[4:8]))}
		}
		off += 8 + sz + sz&1
	}
	return audioFormat{}
}

func aiffFormat(f io.ReaderAt) audioFormat {
	var h [12]byte
	if _, err := f.ReadAt(h[:], 0); err != nil || string(h[:4]) != "FORM" {
		return audioFormat{}
	}
	aifc := string(h[8:12]) == "AIFC"
	var c [30]byte
	for off := int64(12); off < 1<<20; {
		if _, err := f.ReadAt(c[:8], off); err != nil {
			return audioFormat{}
		}
		sz := int64(binary.BigEndian.Uint32(c[4:8]))
		if string(c[:4]) == "COMM" {
			if _, err := f.ReadAt(c[8:30], off+8); err != nil {
				return audioFormat{}
			}
			lossless := true
			if aifc && sz >= 22 {
				// AIFF-C: only the uncompressed codes count
				switch string(c[26:30]) {
				case "NONE", "sowt", "twos", "fl32", "fl64", "in24", "in32":
				default:
					lossless = false
				}
			}
			return audioFormat{lossless, int(binary.BigEndian.Uint16(c[14:16])), int(ieeeExtended(c[16:26]) + 0.5)}
		}
		off += 8 + sz + sz&1
	}
	return audioFormat{}
}

// mp4Format: ALAC is lossless, AAC is not. The first track's sample
// description (moov/trak/mdia/minf/stbl/stsd) names the codec; the ALAC
// configuration box carries the bit depth and sample rate.
func mp4Format(f io.ReaderAt, size int64) audioFormat {
	from, to := int64(0), size
	for _, name := range []string{"moov", "trak", "mdia", "minf", "stbl", "stsd"} {
		a, b, ok := mp4Find(f, from, to, name)
		if !ok {
			return audioFormat{}
		}
		from, to = a, b
	}
	var e [8 + 28 + 8 + 4 + 24]byte
	if _, err := f.ReadAt(e[:], from+8); err != nil {
		return audioFormat{}
	}
	if string(e[4:8]) != "alac" {
		return audioFormat{}
	}
	af := audioFormat{lossless: true, bits: int(binary.BigEndian.Uint16(e[8+18 : 8+20])), rate: int(binary.BigEndian.Uint32(e[8+24:8+28]) >> 16)}
	// the "alac" configuration box after the 28-byte sample entry
	if cfg := e[36:]; string(cfg[4:8]) == "alac" {
		c := cfg[12:]
		af.bits = int(c[5])
		if r := int(binary.BigEndian.Uint32(c[20:24])); r > 0 {
			af.rate = r
		}
	}
	return af
}
