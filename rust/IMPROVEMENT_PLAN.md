# betterPod Rust: improvement plan

Written on 2026-11-13 against commit `4018d9c` (betterPod Rust 0.2.20).
It covers four things: CI time, runtime performance, code quality and the
split of the code into smaller units. Line numbers are those of `4018d9c`.
If a line has moved, search for the name.

The Go app will be removed at some point. This plan changes **only the Rust
app**. It does not improve, speed up or refactor anything that belongs to Go.

## Progress

Tick a box in the same commit that completes the step.

- Phase A, CI: [ ] A1 · [ ] A2 · [ ] A3 · [ ] A4
- Phase B, performance: [ ] B0 · [ ] B1 · [ ] B2 · [ ] B3 · [ ] B4 · [ ] B5 · [ ] B6 · [ ] B7 · [ ] B8 · [ ] B9
- Phase C, code quality: [ ] C1 · [ ] C2 · [ ] C3 · [ ] C4 · [ ] C5
- Phase D, split: [ ] D0 · [ ] D1 · [ ] D2 · [ ] D3

Recommended order: A1, A2, A3, B0, B3, B4, B2, B5, B1, then the rest of B,
then C, then D. A4 needs B0. Every step is independent unless it says so.

## 1. Rules

1. **Scope.** Only `rust/**` and `.github/workflows/rust.yml` may change.
   Do not touch `src/`, `tools/`, `port/`, `mac/`, the other workflows or the
   root documents. The Go build must keep working until it is removed.
2. **One step, one commit.** The subject is a plain English sentence, as in
   the history: "Run Rust CI in parallel with cached image and dependencies".
   The body states what was measured, with numbers.
3. **No release by accident.** A new `version` in `rust/Cargo.toml` together
   with a matching section in `rust/CHANGELOG.md` publishes a release to
   users (job `publish` in `rust.yml`). Do not change the version or add a
   changelog section unless the owner asks for a release.
4. **Pushes.** Every push that touches `rust/**` starts the "Rust app"
   workflow. Phase A steps are verified in CI and need a push. Ask the owner
   before pushing.
5. **Design is a contract.** Read `DESIGN.md`. No step may change what the
   user sees or how a control behaves, unless the step says so and the owner
   agreed. User-visible strings do not change.
6. **Audio first.** A step that makes the interface cheaper but touches the
   audio path needs an underrun comparison before and after.
7. **English everywhere**, short plain sentences, in code, comments and
   commits.
8. **Measure before and after.** A performance step is kept only if its own
   acceptance figure is met. Otherwise revert it and write the result in the
   log. A negative result is a valid result.
9. **Do not weaken checks.** No deleted or ignored tests, no blanket
   `#[allow]`, no relaxed assertions to make a step pass.
10. **When the plan and the code disagree, the code wins.** Stop, write the
    difference in the results log and adapt the step without widening it.
11. **Stop and ask** when a step needs a behavior change, a decision listed
    under "Decisions for the owner", or the console.

### Commands

The reference environment is Linux in Docker. macOS runs are not a reference:
some tests fail there for environment reasons only.

| Purpose | Command |
|---|---|
| All tests | `rust/tools/build.sh test` |
| Console binary (AArch64, glibc ≤ 2.28) | `rust/tools/build.sh knulli` |
| Tests and both builds | `rust/tools/build.sh all` |
| Package | `rust/tools/package.sh` |
| Real SDL/GLES controller routes | `rust/tools/check-app.sh <music fixture>` |
| Benchmark with frame metrics | `rust/tools/smoke.sh <music fixture>` |
| Fixed screens against the Go reference | `rust/tools/visual-parity.sh` |
| CI timings | `gh run list --workflow rust.yml --limit 5`, then `gh run view <id> --json jobs` |

To prove that a refactor changed nothing, save the sorted test names before
and after and compare them:

```sh
rust/tools/build.sh test 2>&1 | grep -E '^test .* \.\.\. ' | sort > /tmp/tests-after.txt
```

### Console measurements

The console is the only valid place for CPU and memory figures. These steps
are done together with the owner.

- The owner starts the app from Ports. Never start it over SSH: it would
  take the display from EmulationStation.
- The app sets the `performance` governor at start (`src/app.rs:567`), so
  all figures are at a fixed frequency.
