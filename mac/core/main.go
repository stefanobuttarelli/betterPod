package main

// main.go — run as a program (not as the app's library), betterPod Share's
// engine sends from the command line, for tests:
//
//	go run ./mac/core <files or folders…>
//
// BETTERPOD_WORMHOLE_URL and BETTERPOD_WORMHOLE_RELAY point it at other
// servers, BETTERPOD_SHARE_FROM names the sender, BETTERPOD_SHARE_CODE
// fixes the code.

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

func extOf(p string) string { return strings.ToLower(filepath.Ext(p)) }

func main() {
	if len(os.Args) < 2 {
		fmt.Println("usage: bpshare <files or folders…>")
		os.Exit(2)
	}
	sel, err := Scan(os.Args[1:])
	if err != nil {
		fmt.Println("error:", err)
		os.Exit(1)
	}
	fmt.Printf("%s: %q by %q, %d songs, %d bytes\n", sel.Kind, sel.Title(), sel.Artist(), sel.Songs, sel.Bytes)
	for _, g := range sel.Groups {
		fmt.Printf("  %q / %q: %d songs, cover %v\n", g.Album, g.Artist, len(g.Songs), g.CoverJPG != nil)
	}
	if len(sel.Skipped) > 0 {
		fmt.Println("  skipped:", sel.Skipped)
	}
	if c := os.Getenv("BETTERPOD_SHARE_COVER_OUT"); c != "" {
		os.WriteFile(c, sel.CoverJPG, 0o644)
	}
	s := StartShare(sel, os.Getenv("BETTERPOD_SHARE_FROM"))
	last := ""
	for {
		st := s.Status()
		line := fmt.Sprintf("%s code %s %d/%d %.0f B/s %s", st.Phase, st.Code, st.Done, st.Total, st.Rate, st.Error)
		if st.Phase != last {
			fmt.Println(line)
			last = st.Phase
		}
		if st.Phase == phDone || st.Phase == phFailed {
			return
		}
		time.Sleep(200 * time.Millisecond)
	}
}
