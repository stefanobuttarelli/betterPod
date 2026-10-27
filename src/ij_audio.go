package main

// ij_audio.go — audio output at each track's own sample rate (nothing is
// resampled before the system mixer), format probes and the system volume.

/*
int audio_set_rate(int rate);
*/
import "C"

import (
	"encoding/binary"
	"io"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
)

// audioPrepareFor reopens the output at the sample rate of the next track.
func audioPrepareFor(path string) {
	rate := probeSampleRate(path)
	if rate > 0 {
		C.audio_set_rate(C.int(rate))
	}
}

// supportedAudioExt: formats the patched SDL_mixer can play (MP3, FLAC,
// Ogg, WAV, AIFF, ALAC and AAC in MP4).
func supportedAudioExt(ext string) bool {
	switch ext {
	case ".mp3", ".flac", ".ogg", ".wav", ".aif", ".aiff", ".m4a", ".mp4", ".alac":
		return true
	}
	return false
}

// probeSampleRate returns the sample rate of an audio file (0 if unknown).
func probeSampleRate(path string) int {
	ext := strings.ToLower(filepath.Ext(path))
	switch ext {
	case ".m4a", ".mp4", ".alac":
		rate, _ := probeMP4(path)
		return rate
	case ".wav":
		return probeWAV(path)
	case ".aif", ".aiff":
		return probeAIFF(path)
	case ".mp3":
		return probeMP3(path)
	}
	return readSampleRate(path)
}

// mp4Codec returns "ALAC", "AAC" or "" for an MP4/M4A file.
func mp4Codec(path string) string {
	_, c := probeMP4Codec(path)
	return c
}

func probeMP4(path string) (int, bool) {
	r, codec := probeMP4Codec(path)
	return r, codec != ""
}

// probeMP4Codec walks the MP4 boxes down to the sample description (stsd).
func probeMP4Codec(path string) (int, string) {
	f, err := os.Open(path)
	if err != nil {
		return 0, ""
	}
	defer f.Close()
	st, err := f.Stat()
	if err != nil {
		return 0, ""
	}
	codec := ""
	var walk func(start, end int64, depth int) (int, bool)
	walk = func(start, end int64, depth int) (int, bool) {
		pos := start
		hdr := make([]byte, 16)
		for pos+8 <= end {
			if _, err := f.ReadAt(hdr[:8], pos); err != nil {
				return 0, false
			}
			size := int64(binary.BigEndian.Uint32(hdr[0:4]))
			typ := string(hdr[4:8])
			h := int64(8)
			if size == 1 {
				if _, err := f.ReadAt(hdr[8:16], pos+8); err != nil {
					return 0, false
				}
				size = int64(binary.BigEndian.Uint64(hdr[8:16]))
				h = 16
			} else if size == 0 {
				size = end - pos
			}
			if size < h {
				return 0, false
			}
			switch typ {
			case "moov", "trak", "mdia", "minf", "stbl":
				if depth < 8 {
					if r, ok := walk(pos+h, pos+size, depth+1); ok {
						return r, true
					}
				}
			case "stsd":
				// version/flags, entry count, then the first sample entry
				b := make([]byte, 84)
				n, _ := f.ReadAt(b, pos+h)
				if n >= 44 && string(b[12:16]) == "mp4a" {
					codec = "AAC"
					return int(binary.BigEndian.Uint32(b[40:44]) >> 16), true
				}
				if n >= 44 && string(b[12:16]) == "alac" {
					codec = "ALAC"
					rate := int(binary.BigEndian.Uint32(b[40:44]) >> 16)
					// the 'alac' box inside the entry carries the full 32-bit rate
					if n >= 80 && string(b[48:52]) == "alac" {
						if r := int(binary.BigEndian.Uint32(b[76:80])); r != 0 {
							rate = r
						}
					}
					return rate, true
				}
			}
			pos += size
		}
		return 0, false
	}
	r, _ := walk(0, st.Size(), 0)
	return r, codec
}

