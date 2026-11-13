# Sony Bluetooth and memory follow-up — 2026-11-06

Branch `hello-rust`; independent Rust app on the RG35XX Plus / Knulli. Tests use a private configuration and the existing shared music read-only. The ordinary Rust executable is replaced with a backup; the stable Go app and real user data are preserved. No publication or release.

## Corrections

- Restored the five original Go glossy Bluetooth device tiles (headphones, earbuds, speaker, gamepad, generic), generated at 192 px for the native 96 px slot before rendering starts. Paired and discovered devices use the same name/BlueZ classifier; WH-1000XM6 uses headphones. Actual console screenshot confirms the icon.
- A transient failed `pactl list` consumes one routing probe instead of ending the retry loop. The same eight-second / sixteen-attempt budget applies.
- Cover Flow -> album -> song left Now Playing's full panel at +640 px. Shared-player transitions now reconcile the navigation depth and panel endpoints, retaining the cover animation. Four return paths use the central pop helper. A real SDL/GLES test repeats ordinary inputs three times; its negative control against the previous implementation fails on the same blank player. The check is included in `check-app.sh`.
- Noto fallback fonts retain the original regular/bold source bytes and prepare only requested outlines. Inter/fontdue and GPOS are unchanged. Bounded glyph-local supersampling preserves thin strokes; existing native atlas caching remains. The isolated font profile falls from 212,972 to 58,020 kB RSS (151.3 MiB). Both weights retain cmap/advances and baseline placement; native-size coverage differences are <=2.22% mean alpha and weighted silhouette overlap >=96.38%. Four real GLES renderer checks include Latin, Greek, Cyrillic, Japanese/Chinese, with no GL errors.
- yt-dlp now uses a supervised process group with nonblocking bounded stdout/stderr. Timeout, cancellation, failure and an exited parent all clean up descendants without joining pipe readers that can block forever. On the H700, Deno uses 64 MiB old-space / 4 MiB semi-space, the resolver group is capped at 224 MiB RSS, and a 96 MiB system reserve / 160 MiB preflight protect audio headroom. Allocation failure becomes a concise YouTube error. No codec, audio buffer, swap, governor or pairing changes.

310 ordinary tests pass (234 library, 70 app, 6 proxy), including twelve supervisor regressions. The targeted real SDL/GLES menu test and font/renderer checks pass separately. AArch64/glibc <=2.28 verification passes. Actual bundled Deno 2.9.8 / V8 15.0 accepts the heap flags (reported heap limit 79,691,776 bytes).

## Original Bluetooth baseline, before the YouTube failure

Sony WH-1000XM6, LDAC/A2DP, negotiated 96 kHz float stereo sink. The native music source is 44.1 kHz / 4,096 frames. Physical display reports 59.941 Hz. The actual betterpod-rust sink-input, not only the default sink, is confirmed routed to Sony, uncorked and unmuted.

| Case | Wall time | Native callback increase | Inferred music advance |
|---|---:|---:|---:|
| Cover Flow navigation | 60.108 s | 647 | 60.093 s |
| Now Playing | 40.056 s | 432 | 40.031 s |
| Screen off | 370.007 s | 3,984 | 369.940 s |

The 296.008 s repeat-one source crosses one track boundary during screen-off. Pause releases the app's Knulli idle marker; resume reacquires it. No external QA awake marker masks the screen-off test, including its final seventy seconds beyond the five-minute idle threshold. All 235 sampled Sony states are running and unmuted; native/Sony PipeWire ERR counters remain zero, without node resets or BlueZ disconnect events inside these windows.

The initial UI intervals and first 117 seconds of screen-off run at zero output volume. The user authorizes 20%, then confirms continuous listening during the remaining approximately 253 seconds. This is finite listening evidence; silent counters alone do not establish audible continuity.

The later OOM leaves shutdown stalled in the SDL audio output. That process needs forced termination and does not flush frames.csv, so this baseline has no valid FPS result. No screen-off FPS is applicable.

## YouTube failure retained as a failed test

