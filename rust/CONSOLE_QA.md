# RG35XX Plus SSH validation — 2026-11-06

Branch: `hello-rust`. This is an independent experimental app, not a stable
release. The installed Go app and both real configuration directories are kept
separate from the copied QA fixtures. All console tests use a muted sink;
Bluetooth and controlled listening are outside this session.

## Bugs reproduced and corrected

- **White screen on Back:** custom controller exits popped the logical page
  without retargeting the shared menu panels. Returning to a split page could
  leave both panels outside the visible area. Video Home also used a full-page
  wrapper for its split projection. A shared pop path now derives the actual
  projected page, restores panel motion and retires the outgoing view. The SDL
  regression exercises actual transitions without snapping endpoints. Hardware
  checks include Podcasts, Videos, Playlists, Receive, settings, curation and
  15 rapid A/Back cycles; no white capture or view-count leak remained.
- **Weather texture corruption:** eviction could delete a texture while CPU
  batches still referenced it. A real GLES test reproduced the missing image
  before the fix and passes after deferring referenced deletions until the next
  frame. This is a concrete shared renderer defect; it does not establish that
  every reported weather animation symptom had the same cause.
- **Rain jumping between poses:** the user subsequently confirmed that the
  cloud snapped sideways and drops jumped vertically. Individual cache misses
  returned the entire icon to the still pose. A worker now prepares a complete
  60-frame atlas and publishes it atomically; drawing samples a phase only
  after the whole loop is ready. Native 168px/256px loops stay within the same
  32 MiB cache budget. Texture coordinates use high precision for large
  atlases. A real GLES check compares all 60 phases at both sizes against the
  original raster equations, including opacity, with no still-pose fallback.
- **Unsafe storage choice:** `/media/SHARE` was an unmounted directory on tmpfs,
  yet appeared as a destination card. Discovery now takes writable storage
  mountpoints from `/proc/mounts`, excludes temporary/read-only filesystems and
  decodes escaped names. No user music was moved during this test.
- **Clipped weather labels:** the forecast now applies the shared measured
  ellipsis before drawing city, status, conditions and detail labels.
- **Perspective image quality:** hardware MSAA smooths polygon edges; static
  artwork uses trilinear mipmaps and up to 4x anisotropic filtering where
  supported. Video textures, font atlases and UI sprites keep their existing
  update paths. Odd-sized cover mipmaps are prepared with an area filter on the
  decoder workers because the driver-generated version failed the synthetic
  minification check. Square and odd-sized checker fixtures now reduce alias
  energy by at least 75% against the ordinary linear sampler. Unsupported MSAA
  retries an ordinary framebuffer. Artwork is
  still bounded to 40 cached textures; mipmaps can add about one third to its
  GPU storage. Source artwork cannot acquire detail it never contained.

At the display's reported refresh target, swap synchronization owns pacing;
there is no second independent frame timer. Lower targets and unsynchronized
runs retain the monotonic timer. This change alone did not make weather run at
60 fps in the measured samples.

## Scope of functional checks

The initial muted sweep covered the shared roots, music/player controls and
options, settings categories and values, keyboard input, five native games,
twelve puzzle frontends, weather day navigation/refresh, savers/visualizers,
maintenance and library entry points. Playlist create/append/remove/delete and
an Internet Archive search used owned copied data. Destructive reset/move/tag
operations and every external-provider path were not exercised on the real
library. The wider sweep was stopped at the user's request to prioritize the
shared white-screen defect. The absence of further observations is not a claim
that every screen and service is bug-free.

## Measurement method

- Same console, 232 tracks / 16 albums, light theme, Full effects, same 296-second
  44.1 kHz M4A on repeat, default internal output muted. CPU governor remained
  `performance` at 1.512 GHz. GPU governor remained `simple_ondemand`; no GPU
  clock or audio buffer reduction was applied.
- `fbset` reports 640x480 at **59.941 Hz**. This supports a nominal 60 Hz UI.
  App swap-completion metrics are not direct measurements of LCD scanout.
- Each UI case has a five-second settling period and approximately 35 measured
  seconds. Cover Flow receives the same physical evdev Left/Right input train.
  Captures and their PNG compression occur outside measured intervals.
- An isolated Go 1.1 benchmark copy has an in-memory CSV collector and opt-in
  target override. Rendering code is unchanged, and the original pre-Rust font
  wrapper is restored. Both the original 30 target and a 60 target are tested.
  A Cover Flow overlay leaves Go's logical underlying screen ID as `home`;
  screenshots verify the actual visible screen.
- Rust's diagnostic socket is explicit, private and disabled in ordinary
  launches. Per-frame metrics stay in memory and are written on clean exit.
  A one-second resource sampler and PipeWire monitor run separately.
- Gaps are counted only when both endpoints belong to the measured interval.
  This excludes a first Go presentation spanning a preceding unchanged/idle
  screen, while retaining real long frames during the workload. No fixed
  leading/trailing second is discarded. Raw CSVs remain available.
- Go's settled Now Playing screen redraws its clock roughly once a second.
  Rust currently redraws that unchanged screen continuously. Their idle redraw
  counts are not a motion-fluidity comparison; Go is more CPU-efficient there.
- Weather's original sprite sequence has 60 phases over three seconds, hence
  20 distinct sprite frames/s. Extra UI swaps do not invent animation content.
- YouTube is opened through the ordinary Recently Watched menu and resolved
  live via Wi-Fi. The clip is `aqz-KE-bpKQ` (Big Buck Bunny). Both video pipelines
  target 30 video frames/s; a 60 Hz UI does not turn it into a 60 fps movie.
  Go renders 640x360 and uses its 44.1 kHz mixer, while Rust pads to 640x480 and
  uses a 48 kHz PCM output. Video CPU results compare the complete pipelines,
  not identical decoder configurations or the two languages in isolation.