func probeWAV(path string) int {
	f, err := os.Open(path)
	if err != nil {
		return 0
	}
	defer f.Close()
	var hdr [12]byte
	if _, err := io.ReadFull(f, hdr[:]); err != nil || string(hdr[0:4]) != "RIFF" || string(hdr[8:12]) != "WAVE" {
		return 0
	}
	for i := 0; i < 64; i++ {
		var ch [8]byte
		if _, err := io.ReadFull(f, ch[:]); err != nil {
			return 0
		}
		size := int64(binary.LittleEndian.Uint32(ch[4:8]))
		if string(ch[0:4]) == "fmt " {
			var fm [8]byte
			if _, err := io.ReadFull(f, fm[:]); err != nil {
				return 0
			}
			return int(binary.LittleEndian.Uint32(fm[4:8]))
		}
		if _, err := f.Seek(size+size&1, io.SeekCurrent); err != nil {
			return 0
		}
	}
	return 0
}

func probeAIFF(path string) int {
	f, err := os.Open(path)
	if err != nil {
		return 0
	}
	defer f.Close()
	var hdr [12]byte
	if _, err := io.ReadFull(f, hdr[:]); err != nil || string(hdr[0:4]) != "FORM" {
		return 0
	}
	for i := 0; i < 64; i++ {
		var ch [8]byte
		if _, err := io.ReadFull(f, ch[:]); err != nil {
			return 0
		}
		size := int64(binary.BigEndian.Uint32(ch[4:8]))
		if string(ch[0:4]) == "COMM" {
			b := make([]byte, 18)
			if _, err := io.ReadFull(f, b); err != nil {
				return 0
			}
			// sample rate: 80-bit IEEE extended at offset 8
			exp := int(binary.BigEndian.Uint16(b[8:10]) & 0x7fff)
			mant := binary.BigEndian.Uint64(b[10:18])
			return int(float64(mant) * math.Pow(2, float64(exp-16383-63)))
		}
		if _, err := f.Seek(size+size&1, io.SeekCurrent); err != nil {
			return 0
		}
	}
	return 0
}

// probeMP3 reads the sample rate of the first frame after the ID3v2 tag.
func probeMP3(path string) int {
	f, err := os.Open(path)
	if err != nil {
		return 0
	}
	defer f.Close()
	off := int64(0)
	var h [10]byte
	if _, err := io.ReadFull(f, h[:]); err == nil && string(h[0:3]) == "ID3" {
		sz := int64(h[6]&0x7f)<<21 | int64(h[7]&0x7f)<<14 | int64(h[8]&0x7f)<<7 | int64(h[9]&0x7f)
		off = 10 + sz
		if h[5]&0x10 != 0 {
			off += 10 // footer
		}
	}
	buf := make([]byte, 16384)
	n, _ := f.ReadAt(buf, off)
	rates := [4][4]int{{11025, 12000, 8000, 0}, {}, {22050, 24000, 16000, 0}, {44100, 48000, 32000, 0}}
	for i := 0; i+4 <= n; i++ {
		if buf[i] != 0xff || buf[i+1]&0xe0 != 0xe0 {
			continue
		}
		// version (bits 4-3), layer (2-1); bitrate (7-4), rate index (3-2)
		// of the next byte
		b1, b2 := buf[i+1], buf[i+2]
		ver, layer, br, sr := (b1>>3)&3, (b1>>1)&3, b2>>4, (b2>>2)&3
		if ver == 1 || layer == 0 || br == 0 || br == 15 || sr == 3 {
			continue
		}
		return rates[ver][sr]
	}
	return 0
}

// ---- system volume and output (Knulli: PipeWire, driven via pactl) ----
//
// The player volume is the console's volume (default sink). Changes made
// elsewhere (console volume keys, Bluetooth headphones) are picked up through
// "pactl subscribe", so the volume bar follows them right away.