- Each screen settles for 5 seconds and is sampled for 30 seconds, twice.
- Screens: **S1** Home with the preview panel moving, music stopped.
  **S2** Now Playing, a lossless FLAC playing. **S3** Now Playing, paused.
  **S4** a search page with the on-screen keyboard open. **S5** a YouTube
  video playing (video steps only).
- Record: process CPU as a percentage of one core, the five busiest threads,
  CPU of reaped child processes, and RSS.

## 2. Baseline

### CI (workflow "Rust app")

| | Warm cache (run 42907529148) | Cold cache (run 43415879513) |
|---|---|---|
| Whole workflow | 3:43 | 6:28 |
| Job `test` | 1:42 (Tests step 0:46) | 3:18 (Tests step 1:55, image load 0:56) |
| Job `knulli` | 3:03 (build step 1:53) | 5:40 (build step 4:36, image load 0:35) |
| Job `image` | 0:17 | 0:17 |
| Job `publish` | 0:23 | 0:23 |

- The cold run is today's norm. The cache key hashes `rust/Cargo.lock` and
  `rust/Cargo.toml`, and both hold the app's own version. Every release
  commit misses the cache and compiles all 323 dependencies again. The last
  four pushes were releases.
- With a warm cache, 1:46 of the 1:53 build step is this crate alone
  (`codegen-units = 1`, `lto = "thin"`).
- The build image is 815 MB compressed. It is built on top of the Go image.
- CI runs no `cargo fmt --check` and no clippy. On 2026-11-13 `cargo fmt
  --check` reported 33 hunks, and clippy on macOS reported 6 dead-code
  warnings (`src/bt_class.rs:23,24,67,79,94`, `DeleteAlbum` at
  `src/app/maintenance_ui.rs:44`).

### Runtime (from `CONSOLE_QA.md`, RG35XX Plus)

| Screen | CPU of one core |
|---|---|
| Now Playing, lossless FLAC playing | 23–25% |
| Now Playing, paused | 8–9% |
| Go app, settled screen | about 12.5% |
| YouTube video | 65.5% |
| RSS | 388–413 MiB (Go: 108–194 MiB) |

The frame loop targets 60 fps (`src/main.rs:55-59`); the Go app animates at
30. A frame that equals the one on screen is not presented
(`src/main.rs:277`), but it is still built in full first. Slow animations
move by a fraction of a pixel per frame, so no frame is equal and none is
skipped. The audio callbacks use only atomics. The interface thread starts
no subprocess and does no blocking I/O.

Everything below that is not in these tables is an estimate, not a
measurement.

### Code

- One Cargo package, `betterpod-rust`: a library (`src/lib.rs`, 42 modules)
  and a binary (`src/main.rs`, `src/app/**`). About 92,000 lines, of which
  about 20,000 are tests.
- `App` (`src/app.rs:396`) has 132 fields. `Extras`
  (`src/app/services.rs:451`) has 86.
- The five largest functions: `Extras::tick` (`services.rs:3437`, 956
  lines), `Extras::choose` (`services.rs:2126`, 754), `App::handle`
  (`app.rs:2001`, 693), `App::tick` (`app.rs:3245`, 651), `App::execute`
  (`app.rs:1575`, 425).
- Module cycles in the library, test code excluded: one group of seven
  (`online`, `curation`, `downloads`, `library`, `metadata`, `podcasts`,
  `video_catalog`) and three pairs (`audio`/`pcm`, `bt_media`/`input`,
  `inline_keys`/`render`).

## 3. Phase A: CI

### A1. Keep the dependency cache across releases

**Why.** This is the largest gain: a release goes from about 6:30 to about
3:45.

**Change.**

1. Add `rust/tools/ci-deps-key.sh`, executable:

   ```sh
   #!/bin/sh
   # CI: a hash of what decides the compiled dependencies. This package's
   # own version is left out, so a release keeps the cache.
   set -eu
   cd "$(dirname "$0")/.."
   {
     awk 'prev == "name = \"betterpod-rust\"" && /^version = / { prev = $0; next } { print; prev = $0 }' Cargo.lock
     awk '!done && /^version = / { done = 1; next } { print }' Cargo.toml
   } | sha256sum | cut -c1-16
   ```

