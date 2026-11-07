# Local validation — 2026-11-07

Branch: `hello-rust`. Target: a separate RG35XX Plus / Knulli application with the betterPod 1.1 feature set. The local checks below preceded the separate on-console session. Hardware fixes and timing measurements are recorded in [CONSOLE_QA.md](CONSOLE_QA.md). No publication or release was performed.

## Local download queue follow-up — 2026-11-07

This follow-up is local only and supersedes the installed candidate below.
The console was not accessed or updated for these changes.

- The single actionable job button is selected blue during running and queued
  work, including Cancel. This is the user's explicit override of Go's grey
  running Cancel button; its geometry and other styling remain unchanged.
- A YouTube request no longer reopens an unrelated active Archive job. Each
  distinct request enters the existing bounded single-worker FIFO lane and
  opens its own progress page. Album and single-song requests are distinct;
  an identical unfinished request reopens its existing job.
- Waiting pages and Downloads show queue positions. Running and waiting rows
  follow execution order. B leaves a job running or queued; Cancel removes only
  that job. A guaranteed start event switches waiting feedback to progress.
- Local held-HTTP/fake-tool regressions exercise Archive then YouTube after
  success, failure and cancellation, browsing during a download, skipping a
  cancelled waiting job, duplicate requests and changing queue positions.
  No live provider media or console data is used.

Verification passes: 277 library, 107 application and 6 proxy cases (390 total;
26 opt-in cases remain separate), plus all six SDL/GLES route checks in
`rust/tools/check-app.sh`. The local AArch64 release build verifies glibc 2.28
compatibility. The package remains local; no SSH or console installation was
performed for this follow-up.

The nine-scene offline Go/Rust capture check repeats both runs with matching
semantic states. Running and queued Cancel are intentionally blue in Rust;
the waiting page's position and B hint are also requested additions. Paired
captures were inspected at 640 × 480. Evidence is under
`build/rust/download-queue-checks/` and `build/rust/youtube-design-checks/`.

## Previous console installation — 2026-11-07

Following the user's explicit request, the verified local candidate was installed
through the existing SSH connection with a checked backup and atomic rename.
Installed SHA-256:
`76ea159e48bb333c11ff8f15726fe60ab86689c2220506d0d9763b4faa556a69`.
The previous executable remains in `.backup-before-youtube-fixes.AaaBEr`.
Only the Rust executable was replaced. Music, configuration, launcher and
libraries were preserved. The player was closed and was not launched remotely;
physical acceptance and the reported memory-stop reproduction remain pending.
The PR remains paused. The local-only verification below predates this install.

## Local YouTube design, loading and memory follow-up

This batch supersedes the search selector and download presentation described
in the historical installation below. It is prepared locally only. No SSH,
console installation, application restart, PR or publication is part of this
batch. [DESIGN.md](../DESIGN.md) now records the binding Go/iPod design and the
separate native Mac design.

- Search options use the round gear beside the field, with Right/Left/A and
  the full checked Search Settings page. Showing is a noninteractive header.
  Video and playlist detail actions preserve the Go order and row treatment.
  The forced Start/Center/End crop selector is removed; artwork uses an
  automatic square preview.
- Foreground searches, opening service results and album-information reads
  mark the source row busy before work begins. Navigation waits, B cancels
  the owned request, and late output cannot open an abandoned destination.
  Initial album/song reads retain their originating list or action sheet.
  Audio and system controls remain available.
- The same ownership applies to tag saves, album joins/separation, cover
  changes and explicit tool actions. Cancelling a write retains completed
  file changes and rescans; it does not clear a newer request's loader.
  Combined metadata searches cancel all requests in their bounded group.
  Main-library separation and curation confirmation keep the pressed source
  visible while the worker runs.
- YouTube Download opens its dedicated job view. B returns to browsing while
  the job continues; Cancel stops it. Home retains Downloads for active and
  recent jobs, and a selected record reopens the same job. Status distinguishes
  completion, cancellation and failure; partial audio commits trigger a scan.
- Retry verifies completed playlist and chapter tracks against bounded private
  receipts before reusing them. Missing or changed tracks get a fresh safe
  destination; existing or externally changed files are never overwritten.
  A fully completed chapter album returns without fetching its source again.
