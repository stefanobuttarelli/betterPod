# betterPod Rust — local experiment

A new, independent Rust application for the Anbernic RG35XX Plus / Knulli H700. Its priority is uninterrupted music, followed by a regular interface cadence. The motion target is 60 fps; that is a target to measure on hardware, not a guarantee.

This branch contains an experimental Rust port of the betterPod 1.1 feature set.
The source-faithful UI and behavior implementation pass is complete locally;
its finite checks and deferred hardware acceptance are recorded in
[PARITY.md](PARITY.md). Historical
prototype results do not validate this expanded workload. The first on-console UI/audio-counter comparison is recorded in
[CONSOLE_QA.md](CONSOLE_QA.md). The subsequent Sony Bluetooth session, its
YouTube memory failure and the corrective verification are recorded in
[BLUETOOTH_QA.md](BLUETOOTH_QA.md). The original app and
its release artifacts remain available. No Rust build is pushed, released or
installed automatically.

## Isolation

The experimental package has its own Ports entry, `betterPod Rust.sh`, and app directory, `betterpod-rust/`. Its settings, playback state, artwork cache and log are separate. The launcher can read the original app's music-folder pointer and library cache, but does not move or modify them. The music directory is shared for playback; tests should use a copied fixture.

Application logic is written in Rust. Native third-party libraries remain: SDL2 for platform access, the existing patched SDL_mixer/codec libraries, and the unchanged MIT-licensed Simon Tatham puzzle engines. Custom game frontends, drawing, input, storage and application logic are Rust. No Go application binary or old custom C frontend is invoked.

Pinned Rust dependency notices are included in `THIRD_PARTY_NOTICES.txt`. Dependencies retain their own licenses, including EUPL-1.2 for magic-wormhole and MPL-2.0 for Symphonia. Native library, puzzle and asset notices are also included in both local packages.

## Build and test locally

Docker isolates the Linux toolchain. The image pins Rust 1.98.1 and builds on the existing Knulli compiler image. `Cargo.lock` pins the dependency resolution. Build targets live in named Docker volumes on the Linux filesystem; only final binaries and test artifacts are copied to `build/rust/`. Two compiler jobs are used by default (`CARGO_BUILD_JOBS` can override this). The host needs Docker; Rust/SDL installation on macOS is not required for these commands.

The Knulli linker uses GNU ld with a SHA-256-pinned glibc 2.28 sysroot. This preserves Rust's Cortex-A53 erratum 843419 workaround, which the older Zig linker cannot forward. Zig still compiles the unchanged third-party C engines and crypto sources.

```sh
rust/tools/build.sh test
rust/tools/build.sh native
rust/tools/smoke.sh /absolute/path/to/copied/music
```

The native Linux executable is `build/rust/betterpod-rust.native`. On an aarch64 Docker host, native smoke tests load the exact patched mixer/codec libraries from `port/betterpod/libs`; other architectures use their system mixer. Smoke tests run all five scripted phases for 30 seconds, require a non-empty library plus advancing active playback, and use Xvfb, Mesa software rendering and dummy audio, with temporary settings and outputs under `build/rust/`. They check initialization, app behavior and drawing. They do not establish physical H700 scanout timing, audible continuity or Bluetooth stability.

On macOS, a separate windowed app can be built locally:

```sh
rust/tools/build-mac.sh
open 'dist/betterPod Rust.app'
```

This requires the host Rust toolchain. The script downloads official SDL2 and SDL2_mixer frameworks at pinned versions with SHA-256 verification, bundles them inside the local app, and signs the bundle locally. It uses `~/Music` by default and private state in `~/Library/Application Support/betterPod Rust`; environment options can select copied fixtures. The Mac mixer is the upstream framework, while the Linux/aarch64 build uses the patched Knulli codec bundle. Mac measurements do not establish console performance or codec parity.

Mac video and the Opus fallback need `ffmpeg` on PATH, or `BETTERPOD_RUST_FFMPEG` set to its executable. Knulli normally supplies it. The Mac preview uses the app's monotonic frame clock rather than SDL's display-link synchronization, which can block an occluded test window. Linux requests vsync by default.

To prepare a console build without deploying it:

```sh
rust/tools/build.sh knulli
rust/tools/package.sh
```

This produces `build/rust/betterpod-rust.knulli` and `dist/betterpod-rust-experimental.zip`. The build validates AArch64 and a glibc requirement no newer than 2.28. The package uses Knulli's SDL2 plus the patched mixer in its own `libs/` folder. Extraction into Ports is a separate, explicit test step; no script here connects to the console.

## Runtime options

All options use the `BETTERPOD_RUST_` prefix. They do not override the stable app's `BETTERPOD_*` environment.