2. In `.github/workflows/rust.yml`, in both `test` and `knulli`, add a step
   before `actions/cache@v4`:

   ```yaml
   - id: deps
     run: echo "key=$(rust/tools/ci-deps-key.sh)" >> "$GITHUB_OUTPUT"
   ```

3. Replace the two cache keys and add a fallback:

   ```yaml
   key: rust-test-${{ needs.image.outputs.key }}-${{ steps.deps.outputs.key }}
   restore-keys: |
     rust-test-${{ needs.image.outputs.key }}-
   ```

   The `knulli` job uses `rust-knulli-` in the same way.

4. In the `paths` filter, add `'!rust/IMPROVEMENT_PLAN.md'` right after
   `rust/**`, so ticking a box in this file does not start a build.

**Verify.**

- Locally, the key must not move with the version. Change the version to
  `9.9.9` in `Cargo.toml` and in the `betterpod-rust` entry of `Cargo.lock`,
  run the script, restore both files, run it again: same output. Checked on
  2026-11-13 with the same logic: 0.2.19 and 0.2.20 give the same hash,
  0.2.17 a different one because dependencies changed after it. On macOS use
  `shasum -a 256` in place of `sha256sum` for this check.
- In CI, the first run after the change is cold: the key name is new. The
  next push must show "Cache restored from key" in both jobs and finish in
  4:15 or less.

**Watch.** With `restore-keys`, artifacts of old dependency versions stay in
the cache after a dependency update. `ci-cache-prune.sh` prints the folder
sizes at the end of each job. Write the sizes of the first cold run in the
log. If `target-knulli` or `target-native` grows past twice that size, add
`-v2` after `rust-test` and `rust-knulli` to start clean.

### A2. A build image that does not need the Go image

**Why.** `rust/tools/Dockerfile` starts `FROM betterpod-build:1`, the Go
image from `tools/Dockerfile`. When Go is removed, the Rust build would
break. The image also carries the Go toolchain for nothing. Expect a small
time gain only: Go is a minor part of the 815 MB. The reason for this step
is independence.

**Change.**

1. Rewrite `rust/tools/Dockerfile` to start from `debian:bookworm-slim` and
   install what the Rust build uses today:
   - packages: `ca-certificates curl xz-utils zip binutils build-essential
     pkg-config python3 libsdl2-dev libsdl2-mixer-dev ffmpeg xvfb xauth
     xdotool x11-utils libegl1-mesa-dev libgles2-mesa-dev libgl1-mesa-dri`;
   - the AArch64 SDL2 files in `/opt/sdl2`: copy the block from
     `tools/Dockerfile` (`dpkg --add-architecture arm64` up to the
     `libSDL2.so` link);
   - Zig 0.13.0, as in `tools/Dockerfile` (`cc-knulli.sh` calls `zig cc`);
   - rustup with the pinned `RUST_VERSION`, the `aarch64-unknown-linux-gnu`
     target, and `rustup component add rustfmt clippy` (for A3);
   - the glibc 2.28 block, unchanged.

   Leave out the Go toolchain and `GOFLAGS`. Add `lame`, `flac` or
   `vorbis-tools` only if a file under `rust/` runs them: check with
   `grep -rnE 'Command::new\("(lame|flac|oggenc|metaflac)"' rust/src rust/tests rust/examples`
   and read the Python and shell tools in `rust/tools` for the same names.
2. Rename the tag to `betterpod-rust-build:2` in every file that names it
   (`grep -rl 'betterpod-rust-build:1' rust .github/workflows/rust.yml`).
   `build.sh` reuses a local image by tag, so an unchanged tag would keep the
   old image on every developer machine.
3. In `rust/tools/build.sh`, delete the line that builds `betterpod-build:1`
   and build with `rust/tools` as the context. The Dockerfile copies nothing
   from the repository.
4. In `rust.yml`, job `image`: the key hashes only `rust/tools/Dockerfile`,
   and only the Rust image is built. Add `ls -l` of the saved archive to the
   build step, to record its size.
5. Three scripts run Go tests inside the Rust image to compare with the Go
   reference: `visual-parity.sh`, `check-youtube-design.sh`,
   `check-console-followup.sh`. Add `rust/tools/parity.Dockerfile`:
   `FROM betterpod-rust-build:2`, then
   `COPY --from=golang:1.24-bookworm /usr/local/go /usr/local/go`, with
   `PATH`, `GOPATH=/go` and `GOFLAGS=-buildvcs=false`. Tag it
   `betterpod-rust-parity:1`. The three scripts use it and build it on
   demand, as `check-app.sh` does with its own image. If a Go test needs one
   more tool, add it to this file, not to the main image. These files go
   away with the Go app.

