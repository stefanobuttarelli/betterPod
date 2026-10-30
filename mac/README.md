# betterPod Share for Mac

Sends songs and albums from a Mac to a betterPod with an 8-digit code, as a betterPod's own Share does (see [NOTES.md](NOTES.md) for how it is used). It only sends; a betterPod receives with Music → Receive.

- `core/`: the engine, in Go: it reads the songs (tags, covers), groups them by album and sends them with magic-wormhole (the vendored wormhole-william, app id `betterpod.xyz/share`). It is the same protocol as `src/ij_share.go`; keep the two in step. Built as a C archive for the app; `go run ./mac/core <files…>` sends from the command line, for tests (`BETTERPOD_WORMHOLE_URL`, `BETTERPOD_WORMHOLE_RELAY`, `BETTERPOD_SHARE_CODE`, `BETTERPOD_SHARE_FROM`).
- `App/`: the app, in SwiftUI (macOS 13+): the same studio, pedestal, laser and digit tiles as the console, at the console's size.
- `build.sh`: builds it on a Mac (Go and Xcode's command line tools): a universal app, signed ad hoc (not notarized: macOS asks before the first launch), and its zip.
- `VERSION`: the app's version. `.github/workflows/mac.yml` builds it on each change and makes the GitHub release `mac-v<version>` with `NOTES.md`.

How songs travel: one album (or some of its songs) goes as the console sends one; one song shows its title on the betterPod; songs of several albums go with one code, in a folder "From <name>" with a subfolder per album, each with its cover, and a collage of the covers on the betterPod's screen.