Opening the public Big Buck Bunny clip through Videos -> Recently Watched exhausts the 1 GB console's RAM before a video player/decoder starts. At OOM, the kernel records approximately 366.5 MiB Rust RSS, 238.0 MiB Deno, 77.0 MiB yt-dlp and 74.2 MiB EmulationStation, with no swap, then kills yt-dlp. The resource sampler has a 234.5 s gap; the native audio callback interval reaches 233.4 s and ALSA reports an underrun. The old process can subsequently report decoder playing=true while its native sink-input remains corked at position zero. A fresh process restores callback/position progress.

This is neither a completed video benchmark nor a clean Bluetooth run. The native `pcm_underruns` field does not count every ALSA xrun; it must not be used to dismiss the logged underrun. Original evidence is preserved separately from the corrected build.

## Library and artwork scaling

A real App/Library/Search/queue profile uses synthetic UTF-8 metadata at 1,000, 10,000 and 50,000 tracks. These are local CPU-allocation/RSS measurements, not console acceptance at those sizes.

- The menu height cache previously retained twelve full old result projections. Weak row identities and a 2 MiB prefix-height cache budget (apart from a single required oversized list) release obsolete result rows. At 50,000 tracks / approximately twelve tracks per album, twelve broad searches fall from 610.38 to 235.63 MiB live Rust allocations; desktop process RSS falls from 715.55 to 426.90 MiB. At 10,000 tracks live allocation falls from 196.52 to 108.65 MiB. Current query results and navigation are preserved.
- Artist searches previously scanned the entire album list for each matched artist. A temporary first-album map makes that pass linear. In the local worst shape of 50,000 distinct albums / 10,000 artists, broad queries fall from 1.46–2.43 seconds to 63–76 milliseconds. The complete old/new result sets are identical, including case, missing covers and first-album precedence.
- The scanner releases imported metadata/folder-cover maps after consuming them. Saving the cache borrows the indexed library instead of cloning it, and streams JSON through a 64 KiB buffer into the same fsync/rename transaction. The existing 64 MiB reader bound also bounds writes. An encoder/limit failure preserves the previous file. No persisted names/schema or user metadata change.
- High-resolution artwork now uses scaled JPEG IDCT and bounded-row Triangle resizing. PNG/WebP and ten pixel types match the previous final RGBA bytes; scaled JPEG differs by approximately 0.75–0.81/255 mean channel value (43.9–45.2 dB PSNR), retaining the final 512 px cover. Large images serialize; normal covers keep two decoder jobs. Linux background priority is inherited by JPEG component helpers. Conservative original-coefficient and input bounds prevent oversized allocations before decoding.

| Isolated 20 MP cover, local Mac process | Previous peak RSS | Corrected peak RSS |
|---|---:|---:|
| Baseline JPEG | 96.58 MiB | 4.66 MiB |
| Progressive JPEG | 124.64 MiB | 61.72 MiB |
| PNG | 89.02 MiB | 60.66 MiB |

GPU cover storage is already bounded to approximately 53.3 MiB for forty 512 px RGBA textures plus mipmaps, rather than one texture per album. The weather cache has a separate 32 MiB bound. Neither grows with the complete library. Metadata and eager UI row projections still grow with track and album counts: 50,000 single-track albums cost more than 50,000 tracks spread across normal albums. The former local profile reaches 304 MiB live allocations just after install, compared with 183 MiB for the latter; this is not proof that 50,000 distinct albums meet the H700 RAM/frame budget. Row virtualization/shared projections are the next substantial steady-memory opportunity.

## Corrected build acceptance

The final controlled run uses the original Knulli SDL/Mali/ALSA libraries. Three Cover Flow album/song/Back cycles pass without blank captures. The native music output remains float stereo 44.1 kHz / 4,096 frames; Sony keeps LDAC, negotiating 48 kHz in this later connection (the initial baseline negotiated 96 kHz). No forced codec, sample-rate or buffer override is introduced.