**Verify.**

- `rust/tools/build.sh all` passes in the new image, and the sorted test
  names equal the ones saved before the change.
- `readelf -d build/rust/betterpod-rust.knulli | grep NEEDED` and the
  "Verified AArch64 / glibc" line are the same before and after.
- `rust/tools/package.sh` passes.
- The three parity scripts give the same result before and after.
- In CI, write the archive size and the "Load the build image" time of both
  jobs in the log (before: 0:35 and 0:56).

### A3. Format and lint gate

**Change.**

1. In `rust/tools/build.sh`, add a mode `lint` that runs `cargo fmt --check`
   and then `cargo clippy --locked --all-targets --target-dir /target-native
   -- -D warnings`. Update the usage line and the `case`.
2. Commit "Format the Rust sources with rustfmt": only the output of `cargo
   fmt`, run with the image's rustfmt so the version is the pinned one. The
   build container mounts the sources read-only, so run a one-off container
   with `rust/` mounted read-write for this.
3. Commit the clippy fixes. Fix each finding. The six dead-code warnings
   were seen on macOS: confirm them on Linux first. If an item is used only
   on Linux, gate it with the same `cfg`; delete it only if nothing uses it
   on any target.
4. In `rust.yml`, job `test`, add a step "Lint" after "Tests" and before the
   cache prune: `RUST_CACHE_DIR="$RUNNER_TEMP/rust-cache" rust/tools/build.sh lint`.

**Verify.** `rust/tools/build.sh lint` passes locally and in CI. The `test`
job must stay shorter than the `knulli` job, so the workflow does not get
slower. Write both job times in the log.

### A4. Shorter release build (needs B0, owner decides)

After A1, the workflow waits on the `knulli` job, and 1:46 of it compiles
this one crate on one code-generation unit.

**Experiment.** Set `codegen-units = 16` in `[profile.release]` and keep
`lto = "thin"`. Measure two things:

- the "Build for Knulli" step time in CI, before and after;
- console CPU on S1, S2 and S3 with both binaries, and the audio underrun
  counters in `frames.csv`.

**Keep it only if** the build is at least 30 seconds shorter, every screen
is within one point of a core of the old binary, and underruns do not rise.
The owner makes the call. The other way to shorten this step is D2.

## 4. Phase B: performance

### B0. See where a frame spends its time

**Why.** Today a sample is recorded only for presented frames, with one
`work_ms` for the whole loop. Skipped iterations are invisible, and nothing
separates logic, drawing and GL work.

**Change.**

1. `src/timing.rs`: add `tick_ms`, `draw_ms`, `finish_ms` and `presented`
   to `FrameSample`. Append the four columns at the **end** of the CSV
   header and rows. Existing columns keep their names and order
   (`smoke.sh` reads them by name).
2. `src/main.rs`, loop at 231–320: take the time after `app.tick` (251),
   after `app.draw` (276) and after `renderer.finish()` (288). `tick_ms`
   runs from the start of the iteration, so it includes input handling.
3. In the skip branch (277–287), before the sleep, push a sample with
   `presented: false`, `finish_ms` and `present_ms` at zero.
4. `rust/tools/smoke.sh`: print, per benchmark phase, the count of presented
   and skipped iterations and the median and 95th percentile of `tick_ms`,
   `draw_ms` and `finish_ms`. Add no new assertion.
5. Add `rust/tools/console-cpu.sh [seconds]`, POSIX `sh` for BusyBox, to run
   on the console. It finds the process with `pidof betterpod-rust`, reads
   `/proc/PID/task/*/stat` twice, and prints: CPU per thread as a percentage
   of one core with the thread name, the process total, the CPU of reaped
   children (fields `cutime` and `cstime` of `/proc/PID/stat`) and `VmRSS`.
   Parse the name up to the last `)`. The tick rate is 100. It writes
   nothing to disk.

**Verify.** Tests pass. `smoke.sh` passes and prints the table. Metrics stay
off unless `BETTERPOD_RUST_METRICS` or `BETTERPOD_RUST_BENCHMARK` is set.