- Full video metadata is cached in bounded private files, while the UI keeps
  only the required fields. Opening and downloading reuse resolved metadata
  with `--load-info-json`; stale-link failures allow one URL fallback. Audio
  selectors, buffers, Deno heap flags and memory guards are unchanged.
- An uncached automatic cover is fetched inside the same background download
  job before extraction. A fast Download press cannot silently omit artwork
  because the passive preview is still loading. Cache reuse and cancellation
  use that job's identity and square-cover treatment.
- Memory diagnostics distinguish insufficient memory before launch, the
  process-group ceiling, the audio reserve and the JavaScript heap, with
  operation and resident-memory figures. Signed media URLs are not logged.

The exact live query `anri timely` returned video `fp2psphgAK4`,
“杏里 Anri - Timely!! (1983) [FULL ALBUM]”, as its first result. A local,
metadata-only probe with the existing 64 MiB Deno flags succeeded: 85,674 bytes
of metadata, 27 formats and 11 chapters; the largest individual child peaked
at 125,604 KiB RSS in Docker. No live audio was downloaded. This does not
reproduce or explain the console's memory stop. Tomorrow's device logs must
identify which guard fired.

A separate synthetic stress fixture reduces a 4,246,515-byte provider response
to a 507-byte UI item while retaining chapters. Five UI clones added
19,587,072 bytes RSS with the full response versus 589,824 bytes with the
projection in an isolated Linux allocation check. These are fixture results,
not measurements of the actual ANRI response or the console.

The final ordinary suite passes 276 library, 106 application and 6 proxy tests
(388 total; 26 opt-in tests remain separate). The local AArch64 release build
verifies a maximum glibc requirement of 2.28. Logs are in
`build/rust/design-followup-checks/`.

Eight deterministic Go/Rust scene pairs render twice at 640 × 480. Both runs
repeat, and captured semantic states match. The pairs were visually inspected,
including focused search options, both detail types, the pressed-row spinner
and progress/error/Retry. The explicit B/background hint is intentional;
renderer rasterization still differs. Reproduce with
`rust/tools/check-youtube-design.sh`; paired images and the report are in
`build/rust/youtube-design-checks/`.

`rust/tools/check-app.sh` also passes its six opt-in SDL/GLES route checks:
general menus including both Wi-Fi entry points, Back/panel ownership, the
playlist mini-player and recent-job reopening, source-preserving album reads
and curation writes, real Listen pause/unplug/restoration, and video
opening/cancellation with actual local decoders. The source-write case verifies
the same merge confirmation before/after B, successful retry and unchanged WAV
bytes. Playback uses synthetic files and a private null sink, not console
audio. The test driver now waits for the existing screensaver exit animation
before navigating to the next section.

Weather and Bluetooth foreground actions use owned cancellable requests;
passive state refresh and the existing background scan remain navigable.
Four UI cases cover ownership, B, stale replies and an Add failure followed by
its committed state. B releases Weather's UI immediately; an in-flight HTTP
connect/read can still take its existing eight-second phase/twenty-second
overall timeout to finish. Bluetooth stops the owned process group before
follow-on commands. Completed device changes remain committed. These tests
use local fixtures and no physical Bluetooth actions.

## Historical YouTube playlist and album search installation

This earlier build used a selectable **Showing: Videos** row for
**Search Settings**. The local follow-up above replaces it with the Go gear.
Mode changes keep the Unicode query,
replace the current search page and reject cancelled or stale results.
The dedicated Videos search remains video-only, matching the Go route.

Playlist details load a bounded flat track list and provide child queues for
Listen and Watch the Videos. Download as Album saves each available song to a
separate numbered file in one album folder, with a common album artist, track
total and selected cover. Fresh owned staging prevents files from one child
being reused by another. Failed or cancelled audio downloads refresh the
library so already committed songs remain visible.

- The ordinary Rust suite passes 261 library, 93 application and 6 proxy tests
  (360 total). The 25 opt-in tests remain separate from this run.
