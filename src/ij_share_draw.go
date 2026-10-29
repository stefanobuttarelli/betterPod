package main

// ij_share_draw.go — Share's screens: a white studio with a glossy floor,
// acrylic pedestals (share_ped_*.png, share_beam_*.png), the cover with its
// laser, white digit tiles and Aqua bars and buttons. (Device pixels.)

import (
	"fmt"
	"image"
	"math"
	"strings"
	"time"
)

const (
	shFloor = 332.0 // the floor line
	shTop   = 292.0 // a pedestal's top (its disc's centre)
	shCX    = 180.0 // the pedestal's centre when there is text at the right
	shRX    = 468.0 // the text's centre
)

var (
	shInk   = Col{40, 44, 52, 1}
	shSub   = Col{110, 118, 130, 1}
	shBody  = Col{70, 76, 88, 1}
	shBlue  = Col{28, 112, 218, 1}
	shGreen = Col{40, 150, 70, 1}
)

func (s *ShareView) Render(ui *IpodUI, v *View, dst *image.RGBA, now time.Time) {
	W, H := dst.Bounds().Dx(), dst.Bounds().Dy()
	if s.bg == nil || s.bg.Bounds().Dx() != W || s.bg.Bounds().Dy() != H {
		s.bg = newCanvas(W, H)
		fillVGradient(s.bg, 0, 0, W, int(shFloor), []GradStop{{0, Col{250, 251, 252, 1}}, {1, Col{214, 219, 226, 1}}})
		fillVGradient(s.bg, 0, int(shFloor), W, H-int(shFloor), []GradStop{{0, Col{236, 239, 243, 1}}, {1, Col{250, 251, 252, 1}}})
		fillRect(s.bg, 0, int(shFloor), W, 1, Col{200, 205, 212, 1})
	}
	blitOpaque(dst, s.bg, 0, 0)
	if s.phase != shCode && s.phase != shDone && s.phase != shFailed {
		ui.app.LastActivityTime = time.Now() // no screensaver while it goes
	}
	s.mu.Lock()
	done, total := s.done, s.total
	s.mu.Unlock()
	p := 0.0
	if total > 0 {
		p = math.Min(1, float64(done)/float64(total))
	}
	s.updateRate(done, now)
	if s.send {
		s.renderSend(ui, dst, now, p)
	} else {
		s.renderReceive(ui, dst, now, p)
	}
	hint := ""
	switch {
	case s.send && (s.phase == shDone || s.phase == shFailed && len(s.unplayable) > 0):
		hint = "[B] back"
	case s.send:
		hint = "[B] stop sharing"
	case s.phase == shCode:
		hint = ""
	case s.phase == shFailed:
		hint = "[A] try again     [B] back"
	case s.phase == shDone:
		hint = "[←] [→] choose     [A] select     [B] back"
	default:
		hint = "[B] cancel"
	}
	if hint != "" {
		shText(dst, hint, float64(W)/2, float64(H)-22, W600, 15, shSub)
	}
}

func (s *ShareView) updateRate(done int64, now time.Time) {
	if s.rateAt.IsZero() {
		s.rateAt, s.rateBytes = now, done
		return
	}
	dt := now.Sub(s.rateAt).Seconds()
	if dt < 0.5 {
		return
	}
	r := float64(done-s.rateBytes) / dt
	if s.rate == 0 {
		s.rate = r
	} else {
		s.rate = s.rate*0.7 + r*0.3
	}
	s.rateAt, s.rateBytes = now, done
}

// shText: centred text, its middle at cy (size in device px).
func shText(dst *image.RGBA, s string, cx, cy float64, w FontWeight, size float64, c Col) {
	f := face(w, size/DPR)
	drawText(dst, f, s, cx-textWidth(f, s)/2, cy+lineCapHeight(size/DPR)*DPR/2, c)
}

// shFit: s shortened to w device px at that size.
func shFit(s string, w FontWeight, size, maxW float64) string {
	return ellipsize(face(w, size/DPR), s, maxW)
}

func (s *ShareView) title() (string, string) {
	t, sub := s.info.Album, s.info.Artist
	if s.info.Title != "" {
		t = s.info.Title
	}
	if s.info.Songs > 1 {
		sub = fmt.Sprintf("%s · %d songs", s.info.Artist, s.info.Songs)
	}
	if s.info.Bytes > 0 {
		sub += " · " + humanBytes(s.info.Bytes)
	}
	return t, sub
}