**Then** run the console protocol once and fill the baseline table in the
log. Every later step in this phase compares against it.

### B1. Frame rate (measure, owner decides)

`README.md` states a 60 fps motion target. `BETTERPOD_RUST_FPS=30` already
exists (`src/main.rs:55`). Measure S1 and S2 at 60 and at 30 and give the
owner the four figures. The choices:

- **Keep 60.** No change.
- **Default 30.** One line at `src/main.rs:58`, plus `README.md` lines 3
  and 83.
- **Slow animations at 30, transitions at 60.** Round the time given to
  slow animations down to a multiple of 1/30 s. The frames in between are
  then equal and the existing skip drops them. Known sites: the Home preview
  pan (`src/app/visual_menu.rs:2083`, 48 px in 20 s), the lossless meter
  (`draw_lossless`, `src/app/visual_player.rs:1289`), and the marquees. The
  Go app runs these at 30, so this matches the reference. The gain is the
  share of `finish_ms` plus `present_ms` from B0, not the whole frame.

Do nothing here before the owner chooses.

### B2. No `pactl` process every two seconds

**Facts.** The volume worker wakes every 2 seconds and runs `pactl
get-sink-volume` (`src/platform.rs:83-113`, `read_system_volume` at 263).
`Extras::tick` asks for `BtRequest::Header` every 2 seconds
(`src/app/services.rs:3439-3442`), which runs `pactl get-default-sink`
(`src/bluetooth.rs:384-392`). Both run for the whole session, screen off
included. A `pactl subscribe` child is already running and wakes the volume
worker on each sink or server change (`src/platform.rs:221-262`).

**Change.**

1. Record whether the subscriber is alive: set a flag when its reader thread
   starts and clear it when the loop ends.
2. Volume worker: wait 60 seconds between safety reads while the subscriber
   is alive, 2 seconds when it is not. Check that shutdown still wakes the
   worker at once.
3. Move the line test into one function that says, for a `pactl subscribe`
   line, whether the volume may have changed and whether the audio route may
   have changed (`'change' on server`, `'new' on sink`, `'remove' on sink`).
   Count route events in an atomic counter that the app can read.
4. `Extras::tick`: request `BtRequest::Header` when the counter moved, every
   30 seconds as a safety net, and every 2 seconds when the subscriber is
   not alive. The 10-second device refresh on the Bluetooth page stays.

**Verify.**

- A unit test for the line function with real sample lines.
- Tests and `check-app.sh` pass (it runs a private PulseAudio sink).
- Console, S3 for 60 seconds: the child CPU from `console-cpu.sh` is near
  zero, against the B0 baseline.
- Console, with the owner: the volume keys move the on-screen volume at
  once. Connecting and disconnecting Bluetooth headphones updates the header
  icon within 2 seconds. After `pactl subscribe` is killed, both still work
  through the fallback.

### B3. Remove the unused level meter

`self.meter` is computed every tick (`src/app.rs:3786-3788`) and never read
(field at 468, initial value at 634). Delete the field and the three lines.
Keep `self.audio.visualizer(&mut self.samples)`: the samples feed the
visualizer. Verify with `grep -rn '\bmeter\b' rust/src/app.rs` and the tests.

### B4. Do not write unchanged game saves

While music plays, `save_playback()` and `save_games()` run every 15
seconds (`src/app.rs:3864-3867`). `save_games` (4221) serializes and writes
even when no game changed. Keep the last bytes written and return early
when the new bytes are equal. `save_playback` stays as it is: the position
moves. Add a test with a temporary data folder: a second save with no
change leaves the file untouched; a change writes it.

### B5. Rounded shapes without trigonometry or allocation

`rounded_outline` (`src/render.rs:1352`) computes 52 sines and cosines and
allocates a `Vec` on every call. It has six call sites (1060–1124) and runs
for every rounded rectangle of every frame.

**Change.** Compute the 13 unit vectors of each quarter once, with the same
expression as today, and keep them in a `OnceLock`. Build the points as
`cx + radius * c` and `cy + radius * s`. Return a fixed `[[f32; 2]; 52]`.

**Verify.** The output must be bit-identical, because frames are compared
bit by bit. Add a unit test that keeps today's function as the reference and
compares `to_bits()` over a grid of positions, sizes and radii, including
radius 0 and a radius larger than half the side. Then compare `draw_ms` on
S4 and on Now Playing with B0. If the keyboard's median `draw_ms` is still
above 4 ms, caching the keyboard's shapes is the next candidate: ask first.

