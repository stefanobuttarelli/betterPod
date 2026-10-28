package main

// ij_movemusic_view.go — the Moving Music screen (ij_musicdir.go does the
// work): the albums move from one card's pile to the other's, the one
// being copied travelling across on an arc as its files are copied (its
// place is how much of it is copied; a small album still takes half a
// second, and the screen catches up when it is far behind); then each one
// is checked, lifted above the middle; then they open into a line.

import (
	"fmt"
	"image"
	"math"
	"strconv"
	"strings"
	"time"

	"golang.org/x/image/font"
)

type moveView struct {
	job       *moveJob
	shownUnit int     // the album the screen shows travelling
	shownFrac float64 // and how far
	last      time.Time
	other     *Album // the stand-in of "Other files"
}

// moveSnap: what the screen needs of the job, read at once.
type moveSnap struct {
	phase, file, err, fromName, toName string
	units                              []*moveUnit
	cur, files, checked, songs         int
	unitDone, doneB, all               int64
	speed                              float64
	move                               bool
}

func (m *moveView) snap() moveSnap {
	j := m.job
	j.mu.Lock()
	defer j.mu.Unlock()
	return moveSnap{j.phase, j.file, j.err, j.fromName, j.toName, j.units, j.cur, j.files, j.checked, j.songs,
		j.unitDone, j.doneB, j.all, j.speed, j.move}
}

func moveRunning(phase string) bool {
	switch phase {
	case "count", "copy", "paused", "verify", "delete":
		return true
	}
	return false
}

func (m *moveView) Animating(now time.Time) bool {
	s := m.snap()
	return moveRunning(s.phase) || m.shownUnit < s.cur
}

func (m *moveView) OnScroll(ui *IpodUI, dir int, velocity float64) {}

// OnCenter: A closes it once it has ended.
func (m *moveView) OnCenter(ui *IpodUI) {
	if !moveRunning(m.snap().phase) {
		ui.hideView()
	}
}

// OnMenu: B asks before stopping (nothing copied is lost, nothing deleted).
func (m *moveView) OnMenu(ui *IpodUI) {
	if !moveRunning(m.snap().phase) {
		ui.hideView()
		return
	}
	ui.showSheetTitled("Stop moving the music? What's copied stays; nothing is deleted.", []SheetOption{
		{Label: "Stop", Danger: true, OnSelect: func() {
			select {
			case <-m.job.stop:
			default:
				close(m.job.stop)
			}
		}},
		{Label: "Hide (it goes on)", OnSelect: func() { ui.hideView() }},
	})
}

func (m *moveView) album(s moveSnap, i int) *Album {
	if i < 0 || i >= len(s.units) {
		return nil
	}
	if a := s.units[i].album; a != nil {
		return a
	}
	if m.other == nil {
		m.other = &Album{Name: "Other files"}
	}
	return m.other
}

// sdIcon: a little memory card as tall as a capital letter of the 12 px
// label (h CSS px), its bottom on the baseline y.
func sdIcon(dst *image.RGBA, x, y, h float64, c Col) {
	d := DPR
	w := h * 0.78
	top := y - h
	fillRoundedGradient(dst, x*d, top*d, (x+w)*d, y*d, 1.2*d, []GradStop{{0, c}, {1, c}}, 1)
	cut := w * 0.34
	fillTriangle(dst, (x+w-cut)*d, top*d-1, (x+w)*d+1, top*d-1, (x+w)*d+1, (top+cut)*d, th.bg)
	for i := 0; i < 3; i++ {
		cx := x + w*0.16 + float64(i)*w*0.2
		fillRect(dst, int(cx*d), int((top+h*0.14)*d), max(1, int(w*0.11*d)), int(h*0.26*d), th.bg)
	}
}