- Nine focused provider/download cases cover playlist search and flat details,
  valid/repeated track order, finite paging, distinct album files, unchanged
  M4A media payloads, no overwrite and cancellation cleanup. Four application
  regressions cover mode routes, query/Back ownership, child playback queues
  and visibility after partial failure/cancellation. These use local fixtures
  and fake yt-dlp replies, not live provider downloads.
- The AArch64 build verifies glibc 2.28 compatibility. Logs are in
  `build/rust/youtube-search-checks/`.
- The binary was installed through the existing SSH session by atomic rename.
  SHA-256 is
  `94d5dca8feb900b4fe11b6189b63d0521a71bd6143ec45e4c2502588cba2265c`.
  The previous executable is `.backup-before-youtube-search.zWzShB`. The
  player was not forcibly restarted. Live provider and playback acceptance
  remain a device check after reopening it. The PR remains paused.

## Local Wi-Fi Library and Mac editor

### Magic and remote album joining follow-up

- `mac/tests/check.sh` passes with the shared local/remote grouping and sorting
  rules, exact partial-selection Magic edits, private grouping resets, and
  pending/saved join acknowledgments. A failed grouping commit remains
  retryable after a successful metadata write.
- The expanded real Rust/Go integration passes with race checks. Tracks in
  different folders join persistently across reconnect and rescan; private
  overrides and covers are reconciled. Encoded AAC and decoded float32 PCM
  remain identical. Focused Rust cases cover invalid identity, protected paths,
  stale files, failed manifest writes and preserved disc/cover choices.
- A separate Mac UI build was paired with the real local Rust fixture. Magic
  identified four tracks across two folders. A single title was corrected
  inside Magic, manual lookup returned to Magic, and Save Changes confirmed
  tags, covers and one persisted album group. Only marked temporary audio was
  used. The save button has an explicit checkmark and text.
- The Mac app builds for arm64 and x86_64 and its signature and ZIP verify.
  The Knulli binary builds for AArch64 with a maximum glibc requirement of 2.28.
- The verified Rust binary was installed on the previously paired console by
  atomic rename. SHA-256 is
  `4c6e82bedab69909eb341c9c31601294009e0de86dcd916b7617566f843d7d29`.
  The old executable remains in `.backup-before-magic-wifi.NXwlcX`. No player
  restart or edit to the console's music/settings was performed; physical
  playback and Wi-Fi acceptance remain a user check after reopening the player.

### Initial Wi-Fi implementation checks

The experimental Rust player now exposes an explicitly started, short-lived
Wi-Fi Library session for the Mac companion. The classic app and its Share
protocol are unchanged. No listener starts at launch, and no console install
or release was performed. Usage and wire details are in [WIFI_LIBRARY.md](WIFI_LIBRARY.md).

- Nine focused Rust server tests pass for authentication, reconnect, frame
  limits, file boundaries, active decoder protection, conflicts, cancellation,
  partial saves and preserving album identity/grouping.
- Six focused Rust application tests pass for the offline page, grouped code,
  manual address, reconnect status and playback path protection. The new
  controller-route assertions remain part of the opt-in SDL/GLES suite.
- `mac/tests/check.sh` passes Go metadata/transport, Swift lookup and editor
  model checks. The model checks include a 51-track save, validation of every
  draft before any write, and preventing remote-to-local write fallback.
- `mac/tests/check-wifi.sh` passes with the real Rust server and Go client on
  localhost, including actual UDP discovery, wrong/correct codes, reconnect,
  library reads, sparse and partial edits, uploaded/removed covers and stale
  file refusal. Encoded AAC media and decoded float32 PCM remain identical.
- Local Mac companion builds support Apple silicon and Intel. Rust Mac and
  AArch64/glibc 2.28 builds compile locally; packages stay in this checkout.

The integration uses marked temporary music and data directories. It does not
read or edit a real device library. Physical Wi-Fi discovery, idle/suspend
behavior, visible console navigation and audible playback during SD-card
writes still need device acceptance.

## Manual bug reports: playlist, menus and downloads

This follow-up changes only the separate Rust app. No console installation or
hardware test was performed for this batch; the ZIP is prepared for manual
acceptance on the device.

