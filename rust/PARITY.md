# Go 1.1 source port — local verification

The binding visual and interaction contract is [DESIGN.md](../DESIGN.md).
Use its Go references and required screen comparisons for every UI change.

This local `hello-rust` branch keeps the independent Rust installation and uses
the Go 1.1 source as the contract for menus, controls, persistence, layout and
animation. The prior playable prototype is not a completion baseline. No console
deployment, hardware benchmark, PR, push or release belonged to this local pass. The later SSH session and its fixes are recorded in [CONSOLE_QA.md](CONSOLE_QA.md).

## Acceptance

- Preserve uninterrupted audio while menu, artwork, files and network work run
  outside the audio callback. Keep bounded work and owned-data isolation.
- Match the original menus, settings, all application features, game rules and
  input behavior, including cancellation, loading, empty and failure paths.
- Compare deterministic Go and Rust screenshots and intermediate animation
  states using identical data, input and time. Record differences honestly.
- Keep original composition when increasing raster resolution or improving
  vector edge quality. A different rasterizer is not expected to produce
  byte-identical antialiasing.
- Finish local integration and build verification before the separate console
  session. A 60 fps target is not a measured hardware result.

## Current local evidence

The local implementation and integration pass is complete as of 2026-11-06.
Generated proofs live under `build/rust/` and are not release assets. These are
finite checks of the implemented feature set, rather than proof of pixel
identity or every possible service, error and transition path.

| Area | Evidence |
|---|---|
| Shared UI and source motion | Two independent Go/Rust capture passes cover 65 scenes, including static screens, four pinned game positions, 18 navigation checkpoints, settings, source keycaps, maintenance, curation, fullscreen video, saved/recent videos and real song/album option sheets. All captured semantic states match and every capture repeats within each backend; pixel differences remain explicit. Native-size review pairs label Go on the left and Rust on the right. |
| UI assets | Seven source-generated icons rasterized at twice their display resolution; silhouette overlap 99.4–100% against actual Go exports after downsampling. The native 512px fallback artwork preserves the original note silhouette. |
| Typography | A genuine size-scaling bug in the original Go kerning wrapper is fixed and tested. The reference width is 272.296875px versus 272.29688px in Rust. A real GLES test verifies that a full glyph atlas preserves earlier and newly drawn text in the same frame. |
| Games | Source ports of Brick, Solitaire, 2048 and Five Letters have focused rule, persistence and motion checks. Twelve unchanged upstream puzzle engines have deterministic source-control/save fixtures and Rust frontend geometry checks. |
| Visualizers and weather | Fixed PCM/phase comparisons and native weather-icon fixtures compare the original equations and composition. Worker shutdown/restart and bounded texture queues are checked separately. |
| Metadata | Five real codecs verify the Read → Preview → Save flow, no writes before confirmation, per-track patches, cover reset and preserved audio content. |
| File and maintenance tools | Synthetic trees verify browsing and scan snapshots, confirmed deletion, containment, resume, ownership and rollback. Database relocation preserves private media, playlists, podcast file references and chosen covers; copied/imported M3U paths and transformed-file checksums survive transfer resume. |
| Services | Focused input, Bluetooth, sharing-preview, tool-removal, archive, playlist and updater checks accompany the shared page adapters. The independent updater remains local until its own feed is configured. |
| Listen | Current/next-file caching, original AAC/SIDX fragment boundaries, pause intent, stale-generation rejection, retry and joined cancellation have focused checks. An actual local HTTP Range and FFmpeg fixture proves that concatenated compressed packets from the cached parts exactly equal the original AAC stream. |
| Audio and video transport | The expanded UI source passes the local ten-format audio fixture, including high-resolution FLAC/ALAC and a streamed 63 MB WAV. Real decoder tests cover pause/resume/seek, source replacement, silent video, audio failure reporting and stop/join; these use dummy audio. |
| Actual application ownership | SDL/GLES controller tests run the real Listen cache and video decoder with a private, paced PulseAudio null sink. They verify row-A controls, ignored generic Listen transport, unplug/resume/next, music restoration, concurrent video resolution, stale Ready rejection, opening cancellation, screen-off seek, EOF/retry and paused music after closing video. No physical audio device is used. |
| Suite and builds | The ordinary final run passes 202 library, 65 application and 6 proxy tests (273 total); opt-in codec, AAC and SDL fixtures run separately. Linux native, AArch64/glibc2.28 and bundled Mac release builds are prepared locally. |

## Console review, 2026-11-09

This follow-up implements the reported Rust console corrections in PR #18.
The requested departures from Go are recorded in `DESIGN.md`.

