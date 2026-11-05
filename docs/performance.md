# Performance validation for betterPod 1.1

Audit date: 2026-11-05. The release goal is continuous audio first, then a regular UI cadence with CPU headroom. The intended UI target is a nominal **30 fps** during motion. This document does not promise glitch-free playback in every condition, 60 fps, or physical display synchronization.

## Hardware and method

- Anbernic RG35XX Plus: Allwinner H700, four Cortex-A53 cores, 1 GB RAM, Mali G31; Knulli Scarab 20260511.
- The console reports 640×480 at **59.941 Hz**; the driver reports a nominal 60 Hz mode. Its separate driver fps counter does not measure betterPod's frame rate.
- The original comparison used source commit `1db00b197fd48c1a496a4d3e84c8dba1ce586309`: original pacing/Go drawing, attempted 60 fps/Go, 30 fps/Go, and 30 fps/C.
- Each run covered eleven scenarios with two settling seconds and about four measured seconds per scenario: home, album scrolling, Now Playing, repeated preference saves, initial/scrolling/covered/warm Cover Flow, cover screensaver, Visualizer and screen off.
- Diagnostic builds used disposable source copies and separate settings, database, artwork cache and logs. Per-loop samples were buffered in RAM; no per-frame screenshots were written during timing runs. CPU profiling was enabled consistently.
- Screen-on runs used 1512 MHz/performance; screen-off used 480 MHz/powersave. Prefetch and the kernel's SD cache were uncontrolled, so an initial entry is not a proven cold-cache test.

Frame statistics distinguish whole-loop work, rendering, SDL presentation cost and intervals between completed presentation calls. A presentation completion is an application/API observation, **not a physical LCD scanout measurement**. Whole-window fps includes intentional still periods; continuous-animation intervals are reported separately.

## Baseline findings

| Workload | Measurement | Meaning |
|---|---|---|
| Repeated preference saves, original pacing | Render p95 9.23 ms; whole-loop work p95 **42.82 ms**; active presentation gap p95 **64.46 ms** | Work outside drawing blocks the interface. |
| Repeated saves with a 30 fps cap | Gap p95 66.78 ms with Go / 66.73 ms with C | A frame cap alone does not remove storage stalls. |
| Cover Flow, same 30 fps cap | Render p95 **24.376 → 18.305 ms** from Go to C | The existing pixel-equivalent C path reduces rendering cost by about 25%. |
| Cover Flow, C/30 | Active gap median 34.14 ms; p95 37.29 ms | Useful headroom, without proof of locked 30 fps. |
| Attempted 60 fps, Now Playing | 59.77 API completions/s; gap p95 30.83 ms, with frequent 11–12 / 29–31 ms intervals | A high average conceals uneven cadence. |

These C/30 figures precede the combined preference, artwork, occlusion and pacing fixes. They are not the final release's after measurements.

Preference saves called `Write → fsync → Close → Rename` synchronously during controls. A separate 100-write probe on the same SD measured fsync-style write cycles at p95 35.03 ms and maximum 72.28 ms; this probe was not a direct timing of the Go handler. Artwork disk reads and fallback scaling also ran during rendering. Cover Flow drew even when a settled opaque view covered it.

The original CPU profile attributed about 22.9% of cumulative samples to software texture rasterization, 19.7% to artwork loading and 12.2% to presentation. Artwork loading produced about 85% of cumulative allocated bytes, including startup/preparation. These are whole-run observations, not steady-frame allocation rates. GC pauses were not the main cause of the observed preference stalls.

## Implemented changes

1. A single preferences worker coalesces immutable snapshots. Atomic synchronization/rename is retained, with durability barriers at shutdown, restart and folder changes. Old-path writes cannot recreate a moved database.
2. Ordinary artwork lookups use RAM and bounded worker queues. Workers read/resize disk images; generations reject outdated results. Common placeholders are prepared before the UI loop.
3. C pixel loops are enabled by default. `BETTERPOD_C_DRAW=0` or `BETTERPOD_GO_DRAW=1` selects the Go fallback. A settled opaque overlay skips hidden Cover Flow drawing while logical state and exit cleanup continue.
4. Whole-loop monotonic deadlines include input/callback work, skip missed slots and target nominal 30 fps motion. Idle controls and screen-off polling remain separate. Video can wake earlier according to its audio clock.
5. PERF logs separate drawn and presented frames, draw cost, whole-loop work and literal continuous-animation presentation gaps over 50 ms.

The audio codec, native sample-rate policy, float mixing and requested audio buffer sizes are unchanged. Standalone Now Playing already cached its static 3D stage; no second cache was added. A larger renderer rewrite is outside these fixes.