func (s *ShareView) renderSend(ui *IpodUI, dst *image.RGBA, now time.Time, p float64) {
	if s.phase == shFailed && len(s.unplayable) > 0 {
		s.renderUnplayable(ui, dst, now)
		return
	}
	// the pedestal breathes while it waits (like a sleeping computer's light)
	beam := 1.0
	if s.phase == shWaiting || s.phase == shPreparing {
		beam = 0.55 + 0.45*(0.5+0.5*math.Sin(float64(now.UnixMilli())/5000*2*math.Pi))
	}
	if s.phase == shDone {
		beam = 0.5
	}
	ui.shPedestal(dst, shCX, "blue", beam)
	cx0, cy0 := shCX-shareCoverPx/2, shTop-24-shareCoverPx
	ui.shCover(dst, s, cx0, cy0, now)
	// the laser: up and down over the cover; what it has sent turns pale
	if s.cover != nil && (s.phase == shMoving || s.phase == shWaiting || s.phase == shPreparing) {
		if s.phase == shMoving {
			sentH := int(p * shareCoverPx)
			if sentH > 0 && s.coverSent != nil {
				sub := s.coverSent.SubImage(image.Rect(0, 0, shareCoverPx, sentH)).(*image.RGBA)
				blit(dst, sub, int(cx0), int(cy0), 1)
			}
		}
		// waiting: a slow sweep up and down; sending: it stays on the edge
		// between what's gone (pale) and what's still to go
		y := cy0 + p*shareCoverPx
		if s.phase != shMoving {
			t := math.Mod(float64(now.UnixMilli())/1000/7.0, 1)
			y = cy0 + shareCoverPx*(0.5-0.5*math.Cos(t*2*math.Pi))
		}
		shLaser(dst, cx0, cx0+shareCoverPx, y, Col{40, 150, 255, 1})
	}
	// the code
	label, lc := "YOUR CODE", shSub
	switch s.phase {
	case shMoving:
		label, lc = "SHARING", shBlue
	case shDone:
		label, lc = "SENT", shGreen
	case shFailed:
		label, lc = "NOT SENT", Col{200, 60, 50, 1}
	}
	shText(dst, label, shRX, 52, W600, 15, lc)
	// the digits flip into place when the code appears
	for i, d := range s.code {
		x := 348 + float64(i)*29
		if i >= 4 {
			x += 8
		}
		ch := d
		if el := now.Sub(s.phaseAt); s.phase == shPreparing && el < time.Duration(300+i*90)*time.Millisecond {
			ch = int(now.UnixMilli()/60+int64(i*3)) % 10
		}
		shDigit(dst, x, 68, 26, 40, fmt.Sprint(ch), false, false)
	}
	t, sub := s.title()
	shText(dst, shFit(t, W700, 20, 270), shRX, 148, W700, 20, shInk)
	shText(dst, shFit(sub, W500, 15, 270), shRX, 172, W500, 15, shSub)
	switch s.phase {
	case shPreparing:
		shText(dst, "Getting ready…", shRX, 222, W500, 15, shBody)
	case shWaiting:
		shText(dst, "Waiting for a friend…", shRX, 214, W600, 17, shBody)
		shText(dst, "On their betterPod: Music → Receive,", shRX, 240, W500, 14, shSub)
		shText(dst, "and these eight digits", shRX, 260, W500, 14, shSub)
		if el := now.Sub(s.phaseAt); el > 90*time.Second {
			shText(dst, "Still here. The music is patient.", shRX, 290, W500, 14, Col{150, 156, 168, 1})
		}
	case shMoving:
		shText(dst, "Sending…", shRX, 222, W500, 15, shBody)
		shAquaBar(dst, 360, 240, 216, p, false)
		shText(dst, fmt.Sprintf("%d%% · %s/s", int(p*100), humanBytes(int64(s.rate))), shRX, 268, W500, 15, shSub)
	case shDone:
		shText(dst, "Sent! It's on their betterPod now.", shRX, 222, W600, 15, shBody)
		shText(dst, shMilestone(s.count), shRX, 246, W500, 13, shBlue)
	case shFailed:
		shText(dst, s.err, shRX, 222, W500, 14, Col{200, 60, 50, 1})
	}
}

