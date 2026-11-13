#!/bin/sh
# Production transport with generated local A/V/silent fixtures; dummy SDL audio.
set -eu
cd "$(dirname "$0")/../.."
mkdir -p build/rust/video-fixtures
docker run --rm -v "$PWD:/repo:ro" -v "$PWD/build/rust:/out" \
  -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
  -v betterpod-rust-target-checks:/target-checks \
  -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" -e SDL_AUDIODRIVER=dummy betterpod-rust-build:1 sh -ec '
    mkdir -p /w; cp -R /repo/rust /w/rust
    ffmpeg -hide_banner -v error -y -f lavfi -i testsrc2=size=320x240:rate=30 -f lavfi -i sine=frequency=440:sample_rate=48000 -t 6 -c:v mpeg4 -q:v 5 -c:a aac /out/video-fixtures/av.mp4
    ffmpeg -hide_banner -v error -y -f lavfi -i testsrc2=size=320x240:rate=30 -t 2 -c:v mpeg4 -q:v 5 -an /out/video-fixtures/silent.mp4
    cargo test --locked --lib video:: --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
    cargo build --locked --example video-check --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
    if [ "$(uname -m)" = aarch64 ]; then export LD_LIBRARY_PATH=/repo/port/betterpod/libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}; fi
    /target-checks/debug/examples/video-check /out/video-fixtures/av.mp4 /out/video-fixtures/silent.mp4
  '
