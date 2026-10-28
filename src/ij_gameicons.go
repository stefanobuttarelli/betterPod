package main

// ij_gameicons.go — the pictures on the right of the Games list: a wall of
// glossy bricks with the ball and the paddle (Brick), three fanned cards
// (Solitaire), and for Weather one of its animated icons, a different one
// each time the row is chosen.

import (
	"image"
	"math"
	"math/rand"
	"time"

	"github.com/fogleman/gg"
)

const gameIconCSS = 112.0

var brickIconImg, solIconImg *image.RGBA

// brickIcon: four rows of bricks, the ball and the paddle.
func brickIcon() *image.RGBA {
	if brickIconImg != nil {
		return brickIconImg
	}
	S := px(gameIconCSS)
	img := image.NewRGBA(image.Rect(0, 0, S, S))
	p := pen{gg.NewContextForRGBA(img), float64(S) / 100}
	dc := p.dc
	rows := [][2]string{{"#ff6b5e", "#d8281c"}, {"#ffb648", "#e57a0c"}, {"#ffe25a", "#e3b30e"}, {"#7fe06a", "#2f9f2a"}}
	bw, bh, gap := 22.0, 9.5, 2.5
	x0 := (100 - (4*bw + 3*gap)) / 2
	for r, c := range rows {
		for i := 0; i < 4; i++ {
			off := 0.0
			n := 4
			if r%2 == 1 {
				off = -(bw + gap) / 2
				n = 5
			}
			_ = n
			x := x0 + off + float64(i)*(bw+gap)
			if r%2 == 1 && i == 0 {
				// half bricks at the ends of the odd rows
				p.brick(x0, 10+float64(r)*(bh+gap), bw/2-gap/2, bh, c)
				p.brick(x0+3*(bw+gap)+bw/2+gap/2, 10+float64(r)*(bh+gap), bw/2-gap/2, bh, c)
				continue
			}
			p.brick(x, 10+float64(r)*(bh+gap), bw, bh, c)
		}
	}
	// the ball
	bx, by, br := 58.0, 70.0, 6.0
	p.circle(bx+0.8, by+1.5, br)
	p.fill(p.solid(rgba(0, 0, 0, 0.25)))
	p.circle(bx, by, br)
	dc.SetFillStyle(p.radial(bx-br*0.4, by-br*0.4, 0, bx, by, br, GradStop{0, hex("#ffffff")}, GradStop{0.6, hex("#c9d3de")}, GradStop{1, hex("#7d8a99")}))
	dc.FillPreserve()
	dc.SetStrokeStyle(p.solid(hex("#5c6774")))
	dc.SetLineWidth(p.u(0.8))
	dc.Stroke()
	// the paddle
	px0, py0, pw, ph := 26.0, 84.0, 40.0, 7.0
	dc.DrawRoundedRectangle(p.u(px0), p.u(py0+1.5), p.u(pw), p.u(ph), p.u(3.5))
	p.fill(p.solid(rgba(0, 0, 0, 0.25)))
	dc.DrawRoundedRectangle(p.u(px0), p.u(py0), p.u(pw), p.u(ph), p.u(3.5))
	dc.SetFillStyle(p.linear(0, py0, 0, py0+ph, GradStop{0, hex("#9fd2ff")}, GradStop{0.5, hex("#2f8ee8")}, GradStop{1, hex("#1660b8")}))
	dc.FillPreserve()
	dc.SetStrokeStyle(p.solid(hex("#0f4c94")))
	dc.SetLineWidth(p.u(0.8))
	dc.Stroke()
	p.gloss(px0+pw/2, py0+1.8, pw/2-2, 1.8, 0.8)
	brickIconImg = img
	return img
}

// brick: a glossy rounded brick.
func (p pen) brick(x, y, w, h float64, c [2]string) {
	dc := p.dc
	dc.DrawRoundedRectangle(p.u(x), p.u(y+1.2), p.u(w), p.u(h), p.u(2))
	p.fill(p.solid(rgba(0, 0, 0, 0.22)))
	dc.DrawRoundedRectangle(p.u(x), p.u(y), p.u(w), p.u(h), p.u(2))
	dc.SetFillStyle(p.linear(0, y, 0, y+h, GradStop{0, hex(c[0])}, GradStop{1, hex(c[1])}))
	dc.FillPreserve()
	dc.SetStrokeStyle(p.solid(rgba(0, 0, 0, 0.35)))
	dc.SetLineWidth(p.u(0.6))
	dc.Stroke()
	dc.DrawRoundedRectangle(p.u(x+1), p.u(y+0.8), p.u(w-2), p.u(h*0.42), p.u(1.5))
	p.fill(p.linear(0, y, 0, y+h*0.45, GradStop{0, rgba(255, 255, 255, 0.75)}, GradStop{1, rgba(255, 255, 255, 0.15)}))
}

