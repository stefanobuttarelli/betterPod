package main

import (
	"sort"
	"strconv"
	"strings"

	"golang.org/x/image/font"
)

const (
	lyricsLineHeight = 28
	lyricsPadX       = 24
	lyricsPadTop     = MENU_TOP_Y + 8
	lyricsAreaH      = SCREEN_HEIGHT - MENU_TOP_Y - STATUS_BAR_HEIGHT
	lyricsVisLines   = lyricsAreaH / lyricsLineHeight
)

// lrcLine represents a single timed lyric line.
type lrcLine struct {
	ts   float64 // timestamp in seconds
	text string
}

// lyricsDisplayLine is a single rendered row (after word-wrap).
type lyricsDisplayLine struct {
	text     string
	lrcIndex int // index into the lrcLine slice
}

// parseLRC detects LRC format and returns timed lines sorted by timestamp.
// Returns nil when no timestamps are found (plain-text fallback).
func parseLRC(raw string) []lrcLine {
	var timed []lrcLine
	hasTimestamp := false

	for _, rawLine := range strings.Split(raw, "\n") {
		rawLine = strings.TrimSpace(rawLine)
		if rawLine == "" {
			continue
		}

		rest := rawLine
		var timestamps []float64
		for strings.HasPrefix(rest, "[") {
			end := strings.Index(rest, "]")
			if end < 0 {
				break
			}
			tag := rest[1:end]
			rest = rest[end+1:]
			if ts, ok := parseLRCTimestamp(tag); ok {
				timestamps = append(timestamps, ts)
				hasTimestamp = true
			}
			// Metadata tags ([ti:…], [ar:…], etc.) are silently skipped.
		}

		text := strings.TrimSpace(rest)
		if len(timestamps) == 0 {
			continue
		}
		for _, ts := range timestamps {
			timed = append(timed, lrcLine{ts: ts, text: text})
		}
	}

	if !hasTimestamp {
		return nil
	}

	sort.Slice(timed, func(i, j int) bool { return timed[i].ts < timed[j].ts })
	return timed
}

// parseLRCTimestamp parses "mm:ss.xx" or "mm:ss" → seconds.
func parseLRCTimestamp(s string) (float64, bool) {
	col := strings.Index(s, ":")
	if col < 0 {
		return 0, false
	}
	mm, err := strconv.Atoi(strings.TrimSpace(s[:col]))
	if err != nil {
		return 0, false
	}
	sec, err := strconv.ParseFloat(strings.TrimSpace(s[col+1:]), 64)
	if err != nil {
		return 0, false
	}
	return float64(mm)*60 + sec, true
}

// activeLRCIndex returns the index of the lrcLine active at pos seconds (-1 if before first).
func activeLRCIndex(lines []lrcLine, pos float64) int {
	active := -1
	for i, l := range lines {
		if l.ts <= pos {
			active = i
		} else {
			break
		}
	}
	return active
}

// wrapLyrics splits raw lyrics text into display lines no wider than maxWidth,
// preserving original line breaks.
func wrapLyrics(text string, maxWidth float64, face font.Face, measure func(string, font.Face) float64) []string {
	var lines []string
	for _, rawLine := range strings.Split(text, "\n") {
		rawLine = strings.TrimRight(rawLine, "\r")
		if rawLine == "" {
			lines = append(lines, "")
			continue
		}
		words := strings.Fields(rawLine)
		if len(words) == 0 {
			lines = append(lines, "")
			continue
		}
		current := words[0]
		for _, word := range words[1:] {
			candidate := current + " " + word
			if measure(candidate, face) <= maxWidth {
				current = candidate
			} else {
				lines = append(lines, current)
				current = word
			}
		}
		lines = append(lines, current)
	}
	return lines
}

// buildLyricsCache parses and word-wraps lyrics for the current track.
// No-op if the cache is already valid for this track.
func (app *MiyooPod) buildLyricsCache() {
	track := app.Playing.Track
	if track == nil || app.LyricsCachedTrack == track.Path {
		return
	}

	app.LyricsCachedTrack = track.Path
	app.LyricsCachedLRC = nil
	app.LyricsCachedDisplay = nil
	app.LyricsPlainLines = nil

	maxWidth := float64(SCREEN_WIDTH - lyricsPadX*2 - 10)

	lrcLines := parseLRC(track.Lyrics)
	if lrcLines != nil {
		app.LyricsCachedLRC = lrcLines
		var display []lyricsDisplayLine
		for i, l := range lrcLines {
			if l.text == "" {
				display = append(display, lyricsDisplayLine{"", i})
				continue
			}
			for _, wl := range wrapLyrics(l.text, maxWidth, app.FontMenu, app.measureString) {
				display = append(display, lyricsDisplayLine{wl, i})
			}
		}
		app.LyricsCachedDisplay = display
	} else {
		app.LyricsPlainLines = wrapLyrics(track.Lyrics, maxWidth, app.FontMenu, app.measureString)
	}
}

// drawLyricsScreen renders the lyrics screen.
// Mirrors the menu's full-redraw approach: dc.Clear() + header + content + status bar.
// The marquee suppression (LastKey != NONE guard in pollMarquee) prevents conflicts
// during key-repeat scroll, same as the menu screen.
func (app *MiyooPod) drawLyricsScreen() {
	dc := app.DC
	track := app.Playing.Track

	dc.SetHexColor(app.CurrentTheme.BG)
	dc.Clear()

	app.drawHeader("Lyrics")

	if track == nil || track.Lyrics == "" {
		dc.SetFontFace(app.FontMenu)
		dc.SetHexColor(app.CurrentTheme.Dim)
		dc.DrawStringAnchored("No lyrics available", SCREEN_WIDTH/2, SCREEN_HEIGHT/2, 0.5, 0.5)
		app.drawStatusBar()
		app.triggerRefresh()
		return
	}

	app.buildLyricsCache()

	if app.LyricsCachedLRC != nil {
		app.drawLRCLines()
	} else {
		app.drawPlainLines()
	}

	app.drawStatusBar()
	app.triggerRefresh()
}