- Full-page playlist lists now draw the mini-player whose space they reserve.
  Empty playlist names keep the keyboard editable and show a bounded message.
  Long notices wrap inside the display; podcast artwork reserves a placeholder
  before loading. Scrollbar rails are centered in their reserved column.
- Menu transitions discard abandoned controller layers. Asynchronous browser
  results target the page that requested them, and leaving a browser cancels
  its pending navigation. A tools-status response cannot replace YouTube search.
- Downloads have a separate bounded worker at background priority. Archive
  receipt reads no longer wait for an entire album transfer. Archive progress
  counts the whole requested album, with received bytes, total size when known,
  track status and an explicit terminal result.
- Home shows Downloads while transfers are active, with individual progress and
  cancellation. Progress stays in Downloads and its source page; completion or
  failure produces one notice. Navigation and music remain independent of the
  download queue. Transfers also retain the automatic-suspend guard when music
  is paused.
- YouTube operations still share one tools process group to bound memory use.
  A concurrent YouTube request shows an explicit, cancellable waiting state.
  This does not serialize Archive transfers with YouTube search.

Focused local regressions cover layout, page ownership, cancelled searches,
YouTube tools callbacks, concurrent browsing/download queues and cumulative
Archive progress. Actual SDL/GLES captures verify the playlist's reserved
mini-player and all three scrollbar widths. Fixture checks do not establish
live-provider availability or audible continuity on the console.

Validation for this batch: 238 library, 83 application and 6 proxy tests pass
in the ordinary suite (327 total; opt-in tests remain excluded). The focused
SDL/GLES playlist/Downloads lifecycle test and scrollbar raster test also pass.
The AArch64 release build verifies a maximum glibc requirement of 2.28. Final
suite, graphics and build logs are under
`build/rust/manual-issues-2026-11-06/`; targeted service logs are under
`build/rust/bugfix-ui/`, with the scrollbar evidence in
`build/rust/scrollbar-qa-tests.log`.

## Local source-port checks before console fixes

The local source-port implementation and integration pass is complete. Its finite
scope, visual differences and deferred acceptance are recorded in
[PARITY.md](PARITY.md).

- Ordinary final suite: 202 library, 65 application and 6 proxy tests pass.
- Both Go and Rust repeat all 65 fixed captures independently; their captured
  semantic states match. Pixels differ, including antialiasing.
- Ten real audio formats and actual FFmpeg A/V transport fixtures pass locally.
  A separate HTTP Range/AAC fixture preserves every original compressed packet.
- Actual SDL/GLES application tests verify the menu routes and Listen/video
  ownership, cancellation, retry and restoration with a private PulseAudio null
  sink. Physical audio and Bluetooth are not used.
- Linux native, AArch64/glibc2.28 and bundled, locally signed Mac release builds
  are produced locally, without deployment or publication.

Current logs are `build/rust/parity-final-all-build.log`,
`parity-final-app-check.log`, `parity-final-mac-build.log`,
`parity-media-check.log`, `listen-aac-roundtrip.log` and
`visual-parity/final65-compare.log` under the same directory. The subsequent hardware results are recorded separately in `CONSOLE_QA.md`.

## Historical prototype evidence

The checks and timings below predate the source-faithful UI and behavior pass.
They do not validate the current animation workload. Hardware testing is a
separate, deferred stage.

### Executed prototype checks