## Results

All final Rust UI cases began and ended on the expected screen with music
playing. The measured rain loop recorded **zero additional still-pose waits**
after its initial load. Its two complete loops plus static icons occupied
27.07 MiB of the existing 32 MiB weather cache.

| Build / scenario | Presentations/s | Gap p95 / p99 / max (ms) | CPU % | Main RSS (MiB) |
|---|---:|---:|---:|---:|
| Go 30 target: coverflow | 28.81 | 43.69 / 68.02 / 85.03 | 146.2 | 131.1 |
| Go 30 target: youtube | 30.00 | 67.32 / 69.97 / 79.80 | 115.7 | 194.0 |
| Go 60 target: coverflow | 29.71 | 40.95 / 45.50 / 57.08 | 149.8 | 108.4 |
| Go 60 target: weather | 19.99 | 69.50 / 72.89 / 77.54 | 66.7 | 131.7 |
| Rust, MSAA 4x and cover filtering: coverflow | 59.97 | 16.77 / 16.85 / 23.25 | 19.9 | 412.8 |
| Rust, MSAA 4x and cover filtering: weather | 29.99 | 33.46 / 33.52 / 50.16 | 20.3 | 413.4 |
| Rust, MSAA 4x and cover filtering: youtube | 29.97 | 33.58 / 34.72 / 39.28 | 65.5 | 387.8 |

CPU 100% means one fully occupied core; the console has four. RSS excludes
child processes and does not account for all driver/GPU memory. Go video adds
about 82 MiB of child RSS at its peak; final Rust video adds about 94 MiB.

Rust Now Playing redraws at 59.86 presentations/s, with 16.78 / 16.89 /
33.42 ms p95 / p99 / maximum gaps and about 21.5% CPU. Go redraws the settled
screen roughly once a second at about 12.5% CPU. This is an idle-efficiency
opportunity for Rust, not evidence that the Go screen visibly animates at 1 fps.

The final YouTube sample ran for 75 seconds and delivered **29.24 actual video
frames/s** (UI swaps about 29.97/s); Go delivered about 30.00 video frames/s.
Rust’s video clock advanced at 1.00009 times wall time, with zero PCM underrun
increments and no receive-drop increment. Wi-Fi received 13.30 MiB during that
window. The video result does not establish 60 fps playback or frame-perfect
30 fps delivery. Some video frames were skipped.

Across 619 active sink observations in the last monitored run, the sink ERR
counter stayed **6 → 6**; music and video app streams stayed **0 → 0**.
Each measured music/UI and video interval also has zero sink/client ERR delta.
Native callback intervals sometimes span about 129 ms with 4,096-frame /
44.1 kHz buffers on the 48 kHz / 2,048-frame graph; callback spacing is not an
xrun count. An earlier combined run had two sink ERR increments during video
setup, outside its measured steady-playback window. Those increments did not
recur in the final monitored run; their audible effect and cause were not
established while muted.

These are finite samples, not a guarantee of permanently locked 60 fps.
The final music-player sample includes four gaps over 25 ms, and weather has
one 50.16 ms gap. Earlier cold-cover motion also recorded a 72.6 ms gap.
Cover Flow in the final sample had no gap over 25 ms. Weather UI remains near
30 swaps/s, with the source-faithful 20 fps sprite content.

The AA-only control preserved Cover Flow at 59.94/s versus 59.91/s without
AA, with CPU about 19.8% versus 18.6%. The full filtered/atlas build above
retains approximately 60/s. The console reports four MSAA samples; it does
not expose anisotropic filtering, so its artwork uses the trilinear fallback.

Validation: **274 ordinary tests passed** (202 library, 66 app, 6 proxy).
Three opt-in renderer tests, the full 60-phase/two-size rain comparison and
the real SDL menu-transition regression passed separately. On-console menu
and rapid-Back checks ended with `ISSUES []`, zero GL errors and no leaked
views. The false RAM-card destination is absent; forecast detail labels use
ellipsis. AArch64 / glibc 2.28 build verification passed.

Final tested binary SHA-256:
`99d3c01402419d9c1d3d1f83bc8772eb6374664d1e2ea8da8a3445a420f6c702`.

The final independent package and installed Rust binary contain these fixes.
The Go installation is preserved. Temporary QA processes and idle overrides
are removed after the session; the ordinary Rust launcher is restored, muted.

## Interpretation and remaining acceptance

The useful gain comes from the rewritten rendering architecture: Rust prepares
GPU geometry, whereas Go rasterizes much of the perspective UI on the CPU.
These measurements do not prove that choosing Rust alone makes any program
faster, nor that the Go application could not adopt a similar renderer.

Muted playback and stable error counters cannot prove audible fidelity. Repeat
controlled listening, natural track changes, long sessions, large libraries,
radio interference and physical Bluetooth controls separately. Downloads,
Share, all savers and adverse service/error cases still need endurance tests.
The independent Rust updater has no published release feed.

Local evidence is retained under `build/rust/console-qa-2026-11-06/`: baseline
and fixed screenshots, controller traces, source manifests, build/test logs,
`perf/*/{frames.csv,cases*.json,resources*.csv,pipewire.log,pipewire.jsonl}` and `perf/summary.json`.
The mistaken initial Go weather/menu samples are excluded after visual review.
One Rust UI sequence also failed its playback/menu precondition before any
measurement and was repeated while the console controls were left untouched.
No PR, push or release was made during this session. The original installed Go
app remains available, alongside the independently installed Rust app.