## Combined after measurements

Two repeated runs of the combined changes completed on the same H700 console, using two settling seconds and approximately **eight measured seconds per scenario**. The original phases measured about four seconds once per configuration; maxima and p99 therefore come from different-sized samples. The same-value preference stress exercises saving without changing system volume.

| Metric | After run 1 | After run 2 |
|---|---:|---:|
| Preference-save whole-loop work p95 | 12.573 ms | 12.716 ms |
| Preference-save active presentation gap p95 | 34.503 ms | 34.342 ms |
| Home active presentation gap p95 | 35.824 ms | 36.578 ms |
| Cover Flow scrolling render p95 | 19.016 ms | 19.247 ms |
| Cover Flow active presentation gap p95 | 36.070 ms | 36.282 ms |
| Up Next over Cover Flow render p95 | 14.167 ms | 12.768 ms |
| Now Playing active presentation gap p95 | 34.598 ms | 34.709 ms |
| PipeWire sink/app ERR changes | 0 → 0 / 0 → 0 | 0 → 0 / 0 → 0 |

The preference workload's original whole-loop p95 was 42.819 ms and its active gap p95 was 64.463 ms. Both repeats substantially reduce that measured stall. No continuous-animation presentation gap exceeded 50 ms in these samples. There was still a **58.091 ms UI loop** in Up Next during run 1, while it was not continuously animating. Rendering/presentation occupied about 14.2 ms; the remaining delay was after presentation in diagnostic accounting/logging. This is not a zero-stall claim. Now Playing's whole-phase count was about 24.7 presentations/s because it includes intentional idle/unchanged frames; active intervals remained near the nominal 30 fps cadence.

The measured build predates only the final failed-write retry fix. Its successful-write and UI paths are unchanged; the final retry behavior received unit/race tests and the final binary smoke check below. These measurements do not establish a physical display lock or a guaranteed minimum rate in other workloads.

## Audio and visual evidence

In the original comparison, approximately 319 seconds and 325 PipeWire snapshots showed sink ERR **0 → 0**; each of the four audit streams also remained **0 → 0**. In the two after runs, the sink stayed **0 → 0 across 252 snapshots**, and each app stream stayed **0 → 0 across 125 observations**, without counter resets. All sampled playback states were playing. The two callback recorders collected **1,895 records with zero recorder drops**.

The source was one RAM-loaded 44.1 kHz M4A; the Sony WH-1000XM6 sink negotiated 96 kHz, with a 48 kHz / 2048-frame graph quantum. Callback intervals are scheduling observations, not xrun counts or proof of correct decoded samples. Listening was not controlled throughout. After the on-device runs, the installed player and the original CPU/Wi-Fi power settings were restored.

Independent visual validation ran the production ARM64 binary under isolated Docker/Xvfb, using dummy audio and temporary data. It captured 29 PNGs covering home, distinct final artwork, Cover Flow scrolling/flip, Up Next and reveal, standalone Now Playing pause/tilt/card, and a complete 30-album screensaver fixture. Two 108,800-pixel cover regions before/after Up Next had zero differing RGB pixels. No blank/stale reveal or geometry failure was observed.

The 29-screen visual build's SHA-256 was `09d41183f6cc8b1da0fd647fef49649a142bde37ea3beea2698c1731efde850e`. The final production binary, including the preferences-retry fix, has SHA-256 `f757bd6ad2c3154a89052919283e4085130ac7a25d602d7681bf4e99bc62028d`. A further two-screen smoke checked its startup/artwork and Cover Flow return from Up Next: the 108,800-pixel cover region again had zero differing RGB pixels. These visual checks do not establish H700 timing, physical scanout cadence or audio fidelity.

## Limits and repeatability

Short sequential runs do not establish endurance across SD cards, temperatures, libraries or radio conditions. The original library contained 198 tracks, with no file at least 48 MiB. Large streamed files, high-rate lossless audio, track transitions, network Listen/video and adverse Bluetooth conditions require separate workloads. A static screen correctly presents no new frames when nothing changes; the small original cover-screensaver fixture did not stress sustained cover flips.

Run `tools/unittest.sh` and `tools/docker-build.sh` for source/package validation. Local untracked audit evidence is retained under `build/perf-audit/` (original `device/`, repeated `device-after/`, CSVs, profiles, JSON summaries and instrumentation) and `build/perf-visual/` (captures and binary/hash checks). Keep performance runs free of frame dumps, compare like-for-like governor/audio settings, retain long gaps, and inspect p95/p99/max alongside CPU headroom and PipeWire ERR changes.
