# betterPod — notes for Claude

betterPod is an iPod-style music player: the iPod classic interface, rebuilt natively from iPod.js, on retro handhelds. One code base runs on two systems:

- **Knulli** on Anbernic handhelds with the Allwinner H700 (aarch64): where betterPod started (as KnulliPod, from MiyooPod).
- **Onion OS** on the Miyoo Mini family (armv7): the Miyoo version, which grew as a fork called MacriPod and joined this repository at 0.67.

Each system has its own launcher, package, build and update manifest. The interface, the library and the player are shared.

## Rules

- **English everywhere:** code, comments, README, changelog, commits. Short plain sentences, in the style of the existing text.
- **One code base.** Shared code lives in `src/`; platform code lives in files of its own (`knulli.go` for Knulli; `onion.go`, `onion.c`, `onion_sleep.go`, `cdraw.*`, `ij_effects.go` for Onion). In a shared file, a difference between the systems is a small `isOnion` / `isKnulli` switch with a comment that says why.
- **Both systems, or a reason.** A new feature works on both. When one system can't carry it, it is switched off there (not removed), with the reason in the code and a line in README's features table. What is off on the Miyoo so far is listed under Onion below.
- **Audio first, then fluidity.** Music must never crackle; then the interface should be smooth. On the Miyoo (two Cortex-A7, 128 MB) anything that adds steady CPU load needs a reason and a measurement (`tools/onion/perf.sh`): an animation that keeps redrawing, a timer, a program run every few seconds.
- **One version.** `BETTERPOD_VERSION` in `src/ij_update.go` is the version on both systems. Each system has its own zip and manifest in `release/` (`latest-knulli.json`, `latest-onion.json`). A fix for one system can be released for that system alone.
- **One changelog.** A change that concerns one system says so ("On the Miyoo: …").
- **Build and test both** before a release that touches shared code.

## The platform switch

`isKnulli` (`knulli.go`) and `isOnion` (`onion.go`) are set by `MIYOOPOD_PLATFORM` (`knulli`, or `miyoo` / `onion`), which the launchers export. Without it, the 32-bit ARM build is Onion and the others are Knulli. `platformAbout()` names the system and its consoles for About.

## Knulli (Anbernic H700)

- **Hardware:** Allwinner H700, 4 × Cortex-A53, 1 GB of RAM, aarch64; a 640×480 screen. Go uses all the cores.
- **System:** Knulli (Batocera-based). betterPod is a PortMaster-style port: `port/betterPod.sh` → `/userdata/roms/ports/betterPod.sh`, the app in `ports/betterpod/` (binary `betterpod`). The launcher sources PortMaster's `control.txt` for the controller mapping.
- **Paths:** music in `/userdata/music` (the "music" network share), else `betterpod/music`; the database in `betterpod/conf` (Settings → Database can move it, cards are under `/media`); the log is `betterpod/miyoopod.log`, the launcher's output `betterpod/log.txt`.
- **Video:** the system's SDL2, with vsync.
- **Audio:** SDL_mixer from `port/betterpod/libs`, patched for ALAC, AAC and 24-bit. The output is reopened at each track's sample rate, with float mixing. The volume is the PipeWire default sink, through `pactl` (`pactl subscribe` follows changes made elsewhere). Bluetooth through `bluetoothctl`, the sound going to its PipeWire sink.
- **Keys:** the gamepad through SDL's game controller (`SDL_GAMECONTROLLERCONFIG` from PortMaster); the sticks act as the arrows, or as a wheel (`MIYOOPOD_STICK_WHEEL=1`). MENU (the function key): tap = screen off, hold 5 s = quit. The volume keys belong to the system. Headphone buttons (wired and Bluetooth) are read from their input devices (`main.c`).
- **Backlight:** the display engine's `/dev/disp` ioctl first, then `batocera-brightness` / `brightness`, then sysfs.
- **Sleep:** Knulli's own.
- **Build:** `tools/build.sh` (Go 1.24, zig cc for aarch64 with glibc 2.28, an SDL2 sysroot in `SDL2_SYSROOT`) → `dist/betterpod-knulli.zip` (and the bridge zip for installations still called KnulliPod).

## Onion OS (Miyoo Mini, Mini Plus, Mini v4, Mini Flip)