### B6. Glyph lookups without SipHash

The `glyphs` and `advances` maps (`src/render.rs:38-39`) are keyed by
`(char, u16, u16)` and use the default hasher. Every drawn or measured
glyph looks them up. Give these two maps a small private multiply-xor
`BuildHasher`. Add no dependency. **Keep it only if** the median `draw_ms`
of the list phases in `smoke.sh` drops by 5% or more.

### B7. A larger pipe for video frames

ffmpeg writes 640×480 RGBA frames, 1,228,800 bytes each, into a pipe with
the default 64 KiB buffer (`src/video.rs:376-401`, thread `video-frames`).
After taking the child's stdout, on Linux, call
`fcntl(fd, F_SETPIPE_SZ, 1 << 20)` and ignore a failure. **Keep it only if**
the app and ffmpeg together use at least 2 points of a core less on S5.

### B8. Memory: find where it goes (measure only)

Add `rust/tools/console-mem.sh`: it sums `Rss` per mapping name from
`/proc/PID/smaps` and prints the 15 largest, the anonymous total and the
heap. Sample after start on Home, after a lossless track starts, in Cover
Flow, and after a video. Suspects to confirm or reject: the 2048×2048 RGBA
glyph atlas held on both CPU and GPU, whole-file audio preload
(`src/audio.rs`), the five parsed fonts, the artwork cache, GPU driver
mappings. Give the owner the table. Change no code in this step.

### B9. `target-cpu=cortex-a53` (low priority)

Set `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUSTFLAGS="-C target-cpu=cortex-a53"`
in the `knulli` branch of `rust/tools/build.sh`. All dependencies rebuild
once. **Keep it only if** S1 and S2 both drop by a point of a core or more.

## 5. Phase C: code quality

No step in this phase changes behavior. After each commit: tests,
`check-app.sh`, and the same sorted test names as before.

### C1. Page stack without `unwrap`

**Facts.** `pages.last_mut().unwrap()` and `pages.last().unwrap()` appear
179 times in 16 files under `src/app`. Six of those files are tests (45
matches). The rest: `services/visual.rs` 30, `services.rs` 29,
`services/downloads_ui.rs` 26, `services/external_foreground.rs` 17,
`services/foreground_ui.rs` 12, `album_read.rs` 11, `services/videos_ui.rs`
4, `services/lists_ui.rs` 3, `services/track_list_ui.rs` 1,
`visual_menu.rs` 1. The release profile has `panic = "abort"`: one broken
assumption here stops the music.

**Change.** Give the page stack two helpers: one that pushes a page and
returns a reference to it, and one that returns the top page as an
`Option`. If a push helper exists already, extend it. "Push, then
`last_mut().unwrap()`" becomes the first helper. Every other site becomes
`let Some(page) = … else { return … }` with the function's existing no-op
result. Test code may keep `unwrap`. One file per commit.

**Done when** `grep -rnE 'pages\.last(_mut)?\(\)\.unwrap\(\)' rust/src`
matches only tests and the one helper.

### C2. Pages known by type, not by title

**Facts.** `services::Page` already has `role: PageRole`
(`src/app/services.rs:257`, 283), yet code compares visible titles:
`"Bluetooth"` 5 times (`services.rs:3444`, 3463 and others),
`"Album Changes"` 3, `"End of the Queue"` 2 (`src/app/feedback.rs:102`,
119), `"Listen"` 2, `"Delete Playlist"` 2, and about ten single ones. List
them with `grep -rnE '\.title (==|!=) "' rust/src`. A changed label would
change behavior silently.

**Change.** For each compared title, add or reuse a `PageRole` variant, set
it where the page is built, and compare the role. `feedback.rs` works on
`app::Page`, which has its own `kind`: use that there. Calls that find a
page by title, such as `self.replace("Bluetooth", …)`, are the same pattern:
convert them when the change stays local, and list the others in the log.

### C3. Destructive rows marked by a flag

**Facts.** A row is drawn red when its label is `Delete` or starts with
`Delete `, `Remove ` or `Clear ` (`src/app/feedback.rs:325-328`).

