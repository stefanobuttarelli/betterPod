#!/bin/sh
# Isolated local screenshots and logic tests of actual production game code.
set -eu
cd "$(dirname "$0")/../.."
mkdir -p build/rust/game-captures
docker run --rm -v "$PWD:/repo:ro" -v "$PWD/build/rust:/out" \
  -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
  -v betterpod-rust-target-checks:/target-checks \
  -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" -e SDL_AUDIODRIVER=dummy -e LIBGL_ALWAYS_SOFTWARE=1 betterpod-rust-build:1 sh -ec '
    mkdir -p /w; cp -R /repo/rust /w/rust; mkdir -p /w/rust/examples
    cp /repo/rust/tools/game-probe.rs /w/rust/examples/game-probe.rs
    cargo test --locked --lib games:: --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
    cargo build --locked --example game-probe --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
    Xvfb :97 -screen 0 640x480x24 +extension GLX > /out/game-captures/xvfb.log 2>&1 &
    display_pid=$!; trap "kill $display_pid 2>/dev/null || true" EXIT
    export DISPLAY=:97
    if [ "$(uname -m)" = aarch64 ]; then export LD_LIBRARY_PATH=/repo/port/betterpod/libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}; fi
    n=0; until xdpyinfo >/dev/null 2>&1; do n=$((n+1)); [ "$n" -lt 50 ] || exit 1; sleep .1; done
    /target-checks/debug/examples/game-probe
    mv /out/game-*.png /out/game-captures/
  '
