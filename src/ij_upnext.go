package main

// ij_upnext.go — Up Next: the songs that will play after the current one
// (Now Playing → Y, or hold A), and the marks on queued songs.
//
// A song added with Play Next or Add to End of Queue carries the queue
// icon before its title in the lists (lines with an arrow up / down) until
// it starts playing. In Up Next, A plays a song at once; Y (or holding A)
// offers Play Next (move it up) and Remove from Up Next.

import (
	"fmt"
	"image"
)

// queuedMarks: songs added to the queue by hand, and how ("queue_next",
// "queue_last"), until they play.
var queuedMarks = map[*Track]string{}

func (ui *IpodUI) markQueued(tracks []*Track, next bool) {
	m := "queue_last"
	if next {
		m = "queue_next"
	}
	for _, t := range tracks {
		queuedMarks[t] = m
	}
	ui.dirtyAllViews()
}

func (ui *IpodUI) dirtyAllViews() {
	for _, st := range [][]*View{ui.split, ui.full} {
		for _, v := range st {
			v.dirty = true
		}
	}
	ui.dirty = true
}

// ---- the queue in play order ----

// trackAt: the track at a play position.
func (q *PlaybackQueue) trackAt(pos int) *Track {
	i := pos
	if q.Shuffle && len(q.ShuffleOrder) > 0 {
		if pos < 0 || pos >= len(q.ShuffleOrder) {
			return nil
		}
		i = q.ShuffleOrder[pos]
	}
	if i < 0 || i >= len(q.Tracks) {
		return nil
	}
	return q.Tracks[i]
}

func (q *PlaybackQueue) length() int {
	if q.Shuffle && len(q.ShuffleOrder) > 0 {
		return len(q.ShuffleOrder)
	}
	return len(q.Tracks)
}

// removeAt takes the song at a play position out of the queue.
func (q *PlaybackQueue) removeAt(pos int) {
	if pos <= q.CurrentIndex || pos >= q.length() {
		return
	}
	if q.Shuffle && len(q.ShuffleOrder) > 0 {
		ti := q.ShuffleOrder[pos]
		q.ShuffleOrder = append(q.ShuffleOrder[:pos], q.ShuffleOrder[pos+1:]...)
		q.Tracks = append(q.Tracks[:ti], q.Tracks[ti+1:]...)
		for i, v := range q.ShuffleOrder {
			if v > ti {
				q.ShuffleOrder[i] = v - 1
			}
		}
		return
	}
	q.Tracks = append(q.Tracks[:pos], q.Tracks[pos+1:]...)
}

// moveNext puts the song at a play position right after the current one.
func (q *PlaybackQueue) moveNext(pos int) {
	to := q.CurrentIndex + 1
	if pos <= to || pos >= q.length() {
		return
	}
	if q.Shuffle && len(q.ShuffleOrder) > 0 {
		v := q.ShuffleOrder[pos]
		so := append(q.ShuffleOrder[:pos:pos], q.ShuffleOrder[pos+1:]...)
		q.ShuffleOrder = append(so[:to:to], append([]int{v}, so[to:]...)...)
		return
	}
	t := q.Tracks[pos]
	ts := append(q.Tracks[:pos:pos], q.Tracks[pos+1:]...)
	q.Tracks = append(ts[:to:to], append([]*Track{t}, ts[to:]...)...)
}

// ---- the screen ----

func (np *NowPlayingView) OnLongPress(ui *IpodUI) { ui.showView(ui.upNextView(), "") }

func (ui *IpodUI) upNextView() *View {
	v := &View{ID: "upNext", Title: "Up Next", Kind: KindFull, Empty: "Nothing up next"}
	v.Build = func() []Item {
		q := ui.app.Queue
		if q == nil || ui.app.Playing.Track == nil {
			return nil
		}
		var items []Item
		n := q.length()
		for pos := q.CurrentIndex + 1; pos < n; pos++ {
			t := q.trackAt(pos)
			if t == nil {
				continue
			}
			pos := pos
			title := t.Title
			if title == "" {
				title = "Unknown Song"
			}
			items = append(items, Item{Label: title, Sublabel: t.Artist, Album: ui.trackAlbum(t),
				QueueMark: queuedMarks[t], NoArrow: true,
				OnSelect: func() { ui.playQueuePos(pos) },
				LongPress: []SheetOption{
					{Label: "Play Next", Icon: "queue_next", OnSelect: func() {
						if q.trackAt(pos) == t {
							q.moveNext(pos)
							ui.refreshView(v)
							ui.showToast("Playing Next")
						}
					}},
					{Label: "Remove from Up Next", Danger: true, OnSelect: func() {
						if q.trackAt(pos) == t {
							q.removeAt(pos)
							delete(queuedMarks, t)
							ui.refreshView(v)
							ui.dirtyAllViews()
						}
					}},
				}})
		}
		if len(items) > 1 {
			items = append(items, Item{Label: "Clear Up Next", Sublabel: fmt.Sprintf("%d songs", len(items)), NoArrow: true,
				OnSelect: func() {
					ui.showSheetTitled("Clear the songs up next?", []SheetOption{{Label: "Clear Up Next", Danger: true, OnSelect: func() {
						for p := q.length() - 1; p > q.CurrentIndex; p-- {
							if t := q.trackAt(p); t != nil {
								delete(queuedMarks, t)
							}
							q.removeAt(p)
						}
						ui.refreshView(v)
						ui.dirtyAllViews()
					}}})
				}})
		}
		return items
	}
	return v
}

// playQueuePos plays the song at a play position now.
func (ui *IpodUI) playQueuePos(pos int) {
	app := ui.app
	q := app.Queue
	if q == nil || pos >= q.length() {
		return
	}
	q.CurrentIndex = pos
	skipDir = 1
	go func() {
		app.playCurrentQueueTrack()
		ui.post(func() {
			ui.onTrackChanged()
			ui.hideView() // back to Now Playing
		})
	}()
}

// drawQueueMark: the queue icon before a song's title in a list.
func drawQueueMark(dst *image.RGBA, mark string, x, cy float64, c Col) {
	drawQueueIcon(dst, x, cy, mark == "queue_next", c)
}
