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
