package main

// ij_anim.go — CSS-like animations: timing functions, tweens and animated
// values driven by the frame clock.

import (
	"math"
	"time"
)

// Easing maps linear progress 0..1 to eased progress.
type Easing func(float64) float64

// cubicBezier returns the CSS cubic-bezier(x1, y1, x2, y2) timing function.
func cubicBezier(x1, y1, x2, y2 float64) Easing {
	bez := func(t, p1, p2 float64) float64 {
		u := 1 - t
		return 3*u*u*t*p1 + 3*u*t*t*p2 + t*t*t
	}
	dbez := func(t, p1, p2 float64) float64 {
		u := 1 - t
		return 3*u*u*p1 + 6*u*t*(p2-p1) + 3*t*t*(1-p2)
	}
	return func(x float64) float64 {
		if x <= 0 {
			return 0
		}
		if x >= 1 {
			return 1
		}
		t := x
		for i := 0; i < 8; i++ {
			d := dbez(t, x1, x2)
			if math.Abs(d) < 1e-6 {
				break
			}
			t -= (bez(t, x1, x2) - x) / d
			if t < 0 {
				t = 0
			} else if t > 1 {
				t = 1
			}
		}
		// refine by bisection
		lo, hi := 0.0, 1.0
		for i := 0; i < 20 && math.Abs(bez(t, x1, x2)-x) > 1e-5; i++ {
			if bez(t, x1, x2) < x {
				lo = t
			} else {
				hi = t
			}
			t = (lo + hi) / 2
		}
		return bez(t, y1, y2)
	}
}
// CSS "ease", "ease-in-out" and "linear"
var (
	easeCSS    = cubicBezier(0.25, 0.1, 0.25, 1)
	easeInOut  = cubicBezier(0.42, 0, 0.58, 1)
	easeLinear = Easing(func(t float64) float64 { return t })
)

// animNow is the animation clock.
var animNow = time.Now

// transition durations from the ipod.js stylesheets
var (
	durSlide      = 300 * time.Millisecond
	durPanel      = 350 * time.Millisecond
	durPreview    = 400 * time.Millisecond
	delayMenuHide = 50 * time.Millisecond
)

// Value returns the value of the tween at now and whether it has finished.
//
// A tween with no duration is finished immediately.
func (t *Tween) Value(now time.Time) (float64, bool) {
	if t.Dur <= 0 {
		return t.To, true
	}
	p := float64(now.Sub(t.Start)) / float64(t.Dur)
	if p <= 0 {
		return t.From, false
	}
	if p >= 1 {
		return t.To, true
	}
	return t.From + (t.To-t.From)*t.Ease(p), false
}

// Anim is an animated value (one CSS transition on one property).
type Anim struct {
	tw     Tween
	active bool
	val    float64
}

// newAnim returns an Anim resting at v.
func newAnim(v float64) *Anim { return &Anim{val: v} }

// Set jumps to v.
func (a *Anim) Set(v float64) { a.val, a.active = v, false }

// To starts a transition from the current value to v.
func (a *Anim) To(v float64, dur time.Duration, ease Easing, delay time.Duration) {
	now := animNow()
	cur := a.Get(now)
	if !a.active && cur == v {
		return
	}
	if a.active && a.tw.To == v {
		return
	}
	a.tw = Tween{From: cur, To: v, Start: now.Add(delay), Dur: dur, Ease: ease}
	a.active = true
}

// Get returns the current value (and ends the tween when it's done).
func (a *Anim) Get(now time.Time) float64 {
	if !a.active {
		return a.val
	}
	v, done := a.tw.Value(now)
	a.val = v
	if done {
		a.active = false
	}
	return v
}

// Target is the value the animation is heading to.
func (a *Anim) Target() float64 {
	if a.active {
		return a.tw.To
	}
	return a.val
}

func (a *Anim) Running() bool { return a.active }

// Tween interpolates From → To over Dur starting at Start.
type Tween struct {
	From, To float64
	Start    time.Time
	Dur      time.Duration
	Ease     Easing
}