type sysVolume struct {
	mu      sync.Mutex
	ok      bool
	bin     string // batocera-audio, used for writes when present
	want    int    // pending write, -1 if none
	cur     int
	lastSet time.Time
	wake    chan struct{}
	changed int32 // external change waiting for the UI (atomic), -1 none
	active  int32 // unused (kept for the UI loop)
	events  chan struct{}
}

var sysVol = &sysVolume{want: -1, cur: -1, changed: -1, wake: make(chan struct{}, 1), events: make(chan struct{}, 1)}

var (
	btState int32 // 1 when the default sink is a Bluetooth device
	btLost  int32 // set when it stops being one (pause like an iPod)
)

func btConnected() bool { return atomic.LoadInt32(&btState) == 1 }
func btLostTake() bool  { return atomic.SwapInt32(&btLost, 0) == 1 }

func findBin(name string) string {
	if p, err := exec.LookPath(name); err == nil {
		return p
	}
	for _, d := range []string{"/usr/bin/", "/usr/sbin/", "/bin/", "/usr/local/bin/"} {
		if st, err := os.Stat(d + name); err == nil && !st.IsDir() {
			return d + name
		}
	}
	return ""
}

func pactlEnv() []string {
	env := os.Environ()
	if _, err := os.Stat("/var/run/pulse/native"); err == nil {
		// Knulli's PipeWire-Pulse socket (same setup as batocera-audio)
		env = append(env, "XDG_RUNTIME_DIR=/var/run", "PULSE_SERVER=unix:/var/run/pulse/native")
	}
	return env
}

func pactlCmd(args ...string) *exec.Cmd {
	c := exec.Command(findBin("pactl"), args...)
	c.Env = pactlEnv()
	return c
}

// readSinkVolume returns the default sink volume in percent, or -1.
func readSinkVolume() int {
	if findBin("pactl") != "" {
		out, err := pactlCmd("get-sink-volume", "@DEFAULT_SINK@").Output()
		if err == nil {
			// "Volume: front-left: 42597 /  65% / -11.23 dB,   front-right: ..."
			s := string(out)
			if i := strings.Index(s, "%"); i > 0 {
				j := i
				for j > 0 && s[j-1] >= '0' && s[j-1] <= '9' {
					j--
				}
				if v, err := strconv.Atoi(s[j:i]); err == nil {
					return v
				}
			}
		}
	}
	if bin := findBin("batocera-audio"); bin != "" {
		out, err := exec.Command(bin, "getSystemVolume").Output()
		if err == nil {
			f := strings.Fields(string(out))
			if len(f) > 0 {
				if v, err := strconv.Atoi(strings.TrimSuffix(f[0], "%")); err == nil && v >= 0 {
					return v
				}
			}
		}
	}
	return -1
}

func defaultSink() (string, bool) {
	if out, err := pactlCmd("get-default-sink").Output(); err == nil {
		return strings.TrimSpace(string(out)), true
	}
	out, err := pactlCmd("info").Output()
	if err != nil {
		return "", false
	}
	for _, l := range strings.Split(string(out), "\n") {
		if strings.HasPrefix(l, "Default Sink:") {
			return strings.TrimSpace(strings.TrimPrefix(l, "Default Sink:")), true
		}
	}
	return "", true
}

// initSystemVolume returns the current system volume (0..1) or -1 if the
// console's volume can't be controlled (then the player uses software volume).
func initSystemVolume() float64 {
	if os.Getenv("MIYOOPOD_SOFT_VOLUME") == "1" {
		return -1
	}
	if findBin("pactl") == "" && findBin("batocera-audio") == "" {
		logMsg("System volume: no pactl/batocera-audio, using software volume")
		return -1
	}
	v := readSinkVolume()
	if v < 0 {
		logMsg("System volume: could not read the sink volume, using software volume")
		return -1
	}
	if v > 100 {
		v = 100
	}
	sysVol.ok = true
	sysVol.cur = v
	sysVol.bin = findBin("batocera-audio")
	go sysVol.writer()
	go sysVol.watch()
	return float64(v) / 100
}