| Area | Change and verification |
| --- | --- |
| YouTube progress | Read exact and estimated byte totals, accept numeric progress on either output stream, retain each playlist file's total and combine it with completed tracks. Archive keeps its cumulative byte semantics. Running jobs show the source spinner. The actual bundled yt-dlp emitted 14 reports, including 13 distinct byte counts, during an isolated local HTTP download. This is not a live YouTube extraction test. |
| Download completion and navigation | Successful results request a library refresh even after job history is removed. Partial/cancelled albums retain their existing refresh path. An actual service callback and scanner fixture publishes a new file and verifies its Album and Artist rows without manual refresh. Downloads is reachable from Music and Videos; Home has no Downloads row. |
| Artwork selection and proportions | The YouTube detail Cover action offers optional source searches and restoration of its original thumbnail. Existing albums can select a YouTube result as a square private cover override. Tests verify completion, cancellation, late-output rejection, unchanged source audio bytes and persisted square artwork. A native-size circular mark in a wide thumbnail remains circular in both detail rows. |
| Playback and Cover Flow | Left/Right cycles time/scrubber/options without changing volume. Requested/loading playback keeps the mini-player out of its paused pose. Entry flip and pause tilt compose continuously, including pause during entry. Cover Flip song changes update artwork, album selection and Back destination. Ten-second idle following resumes on later track changes. |
| Artwork cache | Frame lookups remain memory-only. Visible covers refresh LRU age, pending requests do not consume the foreground request budget, and the center cover is requested first. Scans retain unchanged GPU textures and invalidate replaced artwork using signatures collected on the scan worker. A 44-cover pressure fixture verifies that the visible texture survives eviction. |
| Screensaver and controls | Active music returns from Cover Flip to Now Playing; paused playback restores the original screen. Hold permits automatic entry. Hold notices are centered; sharing uses Enter; scrollbars fade after navigation. Music → Search → Lossless lists albums containing at least one confirmed lossless track. |
| Loading, battery and exit | The real Music search detail → Watch input route opens the full video loader and B restores its selection. Long durations show hours. System battery selection ignores device-scoped peripheral batteries; a fixture selects 82% over 50% and 40% alternatives. Shutdown continues presenting the 12-spoke indicator while workers stop and preferences flush. By request, the header battery departs from Go's `level / 85` fill: it follows the real charge in 5% steps with one-point flicker suppression, so Go/Rust captures differ in the battery fill. Actual device gauge readings still need hardware comparison. |
| Wi-Fi Library | The dedicated console screen presents Start, Starting/Cancel, an eight-digit pairing code, Connected, and retryable failure states. It reuses the source header, fonts, Wi-Fi glyph and blue job button. Five native-size states are captured; no Mac UI components are moved into the console. |

Validation for this pass:

- 401 ordinary tests: 283 library, 112 application and 6 proxy cases. The 27
  opt-in tests remain separately identified; seven actual SDL/GLES application
  route tests pass, including the new multi-flow regression fixture.
- Linux native and Knulli AArch64 release builds pass; Knulli requires no newer
  than glibc 2.28. A 30-second smoke run loads 120 tracks, advances playback and
  visits all five scripted screens with dummy audio.
- Two independent capture passes for each of 64 general and 9 YouTube scenes
  are repeatable. Native-size images were inspected. Semantic differences are
  explicit: the existing Rust Wi-Fi Library entry, requested Lossless entry,
  removed Effects choice, and an actionable Cover row even when a video has no
  thumbnail. Associated
  list positions differ. Pixel differences include scrollbar fading, selected
  Cancel/background hints, the download spinner and renderer antialiasing.
- `build/rust/review-*.log`, `app-captures/review-*.png`, `visual-parity/`,
  `youtube-design-checks/` and `review-ytdlp-progress.json` contain local evidence.
  Generated media, preferences, binaries and captures are excluded from commits.

The Effects menu and saved Full/Light choice are removed. Artwork previews,
weather icons and sheet/toast transitions always use Full behavior. Legacy
Light preferences are ignored without changing other saved settings.
This pass does not install on the handheld, release a package, validate live
YouTube challenges, or establish audible playback, physical battery accuracy,
Wi-Fi/Bluetooth reliability or H700 frame timing.

## Console follow-up, 2026-11-09

This pass addresses the subsequent playlist, search and Share reports.

- Playlist Y/options offers confirmed deletion for private and imported M3U
  files. The input-route fixture verifies cancellation and deletion while
  preserving song bytes. Filesystem tests reject outside paths and symlinks.
- Archive detail rows share one album and playback queue instead of copying
  both for every track. A 500-track fixture verifies shared ownership. Stored
  detail metadata excludes the duplicated original provider response.
- Search rejects non-object provider JSON before mutable indexing can panic.
  Failed Find Info searches retain the manual query and page for retry. The
  four-provider failure fixture uses real workers with invalid local input;
  it does not reproduce the original live-service crash.