| Case | UI swaps/s | Gap p95 / p99 / max (ms) | Pipeline CPU, one core | Main RSS | Minimum MemAvailable |
|---|---:|---:|---:|---:|---:|
| Cover Flow, 45 s | 59.94 | 16.79 / 17.18 / 29.77 | 24.8% | 241.9 MiB | 498.3 MiB |
| Now Playing, 30 s | 59.68 | 16.78 / 17.63 / 33.73 | 26.2% | 242.3 MiB | 497.4 MiB |
| YouTube, 75 s | 29.97 | 33.66 / 34.99 / 50.06 | 65.3% | 264.3 MiB | 403.8 MiB |

CPU 100% means one of the four cores; the video figure includes decoder children, which peak at 85.65 MiB additional RSS. Audio services separately use approximately 12% of one core during music and 13% during video. Video delivers 29.65 distinct frames/s, advances its audio clock at 0.999976 times wall time, receives 9.66 MiB over Wi-Fi and has no receive-drop or PCM-underrun increment. It resumes the same Big Buck Bunny clip around 159 seconds, so this is not a content-identical new Go A/B comparison. The user confirmed continuous video listening in the preceding successful low-memory 75-second sample; that response is not relabelled as fresh listening to every later run.

Sony and music/video PipeWire nodes stay at ERR 0 with no resets in all measured windows. The internal sink already had seven historical errors and remains 7 -> 7. Actual native stream routing is recorded separately from default-sink observations. Frame boundary gaps are excluded only when the previous presentation falls outside the measured interval; no arbitrary leading/trailing second is discarded. Swap completion is not physical LCD scanout. There are two Cover Flow and nine Now Playing gaps above 25 ms, plus one video gap just above 50 ms: these samples do not prove permanently locked 60/30 fps.

## Bluetooth reconnect and frontend handoff

Deliberate disconnect/reconnect initially exposes an independent output stall: the decoder says Play but callbacks and position freeze. Both the ALSA Pulse plugin and a privately rebuilt native SDL Pulse backend reproduce it; neither is adopted as a fix. The common cause is the Rust launcher freezing EmulationStation with SIGSTOP while its live audio client remains connected to the Bluetooth graph. That stopped client cannot acknowledge graph changes. Resuming the frontend releases the stalled output. The stable Go launcher does not perform this SIGSTOP.

In the controlled original-SDL test, only ES's own input is muted and pinned to the stable internal sink while ES is running, then the frontend is stopped. Both UI reconnect and physical-link loss forced during Find New Devices then restore actual audio: each three-second check advances by 3.065 seconds and 33 callbacks. During scan, pause is observed 2.469 seconds after issuing disconnect; bluetoothctl itself takes 2.437 seconds. Normal application shutdown completes in 2.03 seconds and flushes the full frame CSV.

The final launcher performs this handoff through the Rust executable before SDL initialization. It identifies streams through their authenticated client PID, process start time and server/client/stream identities; it preserves mute and target metadata, including follow-default behavior. Pinning is explicit even when internal audio is initially the default. It verifies parking before SIGSTOP, resumes ES before graph restoration or waiting for app shutdown, and restores the original process state after exit. A private journal supports rollback/stale-owner recovery. No global sink/default, unrelated stream or firmware library is changed. Nine automated fixtures cover these transitions and identity reuse. Final executable/launcher integration also passes on the console: the new launcher parks exactly one verified ES stream, starts the 232-track library with the original Mali/ALSA drivers, then closes and restores mute, routing metadata and process state in 1.80 seconds. Its journal is removed. The normal Rust app is installed and reopened with its real private configuration, QA and metrics disabled, at 20% output volume.

Installed binary SHA-256: `a69e1d86ca33c5160051729175d94c264b48cb011b031d4a95b12b251f4dbd0f`.

## Evidence and limits

Local untracked artifacts: `build/rust/bluetooth-qa-2026-11-06/`, including `original-device/`, `original-results.tar.gz`, supervisor/unit/cross-build logs and the menu negative control. Font comparison/profile artifacts are under `build/rust/`.

Tests retain the existing pairing, codec and audio buffers. Actual physical headset buttons and long-term endurance need their own observations. These finite samples do not guarantee permanent 60 fps or uninterrupted Bluetooth in every radio environment.

## Tucson pairing follow-up — 2026-11-13