**Change.** Add `danger: bool` to `app::Row` (`src/app.rs:196`) with a
builder method. Mark every construction site whose label matches the rule,
`format!` labels included. The drawing code reads the flag and, during the
migration, asserts in debug builds that the flag equals the old rule. Run
all tests and `check-app.sh`. The last commit removes the rule and the
assertion. Check first that no code assigns `row.label` after construction.
This is the lowest priority of the phase.

### C4. The five largest functions

Targets and sizes are in the baseline. One function per commit.

- Move only. Each contiguous block becomes a private method, called at the
  same place and in the same order.
- Do not reorder, merge conditions, rename fields or "simplify" while
  moving.
- A block that leaves the outer function (`return`, `?`, `break`,
  `continue`) is moved only if the control flow stays the same. Otherwise
  leave it in place.
- Stop when none of the five is longer than about 150 lines. Do not chase a
  number.
- `App::tick` is on the frame path: compare `tick_ms` from B0 before and
  after.

### C5. One copy of the safety-sensitive helpers

- The "is this an audio file" extension test exists near
  `src/library.rs:615`, `src/playlists.rs:505`, `src/curation.rs:232`,
  `src/files.rs:538` and `src/share.rs:916`, and two more lists are in
  `src/downloads.rs` (451, 732). Compare the sets first. Merge only those
  that are equal. List the different ones in the log and leave them: they
  may mirror the Go app on purpose.
- The directory walk with `openat` and `O_NOFOLLOW` is written twice
  (`src/files.rs:489-520`, `src/video_catalog.rs:601-630`). If the two are
  equal in behavior, keep one. If they differ, stop and report.

## 6. Phase D: split

Phases A to C fit on the current branch. Phase D is large: ask the owner
where it lands before starting (see "Decisions for the owner").

### D0. A dependency report

Add `rust/tools/module-deps.py`. For each module in `src/lib.rs` it prints
the other library modules it names through `crate::`, and the pairs that
name each other. It skips `#[cfg(test)]` modules and test files. Its first
output must match the cycles in the baseline.

### D1. Remove the cycles

The group of seven exists because `online` is two things: a shared
vocabulary that the domain modules import, and the service that calls those
same modules. Move the vocabulary down into leaf modules. Keep a `pub use`
in `online`, so no path changes in `src/app/**`.

| Step | What moves | Who imports it today |
|---|---|---|
| D1.1 | `input::Action` to a leaf module, re-exported by `input` | `games`, `puzzles`, `saver`, `bt_media` |
| D1.2 | `online::Context` (job and cancellation) to a leaf module | `bluetooth`, `worker_task`, `frontend_audio`, `listen`, `metadata`, `podcasts`, `video_catalog`, `downloads`, `curation` |
| D1.3 | HTTP and parsing helpers `USER_AGENT`, `http_agent`, `validate_http`, `base`, `encode`, `text`, `number` | `weather`, `stream_proxy`, `listen`, `metadata`, `podcasts`, `video_catalog`, `downloads`, `curation` |
| D1.4 | Data types `Provider`, `OnlineItem`, `StreamSource` | `listen`, `metadata`, `video`, `video_catalog`, `downloads`, `curation` |
| D1.5 | Process and tool helpers out of `downloads`: `run_command` (`src/downloads/process.rs:23`), `private_dir`, `ffmpeg_path`, `TOOLS`, `safe_name` | `bluetooth`, `frontend_audio`, `listen`, `share`, `video`, `video_catalog`, `curation` |
| D1.6 | The edges left over, one by one | see below |

Edges left for D1.6, each to be read before deciding:

- `library` uses `curation::{set_cover_file, embedded_cover_paths, apply}`
  while `curation` uses `library::{Library, Track, Album}`.
- `video_catalog` uses `online::{Task, Services, Output, Event}` while
  `online` calls `video_catalog`.
- `downloads` uses four YouTube helpers from `online`
  (`youtube_playlist_url`, `youtube_item`, `youtube_ui_projection`,
  `youtube_playlist_entries`).
- `downloads` uses `video_catalog::{publish_saved, existing_saved}`.

The rule for each: the symbol goes to the lowest module that can hold it
without importing upward. If a move needs a behavior change, stop and
report.

One step per commit, moves only. **Done when** `module-deps.py` reports no
pair except `audio`/`pcm` and `inline_keys`/`render`. Those two are single
units: leave them.