// solitaireIcon: three cards fanned out, the ace of spades in front.
func (ui *IpodUI) solitaireIcon() *image.RGBA {
	if solIconImg != nil {
		return solIconImg
	}
	S := px(gameIconCSS)
	img := image.NewRGBA(image.Rect(0, 0, S, S))
	dc := gg.NewContextForRGBA(img)
	u := float64(S) / 100
	cards := []struct {
		rank, suit string
		red        bool
		angle      float64
	}{{"Q", "sol_diamond", true, -18}, {"K", "sol_heart", true, 0}, {"A", "sol_spade", false, 18}}
	cw, ch := 46.0, 64.0
	for i, c := range cards {
		dc.Push()
		cx, cy := 50.0+float64(i-1)*14, 54.0+math.Abs(float64(i-1))*3
		dc.RotateAbout(gg.Radians(c.angle), cx*u, (cy+30)*u)
		x, y := cx-cw/2, cy-ch/2
		// shadow, card, edge
		dc.DrawRoundedRectangle((x+1)*u, (y+2)*u, cw*u, ch*u, 5*u)
		dc.SetRGBA(0, 0, 0, 0.25)
		dc.Fill()
		dc.DrawRoundedRectangle(x*u, y*u, cw*u, ch*u, 5*u)
		g := gg.NewLinearGradient(0, y*u, 0, (y+ch)*u)
		g.AddColorStop(0, c8(hex("#ffffff")))
		g.AddColorStop(1, c8(hex("#e9ebee")))
		dc.SetFillStyle(g)
		dc.FillPreserve()
		dc.SetRGBA255(150, 155, 162, 255)
		dc.SetLineWidth(0.9 * u)
		dc.Stroke()
		// rank and suit
		col := rgb(30, 32, 36)
		if c.red {
			col = hex("#d42a1e")
		}
		dc.SetColor(c8(col))
		dc.SetFontFace(face(W700, 13*u/DPR))
		dc.DrawStringAnchored(c.rank, (x+7)*u, (y+9)*u, 0.5, 0.5)
		suit := ui.asset(c.suit)
		if suit != nil {
			sw := float64(suit.Bounds().Dx())
			want := 22 * u
			dc.Push()
			sc := want / sw
			dc.Translate((x+cw/2)*u, (y+ch/2+3)*u)
			dc.Scale(sc, sc)
			dc.DrawImageAnchored(suit, 0, 0, 0.5, 0.5)
			dc.Pop()
			// small suit under the rank
			dc.Push()
			sc2 := 7 * u / sw
			dc.Translate((x+7)*u, (y+18)*u)
			dc.Scale(sc2, sc2)
			dc.DrawImageAnchored(suit, 0, 0, 0.5, 0.5)
			dc.Pop()
		}
		dc.Pop()
	}
	solIconImg = img
	return img
}

// drawImgPreview: a picture with a label under it, on the grey gradient
// (like drawIconPreview).
func (ui *IpodUI) drawImgPreview(dst *image.RGBA, img *image.RGBA, label, sub string) {
	w := dst.Bounds().Dx()
	h := dst.Bounds().Dy()
	if greyPreviewBg == nil || greyPreviewBg.Bounds().Dy() != h {
		greyPreviewBg = newCanvas(w, h)
		fillVGradient(greyPreviewBg, 0, 0, w, h, th.preview)
	}
	blitOpaque(dst, greyPreviewBg, 0, 0)
	ic := float64(img.Bounds().Dy()) / DPR
	lh, asc := lineBox(16)
	slh, sasc := lineBox(12)
	total := ic + 12 + lh
	if sub != "" {
		total += slh
	}
	y := (float64(h)/DPR - total) / 2
	blit(dst, img, (w-img.Bounds().Dx())/2, px(y), 1)
	y += ic + 12
	f := face(W500, 16)
	drawText(dst, f, label, (float64(w)-textWidth(f, label))/2, (y+asc)*DPR, colWhite)
	if sub != "" {
		y += lh
		sf := face(W500, 12)
		drawText(dst, sf, sub, (float64(w)-textWidth(sf, sub))/2, (y+sasc)*DPR, rgba(255, 255, 255, 0.75))
	}
}

// ---- Weather: a different animated icon each time ----

var wxPrev = struct {
	kind  wxKind
	frame int
}{kind: wxPartlySun, frame: -1}

var wxPrevNames = map[wxKind]string{
	wxSun: "Sunny", wxMoon: "Clear Night", wxPartlySun: "Partly Cloudy", wxPartlyMoon: "Partly Cloudy",
	wxCloudy: "Cloudy", wxFog: "Fog", wxDrizzle: "Drizzle", wxRain: "Rain", wxSnow: "Snow",
	wxSleet: "Sleet", wxThunder: "Thunderstorm",
}

// nextWxPreview picks another icon (never the same twice in a row).
func nextWxPreview() {
	k := wxPrev.kind
	for k == wxPrev.kind {
		k = wxKind(rand.Intn(int(wxThunder) + 1))
	}
	wxPrev.kind = k
}

func wxFrameAt(now time.Time) int {
	return int(math.Mod(float64(now.UnixNano())/1e9, wxLoopSecs) / wxLoopSecs * wxFrames)
}

func (ui *IpodUI) drawWeatherPreview(dst *image.RGBA, now time.Time) {
	f := wxFrameAt(now)
	wxPrev.frame = f
	ui.drawImgPreview(dst, wxIcon(wxPrev.kind, px(gameIconCSS+16), f), "Weather", wxPrevNames[wxPrev.kind])
}

// wxPreviewAnimating: the Weather preview is on screen and its icon has a
// new frame.
func (ui *IpodUI) wxPreviewAnimating(now time.Time) bool {
	return ui.preview == PrevWeather && ui.previewHide.Get(now) < 1 && wxFrameAt(now) != wxPrev.frame
}