// drawLRCLines renders timed LRC lyrics with auto-scroll and active-line highlight.
func (app *MiyooPod) drawLRCLines() {
	dc := app.DC
	display := app.LyricsCachedDisplay

	pos := 0.0
	if app.Playing != nil {
		pos = app.Playing.Position
	}
	activeLRC := activeLRCIndex(app.LyricsCachedLRC, pos)

	// Find first display line for the active lrc entry.
	activeDisplay := -1
	if activeLRC >= 0 {
		for i, dl := range display {
			if dl.lrcIndex == activeLRC {
				activeDisplay = i
				break
			}
		}
	}

	// Auto-scroll: keep active line roughly centered unless user scrolled manually.
	if activeDisplay >= 0 && !app.LyricsManualScroll {
		target := activeDisplay - lyricsVisLines/2
		if target < 0 {
			target = 0
		}
		app.LyricsScrollOffset = target
	}

	// Clamp.
	maxOff := len(display) - lyricsVisLines
	if maxOff < 0 {
		maxOff = 0
	}
	if app.LyricsScrollOffset > maxOff {
		app.LyricsScrollOffset = maxOff
	}
	if app.LyricsScrollOffset < 0 {
		app.LyricsScrollOffset = 0
	}

	dc.SetFontFace(app.FontMenu)
	end := app.LyricsScrollOffset + lyricsVisLines
	if end > len(display) {
		end = len(display)
	}
	for i, dl := range display[app.LyricsScrollOffset:end] {
		if dl.text == "" {
			continue
		}
		y := float64(lyricsPadTop + i*lyricsLineHeight + lyricsLineHeight/2)
		if dl.lrcIndex == activeLRC {
			dc.SetHexColor(app.CurrentTheme.Progress)
		} else {
			dc.SetHexColor(app.CurrentTheme.ItemTxt)
		}
		dc.DrawStringAnchored(dl.text, float64(lyricsPadX), y, 0, 0.5)
	}

	app.drawLyricsScrollbar(len(display), maxOff)
}

// drawPlainLines renders plain-text (non-LRC) lyrics.
func (app *MiyooPod) drawPlainLines() {
	dc := app.DC
	lines := app.LyricsPlainLines

	maxOff := len(lines) - lyricsVisLines
	if maxOff < 0 {
		maxOff = 0
	}
	if app.LyricsScrollOffset > maxOff {
		app.LyricsScrollOffset = maxOff
	}
	if app.LyricsScrollOffset < 0 {
		app.LyricsScrollOffset = 0
	}

	dc.SetFontFace(app.FontMenu)
	end := app.LyricsScrollOffset + lyricsVisLines
	if end > len(lines) {
		end = len(lines)
	}
	for i, line := range lines[app.LyricsScrollOffset:end] {
		if line == "" {
			continue
		}
		y := float64(lyricsPadTop + i*lyricsLineHeight + lyricsLineHeight/2)
		dc.SetHexColor(app.CurrentTheme.ItemTxt)
		dc.DrawStringAnchored(line, float64(lyricsPadX), y, 0, 0.5)
	}

	app.drawLyricsScrollbar(len(lines), maxOff)
}

// drawLyricsScrollbar draws the right-edge scroll indicator.
func (app *MiyooPod) drawLyricsScrollbar(totalLines, maxOff int) {
	dc := app.DC
	if totalLines <= lyricsVisLines {
		return
	}
	barH := float64(lyricsAreaH)
	thumbH := barH * float64(lyricsVisLines) / float64(totalLines)
	thumbY := float64(lyricsPadTop)
	if maxOff > 0 {
		thumbY += (barH - thumbH) * float64(app.LyricsScrollOffset) / float64(maxOff)
	}

	dc.SetHexColor(app.CurrentTheme.ProgBG)
	dc.DrawRectangle(SCREEN_WIDTH-6, float64(lyricsPadTop), 4, barH)
	dc.Fill()

	dc.SetHexColor(app.CurrentTheme.Progress)
	dc.DrawRectangle(SCREEN_WIDTH-6, thumbY, 4, thumbH)
	dc.Fill()
}

// handleLyricsKey handles input on the lyrics screen.
func (app *MiyooPod) handleLyricsKey(key Key) {
	switch key {
	case UP:
		if app.LyricsScrollOffset > 0 {
			app.LyricsScrollOffset--
			app.LyricsManualScroll = true
			app.drawLyricsScreen()
		}
	case DOWN:
		app.LyricsScrollOffset++
		app.LyricsManualScroll = true
		app.drawLyricsScreen()
	case A:
		// Re-enable auto-follow
		app.LyricsManualScroll = false
		app.drawLyricsScreen()
	case B, START:
		app.setScreen(ScreenNowPlaying)
		app.LyricsScrollOffset = 0
		app.LyricsManualScroll = false
		app.drawCurrentScreen()
	}
}

// measureString returns the pixel width of s rendered in face.
func (app *MiyooPod) measureString(s string, face font.Face) float64 {
	app.DC.SetFontFace(face)
	w, _ := app.DC.MeasureString(s)
	return w
}