- **Hardware:** SigmaStar SSD202D, 2 × Cortex-A7, 128 MB of RAM (Onion keeps about half), armv7 hard-float, glibc 2.28. A 640×480 screen (without a `full_resolution` file Onion runs the app at 640×480 on the v4 and Flip too). Wi-Fi on the Mini Plus, v4 and Flip; volume keys on the Mini Plus.
- **System:** Onion OS. The app is `onion/betterpod/` → `/mnt/SDCARD/App/betterpod/` (binary `betterpod`, `launch.sh`, `config.json`, the Miyoo's SDL2 in `libs/`). Apps run from `launch.sh` through Onion's `runtime.sh`, which starts keymon and MainUI again afterwards: a launcher must not start MainUI itself.
- **Paths:** music in `/mnt/SDCARD/Media/Music`; the database in `App/betterpod/conf` (or `.betterpod` in the music folder; one card, so no `/media`); the log is `App/betterpod/miyoopod.log` (kept under 1 MB).
- **`launch.sh`** stops Onion's audioserver (with Onion's `stop_audioserver.sh`), kills keymon (it would suspend the app on POWER and own the volume keys) and sets the CPU governor to performance; after the app it puts the governor and `scaling_min_freq` back and switches the panel on.
- **SDL2** (from MiyooPod 0.0.8, steward-fu's `mmiyoo` build, SDL 2.0.20): the buttons are keyboard keys (A space, B left Ctrl, X left Shift, Y left Alt, L e, R t, L2 Tab, R2 Backspace, SELECT right Ctrl, START Return, MENU Escape). No joystick and no game controller subsystem. Its input thread reads `/dev/input/event0` without grabbing it; POWER and the volume keys are not mapped, so `onion.c` reads them itself.
- **Video:** the renderer only copies whole textures reliably (sub-rectangle copies are misplaced), so Onion uploads full frames. The present is `FBIOPAN_DISPLAY`, which waits for the vertical blank (11–13 ms), hence the present thread in `main.c`. No logical size, no fullscreen flag. The hottest pixel loops are in C (`cdraw.c`).
- **Audio:** S16, 44.1 kHz, buffers of 2048 frames, no rate switching (the driver assumes 2 bytes per sample). The driver sends each buffer with `MI_AO_SendFrame(…, 1 ms)` and sleeps; `onion.c` exports its own `MI_AO_SendFrame`, which waits up to 1 s for room, so nothing is dropped (MI_AO queues 6 buffers). The volume is MI_AO's hardware volume (Onion's `volume.h` ioctl and curve, scaled to end at 0 dB). Formats: MP3, FLAC, Ogg, WAV, AIFF; the Miyoo's SDL_mixer has no AAC or ALAC decoder, so `.m4a` is skipped.
- **Memory:** Go keeps to one core (`GOMAXPROCS=1`, the other is left to SDL_mixer's decoding), a 48 MB soft limit, a 16 MB artwork cache; tracks up to 24 MB play from RAM, bigger ones are read ahead gently. The live heap sits around 35 MB, so whatever a frame allocates soon makes the GC run again and again: a screen that animates must reuse its buffers (`aaPoly` once made a 640×480 rasterizer per polygon, 2.4 MB a call, and the GC ran 6 times a second while Share waited). `GODEBUG=gctrace=1 tools/onion/smoketest.sh` shows the GC along the walk.
- **Keys:** the d-pad is the wheel; B and MENU are back; POWER tap = screen off with the music playing, hold 5 s = quit; the volume keys set the volume (also with the screen off); MENU + volume keys and L2 / R2 set the brightness in Onion's eleven steps (`3·e^(0.350656·level)` of the PWM duty cycle).
- **Sleep** (`onion_sleep.go`): killing keymon also kills Onion's sleep timer, so betterPod does its job. With nothing playing, system.json's `hibernate` minutes (5 by default, 0 = never) without a key turn the screen off, and a minute later `/tmp/.offOrder` is written and the app quits, so `runtime.sh` powers off; the screen off by POWER powers off after the timer and one minute. Never while music plays, while charging or while something is under way (a download, a scan, a cover search, tags being written, an update). The screen off is the backlight at 0 and the LCD panel off (gpio4, as Onion's `display_setScreen`); 10 s after the music stops, `scaling_min_freq` goes to the lowest and MI_AO is muted. `tools/onion/sleeptest.sh` checks it.
- **Battery:** batmon's `/tmp/percBat`; charging from gpio59 (Mini, model 283) or `axp_test` (Mini Plus, 354), the model in `/tmp/deviceModel`.
- **SD card:** FAT, so chmod can fail (Software Update ignores it) and names must avoid `: * ? " < > |`.
- **Off on the Miyoo, and why:** YouTube (yt-dlp needs about 90 MB of Python and more than 96 MB of QuickJS heap; measured under emulation), Bluetooth (none), Music Folder (one card slot), the screensaver's Visualizer (the whole screen 30 times a second with an FFT), the lossless badge (its bars keep Now Playing redrawing, and the output is 16-bit anyway), headphone buttons (no media input device). Toned down: Settings → **Effects** (on both systems) is Light by default on the Miyoo (no Ken Burns pan, still Weather icons, sheets without their slide); Internet Archive downloads redraw at most 5 times a second and don't offer ALAC; the screensaver keeps 48 covers ready instead of 240; Share's screen, which moves for as long as a share waits and runs, is drawn 10 times a second with Light effects; the Wi-Fi icon isn't read while the screen is off; there is no "back from sleep" (the Miyoo never suspends under betterPod: a jump of the clock is only the time being set).
- **Build** (Docker, Go 1.24, zig cc for `arm-linux-gnueabihf.2.28`): `tools/onion/build.sh` → `dist/sdcard/App/betterpod/` and `dist/betterpod-v<version>-onion.zip` (unzip at the root of the SD card). Check after a build: `readelf -d` NEEDED is only libSDL2.so, libSDL2_mixer.so, libc, libm, libpthread, libdl; the highest GLIBC symbol is at most 2.28 (2.7 so far, the Puzzles' `__isoc99_sscanf`); `MI_AO_SendFrame` is in `--dyn-syms` (exported by Go's `-rdynamic`).
- **Tests** (all in Docker):
  - `tools/onion/unittest.sh`: `tools/onion/unittest/*_test.go` (tagged `//go:build ignore`, copied into `src` to run natively): the update zip, versions, volume and brightness curves, the sleep timer's decisions, key directions, the C pixel loops byte for byte against Go, Album Info, Podcasts.
  - `tools/onion/smoketest.sh`: the real ARM binary emulated with Debian's SDL2 in Xvfb, keys through xdotool and POWER / volume through `MIYOOPOD_ONION_INPUT`; a PNG per step. About 7 minutes: run it in the background.
  - `tools/onion/sleeptest.sh`: the sleep timer with a 3-second "minute". About a minute.
  - `tools/onion/perf.sh`: the interface natively on one core in Onion mode, fps and main-thread load per scenario, and a CPU profile. The host is much faster than the A7: compare versions and scenarios, never absolute times. `tools/onion/bench.sh` compares Go and C pixel loops on emulated ARM (only scalar comparisons mean anything under QEMU).
- **Release:** `tools/onion/release.sh "what's new" …` copies the zip to `release/betterpod-v<version>-onion.zip` and writes `release/latest-onion.json`; commit and push both.

## The build, for both

The binary is built from the file list `src/*.go`, not as a package, because `main.go` and `pz_*.go` include C files (`main.c`, `audio.c`, `onion.c`, `src/puzzles/*.c`). Go's build cache doesn't see those C files change: `tools/onion/build.sh` puts their hash in `CGO_CFLAGS`.

## Software Update

Settings → Software Update reads `release/latest-<system>.json` (`updateDistro()`: `knulli` or `onion`) through GitHub's API, with raw.githubusercontent.com as the fallback, and downloads the zip at its `path`, checking size and SHA-256. Knulli's zip holds `betterPod.sh` and `betterpod/`; Onion's holds `App/betterpod/` (unzipped at the root of the card by hand), and the update takes what is under `App/`. The files are replaced one by one with atomic renames, the old binary is kept as `betterpod.prev`, and `conf/` is never touched.

## Where things are

| Path | Contents |
|---|---|
| `src/ij_*.go` | The interface: views, Cover Flow, Now Playing, settings, apps, Software Update |
| `src/main.c`, `src/audio.c` | SDL2 input, display, vsync and the present thread; SDL_mixer playback |
| `src/knulli.go` | Knulli: paths, backlight, the platform switch |
| `src/onion.go`, `src/onion.c`, `src/onion_sleep.go` | Onion: keys, MI_AO volume and its queue, backlight, battery, memory, sleep |
| `src/cdraw.c`, `src/cdraw.go`, `src/ij_effects.go` | The Miyoo's C pixel loops; Settings → Effects |
| `port/` | Knulli's package: `betterPod.sh`, `betterpod/` (assets, fonts, libs), shared assets |
| `onion/betterpod/` | Onion's app folder: `launch.sh`, `config.json`, icon, the Miyoo's SDL2 |
| `tools/build.sh` | Knulli's build |
| `tools/onion/` | Onion's build, tests, perf and release |
| `release/` | The zips and the manifest of each system |

## When something goes wrong on a console

Ask for the log (`betterpod/miyoopod.log` on Knulli, `App/betterpod/miyoopod.log` on Onion). For stutter, `MIYOOPOD_PERF=1` in the launcher logs drawing times every second; on the Miyoo the log also reports, on exit, the audio buffers MI_AO played and refused, and each screen off notes the CPU's clock. The Onion launcher lists switches for quick experiments (`MIYOOPOD_PRESENT_THREAD=0`, `MIYOOPOD_AO_TIMEOUT=0`, `MIYOOPOD_AUDIO_CHUNK`…).
