//go:build ignore

// Scratch-copy fixture hooks only. Actual Go search/detail/job renderers run.
package main

import (
	"testing"
	"time"
)

var qaYTSearchEnabled bool
var qaYTInfo *ytInfo

func qaYTEntries() []ytEntry {
	return []ytEntry{
		{ID: "abcdefghijk", Title: "Fixture Video", Channel: "Fixture Artist", Duration: 245, URL: "https://www.youtube.com/watch?v=abcdefghijk"},
		{ID: "PLfixture", Title: "Fixture Album", Channel: "Fixture Artist", Type: "playlist", Count: 3, URL: "https://www.youtube.com/playlist?list=PLfixture"},
	}
}
func qaYTSeedSearch(term, ran *string, busy, playlists *bool, entries *[]ytEntry) {
	if !qaYTSearchEnabled {
		return
	}
	*term = "Fixture Artist"
	*ran = *term
	*busy = false
	*playlists = false
	*entries = qaYTEntries()
}
func qaSeedYouTube(t *testing.T, ui *IpodUI, screen string) bool {
	t.Helper()
	qaYTSearchEnabled = false
	qaYTInfo = nil
	var view *View
	switch screen {
	case "youtube-search", "youtube-gear", "youtube-row-loader":
		qaYTSearchEnabled = true
		view = ui.youtubeSearchView(false)
		ui.keyboard = nil
		view.Index = 0
		if screen == "youtube-gear" {
			view.gearOn = true
		}
		if screen == "youtube-row-loader" {
			view.Index = 2
			build := view.Build
			view.Build = func() []Item { items := build(); items[2].NoArrow = true; return items }
			view.Items = view.Build()
		}
	case "youtube-settings":
		playlists := true
		view = ui.ytSearchSettingsView(&playlists, func() {})
	case "youtube-video-detail", "youtube-playlist-detail":
		head := ytEntry{ID: "abcdefghijk", Title: "Fixture Video", Channel: "Fixture Artist", Duration: 245, URL: "https://www.youtube.com/watch?v=abcdefghijk"}
		info := &ytInfo{ytEntry: head, Chapters: []ytChapter{{Start: 0, End: 80, Title: "First Song"}, {Start: 80, End: 160, Title: "Second Song"}, {Start: 160, End: 245, Title: "Third Song"}}}
		if screen == "youtube-playlist-detail" {
			head = ytEntry{ID: "PLfixture", Title: "Fixture Album", Channel: "Fixture Artist", Type: "playlist", Count: 3, URL: "https://www.youtube.com/playlist?list=PLfixture"}
			info = &ytInfo{ytEntry: head, Entries: []ytEntry{
				{ID: "abcdefghijk", Title: "First Song", Channel: "Fixture Artist", Duration: 245},
				{ID: "lmnopqrstuv", Title: "Second Song", Channel: "Fixture Artist", Duration: 245},
				{ID: "wxyz1234567", Title: "Third Song", Channel: "Fixture Artist", Duration: 245},
			}}
		}
		qaYTInfo = info
		view = ui.ytItemView(head, nil)
	case "youtube-download", "youtube-download-error", "youtube-download-queued":
		iaCur = &iaDownload{item: &iaItem{Title: "Fixture Album", Creator: "Fixture Artist"}, source: "YouTube", format: "M4A", whole: true, state: "Track 2 of 3 · Downloading", done: 5000000, total: 10000000, files: []iaFile{{Name: "one"}, {Name: "two"}, {Name: "three"}}, cancel: make(chan struct{})}
		if screen == "youtube-download-error" {
			iaCur.finished = true
			iaCur.err = "Fixture connection failed"
			iaCur.state = "Stopped"
			iaCur.again = func() {}
		}
		if screen == "youtube-download-queued" {
			iaCur.item.Title = "Next Album"
			iaCur.state = "Waiting in queue"
			iaCur.retry = "1 download ahead"
			iaCur.done = 0
		}
		view = ui.iaDownloadView()
	default:
		return false
	}
	view.ID = screen
	ui.showView(view, "")
	return true
}
func qaYTPostDraw(ui *IpodUI, screen string) {
	if screen == "youtube-row-loader" {
		// User-requested addition: canonical Go spinner, at Rust's fixed phase.
		drawSpinnerIn(ui.fb, 616, 224, 12, time.Unix(0, 1000000000), colWhite)
	}
}