### D2. Workspace: decide with numbers

Split into crates only if it pays. Measure first:

- `cargo build --release --timings` for the console target, today;
- the A4 result.

A workspace certainly shortens test and debug builds. Its effect on the
release build is unknown with `codegen-units = 1` and thin LTO. Give the
owner the figures. If a workspace is chosen, these must hold:

- `env!("CARGO_PKG_VERSION")` is the version users see and the updater
  compares (`src/main.rs:62`, 156; `src/app/settings.rs:139`;
  `src/app/parity_model.rs:569`). It stays on the binary package.
- `build.rs` finds the puzzle sources relative to `CARGO_MANIFEST_DIR`.
- Tests that find assets relative to `CARGO_MANIFEST_DIR`
  (`src/preview_icons.rs:610`, `src/font_metrics.rs:89`, `src/art.rs:664`,
  `src/render/fallback.rs:214`).
- Scripts in `rust/tools` that pass `--bin betterpod-rust` or `--lib` test
  filters.
- `rust/tools/ci-cache-prune.sh`, which deletes this crate's artifacts by
  name.

### D3. `App` and `Extras` (outline, after D1)

Write the detailed steps once D1 is done, because they depend on its
result. The direction:

1. One page and row vocabulary where there are three today (`app::Page` and
   `Row`, `services::Page` and `Item`, `maintenance_ui::Page` and `Row`),
   in a module that does not know `App`.
2. State grouped into owned sub-structs. Start with the 55 `App` fields
   that only one file touches.
3. New screens follow the components that return effects (`Files`,
   `Curation`, `Quiz`, `Keyboard`, `maintenance_ui::Controller`, `Games`),
   not new `impl App` blocks.

## 7. Decisions for the owner

| When | Decision |
|---|---|
| After B0 and the B1 figures | Frame rate: 60, 30, or slow animations at 30 |
| After the A4 figures | `codegen-units = 16`, yes or no |
| After B8 | Which memory item, if any, is worth a change |
| Before D1 | Phase D on this branch or in a later PR |
| After D2 | Workspace, yes or no |
| Any time | When to cut a release with the changes made so far |

## 8. Not planned, and why

- **Anything for Go**: `checks.yml`, `tools/`, `src/*.go`. Go is leaving.
- **Removing the `image` job from the critical path.** It saves about 15
  seconds and duplicates the image build in two jobs.
- **Sparse checkout, one image per job.** A few seconds each, for more
  moving parts.
- **Skipping the frame build when idle.** A paused screen already costs
  less than the Go app (8–9% against about 12.5%). A wrong "nothing
  changed" guess leaves a stale screen. B5 and B6 lower the same cost
  safely.
- **Caching `dark()`.** `localtime_r` about 25 times a frame is negligible.
- **The CPU governor.** It stays `performance`: audio first. Only the
  comment at `src/platform.rs:521` is wrong, since the app always enables
  it.
- **The `audio`/`pcm` and `inline_keys`/`render` cycles.** Each pair is one
  unit.
- **Typed errors instead of `Result<_, String>`**, settings as enums, a
  generic worker type, index loops in pixel code. Wide changes with no
  measured gain. Unknown setting values fall back to defaults today; a
  `serde` enum would reject them.

## 9. When the Go app is removed (not part of this plan)

What the Rust app still takes from outside `rust/`:

- `rust/build.rs` compiles `../src/puzzles` and reads `../src/words`.
- Assets and libraries come from `port/betterpod/assets` and
  `port/betterpod/libs`: `rust/tools/package.sh`, `rust/tools/build.sh`
  (link libraries), the default asset path at `src/main.rs:41`, and the
  four tests listed under D2.
- `DESIGN.md` at the repository root is copied into the package.
- The three parity scripts and the five `rust/tools/*-go-test.go` files
  compare against the Go app. They go with it, as does
  `rust/tools/parity.Dockerfile` from A2.
- The launcher and `src/main.rs` protect and import the data of an
  installed Go app. Keep that while users may still have it.
- `rust.yml`: the trigger paths `src/puzzles/**` and `src/words/**`, and the
  comment about "Latest" staying on the Go releases.

## 10. Results log

Add a row per measurement. Keep failed attempts.

| Date | Step | What was measured | Before | After | Kept |
|---|---|---|---|---|---|
| | | | | | |
