// whtest: send or receive a file through a wormhole server (a test tool).
//
//	whtest send <url> <relay> <code> <file>
//	whtest recv <url> <relay> <code> <outdir>
package main

import (
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"

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
		code, res, err := c.SendDirectory(ctx, name, entries, wormhole.WithCode(os.Args[4]))
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
		n, err := io.Copy(out, m)
		out.Close()
		fmt.Println("got", m.Name, n, err)
	}
}