func (s *ShareView) renderReceive(ui *IpodUI, dst *image.RGBA, now time.Time, p float64) {
	W := float64(dst.Bounds().Dx())
	if s.phase == shFailed && len(s.unplayable) > 0 {
		s.renderUnplayable(ui, dst, now)
		return
	}
	if s.egg != "" && now.Sub(s.eggAt) > 4*time.Second {
		s.egg = "" // (the easter egg is over: the digits again)
	}
	if s.phase == shCode || (s.phase == shFailed && s.cover == nil && s.info.Album == "") || s.phase == shConnecting {
		// the digits, and the empty pedestal waiting
		on := 0.3
		if s.phase == shConnecting {
			on = 0.55 + 0.45*(0.5+0.5*math.Sin(float64(now.UnixMilli())/900*2*math.Pi))
		}
		if s.egg != "" && codeString(s.code) == "00000000" {
			// the void: the light flickers out
			el := now.Sub(s.eggAt).Seconds()
			on = 0.3 * math.Max(0, 1-el) * (0.5 + 0.5*math.Sin(el*40))
		}
		ui.shPedestal(dst, W/2, "green", on)
		shText(dst, "Type the code shown on your friend's betterPod", W/2, 40, W500, 15, shBody)
		pi := "31415926535897932384626433832795"
		for i := 0; i < 8; i++ {
			x := 70 + float64(i)*60
			if i >= 4 {
				x += 20
			}
			ch := fmt.Sprint(s.code[i])
			if s.egg != "" && codeString(s.code) == "31415926" {
				// π rolls by
				k := int(now.Sub(s.eggAt).Milliseconds()/180) + i
				ch = string(pi[k%len(pi)])
			}
			sel := s.phase == shCode && i == s.cur
			if sel {
				shText(dst, fmt.Sprint((s.code[i]+1)%10), x+24, 92-18, W600, 17, Col{170, 176, 186, 1})
				shText(dst, fmt.Sprint((s.code[i]+9)%10), x+24, 92+94, W600, 17, Col{170, 176, 186, 1})
			}
			shDigit(dst, x, 92, 48, 76, ch, sel, false)
		}
		switch {
		case s.egg != "":
			shText(dst, s.egg, W/2, 216, W600, 15, shBlue)
		case s.phase == shConnecting:
			shText(dst, "Looking for your friend's betterPod…", W/2, 216, W600, 15, shBody)
		case s.phase == shFailed:
			shText(dst, s.err, W/2, 216, W600, 15, Col{200, 60, 50, 1})
		default:
			shText(dst, "[↑] [↓] digit      [←] [→] move      [A] receive", W/2, 216, W500, 14, shSub)
		}
		return
	}
	if s.phase == shDone {
		s.renderReceived(ui, dst, now)
		return
	}
	ui.shPedestal(dst, shCX, "green", 1)
	cx0, cy0 := shCX-shareCoverPx/2, shTop-24-shareCoverPx
	built := shareCoverPx
	if s.phase == shMoving {
		// the cover is built along with the whole transfer (it comes first)
		built = int(math.Min(1, 0.06+p*0.94) * shareCoverPx)
	}
	if s.cover == nil {
		// not here yet: its outline
		strokeRounded(dst, cx0, cy0, cx0+shareCoverPx, cy0+shareCoverPx, 0, 1, Col{40, 190, 110, 0.5}, 1)
		for y := cy0 + 6; y < cy0+shareCoverPx; y += 8 {
			fillRect(dst, int(cx0)+1, int(y), shareCoverPx-2, 1, Col{40, 190, 110, 0.12})
		}
	} else if built >= shareCoverPx {
		ui.shCover(dst, s, cx0, cy0, now)
	} else {
		shCoverShadow(dst, cx0, cy0, 0.5)
		blit(dst, s.cover.SubImage(image.Rect(0, 0, shareCoverPx, built)).(*image.RGBA), int(cx0), int(cy0), 1)
		// the next lines arriving, every other one, fading out
		for k := 0; k < 16 && built+k < shareCoverPx; k += 2 {
			a := 1 - float64(k)/16
			if shHash(built+k)%100 > int(a*100) {
				continue
			}
			blit(dst, s.cover.SubImage(image.Rect(0, built+k, shareCoverPx, built+k+1)).(*image.RGBA), int(cx0), int(cy0)+built+k, a)
		}
		strokeRounded(dst, cx0, cy0, cx0+shareCoverPx, cy0+shareCoverPx, 0, 1, Col{40, 190, 110, 0.45}, 1)
		for y := float64(built + 18); y < shareCoverPx; y += 8 {
			fillRect(dst, int(cx0)+1, int(cy0+y), shareCoverPx-2, 1, Col{40, 190, 110, 0.12})
		}
		shLaser(dst, cx0, cx0+shareCoverPx, cy0+float64(built), Col{40, 200, 110, 1})
	}
	label := "RECEIVING"
	switch s.phase {
	case shSaving:
		label = "SAVING"
	case shDone:
		label = "RECEIVED"
	case shFailed:
		label = "STOPPED"
	}
	lc := shGreen
	if s.phase == shFailed {
		lc = Col{200, 60, 50, 1}
	}
	shText(dst, label, shRX, 62, W600, 15, lc)
	t, sub := s.title()
	shText(dst, shFit(t, W700, 20, 270), shRX, 104, W700, 20, shInk)
	shText(dst, shFit(sub, W500, 15, 270), shRX, 128, W500, 15, shSub)
	from := "from a friend's betterPod"
	if s.info.From != "" {
		from = "from " + s.info.From
	}
	switch s.phase {
	case shMoving:
		shText(dst, from, shRX, 172, W500, 15, shBody)
		shAquaBar(dst, 360, 190, 216, p, true)
		line := fmt.Sprintf("%d%% · %s/s", int(p*100), humanBytes(int64(s.rate)))
		if s.info.Songs > 1 {
			line = fmt.Sprintf("Song %d of %d · %d%% · %s/s", min(s.info.Songs, 1+int(p*float64(s.info.Songs))), s.info.Songs, int(p*100), humanBytes(int64(s.rate)))
		}
		shText(dst, line, shRX, 218, W500, 15, shSub)
	case shSaving:
		shText(dst, "Putting it in your Music…", shRX, 172, W500, 15, shBody)
	case shDone:
		shText(dst, "In your library, under Albums", shRX, 168, W500, 15, shBody)
		if s.have > 0 {
			shText(dst, fmt.Sprintf("You already had %d %s by %s", s.have, pluralize(s.have, "album", "albums"), s.info.Artist), shRX, 190, W500, 15, shSub)
		}
		if m := shMilestone(s.count); m != "" {
			shText(dst, m, shRX, 276, W500, 13, shBlue)
		}
		if s.playable {
			shAquaButton(dst, shRX, 232, 160, "Play")
		} else {
			shText(dst, "Adding it to your library…", shRX, 232, W500, 14, shSub)
		}
	case shFailed:
		shText(dst, s.err, shRX, 172, W500, 14, Col{200, 60, 50, 1})
	}
}

