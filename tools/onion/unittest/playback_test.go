//go:build ignore

// Tests for the saved session (playback_state.go): the current song stays
// current when the queue comes back, with shuffle on or off and songs gone
// from the library. Run with tools/onion/unittest.sh.

package main

import "testing"

func restoreLib(paths ...string) func(string) *Track {
	m := map[string]*Track{}
	for _, p := range paths {
		m[p] = &Track{Path: p}
	}
	return func(p string) *Track { return m[p] }
}

// currentPath: the song at play position cur.
func currentPath(tracks []*Track, cur int, order []int) string {
	i := cur
	if order != nil {
		i = order[cur]
	}
	return tracks[i].Path
}

func TestRestoreQueue(t *testing.T) {
	all := restoreLib("a", "b", "c", "d", "e")
	cases := []struct {
		name string
		ps   PlaybackState
		lib  func(string) *Track
		want string
	}{
		{"plain", PlaybackState{TrackPaths: []string{"a", "b", "c"}, CurrentIndex: 1}, all, "b"},
		{"plain, one gone before", PlaybackState{TrackPaths: []string{"a", "x", "c"}, CurrentIndex: 2}, all, "c"},
		{"plain, the current gone", PlaybackState{TrackPaths: []string{"a", "x", "c"}, CurrentIndex: 1}, all, "c"},
		// shuffle: CurrentIndex is a place in ShuffleOrder, not a track
		{"shuffle, old file", PlaybackState{TrackPaths: []string{"a", "b", "c", "d"}, CurrentIndex: 1,
			Shuffle: true, ShuffleOrder: []int{3, 0, 2, 1}}, all, "a"},
		{"shuffle, with path", PlaybackState{TrackPaths: []string{"a", "b", "c", "d"}, CurrentIndex: 2, CurrentPath: "c",
			Shuffle: true, ShuffleOrder: []int{3, 0, 2, 1}}, all, "c"},
		{"shuffle, one gone", PlaybackState{TrackPaths: []string{"a", "x", "c", "d"}, CurrentIndex: 3, CurrentPath: "c",
			Shuffle: true, ShuffleOrder: []int{1, 3, 0, 2}}, all, "c"},
		{"shuffle, the current gone", PlaybackState{TrackPaths: []string{"a", "x", "c", "d"}, CurrentIndex: 1, CurrentPath: "x",
			Shuffle: true, ShuffleOrder: []int{3, 1, 0, 2}}, all, "a"},
		{"shuffle, no order", PlaybackState{TrackPaths: []string{"a", "b", "c"}, CurrentIndex: 2, Shuffle: true}, all, "c"},
		{"shuffle, bad order", PlaybackState{TrackPaths: []string{"a", "b", "c"}, CurrentIndex: 0, CurrentPath: "b",
			Shuffle: true, ShuffleOrder: []int{9, 1}}, all, "b"},
	}
	for _, c := range cases {
		tracks, cur, order := restoreQueue(&c.ps, c.lib)
		if c.ps.Shuffle && len(order) != len(tracks) {
			t.Errorf("%s: order %v for %d tracks", c.name, order, len(tracks))
			continue
		}
		if got := currentPath(tracks, cur, order); got != c.want {
			t.Errorf("%s: current %q, want %q", c.name, got, c.want)
		}
	}
	if tr, _, _ := restoreQueue(&PlaybackState{TrackPaths: []string{"x"}}, all); len(tr) != 0 {
		t.Errorf("nothing left: %d tracks", len(tr))
	}
}

func TestQueueKey(t *testing.T) {
	a := []*Track{{Path: "ab"}, {Path: "c"}}
	b := []*Track{{Path: "a"}, {Path: "bc"}}
	if queueKey(a, nil) == queueKey(b, nil) {
		t.Error("paths split differently give the same key")
	}
	if queueKey(a, []int{0, 1}) == queueKey(a, []int{1, 0}) {
		t.Error("orders give the same key")
	}
	if queueKey(a, []int{0, 1}) != queueKey(a, []int{0, 1}) {
		t.Error("the same queue gives two keys")
	}
}