| Variable | Purpose |
|---|---|
| `BETTERPOD_RUST_MUSIC_DIR` | Music directory to read. |
| `BETTERPOD_RUST_DATA_DIR` | Default independent writable state/cache directory. |
| `BETTERPOD_RUST_BOOTSTRAP_DIR` | Private root for this app's saved data/music pointers. |
| `BETTERPOD_RUST_IMPORT_CACHE` | Optional old library index to read, without writing it. |
| `BETTERPOD_RUST_ASSETS_DIR` | Fonts and artwork assets. |
| `BETTERPOD_RUST_IDLE_FILE` | App-owned Knulli battery-saver pause marker; launcher cleans it on exit. |
| `BETTERPOD_RUST_FPS` | Motion target, default 60; 30 provides a comparison. |
| `BETTERPOD_RUST_VSYNC` | Override swap synchronization: `0` off, `1` on. At the reported display rate, swap owns pacing without a second timer. |
| `BETTERPOD_RUST_MSAA` | GPU edge antialiasing: `4` (default), `2` or `0`. Unsupported multisampling retries an ordinary framebuffer. |
| `BETTERPOD_RUST_BENCHMARK=1` | Run the scripted benchmark workload. |
| `BETTERPOD_RUST_METRICS` | Opt-in in-memory frame/audio/video metrics, written to CSV at exit; works with ordinary navigation too. |
| `BETTERPOD_RUST_QA_DIR` | Explicit local diagnostic Unix socket and PNG directory. Use `/tmp/…` on Knulli; exFAT cannot host a Unix socket. Disabled by default. |
| `BETTERPOD_RUST_QA_TRACE_INPUT` | Opt-in physical input logging; leave unset during timing runs. |
| `BETTERPOD_RUST_EXIT_AFTER_SECONDS` | Bounded local smoke/benchmark duration. |
| `BETTERPOD_RUST_SCREENSHOT` | PNG output before a bounded run exits. |
| `BETTERPOD_RUST_SWAP_AB=0` | Use SDL's face-button labels instead of Anbernic's mapping. |
| `BETTERPOD_RUST_INSTALLATION` | Ports root for the independent Rust updater. |
| `BETTERPOD_RUST_UPDATE_FEED` | Optional explicitly configured Rust-only manifest URL; no feed by default. |

## Controls

| Console | Desktop keyboard | Action |
|---|---|---|
| D-pad / stick | Arrows | Navigate; held directions repeat. |
| A | Enter / A | Tap to select; hold 0.5 s for options (Solitaire: undo). |
| B | Backspace / B / Escape | Tap to go back; hold 0.5 s for Home. |
| X | Space / X | Play or pause music. |
| START | F1 | Game menu/pause; otherwise play or pause. |
| Y | Y / left Alt | Options. |
| L / R | E / T or P / N | Previous / next. |
| SELECT | H / right Ctrl | Hold control (press/release). |
| — | F / R | Search. |
| MENU | M / Application | Press/release screen control; hold behavior belongs to the app. |
| L2 / R2 | Page Down / Page Up | Brightness down / up. |
| System volume | Minus / Equals | Volume down / up. |
| — | Q | Quit. |

Headset media play/pause/next/previous keys are read by a separate Linux worker without grabbing input devices. Knulli owns the physical volume and POWER buttons. The experimental input layer does not intercept those buttons through a second key-mapping process.

Before suspending EmulationStation, the Knulli launcher uses the Rust executable to park only its verified audio streams on the stable internal output. It restores their mute/routing policy and process state on exit. This prevents a frozen frontend client from blocking Bluetooth reconnection; firmware audio/graphics libraries stay unchanged.

## Rewrite coverage

A rewrite is complete only when behavior and data handling have been checked against the stable app. Keep the following as a verification checklist rather than declaring completion from a rendering demo:

- Library scan/import, metadata, albums/artists/songs and read-only source handling.
- Playback queue, shuffle/repeat, seek, pause/resume and next/previous.
- Independent durable preferences and playback restoration.
- Home/list navigation, Now Playing, Cover Flow, Up Next and artwork invalidation.
- Knulli controls, brightness, screen off, battery-saver protection and cleanup.
- Playlists, search and media options.
- Bluetooth/system volume integration and headset controls.
- Online search, downloads, Listen, podcasts, video and Share.
- Album metadata editing, database/music moves and recovery behavior.
- Screensavers, apps/games, themes and accessibility settings.

A local graphics smoke cannot validate every item. Hardware runs must inspect frame-time percentiles, missed refreshes, CPU/RAM and PipeWire error deltas together with listening. Keep audio formats and buffers comparable; preserve the old app as the baseline.

## Filesystem and update safety