func shHash(n int) int {
	x := uint32(n)*2654435761 + 12345
	x ^= x >> 13
	return int(x % 1000)
}

// shPedestal: the pedestal at cx (its top at shTop), its beam at opacity on.
// The colour is blue (sending), green (receiving) or red (refused).
func (ui *IpodUI) shPedestal(dst *image.RGBA, cx float64, colour string, on float64) {
	ped := ui.shareAsset("share_ped_" + colour)
	beam := ui.shareAsset("share_beam_" + colour)
	if beam != nil {
		blit(dst, beam, int(cx)-130, int(shTop)-226, on)
	}
	if ped != nil {
		blit(dst, ped, int(cx)-130, int(shTop)-24, 1)
	}
}

// shareAsset: a picture of Share's; a "…_red" one is made once from the
// green one, its green light turned red (its greys stay as they are).
func (ui *IpodUI) shareAsset(name string) *image.RGBA {
	if !strings.HasSuffix(name, "_red") {
		return ui.asset(name)
	}
	ui.assetsMu.Lock()
	a, ok := ui.assets[name]
	ui.assetsMu.Unlock()
	if ok {
		return a
	}
	g := ui.asset(strings.TrimSuffix(name, "_red") + "_green")
	r := image.NewRGBA(g.Bounds())
	for i := 0; i < len(g.Pix); i += 4 {
		// (r, g, b) → (g, r, r): green becomes red, a grey stays grey
		r.Pix[i], r.Pix[i+1], r.Pix[i+2], r.Pix[i+3] = g.Pix[i+1], g.Pix[i], g.Pix[i], g.Pix[i+3]
	}
	ui.assetsMu.Lock()
	ui.assets[name] = r
	ui.assetsMu.Unlock()
	return r
}

func shCoverShadow(dst *image.RGBA, x, y float64, a float64) {
	for i := 6.0; i >= 1; i-- {
		fillRoundedGradient(dst, x+4-i*1.5, y+6-i, x+shareCoverPx-4+i*1.5, y+shareCoverPx+4+i, i*2,
			[]GradStop{{0, Col{40, 50, 70, 0.025 * a * 2}}, {1, Col{40, 50, 70, 0.025 * a * 2}}}, 1)
	}
}

