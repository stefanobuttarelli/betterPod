# Go 1.1 source port — local verification

This local `hello-rust` branch keeps the independent Rust installation and uses
the Go 1.1 source as the contract for menus, controls, persistence, layout and
animation. The prior playable prototype is not a completion baseline. No console
deployment, hardware benchmark, PR, push or release belongs to this pass.

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

## Separate acceptance stage

The console run remains deliberately separate. Verify audible continuity,
Bluetooth, controls, power behavior and frame-time distribution on the Anbernic
with the final UI workload. Provider availability, challenge handling and rate
limits also remain unverified by local fixtures. Broader service/error/motion
coverage can extend the finite checks above; it must not be confused with
unimplemented screens or proof of every behavior.

The detailed source audit and generated comparison reports are in
`build/rust/visual-parity/`. Historical performance numbers in `VALIDATION.md`
refer to the earlier prototype and must not be used to claim 60 fps for this UI.
