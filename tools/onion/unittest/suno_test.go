//go:build ignore

// Tests for Suno (ij_suno.go): what a typed code points to, the profile
// name, and what Suno's answers become. Run with tools/onion/unittest.sh.

package main

import (
	"encoding/json"
	"testing"
)

func TestSunoParseCode(t *testing.T) {
	id := "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0"
	for _, c := range []struct {
		in, id, short string
		playlist, ok  bool
	}{
		{"https://suno.com/song/" + id, id, "", false, true},
		{"suno.com/playlist/" + id + "?sh=x", id, "", true, true},
		{id, id, "", false, true},
		{"0F1E2D3C4B5A69788796A5B4C3D2E1F0", id, "", false, true},
		{"suno.com/s/AbC123xYz", "", "AbC123xYz", false, true},
		{" AbC123xYz ", "", "AbC123xYz", false, true},
		{"https://suno.com/s/AbC123xYz/", "", "AbC123xYz", false, true},
		{"not a code", "", "", false, false},
		{"ab", "", "", false, false},
	} {
		tg, short, ok := sunoParseCode(c.in)
		if ok != c.ok || tg.ID != c.id || short != c.short || tg.Playlist != c.playlist {
			t.Errorf("%q: got %+v %q %v", c.in, tg, short, ok)
		}
	}
}

func TestSunoHandle(t *testing.T) {
	for in, want := range map[string]string{"@lunawaves": "lunawaves", "suno.com/@kai.beats": "kai.beats",
		" name ": "name", "https://suno.com/@x/songs?a=1": "x"} {
		if got := sunoHandle(in); got != want {
			t.Errorf("%q: %q, want %q", in, got, want)
		}
	}
}

func TestSunoClip(t *testing.T) {
	var j sunoClipJSON
	json.Unmarshal([]byte(`{"id":"abc","title":" Neon ","audio_url":"https://audiopipe.suno.ai/?item_id=abc",
		"display_name":"","handle":"luna","status":"complete","created_at":"2025-03-01T00:00:00Z",
		"metadata":{"tags":"dreamy synthwave, female vocals","duration":95.5}}`), &j)
	c, ok := j.clip()
	if !ok || c.Title != "Neon" || c.Artist != "luna" || c.Audio != "https://cdn1.suno.ai/abc.mp3" ||
		c.Year != 2025 || c.genre() != "Dreamy synthwave" || c.Duration != 95.5 {
		t.Errorf("%+v %v", c, ok)
	}
	j.Status = "submitted"
	if _, ok := j.clip(); ok {
		t.Error("a song not finished was kept")
	}
}