// shCover: the cover (or the default one) with its shadow and gloss.
func (ui *IpodUI) shCover(dst *image.RGBA, s *ShareView, x, y float64, now time.Time) {
	c := s.cover
	if c == nil {
		c = ui.defaultArtSized(shareCoverPx)
	}
	a := 1.0
	if !s.coverAt.IsZero() && s.send {
		a = math.Min(1, float64(now.Sub(s.coverAt))/float64(250*time.Millisecond))
	}
	shCoverShadow(dst, x, y, a)
	blit(dst, c, int(x), int(y), a)
	// a diagonal gloss, mid-2000s
	aaPoly(dst, [][2]float64{{x, y}, {x + shareCoverPx, y}, {x + shareCoverPx, y + shareCoverPx*0.28}, {x, y + shareCoverPx*0.46}}, Col{255, 255, 255, 0.15 * a})
	if s.phase == shDone && !s.send {
		// a soft flash when it's complete
		el := now.Sub(s.phaseAt).Seconds()
		if el < 1.2 {
			fl := 1 - el/1.2
			for i := 1.0; i <= 4; i++ {
				strokeRounded(dst, x-i*2, y-i*2, x+shareCoverPx+i*2, y+shareCoverPx+i*2, i*2, 2, Col{120, 230, 170, 0.35 * fl / i}, 1)
			}
		}
	}
}

// renderUnplayable: the receiving betterPod can't play the songs' format (a
// Miyoo and AAC): the share stopped before anything moved, and both screens
// say so. The pedestal's light turns red, and on it, instead of the cover,
// a file's tag with the format and a red "no" badge, shaken once as a
// wrong password is.
func (s *ShareView) renderUnplayable(ui *IpodUI, dst *image.RGBA, now time.Time) {
	ui.shPedestal(dst, shCX, "red", 0.35) // (its light turns red, on both sides)
	dx := 0.0
	if el := now.Sub(s.phaseAt).Seconds(); el < 0.6 {
		dx = 9 * math.Sin(el*38) * (1 - el/0.6)
	}
	shFileTag(dst, shCX+dx, shTop-24, shExtOf(s.unplayable[0]))

	red := Col{200, 60, 50, 1}
	shText(dst, "FORMAT NOT SUPPORTED", shRX, 52, W600, 15, red)
	t, sub := s.title()
	if t != "" {
		shText(dst, shFit(t, W700, 20, 270), shRX, 96, W700, 20, shInk)
	}
	if strings.TrimSpace(strings.Trim(sub, "·")) != "" {
		shText(dst, shFit(sub, W500, 15, 270), shRX, 120, W500, 15, shSub)
	}
	who := "This betterPod can't play it"
	if s.send {
		who = "Their betterPod can't play it"
	}
	shText(dst, who, shRX, 166, W600, 17, shBody)
	names := strings.Join(s.unplayable, ", ")
	shText(dst, shFit(names, W600, 15, 280), shRX, 196, W600, 15, red)
	verb := "isn't"
	if len(s.unplayable) > 1 {
		verb = "aren't"
	}
	shText(dst, shFit(verb+" supported on "+s.unplayOn, W500, 14, 280), shRX, 218, W500, 14, shSub)
	tip := Col{150, 156, 168, 1}
	shText(dst, "MP3, FLAC, Ogg, WAV and AIFF", shRX, 262, W500, 13, tip)
	shText(dst, "play on every betterPod", shRX, 280, W500, 13, tip)
}

// shExtOf: the extension in a format's name ("AAC / ALAC (.m4a)" → ".m4a").
func shExtOf(name string) string {
	i, j := strings.LastIndex(name, "("), strings.LastIndex(name, ")")
	if i >= 0 && j > i+1 {
		return name[i+1 : j]
	}
	return name
}

