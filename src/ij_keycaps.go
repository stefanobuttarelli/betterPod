package main

// ij_keycaps.go — buttons named in text are drawn as buttons: "[A]" becomes
// an A in a circle (A, B, X, Y: the round face buttons), "[L]", "[R]",
// "[L2]", "[R2]", "[START]" and "[SELECT]" a rounded rectangle, "[DPAD]" a
// little d-pad, and "[↑]" "[↓]" "[←]" "[→]" "[◀]" "[▶]" an arrow in a
// rounded square. drawText and textWidth do it for any text, so a hint
// only has to write "[A] receive".

import (
	"image"
	"strings"

	"golang.org/x/image/font"
)

type keyShape int

const (
	keyRound keyShape = iota
	keyPill
	keySquare
	keyDpad
)

var keyTokens = map[string]keyShape{
	"A": keyRound, "B": keyRound, "X": keyRound, "Y": keyRound,
	"L": keyPill, "R": keyPill, "L2": keyPill, "R2": keyPill, "START": keyPill, "SELECT": keyPill,
	"↑": keySquare, "↓": keySquare, "←": keySquare, "→": keySquare, "◀": keySquare, "▶": keySquare, "▲": keySquare, "▼": keySquare,
	"DPAD": keyDpad,
}

type textPart struct {
	s   string
	key bool
}

// keyParts splits s into text and keys (nil when there are none).
func keyParts(s string) []textPart {
	if strings.IndexByte(s, '[') < 0 {
		return nil
	}
	var parts []textPart
	found := false
	rest := s
	for {
		i := strings.IndexByte(rest, '[')
		if i < 0 {
			break
		}
		j := strings.IndexByte(rest[i:], ']')
		if j < 0 {
			break
		}
		tok := rest[i+1 : i+j]
		if _, ok := keyTokens[tok]; !ok {
			parts = append(parts, textPart{s: rest[:i+1]})
			rest = rest[i+1:]
			continue
		}
		if i > 0 {
			parts = append(parts, textPart{s: rest[:i]})
		}
		parts = append(parts, textPart{s: tok, key: true})
		found = true
		rest = rest[i+j+1:]
	}
	if !found {
		return nil
	}
	if rest != "" {
		parts = append(parts, textPart{s: rest})
	}
	return parts
}

// keyMetrics: a key's height, and the size of its label, for text in f.
func keyMetrics(f font.Face) (h, labelCSS float64) {
	sz := faceSize(f)
	if sz == 0 {
		sz = float64(f.Metrics().Ascent) / 64 / DPR / 0.97
	}
	cap := lineCapHeight(sz) * DPR
	return cap * 1.85, sz * 0.62
}

func keyWidth(f font.Face, tok string) float64 {
	h, ls := keyMetrics(f)
	switch keyTokens[tok] {
	case keyRound, keySquare, keyDpad:
		return h
	}
	lf := face(W700, ls)
	return textWidth(lf, tok) + h*0.7
}

func textWidthKeys(f font.Face, parts []textPart) float64 {
	w := 0.0
	for _, p := range parts {
		if p.key {
			w += keyWidth(f, p.s)
		} else {
			w += float64(font.MeasureString(f, p.s)) / 64
		}
	}
	return w
}

func drawTextKeys(dst *image.RGBA, f font.Face, parts []textPart, x, baseline float64, c Col) {
	h, ls := keyMetrics(f)
	cap := h / 1.85
	cy := baseline - cap/2
	for _, p := range parts {
		if !p.key {
			drawTextPlain(dst, f, p.s, x, baseline, c)
			x += float64(font.MeasureString(f, p.s)) / 64
			continue
		}
		w := keyWidth(f, p.s)
		drawKeyCap(dst, p.s, x, cy, w, h, ls, c)
		x += w
	}
}

// drawKeyCap: one button, its middle at cy.
func drawKeyCap(dst *image.RGBA, tok string, x, cy, w, h, labelCSS float64, c Col) {
	y0, y1 := cy-h/2, cy+h/2
	fill := Col{c.R, c.G, c.B, c.A * 0.14}
	edge := Col{c.R, c.G, c.B, c.A * 0.85}
	bw := maxf(1, h/12)
	switch keyTokens[tok] {
	case keyRound:
		fillRoundedGradient(dst, x, y0, x+w, y1, h/2, []GradStop{{0, fill}, {1, fill}}, 1)
		strokeRounded(dst, x, y0, x+w, y1, h/2, bw, edge, 1)
	case keyPill:
		fillRoundedGradient(dst, x, y0, x+w, y1, h*0.32, []GradStop{{0, fill}, {1, fill}}, 1)
		strokeRounded(dst, x, y0, x+w, y1, h*0.32, bw, edge, 1)
	case keySquare:
		fillRoundedGradient(dst, x, y0, x+w, y1, h*0.24, []GradStop{{0, fill}, {1, fill}}, 1)
		strokeRounded(dst, x, y0, x+w, y1, h*0.24, bw, edge, 1)
	case keyDpad:
		// a plus: two crossed bars
		t := h * 0.36
		cx := x + w/2
		fillRoundedGradient(dst, cx-t/2, y0, cx+t/2, y1, t*0.25, []GradStop{{0, edge}, {1, edge}}, 1)
		fillRoundedGradient(dst, x, cy-t/2, x+w, cy+t/2, t*0.25, []GradStop{{0, edge}, {1, edge}}, 1)
		return
	}
	lf := face(W700, labelCSS)
	if tok == "START" || tok == "SELECT" || len(tok) == 2 {
		lf = face(W700, labelCSS*0.86)
	}
	lw := float64(font.MeasureString(lf, tok)) / 64
	drawTextPlain(dst, lf, tok, x+(w-lw)/2, cy+lineCapHeight(faceSize(lf))*DPR/2, edge)
}

func maxf(a, b float64) float64 {
	if a > b {
		return a
	}
	return b
}