- Videos search has readable failure/retry feedback and a tool-repair action
  when appropriate. Its query and video-only scope survive Back. The existing
  Music → Search → Lossless entry is verified through actual controller input.
- Scrollbars reappear after visible rows or loading state change, then fade
  with the existing timing. Captures cover expiry, loaded rows and navigation.
- Share drops the name-entry header when entering Send or Receive. Long album
  and artist text uses the source clipped marquee. The locally generated code
  precedes packaging and relay connection; progress still reports Preparing
  until the relay is ready. Cancelled callbacks cannot alter a reopened screen.
- Sharing restores the Go cover size, shadow, gloss and digit borders. Waiting
  adds the requested gentle float; Sent returns the cover over the source
  0.6-second transition. The shared Go/Rust capture harness covers name entry,
  Receive, Preparing, Waiting and Sent, with additional Rust float and return
  checkpoints. Native-size 640 × 480 images were inspected. Expected differences
  are the requested float in place of Go's waiting scan and the fixture battery
  indicator. The returning cover still uses its original colors instead of
  Go's desaturated, striped ghost treatment; renderer antialiasing also differs.

The connected 0.2.10 console reproduced the YouTube start failure during a
user-triggered search. A bounded process trace recorded `clone(...)= -1 ENOMEM`
while the app reported 644 MiB resident and 157 MiB available. The installed
yt-dlp had executable permissions and ran `--version` successfully over SSH.
The resolver now avoids `pre_exec`, which forced a full player fork, and uses a
small positional-argument shell through the normal spawn path. The tool and
its descendants retain OOM priority 1000 and their cancellable process group.
Tests cover argument quoting, environment, working directory, OOM priority and
descendant cancellation. No system memory settings or audio buffers change.

Validation: 419 ordinary cases (296 library, 117 application, 6 proxy) and the
opt-in SDL/GLES follow-up route/capture test pass. Native Linux and Knulli
AArch64/glibc 2.28 builds pass. `rust/tools/check-console-followup.sh` reproduces
the private fixtures and Go/Rust captures. Evidence is kept under ignored
`build/rust/console-followup/` and `build/rust/followup-*.log`.

The available console log covers the current launch and contains no original
Archive or Find Info crash backtrace. Their precise historical crash causes
remain unconfirmed. Live album exchange and corrected YouTube search on the
handheld still require acceptance testing.

The subsequent user-authorized SSH installation replaces only the Rust binary.
The old binary is backed up, both checksums are verified, and configuration,
music, launchers and the original Go installation are preserved. Native Ports
launch starts the new binary with the physical controller and 657 tracks /
25 albums. The running executable's SHA-256 is
`bcd7fb5021fbc67b486606468c67120b269f71db1b4595a30969496ad9f51332`.
The installation receipt is under ignored
`build/rust/console-followup-install/`; functional device acceptance is pending.
At the user's subsequent request, all 23 identified console backup items and
old test executables were removed, including this installation's rollback copy.
Both installed apps, their launchers, current configuration and music were
preserved. The cleanup receipt is in `build/rust/console-backup-cleanup/`.

## Share dark theme, 2026-11-09

The approved dark studio extends the Go light-only Share scene with the console
dark palette. Active and outgoing Share/Receive pages consume the current theme,
including automatic theme changes. Code tiles, text, hints, progress tracks,
completion controls and pedestals adapt without changing geometry or motion.
Pedestal variants are prepared once during asset loading; frame rendering uses
the existing textures. Album artwork is never tinted.

The follow-up SDL/GLES fixture captures both themes at 640 × 480, including name
entry, code entry, preparing, waiting, sending completion, connection/error,
receiving/saving, received actions, Receive Another and Back. A decoded color
fixture verifies identical artwork pixels in both themes. Native-size dark
captures were inspected; all nine prior light Share captures remain byte-identical.
The ordinary 419 tests pass, and the extended opt-in capture/input test passes.
Evidence is under ignored `build/rust/console-followup/` and
`build/rust/share-dark-*.log`. Live transfer and handheld display checks remain
separate from the private renderer fixtures.

## Separate acceptance stage

The first muted console run is now recorded in `CONSOLE_QA.md`. Further acceptance must verify audible continuity,
Bluetooth, controls, power behavior and frame-time distribution on the Anbernic
with the final UI workload. Provider availability, challenge handling and rate
limits also remain unverified by local fixtures. Broader service/error/motion
coverage can extend the finite checks above; it must not be confused with
unimplemented screens or proof of every behavior.

The detailed source audit and generated comparison reports are in
`build/rust/visual-parity/`. Historical performance numbers in `VALIDATION.md`
refer to the earlier prototype and must not be used to claim 60 fps for this UI.