// shFileTag: a file's tag standing on the pedestal (its bottom at y), its
// corner folded, with the format on it and a red "no" badge.
func shFileTag(dst *image.RGBA, cx, y float64, ext string) {
	const w, h, f = 116.0, 146.0, 30.0 // size, the folded corner
	x0, y0, x1, y1 := cx-w/2, y-h, cx+w/2, y
	for i := 4.0; i >= 1; i-- { // its shadow, under the folded corner's line
		fillRoundedGradient(dst, x0+3-i, y0+f, x1-3+i, y1+3+i, i*2,
			[]GradStop{{0, Col{40, 50, 70, 0}}, {1, Col{40, 50, 70, 0.06}}}, 1)
	}
	edge := Col{168, 175, 186, 1}
	body := [][2]float64{{x0, y0}, {x1 - f, y0}, {x1, y0 + f}, {x1, y1}, {x0, y1}}
	aaPoly(dst, body, Col{250, 251, 252, 1})
	fillRoundedGradient(dst, x0+1, y1-h*0.5, x1-1, y1-1, 0, []GradStop{{0, Col{214, 219, 226, 0}}, {1, Col{214, 219, 226, 0.7}}}, 1)
	strokePoly(dst, body, 1.2, edge)
	fold := [][2]float64{{x1 - f, y0}, {x1 - f, y0 + f}, {x1, y0 + f}}
	aaPoly(dst, fold, Col{222, 227, 234, 1})
	strokePoly(dst, fold, 1.2, edge)
	shText(dst, ext, cx, y0+h*0.46, W700, 30, shInk)
	// the badge: a glossy red disc with a white bar
	bx, by, r := x1-6, y1-10, 23.0
	fillRoundedGradient(dst, bx-r, by-r+3, bx+r, by+r+3, r, []GradStop{{0, Col{60, 20, 20, 0.18}}, {1, Col{60, 20, 20, 0.18}}}, 1)
	fillRoundedGradient(dst, bx-r, by-r, bx+r, by+r, r, []GradStop{{0, Col{255, 112, 98, 1}}, {1, Col{196, 36, 30, 1}}}, 1)
	strokeRounded(dst, bx-r, by-r, bx+r, by+r, r, 2, colWhite, 1)
	fillRoundedGradient(dst, bx-12, by-3.5, bx+12, by+3.5, 3.5, []GradStop{{0, colWhite}, {1, colWhite}}, 1)
	fillRoundedGradient(dst, bx-r+4, by-r+3, bx+r-4, by-2, r-4, []GradStop{{0, Col{255, 255, 255, 0.45}}, {1, Col{255, 255, 255, 0.04}}}, 1)
}

// shLaser: a glowing line from x0 to x1 at y, with bright ends.
func shLaser(dst *image.RGBA, x0, x1, y float64, c Col) {
	for i, h := range []float64{9, 5, 3} {
		a := []float64{0.12, 0.25, 0.9}[i]
		fillRoundedGradient(dst, x0-12, y-h/2, x1+12, y+h/2, h/2, []GradStop{{0, Col{c.R, c.G, c.B, a}}, {1, Col{c.R, c.G, c.B, a}}}, 1)
	}
	fillRoundedGradient(dst, x0-6, y-0.5, x1+6, y+0.5, 0.5, []GradStop{{0, colWhite}, {1, colWhite}}, 1)
	for _, xe := range []float64{x0 - 12, x1 + 12} {
		fillRoundedGradient(dst, xe-6, y-6, xe+6, y+6, 6, []GradStop{{0, Col{255, 255, 255, 0.5}}, {1, Col{255, 255, 255, 0.5}}}, 1)
		fillRoundedGradient(dst, xe-3, y-3, xe+3, y+3, 3, []GradStop{{0, c}, {1, c}}, 1)
	}
}

// shDigit: a white glossy tile (blue when selected) with a digit.
func shDigit(dst *image.RGBA, x, y, w, h float64, ch string, sel, dim bool) {
	top, bot, tc, edge := Col{255, 255, 255, 1}, Col{222, 227, 234, 1}, Col{50, 56, 66, 1}, Col{168, 175, 186, 1}
	if sel {
		top, bot, tc, edge = Col{120, 190, 255, 1}, Col{28, 112, 218, 1}, colWhite, Col{40, 110, 210, 1}
	}
	if dim {
		tc = Col{180, 186, 196, 1}
	}
	fillRoundedGradient(dst, x, y+2, x+w, y+h+2, 8, []GradStop{{0, Col{40, 50, 70, 0.12}}, {1, Col{40, 50, 70, 0.12}}}, 1)
	fillRoundedGradient(dst, x, y, x+w, y+h, 7, []GradStop{{0, top}, {1, bot}}, 1)
	fillRoundedGradient(dst, x+1, y+h/2, x+w-1, y+h-1, 6, []GradStop{{0, Col{0, 0, 0, 0.04}}, {1, Col{0, 0, 0, 0.04}}}, 1)
	strokeRounded(dst, x, y, x+w, y+h, 7, 1, edge, 1)
	size := h * 0.6
	shText(dst, ch, x+w/2, y+h/2, W600, size, tc)
	g := 0.45
	if sel {
		g = 0.35
	}
	fillRoundedGradient(dst, x+1, y+1, x+w-1, y+h/2, 6, []GradStop{{0, Col{255, 255, 255, g}}, {1, Col{255, 255, 255, g * 0.6}}}, 1)
	fillRect(dst, int(x), int(y+h/2), 3, 1, Col{120, 128, 140, 1})
	fillRect(dst, int(x+w)-3, int(y+h/2), 3, 1, Col{120, 128, 140, 1})
}