Music transfer and database maintenance use one worker with throttled I/O; Linux also requests low CPU and idle disk priority for that worker. Music is copied, hashed, read back and verified before switching roots. Removing originals requires explicit confirmation and an acknowledgement that application clients have switched. Interrupted music moves retain a manifest that can resume even after the pointer switched. File and directory entries are synced before recording verified copies and before removing originals; Linux uses `syncfs` only when a filesystem does not support directory `fsync`. Other platforms provide best effort for unsupported directory sync. Database moves require flushed state and stopped clients, preserve cover choices, and remove only verified files from this app's allowlist. Clear Artwork preserves deliberately chosen covers; Clear App Data preserves private downloads and tools. The separate Everything Including Tools reset is explicit.

The independent updater shows **Local build** until a Rust feed is configured. It rejects stable betterPod manifests, foreign installation markers, untrusted archive paths, symlinks, configuration overwrites, incorrect size/SHA-256 and executables with the wrong architecture. Commit installs the executable last and keeps an on-disk rollback journal. A failed running transaction restores prior files. Startup recovery rolls back an incomplete journal before app clients open; after rollback the process must restart. This is not a fully atomic directory/version switch: native libraries can be loaded before Rust startup recovery runs. The prototype therefore rejects replacement of existing codec libraries, and enabling a real update feed still requires review of loader compatibility and recovery. No update is downloaded, applied or published by the build/test scripts.

Useful local regression commands:

```sh
rust/tools/check-filesystem.sh
rust/tools/check-storage.sh
rust/tools/check-stream-proxy.sh
rust/tools/check-media.sh
rust/tools/check-app.sh
rust/tools/check-video.sh
rust/tools/check-games.sh
rust/tools/check-puzzles.sh
rust/tools/check-weather.sh
```

Filesystem/update tests use temporary synthetic fixtures and loopback HTTP only. Video tests generate A/V and silent clips and use dummy audio. These checks do not replace listening or physical Bluetooth tests on the console.

## Rust service implementation and verification

The interactive service routes use bounded workers. Network requests, RSS parsing, artwork decoding, downloads, metadata edits and filesystem operations run outside the render thread. Jobs expose progress and cancellation. Listen, services, playlists, Share, file browsing and curation have explicit joined shutdown barriers before a database move or reset. Playback state accepts a shared immutable queue; its JSON is serialized by the state writer rather than by the UI.

| Area | Implemented Rust behavior | Verification and remaining work |
|---|---|---|
| Library and playlists | Canonical music-root scan, read-only legacy index/M3U import, private completed downloads and received songs, folder/embedded artwork, playlist editing in private data only. | Unit fixtures cover Unicode/relative paths, stale and poisoned caches, symlink escapes, private downloads and durable writes. Imported playlists remain read-only. |
| Online sources | Internet Archive, YouTube/playlist search, iTunes podcast directory, MusicBrainz, iTunes, Deezer and Wikipedia metadata; real provider APIs and cancellable yt-dlp resolution. | UI routes and worker implementations exist. Unit tests and local HTTP fixtures do not establish live-provider availability, search quality or rate-limit behavior. |
| Listen | Inline loading/play/pause/retry rows, immediate preservation of the user's music, private temporary current/next downloads, AAC/M4A SIDX parts grouped at whole fragments around 20 seconds then 180 seconds, whole-file fallback, and music restoration on leaving, Download or final EOF. | Real loopback tests cover current/next bounds, reuse, retry, EOF and joined cancellation. An offline FFmpeg test generated AAC, fetched its actual HTTP byte ranges and verified that concatenating the compressed packets from all remuxed parts exactly reproduces the original packets. Decoder cadence, audible transitions and live YouTube behavior still need their separate checks. |
| Downloads and podcasts | Length/identity-checked resumable HTTP, source-codec YouTube extraction, chapter splitting, private subscription/download records, actual-container sniffing, selected episode/whole-feed download deletion. | Local HTTP tests exercise resume and reject HTML pretending to be audio; a mislabelled podcast response keeps the correct container. Whole-feed download deletion preserves its subscription; unsubscribe is separate. Concurrent feed download/deletion is refused. |
| Covers and album information | Worker-cached previews, square cover anchors for YouTube, manual tag editing, conservative track matching, persistent chosen covers, logical merges and separation, Missing Info/Missing Covers lists. | Synthetic crop/cache tests and controller tests pass. Logical operations write `curation.json` and `chosen-covers/`, preserving source audio/tag bytes. Cache invalidation includes the manifest content digest to handle coarse exFAT timestamps. Explicit tag editing is a separate operation that does modify the selected music file. |
| Share | Eight-digit betterPod codes, `betterpod.xyz/share`, SPAKE2, encrypted transit records, directory ZIP offers, manifest/receiver information and SHA-256 acknowledgement. | Actual synthetic transfers passed Rust↔Rust and both directions against the exact vendored stable Go fork, including a `000` nameplate. Debug and optimized Rust peers were tested through loopback-only rendezvous/relay services, with no public service or user music involved. Physical Mac/console UI interaction, public relay/NAT behavior and H700 resource use remain separate checks. |
| Screensavers | Cover Flip mosaic, album track cards, clock/song overlays, Bars, Waves, Tunnel, Stars and Spectrogram, eight palettes and cover-derived colours. | A fixed 1024-sample Hann FFT supplies 40 logarithmic bands; tone placement, invalid samples, silence decay and selection behavior have focused tests. GPU geometry replaces the old CPU framebuffer effects. Physical display cadence and audio under each effect still need measurement. |
| File and library tools | Worker file browsing, explicit confirmed deletion, cached library statistics, disk capacity and recorded scanned/skipped-folder reasons. | Source deletion is limited to selected music/private-download roots, protects application data, preflights directory contents, refuses symlinks and uses pinned directory handles. Unit fixtures verify confirmation, containment and private-download handling; real SDL/GLES controller navigation also passes. |