// set queues a new volume (0..100); writes are coalesced in the background.
func (s *sysVolume) set(pct int) {
	s.mu.Lock()
	s.want = pct
	s.cur = pct
	s.lastSet = time.Now()
	s.mu.Unlock()
	select {
	case s.wake <- struct{}{}:
	default:
	}
}

func (s *sysVolume) writer() {
	for range s.wake {
		for {
			s.mu.Lock()
			v := s.want
			s.want = -1
			s.mu.Unlock()
			if v < 0 {
				break
			}
			if s.bin != "" {
				_ = exec.Command(s.bin, "setSystemVolume", strconv.Itoa(v)).Run()
			} else {
				_ = pactlCmd("set-sink-volume", "@DEFAULT_SINK@", strconv.Itoa(v)+"%").Run()
			}
			s.mu.Lock()
			s.lastSet = time.Now()
			s.mu.Unlock()
		}
	}
}

// watch follows "pactl subscribe" (falls back to polling every 2 s) and
// refreshes volume and Bluetooth state on every sink/server change.
func (s *sysVolume) watch() {
	go s.refresher()
	s.kick()
	fails := 0
	for fails < 5 {
		c := pactlCmd("subscribe")
		c.SysProcAttr = &syscall.SysProcAttr{Pdeathsig: syscall.SIGKILL} // never outlive the app
		out, err := c.StdoutPipe()
		if err == nil {
			err = c.Start()
		}
		if err != nil {
			fails++
			time.Sleep(2 * time.Second)
			continue
		}
		started := time.Now()
		buf := make([]byte, 4096)
		var line []byte
		for {
			n, err := out.Read(buf)
			for _, b := range buf[:n] {
				if b != '\n' {
					line = append(line, b)
					continue
				}
				l := string(line)
				line = line[:0]
				if strings.Contains(l, " on sink #") || strings.Contains(l, " on server") || strings.Contains(l, " on card") {
					s.kick()
				}
			}
			if err != nil {
				break
			}
		}
		_ = c.Wait()
		if time.Since(started) < 5*time.Second {
			fails++
		} else {
			fails = 0
		}
		time.Sleep(time.Second)
	}
	logMsg("System volume: pactl subscribe unavailable, polling")
	for {
		time.Sleep(2 * time.Second)
		s.kick()
	}
}

func (s *sysVolume) kick() {
	select {
	case s.events <- struct{}{}:
	default:
	}
}

func (s *sysVolume) refresher() {
	for range s.events {
		time.Sleep(60 * time.Millisecond) // coalesce bursts of events
		select {
		case <-s.events:
		default:
		}
		if sink, ok := defaultSink(); ok && os.Getenv("MIYOOPOD_BT_ICON") != "0" {
			on := int32(0)
			if strings.HasPrefix(sink, "bluez_output.") || strings.HasPrefix(sink, "bluez_sink.") {
				on = 1
			}
			if old := atomic.SwapInt32(&btState, on); old != on {
				logMsg("Audio output: " + sink)
				if old == 1 && on == 0 {
					atomic.StoreInt32(&btLost, 1)
				}
			}
		}
		v := readSinkVolume()
		if v < 0 {
			continue
		}
		if v > 100 {
			v = 100
		}
		s.mu.Lock()
		// ignore echoes of our own writes while the wheel is moving
		own := s.want >= 0 || time.Since(s.lastSet) < 700*time.Millisecond
		if !own && v != s.cur {
			s.cur = v
			atomic.StoreInt32(&s.changed, int32(v))
		}
		s.mu.Unlock()
	}
}

// takeChange returns a volume changed outside the app (0..100) or -1.
func (s *sysVolume) takeChange() int {
	return int(atomic.SwapInt32(&s.changed, -1))
}

// startBluetoothWatch: Bluetooth state comes from the same watcher; when the
// system volume isn't used, run the watcher just for the icon.
func startBluetoothWatch() {
	if os.Getenv("MIYOOPOD_BT_ICON") == "0" || sysVol.ok || findBin("pactl") == "" {
		return
	}
	go sysVol.watch()
}
