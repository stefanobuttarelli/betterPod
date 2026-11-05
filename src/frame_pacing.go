package main

import "time"

const animatedFrameInterval = time.Second / 30

// framePacer schedules whole UI loops against monotonic deadlines. Keeping
// the deadline, rather than starting a new timer after each render, absorbs
// normal sleep jitter without adding input and callback work to the period.
// Missed slots are skipped: a late frame never starts a catch-up burst.
// The audio clock may request an earlier wake for the next video frame.
type framePacer struct {
	next     time.Time
	interval time.Duration
}

func (p *framePacer) delay(start, now time.Time, interval time.Duration, wakeBefore time.Time) time.Duration {
	if interval <= 0 {
		p.next = time.Time{}
		p.interval = 0
		return 0
	}
	deadline := p.next
	if deadline.IsZero() || interval != p.interval {
		deadline = start.Add(interval)
	}
	p.interval = interval
	if now.After(deadline) {
		missed := now.Sub(deadline)/interval + 1
		deadline = deadline.Add(missed * interval)
	}
	p.next = deadline.Add(interval)
	if !wakeBefore.IsZero() && wakeBefore.Before(deadline) {
		// Video has its own clock; the next loop starts a fresh UI schedule.
		p.next = time.Time{}
		deadline = wakeBefore
	}
	return max(deadline.Sub(now), 0)
}