Synthetic AAC, ALAC, MP3, FLAC and Ogg files were generated locally for metadata roundtrips. Independent FFmpeg/ffprobe checks found identical decoded-audio SHA-256 values before/after editing and preserved unrelated comment, composer and copyright fields plus track/disc totals. AAC and ALAC in the same `.m4a` container are distinguished by codec, rather than assigning a lossless badge from the filename.

The inline tests live with `library`, `storage`, `playlists`, `podcasts`, `downloads`, `listen`, `metadata`, `share`, `saver`, `curation` and `files`; the two metadata codec fixture tests are ignored by default and require `BETTERPOD_METADATA_FIXTURES` to point to generated media. The ignored `listen::qa::native_sidx_parts_preserve_every_compressed_aac_packet_exactly_once` test generates its own media and requires local FFmpeg. Session evidence includes `build/rust-library-tests/fixtures/independent-audio-check.json`, `build/rust-share-local/interop-debug-proof.json`, `build/rust-share-local/interop-proof.json` and `build/rust/listen-aac-roundtrip.log`. These are generated local verification artifacts, not release assets. They do not replace complete application/controller and hardware testing.

## Local data and migration boundaries

There is no automatic migration or replacement of the stable installation. The launcher can read its music pointer and library index; the Rust app creates fresh isolated preferences, playback, subscriptions, game state and private playlists. Original M3U files are imported for reading. New downloads and received music go under the Rust data directory's `downloads/`, and appear in its library alongside the shared music folder.

Database moves include `settings.json`, `playback.json`, `games.json`, `podcasts.json`, `video-history.json`, `curation.json`, private playlists/downloads, saved `videos/` and `chosen-covers/`. `artwork/` is a disposable cache; chosen covers and logical grouping are persistent user choices. All workers and the state writer stop before switching roots; constructing a fresh app reconnects clients to the selected roots. Explicit music transfer, deletion or tag-edit commands are the operations that can change shared music, and must be tested on copied fixtures first.

No branch publication, pull request, stable migration or release is performed by these local build/test tools. The source port and local UI/actor checks do not establish every live-service outcome or physical performance. The separate Anbernic acceptance session must prioritize listening and verify frame timing, controls and Bluetooth with the completed UI workload.

Weather animation loops are prepared completely on a worker and published as
one texture atlas, so a missing phase cannot reset the icon to its still pose.
The original 20 fps sprite sequence remains unchanged.

Static cover art uses mipmaps and bounded anisotropic filtering for cleaner
perspective images. Odd-sized mip levels are prepared on the artwork workers;
video frames and glyph atlases do not generate mipmaps. The 40-cover cache
remains bounded; mipmaps add up to about one third to its GPU storage.

Bluetooth device rows include the five source device pictures, generated at
twice their display size and classified by name/BlueZ icon. The local follow-up
and pending hardware checks are recorded in [BLUETOOTH_QA.md](BLUETOOTH_QA.md).

## SSH diagnostics

Only an explicit `BETTERPOD_RUST_QA_DIR=/tmp/bp-qa` enables the private
`control.sock` (mode 0600). It accepts a JSON line such as
`{"action":"b","frames":30,"capture":"back"}` and returns the projected menu,
panel positions, playback state and optional screenshot path. Actions enter the
ordinary controller handler. `frames` is bounded to 120; omit `action` for an
observation. PNG encoding and socket/file I/O run on a worker. Captures perform
GPU readback, so take them outside measured intervals. No listener is created
in an ordinary launch. `tools/console-ui.py` optionally drives Linux input and
`fbgrab` for comparison with the Go app; it requires an explicit device/socket.

Metrics distinguish swap completion from video frames delivered to the UI.
They do not establish LCD scanout, audible quality or Bluetooth continuity.
