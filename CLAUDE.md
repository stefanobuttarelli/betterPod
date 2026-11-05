# betterPod — project notes

betterPod is an iPod-style music player for Anbernic handhelds running Knulli on the Allwinner H700 (aarch64). The interface is rebuilt natively from iPod.js. Knulli is the only supported console platform. `mac/` contains the separate betterPod Share companion app.

## Rules

- **English everywhere:** code, comments, README, changelog and commits. Use short, plain sentences.
- **Audio first, then fluidity.** Music must not crackle. Avoid steady work in frame loops; measure before adding it.
- **One version.** `BETTERPOD_VERSION` in `src/ij_update.go` names the app and package. `APP_VERSION` aliases it for the classic interface.
- **One changelog.** Each release has a `## v<version> (YYYY-MM-DD)` section. A stable release summarizes changes since the previous stable release.
- **Build and test Knulli** before release: `tools/unittest.sh`, `tools/docker-build.sh`. Run `python3 -m unittest discover -s tools -p 'test_gh_releases.py'` for release-script changes.
- **Preserve existing data.** `src/compat.go` keeps the established database filenames and accepts the environment prefix from older Knulli launchers. The launcher and current options use `BETTERPOD_*`. Do not rename persisted data without a tested migration. Keep third-party copyright and license notices.

## Knulli runtime

- **Hardware:** Allwinner H700, four Cortex-A53 cores, 1 GB RAM, aarch64, 640×480 display. Go can use all cores.
- **Launch:** `port/betterPod.sh` → `/userdata/roms/ports/betterPod.sh`; application files in `ports/betterpod/`, binary `betterpod`. PortMaster's `control.txt` supplies controller mappings.
- **Paths:** `/userdata/music` (the network music share), otherwise `betterpod/music`; database defaults to `betterpod/conf`, movable in Settings → Database; additional cards under `/media`. App log `betterpod/betterpod.log`, launcher output `betterpod/log.txt`.
- **Video:** system SDL2 with vsync. C pixel loops remain an optional optimization (`BETTERPOD_C_DRAW=1`); Go drawing is the default. Settings → Effects offers Full (default) and Light.
- **Audio:** bundled SDL_mixer, patched for ALAC, AAC and 24-bit FLAC; float mixing and native sample-rate switching. Volume uses PipeWire's default sink through `pactl`, followed with `pactl subscribe`. Bluetooth uses `bluetoothctl` and PipeWire.
- **Controls:** SDL game controller and PortMaster mapping; sticks act as arrows or a wheel (`BETTERPOD_STICK_WHEEL=1`). MENU tap turns the screen off; hold 5 seconds quits. System volume and POWER remain owned by Knulli. Wired/Bluetooth headphone buttons are read from input devices.
- **Backlight:** `/dev/disp` ioctl, then system brightness tools, then sysfs. The launcher restores brightness and governors after the app exits.
- **Idle:** Knulli's idlewatcher can mute after five minutes and suspend after fifteen despite active audio. During music, Listen and video playback, `knulli_idle.go` holds an app-owned `/var/run/battery-saver/*.pause` marker. Loading/buffering and brief track transitions stay protected; pause, stop and exit release it. The launcher cleans up after a binary crash. Manual POWER behavior is unchanged.

## Build and tests

The binary is built from the explicit `src/*.go` file list, because `main.go` and `pz_*.go` include C implementations. Build scripts hash the C sources and headers into `CGO_CFLAGS` to invalidate Go's cache when they change.

- `tools/Dockerfile` builds the reproducible Go 1.24 / Zig compiler image and aarch64 SDL2 sysroot.
- `tools/docker-build.sh` builds for `aarch64-linux-gnu.2.28`, verifies ELF dependencies and GLIBC requirements, and writes `build/betterpod`, `dist/betterpod-knulli.zip` and the old KnulliPod bridge package.
- `tools/build.sh` is the native build; it needs `SDL2_SYSROOT`, Go and Zig (`CC` can override the compiler).
- `tools/unittest.sh` runs `tools/unittest/*_test.go` natively against Debian SDL2 in Docker. Test files carry `//go:build ignore` and are copied into `src` by the harness. Tests cover playback, streaming, metadata edits, updates, database/file safety, drawing and the idle guard.