func (m *moveView) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	s := m.snap()
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	fillRect(dst, 0, 0, W, H, th.bg)
	wC := float64(W) / DPR
	center := func(f font.Face, t string, cx, y float64, c Col) {
		t = ellipsize(f, t, (wC-40)*DPR)
		drawText(dst, f, t, cx*DPR-textWidth(f, t)/2, y*DPR, c)
	}
	_, a12 := lineBox(12)
	_, a11 := lineBox(11)
	gb := func(b int64) string { return fmt.Sprintf("%.1f GB", float64(b)/1e9) }
	n := len(s.units)
	verb := "moved"
	if !s.move {
		verb = "copied"
	}
	switch {
	case s.phase == "count":
		center(face(W700, 14), "Counting the files…", wC/2, 90+a12, th.text)
		center(face(W500, 11), "Then they're copied, checked, and only then deleted here", wC/2, 110+a11, th.sub)
		return
	case s.phase == "failed" || s.phase == "stopped":
		title, sub := "Not moved", s.err
		if s.phase == "stopped" {
			title, sub = "Stopped", "What's copied stays on "+s.toName+"; nothing was deleted. Move again to go on."
		}
		center(face(W700, 15), title, wC/2, 80+a12, hex("#d42a1e"))
		f := face(W500, 12)
		for i, l := range wrapLines(f, sub, (wC-50)*DPR, 3) {
			center(f, l, wC/2, 102+float64(i)*16+a12, th.sub)
		}
		center(face(W500, 11), "The music is still where it was", wC/2, 156+a11, th.sub)
		drawJobButton(dst, th, "Close", false)
		return
	case s.phase == "done":
		mid := wC / 2
		for k := 3; k >= 0; k-- {
			for _, sgn := range []int{-1, 1} {
				if k == 0 && sgn == 1 || n == 0 {
					continue
				}
				i := ((n/2+sgn*k)%n + n) % n
				sz := 84.0 - float64(k)*12
				x := mid + float64(sgn*k)*46 - sz/2
				y := 22 + (84-sz)/2
				op := 1 - 0.18*float64(k)
				r := ui.thumb(m.album(s, i), sz)
				fillRect(dst, px(x)-1, px(y)-1, px(sz)+2, px(sz)+2, th.artBorder)
				blit(dst, r, px(x), px(y), op)
				rh := px(sz * 0.25)
				for yy := 0; yy < rh; yy++ {
					row := r.SubImage(image.Rect(0, r.Bounds().Dy()-1-yy, r.Bounds().Dx(), r.Bounds().Dy()-yy)).(*image.RGBA)
					blit(dst, row, px(x), px(y+sz)+2+yy, op*0.22*(1-float64(yy)/float64(rh)))
				}
			}
		}
		songs := strconv.Itoa(s.songs) + " songs"
		if s.songs == 0 {
			songs = strconv.Itoa(s.files) + " files"
		}
		center(face(W700, 15), fmt.Sprintf("All %s %s to %s", commas(songs), verb, s.toName), wC/2, 140+a12, th.text)
		center(face(W500, 12), fmt.Sprintf("%s · %d %s · checked", gb(s.all), n, pluralize(n, "folder", "folders")), wC/2, 158+a12, th.sub)
		drawJobButton(dst, th, "Done", true)
		return
	}
	// the album shown travelling: it follows the copy, at least half a
	// second per album, and jumps ahead when far behind
	dt := now.Sub(m.last).Seconds()
	if m.last.IsZero() || dt > 0.2 {
		dt = 0.2
	}
	m.last = now
	if s.phase == "copy" || s.phase == "paused" {
		if s.cur-m.shownUnit > 3 {
			m.shownUnit, m.shownFrac = s.cur-1, 0.5
		}
		target := 1.0
		if m.shownUnit == s.cur && n > 0 {
			target = float64(s.unitDone) / math.Max(1, float64(s.units[s.cur].bytes))
		}
		if m.shownFrac < target {
			m.shownFrac = math.Min(target, m.shownFrac+math.Max((target-m.shownFrac)*dt*6, dt*0.6))
			if m.shownUnit < s.cur {
				m.shownFrac = math.Min(1, m.shownFrac+dt*2) // landing: quick
			}
		}
		if m.shownUnit < s.cur && m.shownFrac >= 1 {
			m.shownUnit, m.shownFrac = m.shownUnit+1, 0
		}
	} else {
		m.shownUnit = s.cur
	}
	const cs = 56.0
	lx, rx, py := 34.0, wC-34-cs, 34.0
	pile := func(x float64, from, to int, badge bool) {
		k := to - from
		show := min(k, 4)
		for j := 0; j < show; j++ {
			i := to - show + j
			off := float64(show-1-j) * 3
			yy := py + off
			fillRect(dst, px(x)+px(2), px(yy)+px(3), px(cs), px(cs), Col{0, 0, 0, 0.12})
			fillRect(dst, px(x)-1, px(yy)-1, px(cs)+2, px(cs)+2, th.artBorder)
			blit(dst, ui.thumb(m.album(s, i), cs), px(x), px(yy), 1)
			if badge && j == show-1 {
				fillRoundedGradient(dst, (x+cs-15)*DPR, (yy+cs-15)*DPR, (x+cs+3)*DPR, (yy+cs+3)*DPR, 9*DPR,
					[]GradStop{{0, hex("#5fd36a")}, {1, hex("#2c9f3a")}}, 1)
				f := face(W700, 11)
				drawText(dst, f, "✓", (x+cs-6)*DPR-textWidth(f, "✓")/2, (yy+cs-6+a11/2)*DPR, colWhite)
			}
		}
	}
	label := func(cx float64, card, line string) {
		f := face(W700, 12)
		tw := textWidth(f, card) / DPR
		ch := lineCapHeight(12)
		iw := ch * 0.78
		x0 := cx - (tw+iw+4)/2
		sdIcon(dst, x0, py+cs+12+a12, ch, th.text)
		drawText(dst, f, card, (x0+iw+4)*DPR, (py+cs+12+a12)*DPR, th.text)
		center(face(W500, 11), line, cx, py+cs+30+a11, th.sub)
	}
	cur := min(m.shownUnit, n-1)
	switch s.phase {
	case "verify":
		pile(lx, cur+1, n, false)
		pile(rx, 0, cur, true)
		label(lx+cs/2, s.toName, fmt.Sprintf("%d to check", max(0, s.files-s.checked)))
		label(rx+cs/2, s.toName, fmt.Sprintf("%d checked", s.checked))
		x, y, sz := wC/2-32, py-4, 64.0
		fillRect(dst, px(x)+px(3), px(y)+px(5), px(sz), px(sz), Col{0, 0, 0, 0.16})
		blit(dst, ui.thumb(m.album(s, cur), sz), px(x), px(y), 1)
		strokeRounded(dst, x*DPR-3, y*DPR-3, (x+sz)*DPR+3, (y+sz)*DPR+3, 4, 3, hex("#3aa0ff"), 1)
	case "delete":
		pile(rx, 0, n, true)
		label(lx+cs/2, s.fromName, "removing the originals")
		label(rx+cs/2, s.toName, gb(s.all)+" arrived")
	default:
		pile(lx, cur+1, n, false)
		pile(rx, 0, cur, false)
		label(lx+cs/2, s.fromName, gb(s.all-s.doneB)+" to move")
		label(rx+cs/2, s.toName, gb(s.doneB)+" arrived")
		if n > 0 {
			e := m.shownFrac * m.shownFrac * (3 - 2*m.shownFrac)
			size := cs * (1 + 0.25*math.Sin(math.Pi*e))
			x := lx + (rx-lx)*e + cs/2 - size/2
			y := py - 16*math.Sin(math.Pi*e) + cs/2 - size/2
			if s.phase == "paused" {
				x += 2 * math.Sin(float64(now.UnixMilli())/40)
			}
			fillRect(dst, px(x)+px(3), px(y)+px(5), px(size), px(size), Col{0, 0, 0, 0.16})
			fillRect(dst, px(x)-1, px(y)-1, px(size)+2, px(size)+2, th.artBorder)
			blit(dst, ui.thumb(m.album(s, cur), size), px(x), px(y), 1)
		}
	}
	ty := 146.0
	switch s.phase {
	case "verify":
		center(face(W700, 13), fmt.Sprintf("Checking %s · %d of %d files", m.album(s, cur).Name, s.checked, s.files), wC/2, ty+a12, th.text)
		center(face(W500, 11), "Each file is read back from "+s.toName+" and compared", wC/2, ty+17+a11, th.sub)
	case "delete":
		center(face(W700, 13), fmt.Sprintf("Removing the originals · %d of %d", s.checked, s.files), wC/2, ty+a12, th.text)
		center(face(W500, 11), "All checked: the copies on "+s.toName+" are good", wC/2, ty+17+a11, th.sub)
	case "paused":
		center(face(W700, 13), "Paused · put the card back", wC/2, ty+a12, hex("#d42a1e"))
		center(face(W500, 11), "It goes on from here; nothing is lost", wC/2, ty+17+a11, th.sub)
	default:
		a := m.album(s, cur)
		t := a.Name
		if a.Artist != "" {
			t += " · " + a.Artist
		}
		center(face(W700, 13), t, wC/2, ty+a12, th.text)
		center(face(W500, 11), s.file, wC/2, ty+17+a11, th.sub)
	}
	bw := wC - 80
	pct := float64(s.doneB) / math.Max(1, float64(s.all))
	if s.phase == "verify" || s.phase == "delete" {
		pct = float64(s.checked) / math.Max(1, float64(s.files))
	}
	ui.drawProgressBar(dst, 40, 180, bw, pct*100, false)
	line := fmt.Sprintf("%d%%", int(pct*100+0.5))
	if s.phase == "copy" && s.speed > 0 {
		left := float64(s.all-s.doneB) / s.speed
		line += " · " + moveETA(left) + " · " + fmt.Sprintf("%.0f MB/s", s.speed/1e6)
	}
	center(face(W700, 12), line, wC/2, 202+a12, th.sub)
}

func moveETA(sec float64) string {
	switch {
	case sec < 60:
		return "less than a minute left"
	case sec < 3600:
		return fmt.Sprintf("%d min left", int(sec/60+0.5))
	}
	return fmt.Sprintf("%d h %d min left", int(sec/3600), int(math.Mod(sec, 3600)/60))
}

// commas: "1284 songs" → "1,284 songs".
func commas(s string) string {
	num, rest, _ := strings.Cut(s, " ")
	var b strings.Builder
	for i, c := range num {
		if i > 0 && (len(num)-i)%3 == 0 {
			b.WriteByte(',')
		}
		b.WriteRune(c)
	}
	if rest != "" {
		b.WriteString(" " + rest)
	}
	return b.String()
}
