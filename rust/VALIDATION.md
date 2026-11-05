# Local validation — 2026-11-05

Branch: `hello-rust`. Target: a separate RG35XX Plus / Knulli application with the betterPod 1.1 feature set. No console connection, installation, publication or release was performed during these checks.

## Executed checks

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

## Local timing

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

On the Anbernic, compare the stable app with the separate Rust app using the same music, formats, volume and headphones. Measure frame-time percentiles, CPU/RAM and PipeWire error deltas while listening through active navigation, each saver, downloads, Share, screen blanking, natural track changes and pause/resume. Test 60 Hz first; use 50/40/30 only if the lower cadence improves measured consistency and preserves uninterrupted audio.

The real provider implementations still need live service checks for availability, challenge handling and rate limits. Physical Bluetooth controls, brightness and power behavior also require the deferred console session. The independent Rust updater has no published feed; it never consumes the stable Go release manifests.

Generated session evidence is under `build/rust/`: `final-all-build.log`, `final-app-qa.log`, `media-check.log`, `final-performance.json`, `mac-final/frames.csv` and the smoke directory named by `latest-smoke.txt`. Metadata and Share evidence is recorded at the paths listed in `README.md`.
