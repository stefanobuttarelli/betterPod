package main

// ij_fonts.go — cached Inter faces at CSS weights/sizes and ipod.js line metrics.

import (
	"fmt"
	"math"
	"os"
	"sync"

	"golang.org/x/image/font"
	"golang.org/x/image/font/opentype"
	"golang.org/x/image/font/sfnt"
)

type FontWeight int

const (
	W400 FontWeight = 400
	W500 FontWeight = 500
	W600 FontWeight = 600
	W700 FontWeight = 700
	W900 FontWeight = 900
)

var fontFiles = map[FontWeight]string{
	W400: "Inter-Regular.otf",
	W500: "Inter-Medium.otf",
	W600: "Inter-SemiBold.otf",
	W700: "Inter-Bold.otf",
	W900: "Inter-Black.otf",
}

// fontFiles maps each CSS weight to its Inter file (assets/fonts); a face
// is built per (weight, size) and kept: the UI uses a handful of sizes.
type faceKey struct {
	w    FontWeight
	size float64
}

// fontMu guards both caches (the loading screen builds faces too).
var fontMu sync.Mutex

// parsed fonts by weight, and built faces by (weight, size)
var fontCache = map[FontWeight]*sfnt.Font{}
var faceCache = map[faceKey]font.Face{}

var fontDir = "./assets/fonts/"
// face returns the Inter face for weight w at CSS size cssSize (device px are
// cssSize*DPR). Parsed fonts and built faces are both cached.
func face(w FontWeight, cssSize float64) font.Face {
	fontMu.Lock()
	defer fontMu.Unlock()
	k := faceKey{w, cssSize}
	if fc, ok := faceCache[k]; ok {
		return fc
	}
	f, ok := fontCache[w]
	if !ok {
		data, err := os.ReadFile(fontDir + fontFiles[w])
		if err != nil {
			panic(fmt.Sprintf("font %s: %v", fontFiles[w], err))
		}
		f, err = opentype.Parse(data)
		if err != nil {
			panic(fmt.Sprintf("font %s: %v", fontFiles[w], err))
		}
		fontCache[w] = f
	}
	fc, _ := opentype.NewFace(f, &opentype.FaceOptions{Size: cssSize * DPR, DPI: 72})
	// remember the CSS size each face was built at (keyed by the face
	// value itself, so drawText callers can recover it)
	//
	faceCache[k] = fc
	faceSizes[fc] = cssSize
	return fc
}

var faceSizes = map[font.Face]float64{}

// sizedFace is face by another name, kept for call sites that compute
// sizes before asking for a face (no scaling of its own).
func sizedFace(w FontWeight, cssSize float64) font.Face { return face(w, cssSize) }

// faceSize returns the CSS size a face was built at.
func faceSize(f font.Face) float64 {
	fontMu.Lock()
	defer fontMu.Unlock()
	return faceSizes[f]
}

// lineBox returns the line-box height and baseline offset (CSS px) for text
// at the given size, from Inter's ascent and descent metrics.
func lineBox(size float64) (float64, float64) {
	a := math.Round(size*0.96875*DPR) / DPR

	h := math.Round(size*0.96875) + math.Round(size*0.2412109375)

	return h, a + baselineTweak[size]/DPR
}

// baselineTweak nudges the baseline for sizes that would otherwise look off
// (CSS px, before DPR): 14 px text sits a pixel too low in the list rows.
var baselineTweak = map[float64]float64{
	14: -1,
}