// shAquaBar: a gel progress bar (blue, or green).
func shAquaBar(dst *image.RGBA, x, y, w, p float64, green bool) {
	fillRoundedGradient(dst, x, y, x+w, y+12, 6, []GradStop{{0, Col{196, 201, 208, 1}}, {1, Col{232, 235, 240, 1}}}, 1)
	top, bot := Col{120, 200, 255, 1}, Col{30, 110, 220, 1}
	if green {
		top, bot = Col{140, 230, 160, 1}, Col{30, 150, 70, 1}
	}
	fw := math.Max(12, w*p)
	fillRoundedGradient(dst, x, y, x+fw, y+12, 6, []GradStop{{0, top}, {1, bot}}, 1)
	fillRoundedGradient(dst, x+1, y+1, x+fw-1, y+6, 3, []GradStop{{0, Col{255, 255, 255, 0.5}}, {1, Col{255, 255, 255, 0.2}}}, 1)
	strokeRounded(dst, x, y, x+w, y+12, 6, 1, Col{150, 158, 170, 1}, 1)
}

func shAquaButton(dst *image.RGBA, cx, cy, w float64, label string) {
	x0, x1 := cx-w/2, cx+w/2
	fillRoundedGradient(dst, x0, cy-20, x1, cy+20, 20, []GradStop{{0, Col{120, 195, 255, 1}}, {1, Col{26, 104, 214, 1}}}, 1)
	fillRoundedGradient(dst, x0+3, cy-18, x1-3, cy, 10, []GradStop{{0, Col{255, 255, 255, 0.45}}, {1, Col{255, 255, 255, 0.15}}}, 1)
	strokeRounded(dst, x0, cy-20, x1, cy+20, 20, 1, Col{30, 90, 180, 1}, 1)
	shText(dst, label, cx, cy, W700, 18, colWhite)
}

// shMilestone: a line for every tenth share.
func shMilestone(n int) string {
	if n <= 0 || n%10 != 0 {
		return ""
	}
	return fmt.Sprintf("Share number %d. A true mixtape maker.", n)
}

