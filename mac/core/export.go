package main

// export.go — the C functions the Swift app calls (go build
// -buildmode=c-archive writes them into libbpshare.h). Everything is JSON
// in C strings; the app frees what it gets with BPFree. Scanning is slow
// with many songs: the app calls BPScan off its main thread.
//
//	BPScan(paths)      the files and folders dropped (a JSON array) →
//	                   {"id", "kind", "title", "artist", "songs", "bytes",
//	                    "cover" (JPEG, base64), "albums": [{"album", "artist",
//	                    "songs", "cover"}], "skipped", "m4a"} or {"error"}
//	BPSend(id, from)   sends that selection → a share's id
//	BPStatus(share)    {"phase", "code", "done", "total", "rate", "error",
//	                    "unplayable", "on"}
//	BPCancel(share)

/*
#include <stdlib.h>
*/
import "C"

import (
	"encoding/json"
	"strings"
	"sync"
	"unsafe"
)

var (
	regMu      sync.Mutex
	selections = map[int]*Selection{}
	shares     = map[int]*Share{}
	nextID     = 1
)

type albumJSON struct {
	Album  string `json:"album"`
	Artist string `json:"artist"`
	Songs  int    `json:"songs"`
	Cover  []byte `json:"cover,omitempty"`
}

type scanJSON struct {
	ID      int         `json:"id,omitempty"`
	Kind    string      `json:"kind,omitempty"`
	Title   string      `json:"title,omitempty"`
	Artist  string      `json:"artist,omitempty"`
	Songs   int         `json:"songs,omitempty"`
	Bytes   int64       `json:"bytes,omitempty"`
	Cover   []byte      `json:"cover,omitempty"`
	Albums  []albumJSON `json:"albums,omitempty"`
	Skipped []string    `json:"skipped,omitempty"`
	M4A     bool        `json:"m4a,omitempty"`
	Error   string      `json:"error,omitempty"`
}

func cJSON(v any) *C.char {
	b, _ := json.Marshal(v)
	return C.CString(string(b))
}

//export BPScan
func BPScan(paths *C.char) *C.char {
	var ps []string
	if err := json.Unmarshal([]byte(C.GoString(paths)), &ps); err != nil {
		return cJSON(scanJSON{Error: err.Error()})
	}
	sel, err := Scan(ps)
	if err != nil {
		return cJSON(scanJSON{Error: err.Error()})
	}
	out := scanJSON{Kind: sel.Kind, Title: sel.Title(), Artist: sel.Artist(), Songs: sel.Songs, Bytes: sel.Bytes,
		Cover: sel.CoverJPG, Skipped: sel.Skipped}
	for _, g := range sel.Groups {
		out.Albums = append(out.Albums, albumJSON{Album: g.Album, Artist: g.Artist, Songs: len(g.Songs), Cover: g.CoverJPG})
		for _, t := range g.Songs {
			if strings.EqualFold(extOf(t.Path), ".m4a") {
				out.M4A = true
			}
		}
	}
	regMu.Lock()
	out.ID = nextID
	selections[nextID] = sel
	nextID++
	regMu.Unlock()
	return cJSON(out)
}

//export BPSend
func BPSend(id C.int, from *C.char) C.int {
	regMu.Lock()
	defer regMu.Unlock()
	sel := selections[int(id)]
	if sel == nil {
		return 0
	}
	s := StartShare(sel, C.GoString(from))
	sid := nextID
	nextID++
	shares[sid] = s
	return C.int(sid)
}

//export BPStatus
func BPStatus(id C.int) *C.char {
	regMu.Lock()
	s := shares[int(id)]
	regMu.Unlock()
	if s == nil {
		return cJSON(Status{Phase: phFailed, Error: "No such share"})
	}
	return cJSON(s.Status())
}

//export BPCancel
func BPCancel(id C.int) {
	regMu.Lock()
	s := shares[int(id)]
	delete(shares, int(id))
	regMu.Unlock()
	if s != nil {
		s.Cancel()
	}
}

//export BPFree
func BPFree(p *C.char) {
	C.free(unsafe.Pointer(p))
}
