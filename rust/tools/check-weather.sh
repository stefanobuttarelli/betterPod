#!/bin/sh
# Fixed offline forecast screenshots plus local HTTP/cache tests.
set -eu
cd "$(dirname "$0")/../.."
mkdir -p build/rust/weather-captures
docker run --rm -v "$PWD:/repo:ro" -v "$PWD/build/rust:/out" \
  -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
  -v betterpod-rust-target-checks:/target-checks \
  -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" -e SDL_AUDIODRIVER=dummy -e LIBGL_ALWAYS_SOFTWARE=1 betterpod-rust-build:1 sh -ec '
    mkdir -p /w; cp -R /repo/rust /w/rust; mkdir -p /w/rust/examples
    grep -q "pub mod weather;" /w/rust/src/lib.rs || printf "\npub mod weather;\n" >> /w/rust/src/lib.rs
    cp /repo/rust/tools/weather-probe.rs /w/rust/examples/weather-probe.rs
    cargo test --locked --lib weather:: --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
    cargo build --locked --example weather-probe --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
    Xvfb :95 -screen 0 640x480x24 +extension GLX > /out/weather-captures/xvfb.log 2>&1 &
    display_pid=$!; trap "kill $display_pid 2>/dev/null || true" EXIT
    export DISPLAY=:95
    if [ "$(uname -m)" = aarch64 ]; then export LD_LIBRARY_PATH=/repo/port/betterpod/libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}; fi
    n=0; until xdpyinfo >/dev/null 2>&1; do n=$((n+1)); [ "$n" -lt 50 ] || exit 1; sleep .1; done
    /target-checks/debug/examples/weather-probe
    mv /out/weather-*.png /out/weather-captures/
  '
