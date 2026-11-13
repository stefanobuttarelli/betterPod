#!/bin/sh
# Local GPU inspection of all real puzzle engines and Rust drawing callbacks.
set -eu
cd "$(dirname "$0")/../.."
mkdir -p build/rust/puzzle-captures
docker run --rm -v "$PWD:/repo:ro" -v "$PWD/build/rust:/out" \
  -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
  -v betterpod-rust-target-checks:/target-checks \
  -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" -e SDL_AUDIODRIVER=dummy -e LIBGL_ALWAYS_SOFTWARE=1 betterpod-rust-build:1 sh -ec '
    mkdir -p /w; cp -R /repo/rust /w/rust; mkdir -p /w/rust/examples
    cp /repo/rust/tools/puzzle-probe.rs /w/rust/examples/puzzle-probe.rs
    cargo test --locked --lib puzzles:: --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
    cargo build --locked --example puzzle-probe --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
    Xvfb :96 -screen 0 640x480x24 +extension GLX > /out/puzzle-captures/xvfb.log 2>&1 &
    display_pid=$!; trap "kill $display_pid 2>/dev/null || true" EXIT
    export DISPLAY=:96
    if [ "$(uname -m)" = aarch64 ]; then export LD_LIBRARY_PATH=/repo/port/betterpod/libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}; fi
    n=0; until xdpyinfo >/dev/null 2>&1; do n=$((n+1)); [ "$n" -lt 50 ] || exit 1; sleep .1; done
    /target-checks/debug/examples/puzzle-probe
    mv /out/puzzle-*.png /out/puzzle-captures/
  '