| Check | Result |
|---|---|
| `tools/build.sh all` | 116 library tests, 10 application tests and 6 loopback proxy tests passed; native Linux and AArch64 release builds succeeded. |
| `tools/check-app.sh` | The opt-in SDL/GLES controller test passed and captured 13 actual screens, including Cover Flow, Now Playing controls, settings, games, puzzles, savers, curation, files, statistics and maintenance cancellation. |
| `tools/check-media.sh` | Playback, pause, resume and seek passed for WAV, AAC, ALAC, MP3, Ogg Vorbis, Opus, AIFF, 24-bit FLAC at 96 kHz, 24-bit ALAC at 192 kHz, and a streamed 63 MB WAV at 192 kHz. |
| Video transport | Local A/V, pause/resume/seek, queued opens/seeks, source replacement, deliberate silent video, failed audio reporting, Listen rejection without audio and stop/join passed. |
| Metadata roundtrips | Two opt-in real-codec tests passed. Independent FFmpeg hashes confirmed identical decoded audio after AAC/ALAC/MP3/FLAC/Ogg tag changes; unrelated tags and totals remained intact. |
| Share interoperability | Actual encrypted Go→Rust and Rust→Go transfers passed against the stable vendored peer, with identical 240,044-byte contents and SHA-256 acknowledgements, including a `000` nameplate. Debug and release Rust were exercised. |
| Filesystem and updates | Focused tests passed for resumable verified transfers, source/data isolation, joined shutdown, symlink handling, confirmation, reset scopes, interrupted update recovery and rollback. Parent-directory changes are synced before verified source cleanup. |
| Rendering | Japanese, Greek and Cyrillic text was visually checked. Four native games, twelve upstream puzzles and weather have additional local logic/rendering fixtures. |
| Knulli binary | ELF64 AArch64; maximum required glibc version 2.28; GNU linker retains the Cortex-A53 workaround. |
| Mac application | Bundled SDL frameworks, local signing, successful 30-second launch/render/audio-simulation run and clean exit. |

The normal test run intentionally ignores the two real-codec tests and the SDL/GLES route test; they were executed separately with their required fixtures. Media and graphics checks use dummy audio. Share checks use local rendezvous/transit servers and synthetic music only.

### Historical local timing

Both final runs used 120 tracks / 12 albums, a 60 Hz app target and all five benchmark phases for 30 seconds. The Linux run presented 1,798 frames; the Mac run presented 1,799. Startup and the final PNG capture are excluded from the percentiles below.

| Phase | Linux VM, software GPU: work p95 | Mac, Apple GPU: work p95 |
|---|---:|---:|
| Home | 2.65 ms | 0.79 ms |
| Cover Flow | 3.75 ms | 0.83 ms |
| Now Playing | 2.40 ms | 0.73 ms |
| Albums | 2.47 ms | 0.81 ms |
| Visualizer | 3.81 ms | 0.83 ms |

Completion gaps had p95 values around 20–21 ms in the VM and 18.2 ms on the Mac. The Mac run also recorded one 37.4 ms gap. These are **not stable 60 fps measurements** and are not measurements of the physical LCD. Neither environment used console scanout synchronization. Audio callbacks advanced throughout the workloads; their observed maximum intervals were 98.7 ms and 94.1 ms respectively, with nominal 4,096-frame / 44.1 kHz buffers. Dummy callback timing cannot establish audible continuity or PipeWire error rates. The PCM underrun counter covers the Rust transport, not the mixer or physical Bluetooth output.

The 10,000-track controller fixture prepares immutable navigation rows on a worker. Its 10,000 cached navigation clones took about 0.10 ms in total locally. Large playback queues are shared with the state writer; JSON serialization and reclamation run outside the UI thread. Artwork decoding uses bounded workers, GPU textures are cached, and text measurement does not rasterize offscreen characters. Screen blanking retains decoder CPU headroom; explicit pause releases the idle marker while natural transitions retain a short grace period.

## Hardware acceptance still required

The initial muted console comparison covers Cover Flow, the music player, weather and live YouTube. Continue acceptance on the Anbernic with controlled listening and headphones. Measure frame-time percentiles, CPU/RAM and PipeWire error deltas while listening through active navigation, each saver, downloads, Share, screen blanking, natural track changes and pause/resume. Test 60 Hz first; use 50/40/30 only if the lower cadence improves measured consistency and preserves uninterrupted audio.

The real provider implementations still need live service checks for availability, challenge handling and rate limits. Physical Bluetooth controls, brightness and power behavior also require the deferred console session. The independent Rust updater has no published feed; it never consumes the stable Go release manifests.

Generated session evidence is under `build/rust/`: `final-all-build.log`, `final-app-qa.log`, `media-check.log`, `final-performance.json`, `mac-final/frames.csv` and the smoke directory named by `latest-smoke.txt`. Metadata and Share evidence is recorded at the paths listed in `README.md`.
