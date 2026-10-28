package main

// ij_recent.go — Recent Searches: the searches of the last 7 days, listed
// under the search field of Internet Archive, YouTube and Podcasts until a
// new search is run (newest first, at most 10 per search screen). Choosing
// one runs it again; Y (or holding A) on one removes it or clears them all.
// Kept in recent_searches.json in the data folder.

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"sync"
	"time"
)

const (
	recentDays = 7
	recentMax  = 10
)

type recentSearch struct {
	Term string    `json:"term"`
	At   time.Time `json:"at"`
}

var recent struct {
	read bool
	m    map[string][]recentSearch // search screen → searches, newest first
	mu   sync.Mutex                // the file writes, in order
}

func recentPath() string { return dataPath("recent_searches.json") }

// recentLoad: on the UI goroutine.
func recentLoad() {
	if recent.read {
		return
	}
	recent.read = true
	recent.m = map[string][]recentSearch{}
	if b, err := os.ReadFile(recentPath()); err == nil {
		_ = json.Unmarshal(b, &recent.m)
	}
}

func recentSave() {
	b, _ := json.MarshalIndent(recent.m, "", "  ")
	go func() {
		recent.mu.Lock()
		defer recent.mu.Unlock()
		if err := writeFileAtomic(recentPath(), b); err != nil {
			logMsg("Recent searches: " + err.Error())
		}
	}()
}

// recentSearches: kind's searches of the last 7 days, newest first.
func recentSearches(kind string) []recentSearch {
	recentLoad()
	cut := time.Now().AddDate(0, 0, -recentDays)
	var out []recentSearch
	for _, r := range recent.m[kind] {
		if r.At.After(cut) {
			out = append(out, r)
		}
	}
	return out
}

// addRecentSearch puts term first (once: the same words in any case).
func addRecentSearch(kind, term string) {
	term = strings.TrimSpace(term)
	if term == "" {
		return
	}
	l := []recentSearch{{term, time.Now()}}
	for _, r := range recentSearches(kind) {
		if !strings.EqualFold(r.Term, term) && len(l) < recentMax {
			l = append(l, r)
		}
	}
	recent.m[kind] = l
	recentSave()
}

func removeRecentSearch(kind, term string) {
	var l []recentSearch
	for _, r := range recentSearches(kind) {
		if r.Term != term {
			l = append(l, r)
		}
	}
	recent.m[kind] = l
	recentSave()
}

func clearRecentSearches(kind string) {
	recentLoad()
	delete(recent.m, kind)
	recentSave()
}

// recentWhen: "Today", "Yesterday", "3 days ago".
func recentWhen(t, now time.Time) string {
	y1, m1, d1 := t.Date()
	day := time.Date(y1, m1, d1, 0, 0, 0, 0, time.Local)
	y2, m2, d2 := now.Date()
	n := int(time.Date(y2, m2, d2, 0, 0, 0, 0, time.Local).Sub(day).Hours()/24 + 0.5)
	switch {
	case n <= 0:
		return "Today"
	case n == 1:
		return "Yesterday"
	}
	return fmt.Sprintf("%d days ago", n)
}

// recentItems: the rows of kind's recent searches (none: nil). pick runs
// one; refresh redraws the screen after one is removed.
func (ui *IpodUI) recentItems(kind string, pick func(term string), refresh func()) []Item {
	rs := recentSearches(kind)
	now := time.Now()
	var items []Item
	for _, r := range rs {
		term := r.Term
		items = append(items, Item{Label: term, Sublabel: "Recent search · " + recentWhen(r.At, now),
			OnSelect: func() { pick(term) },
			LongPress: []SheetOption{
				{Label: "Remove from Recent Searches", Danger: true, OnSelect: func() {
					removeRecentSearch(kind, term)
					refresh()
				}},
				{Label: "Clear Recent Searches", Danger: true, OnSelect: func() {
					clearRecentSearches(kind)
					refresh()
				}},
			}})
	}
	return items
}
