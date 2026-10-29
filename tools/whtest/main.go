// whtest: send or receive a file through a wormhole server (a test tool).
//
//	whtest send <url> <relay> <code> <file>
//	whtest senddir <url> <relay> <code> <dir>   (OFFER_INFO='{"exts":[".m4a"]}': Share's formats in the offer)
//	whtest recv <url> <relay> <code> <outdir>
package main

import (
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/psanford/wormhole-william/wormhole"
)

func main() {
	c := wormhole.Client{RendezvousURL: os.Args[2], TransitRelayAddress: os.Args[3], AppID: os.Getenv("APPID")}
	ctx := context.Background()
	switch os.Args[1] {
	case "send":
		f, err := os.Open(os.Args[5])
		if err != nil {
			panic(err)
		}
		code, res, err := c.SendFile(ctx, filepath.Base(os.Args[5]), f, wormhole.WithCode(os.Args[4]))
		if err != nil {
			panic(err)
		}
		fmt.Println("code", code)
		r := <-res
		fmt.Println("sent", r.OK, r.Error)
	case "senddir":
		// every file of a folder (betterpod-share.json and cover.jpg first)
		dir := os.Args[5]
		name := filepath.Base(dir)
		files, _ := os.ReadDir(dir)
		var entries []wormhole.DirectoryEntry
		add := func(fn string) {
			p := filepath.Join(dir, fn)
			entries = append(entries, wormhole.DirectoryEntry{Path: name + "/" + fn, Mode: 0o644,
				Reader: func() (io.ReadCloser, error) { return os.Open(p) }})
		}
		for _, first := range []string{"betterpod-share.json", "cover.jpg"} {
			if _, err := os.Stat(filepath.Join(dir, first)); err == nil {
				add(first)
			}
		}
		for _, f := range files {
			if f.Name() != "betterpod-share.json" && f.Name() != "cover.jpg" {
				add(f.Name())
			}
		}
		opts := []wormhole.SendOption{wormhole.WithCode(os.Args[4])}
		if v := os.Getenv("OFFER_INFO"); v != "" {
			opts = append(opts, wormhole.WithOfferInfo([]byte(v)))
		}
		code, res, err := c.SendDirectory(ctx, name, entries, opts...)
		if err != nil {
			panic(err)
		}
		fmt.Println("code", code)
		r := <-res
		fmt.Println("sent", r.OK, r.Error)
	case "recv":
		m, err := c.Receive(ctx, os.Args[4])
		if err != nil {
			panic(err)
		}
		out, _ := os.Create(filepath.Join(os.Args[5], m.Name))
		var r io.Reader = m
		if os.Getenv("SLOW") != "" {
			r = &slowReader{m}
		}
		n, err := io.Copy(out, r)
		out.Close()
		fmt.Println("got", m.Name, n, err)
	}
}

// slowReader: small reads with pauses, to watch the sender's progress.
type slowReader struct{ r io.Reader }

func (s *slowReader) Read(p []byte) (int, error) {
	if len(p) > 16<<10 {
		p = p[:16<<10]
	}
	time.Sleep(40 * time.Millisecond)
	return s.r.Read(p)
}