## Software Update and releases

Settings → Software Update reads `release/latest-knulli.json` through GitHub's contents API, with raw.githubusercontent.com as fallback. With Beta Versions on, it also reads `release/beta-knulli.json` and offers the newer version. `version.go` sorts a beta before its final release.

The package contains `betterPod.sh` and `betterpod/`. The updater verifies size and SHA-256, stages every file next to its target as `.kpnew`, then renames them into place with the binary last. The previous binary is `betterpod.prev`; `conf/` is never replaced.

For a release:

1. Update the version and CHANGELOG; run the tests and build.
2. Copy the verified ZIP to `release/betterpod-v<version>-knulli.zip` and update `latest-knulli.json` (stable) or `beta-knulli.json` (beta) with its version, path, URL, size, SHA-256 and notes.
3. Use a commit subject beginning `v<version>:` and include all source and release artifacts in that commit.
4. Prepare a PR. A merge changing CHANGELOG on `main` runs `.github/workflows/releases.yml`. `tools/gh_releases.py` publishes the newest CHANGELOG section after checking the package against its manifest and the release commit. Stable releases become Latest; betas are prereleases.

`release/latest.json` and the bridge launcher serve historical installations still named KnulliPod. Keep their legacy binary names only for that compatibility. Existing published releases and historical changelog sections are archival records.

## Videos and Listen

`video.go`, `ij_video.go` and `ij_videodl.go` implement video playback and downloads. The player covers the screen and shows its title/time for 2.5 seconds after input. Knulli's `/usr/bin/ffmpeg` decodes video; a static build is offered only when absent. Two ffmpeg processes provide sized RGBA frames and audio through `Mix_HookMusic`; audio sample count is the playback clock. They restart after seeking.

YouTube streams pass through a localhost server that fetches 10 MB ranges. Each candidate URL is probed before playback; rejected ranges retry, then refresh formats once. `ytWaitLinks` waits for yt-dlp's `available_at`, because early requests can return 403. Deno is preferred over QuickJS to solve challenges faster. Listen yields to video and keeps local music paused. Downloaded MP4s live in the database's `videos/` folder, outside the scanned music tree. `BETTERPOD_PERF=1` logs timings; `BETTERPOD_VIDEO_AV_MS` adjusts audio/video alignment.

## Album Info and tags

An online result replaces every supplied field, including existing titles, numbering and artist values. Missing source fields leave current tags intact. Complete equal-sized tracklists match by disc and track order before titles or lengths. YouTube filenames (`YouTube/<artist>/<album>/NN Title.m4a`) retain original order even after incorrect tag edits; otherwise complete tag numbering establishes it. Partial/unnumbered tracklists use individual matches, with partial matches and count differences reported. Track totals are inferred only from a complete source list.

`tagwrite.go` handles MP3, FLAC and Ogg. `tagwrite_m4a.go` uses the `ij_m4a.go` box parser: audio remains at its original offsets, untouched boxes/tags are copied, and the new `moov` is appended. Temporary output is synced and renamed only on success. Metadata is limited to 16 MB; fragmented or unsafe layouts are refused. Tests decode real AAC/ALAC audio before and after editing and check unrelated metadata.

## Source map

| Path | Contents |
|---|---|
| `src/ij_*.go` | Interface, views, settings, apps, library, Software Update |
| `src/main.c`, `src/audio.c` | SDL2 input/display and SDL_mixer playback |
| `src/knulli.go`, `src/knulli_idle.go` | Knulli hardware helpers and playback idle protection |
| `src/compat.go` | Compatibility with existing Knulli data and launchers |
| `src/cdraw.*`, `src/ij_effects.go` | Optional C pixel loops and Full/Light effects |
| `port/` | Launcher, assets, fonts and bundled libraries |
| `tools/` | Build, tests, release tooling and historical recovery utilities |
| `release/` | Knulli packages and stable/beta manifests |
| `mac/` | Share for Mac companion app |

## Troubleshooting

Read `betterpod/betterpod.log`. `BETTERPOD_PERF=1` records drawing/video timings once per second. For silent Bluetooth audio while playback advances, check the named PipeWire sink's mute state and the app's idle marker before changing the codec or reconnecting devices.
