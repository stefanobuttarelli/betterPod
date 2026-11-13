//go:build ignore

package main

import (
	"golang.org/x/image/font"
	"golang.org/x/image/font/opentype"
	"golang.org/x/image/math/fixed"
	"math"
	"os"
	"path/filepath"
	"testing"
)

func TestInterKerningUsesPhysicalFaceSize(t *testing.T) {
	data, err := os.ReadFile(filepath.Join("..", "port", "betterpod", "assets", "fonts", "Inter-Regular.otf"))
	if err != nil {
		t.Fatal(err)
	}
	parsed, err := opentype.Parse(data)
	if err != nil {
		t.Fatal(err)
	}
	var previous fixed.Int26_6
	for _, size := range []float64{8, 12, 24} {
		primary, err := opentype.NewFace(parsed, &opentype.FaceOptions{Size: size * DPR, DPI: 72})
		if err != nil {
			t.Fatal(err)
		}
		f := &fbFace{pri: primary, priFont: parsed, w: W400, size: size, has: map[rune]bool{}}
		a, _ := parsed.GlyphIndex(&f.buf, 'A')
		v, _ := parsed.GlyphIndex(&f.buf, 'V')
		want, err := parsed.Kern(&f.buf, a, v, fixed.Int26_6(math.Round(size*DPR*64)), font.HintingNone)
		if err != nil {
			t.Fatal(err)
		}
		got := f.Kern('A', 'V')
		if got != want || got >= 0 {
			t.Fatalf("size %v: AV kerning %v, want %v", size, got, want)
		}
		if previous != 0 && got >= previous {
			t.Fatalf("larger text retained unscaled kerning: %v >= %v", got, previous)
		}
		previous = got
		primary.Close()
	}
}