// renderReceived: the received screen. The pedestal sinks away, the cover
// turns over once while it rises to the middle and grows; then RECEIVED,
// the title, the artist, and Listen Now / Receive Another.
func (s *ShareView) renderReceived(ui *IpodUI, dst *image.RGBA, now time.Time) {
	W := float64(dst.Bounds().Dx())
	el := now.Sub(s.phaseAt).Seconds()
	ease := func(a, b float64) float64 { // 0→1 between a and b seconds
		t := math.Max(0, math.Min(1, (el-a)/(b-a)))
		return t * t * (3 - 2*t)
	}
	// the pedestal goes down and fades
	if pg := ease(0, 0.6); pg < 1 {
		ped, beam := ui.asset("share_ped_green"), ui.asset("share_beam_green")
		dy := int(pg * 40)
		blit(dst, beam, int(shCX)-130, int(shTop)-226+dy, 1-pg)
		blit(dst, ped, int(shCX)-130, int(shTop)-24+dy, 1-pg)
	}
	// the cover: from the pedestal to the middle, one turn on the way
	m := ease(0.25, 1.25)
	big := s.coverBig
	if big == nil {
		if s.cover != nil {
			big = scaleImage(s.cover, shareBigPx, shareBigPx)
			s.coverBig = big
		} else {
			big = ui.defaultArtSized(shareBigPx)
		}
	}
	x0, y0, sz0 := shCX-shareCoverPx/2, shTop-24-shareCoverPx, float64(shareCoverPx)
	sz1 := float64(shareBigPx)
	x1, y1 := W/2-sz1/2, 62.0
	size := sz0 + (sz1-sz0)*m
	cx := x0 + sz0/2 + (x1+sz1/2-(x0+sz0/2))*m
	cy := y0 + sz0/2 + (y1+sz1/2-(y0+sz0/2))*m
	turn := math.Cos(m * 2 * math.Pi) // 1 → −1 → 1: once round
	w := math.Max(2, size*math.Abs(turn))
	if m >= 1 {
		shBigCover(dst, big, x1, y1)
	} else if turn > 0 {
		img := scaleImageFast(big, int(w), int(size))
		blit(dst, img, int(cx-w/2), int(cy-size/2), 1)
	} else {
		// its back: plain white, with a soft edge
		fillRoundedGradient(dst, cx-w/2, cy-size/2, cx+w/2, cy+size/2, 2,
			[]GradStop{{0, Col{250, 251, 253, 1}}, {1, Col{220, 225, 232, 1}}}, 1)
		strokeRounded(dst, cx-w/2, cy-size/2, cx+w/2, cy+size/2, 2, 1, Col{190, 196, 206, 1}, 1)
	}
	// the words and the buttons come in
	a := ease(1.1, 1.6)
	if a <= 0 {
		return
	}
	shText(dst, "RECEIVED", W/2, 40, W600, 15, Col{shGreen.R, shGreen.G, shGreen.B, a})
	t, sub := s.title()
	shText(dst, shFit(t, W700, 22, W-60), W/2, y1+sz1+26, W700, 22, Col{shInk.R, shInk.G, shInk.B, a})
	shText(dst, shFit(sub, W500, 15, W-60), W/2, y1+sz1+50, W500, 15, Col{shSub.R, shSub.G, shSub.B, a})
	if m := shMilestone(s.count); m != "" {
		shText(dst, m, W/2, y1+sz1+70, W500, 13, Col{shBlue.R, shBlue.G, shBlue.B, a})
	}
	by := y1 + sz1 + 104
	listen := "Listen Now"
	if !s.playable {
		listen = "Adding…"
	}
	shButton(dst, W/2-104, by, 190, listen, s.btn == 0, !s.playable, a)
	shButton(dst, W/2+104, by, 190, "Receive Another", s.btn == 1, false, a)
}

// shBigCover: the received cover, with its shadow and gloss.
func shBigCover(dst *image.RGBA, img *image.RGBA, x, y float64) {
	n := float64(img.Bounds().Dx())
	for i := 6.0; i >= 1; i-- {
		fillRoundedGradient(dst, x+4-i*1.5, y+6-i, x+n-4+i*1.5, y+n+4+i, i*2,
			[]GradStop{{0, Col{40, 50, 70, 0.05}}, {1, Col{40, 50, 70, 0.05}}}, 1)
	}
	blit(dst, img, int(x), int(y), 1)
	aaPoly(dst, [][2]float64{{x, y}, {x + n, y}, {x + n, y + n*0.28}, {x, y + n*0.46}}, Col{255, 255, 255, 0.15})
}

// shButton: an Aqua button: blue when selected, white otherwise.
func shButton(dst *image.RGBA, cx, cy, w float64, label string, sel, dim bool, a float64) {
	x0, x1 := cx-w/2, cx+w/2
	top, bot, edge, tc := Col{255, 255, 255, 1}, Col{218, 223, 230, 1}, Col{160, 168, 180, 1}, shInk
	if sel {
		top, bot, edge, tc = Col{120, 195, 255, 1}, Col{26, 104, 214, 1}, Col{30, 90, 180, 1}, colWhite
	}
	if dim {
		tc = Col{tc.R, tc.G, tc.B, 0.6}
	}
	fillRoundedGradient(dst, x0, cy-19+2, x1, cy+19+2, 19, []GradStop{{0, Col{40, 50, 70, 0.12}}, {1, Col{40, 50, 70, 0.12}}}, a)
	fillRoundedGradient(dst, x0, cy-19, x1, cy+19, 19, []GradStop{{0, top}, {1, bot}}, a)
	fillRoundedGradient(dst, x0+3, cy-17, x1-3, cy, 10, []GradStop{{0, Col{255, 255, 255, 0.5}}, {1, Col{255, 255, 255, 0.15}}}, a)
	strokeRounded(dst, x0, cy-19, x1, cy+19, 19, 1, edge, a)
	shText(dst, label, cx, cy, W700, 16, Col{tc.R, tc.G, tc.B, tc.A * a})
}
