package main

// ij_volkeys.go — the console's volume keys, read straight from the input
// devices that have them (gpio-keys on the H700).
//
// Knulli itself changes the volume when they are pressed; KnulliPod only
// watches, so the device is opened read-only and never grabbed. A press
// shows the volume overlay at once (also at 0% or 100%, when the volume does
// not change) and asks the volume watcher to read the new level, so the
// overlay does not depend on PipeWire reporting the change.

import (
	"encoding/binary"
	"os"
	"path/filepath"
	"sync/atomic"
	"syscall"
	"time"
	"unsafe"
)

const (
	evKey      = 0x01
	keyVolDown = 114
	keyVolUp   = 115
)

// volKeyPressed is set by the reader on a volume key press and taken by the
// UI loop.
var volKeyPressed int32

func volKeyTake() bool { return atomic.SwapInt32(&volKeyPressed, 0) == 1 }

// hasVolumeKeys asks the device (EVIOCGBIT for EV_KEY) whether it reports
// KEY_VOLUMEUP or KEY_VOLUMEDOWN.
func hasVolumeKeys(f *os.File) bool {
	var bits [96]byte // KEY_MAX / 8 + 1
	const ioRead = 2
	req := uintptr(ioRead<<30 | len(bits)<<16 | 'E'<<8 | (0x20 + evKey))
	_, _, e := syscall.Syscall(syscall.SYS_IOCTL, f.Fd(), req, uintptr(unsafe.Pointer(&bits[0])))
	if e != 0 {
		return false
	}
	has := func(k int) bool { return bits[k/8]&(1<<(k%8)) != 0 }
	return has(keyVolUp) || has(keyVolDown)
}

// startVolumeKeyWatch starts a reader for every input device with volume
// keys.
func startVolumeKeyWatch() {
	if os.Getenv("MIYOOPOD_VOLUME_KEYS") == "0" {
		return
	}
	paths, _ := filepath.Glob("/dev/input/event*")
	n := 0
	for _, p := range paths {
		f, err := os.OpenFile(p, os.O_RDONLY, 0)
		if err != nil {
			continue
		}
		if !hasVolumeKeys(f) {
			f.Close()
			continue
		}
		n++
		logMsg("Volume keys: watching " + p)
		go readVolumeKeys(f)
	}
	if n == 0 {
		logMsg("Volume keys: no input device with volume keys (overlay follows the system volume only)")
	}
}

// readVolumeKeys reads input_event records (64-bit layout: 16-byte time,
// type, code, value) and flags volume key presses and repeats.
func readVolumeKeys(f *os.File) {
	defer f.Close()
	size := 24
	if unsafe.Sizeof(uintptr(0)) == 4 {
		size = 16
	}
	buf := make([]byte, size*16)
	for {
		n, err := f.Read(buf)
		if err != nil {
			time.Sleep(time.Second)
			continue
		}
		for o := 0; o+size <= n; o += size {
			ev := buf[o+size-8:]
			typ := binary.LittleEndian.Uint16(ev[0:2])
			code := binary.LittleEndian.Uint16(ev[2:4])
			val := int32(binary.LittleEndian.Uint32(ev[4:8]))
			if typ == evKey && (code == keyVolUp || code == keyVolDown) && val != 0 {
				atomic.StoreInt32(&volKeyPressed, 1)
				// the system changes the volume right after: read it
				time.AfterFunc(120*time.Millisecond, sysVol.kick)
				time.AfterFunc(400*time.Millisecond, sysVol.kick)
			}
		}
	}
}