The user reported `AuthenticationCanceled` before any code appeared on a
Hyundai Tucson, with pairing initiated from the console. Inspection of the
installed 0.2.20 on Knulli (firmware date 20260511) found a concrete startup
failure: zbus looked for `/var/run/dbus/system_bus_socket`, but this firmware
has only `/run/dbus/system_bus_socket`. The app logged `no system bus`, so its
interactive agent, media player and phone-class setup never became active.
The adapter still reported class `0x006c0000`. Its advertised services already
included Audio Source and Handsfree Audio Gateway (`0x111f`); this observation
does not support adding a new telephone profile as the first fix.

The 0.2.21 build selects the existing `/run` system socket and
respects explicit system-bus addresses. It rechecks the socket on retries and
registers the interactive agent before each outgoing pairing. A separate input
bug blocked A and PIN editing while Connect waited; the existing confirmation
sheet and keyboard now receive those inputs while the underlying list stays
locked. The two input regressions were reproduced before the fix.

Validation: 474 ordinary tests passed (340 library, 128 app, 6 proxy); 34
existing opt-in tests remained ignored. The private D-Bus test actually ran,
covering registration, confirmation, rejection and local cancellation. The
Docker image now includes its required daemon. A local concurrent link ran
out of build-container memory; the final full suite passed with one Cargo
build job. AArch64/glibc 2.28 validation and package integrity checks passed.

A three-second headless probe of the corrected media worker on the actual
console connected to `/run/dbus/system_bus_socket` and registered the
KeyboardDisplay agent and media player. The probe did not pair, open audio,
change the device class or access the music library, and was removed afterward.
Five native-size Go/Rust component comparisons were inspected. All five Rust
captures were pixel-identical before and after this input change. Existing
differences include Make Visible, shorter row copy, the playing-track header
over the confirmation, and the lack of a dedicated visible typed-PIN field;
Go has no car-pairing flow, so its generic sheet and keyboard are the references.

Local evidence is under `build/rust/bluetooth-0.2.21/` and
`build/rust/bluetooth-visual-2026-11-13/`. This confirms the system-bus fix and
the application input regressions, not successful Tucson pairing or audible
car playback. Those require another on-car test.

### User confirmation and cover-art investigation

After the SSH installation, the user confirmed that the Tucson connection
works. The retained app log records a confirmation requested for Tucson, a yes
answer on the console and successful pairing after 16.5 seconds. Cover art is
still absent.

Read-only inspection confirmed BlueZ 5.82 on the console. The Tucson's cached
AVRCP Controller SDP record advertises version 1.6 and SupportedFeatures
`0x03c1`, including GetImageProperties (`0x0080`), GetImage (`0x0100`) and
GetLinkedThumbnail (`0x0200`). The car advertises the required receiving
capabilities; this is not evidence of a completed image transfer.

Stock BlueZ 5.82's local-player metadata parser ignores `mpris:artUrl` and its
AVRCP Target does not advertise a Cover Art responder. It cannot send the app's
images merely by adding that metadata property. The proposed upstream v4
implementation dated 2026-10-09 changes both bluetoothd and obexd and was
closed without being merged on 2026-10-24. It also expects a standard
session-bus MPRIS player, whereas betterPod registers its player directly on
the system bus. An experimental integration therefore requires coordinated
system-service and application work. No firmware/service replacement was
performed during this investigation.

The app also needs to publish its already-resolved artwork (including embedded
covers), valid escaped file URIs and refreshed cover metadata before such a
responder can consume it. Those changes alone cannot make the installed BlueZ
send covers.

Sources: [BlueZ 5.82 metadata parser](https://github.com/bluez/bluez/blob/5.82/profiles/audio/media.c),
[AVRCP implementation and feature bits](https://github.com/bluez/bluez/blob/5.82/profiles/audio/avrcp.c),
[upstream cover-art proposal and review](https://lore-kernel.gnuweeb.org/linux-bluetooth/20260803195620.255866-1-jan.brummer%40tabos.org/T/).
Local evidence: `build/rust/bluetooth-covers-2026-11-13/`.
