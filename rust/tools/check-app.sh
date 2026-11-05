#!/bin/sh
# Opt-in actual SDL/GLES controller routes. Separate scratch data and target cache.
set -eu
cd "$(dirname "$0")/../.."
MUSIC=${1:-build/desktop/music}
[ -d "$MUSIC" ] || { echo "Supply the existing local audio fixture as argument" >&2; exit 2; }
MUSIC=$(CDPATH= cd -- "$MUSIC" && pwd)
mkdir -p build/rust/app-captures
ROOT=$PWD
docker run --rm -v "$ROOT:/repo:ro" -v "$MUSIC:/music:ro" -v "$ROOT/build/rust:/out" \
 -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
 -v betterpod-rust-target-appchecks:/target-appchecks \
 -e CARGO_BUILD_JOBS=2 -e SDL_AUDIODRIVER=dummy -e DISPLAY=:94 -e LIBGL_ALWAYS_SOFTWARE=1 \
 -e BETTERPOD_RUST_QA_MUSIC=/music -e BETTERPOD_RUST_ASSETS_DIR=/repo/port/betterpod/assets \
 -e BETTERPOD_RUST_QA_OUTPUT=/out/app-captures \
 betterpod-rust-build:1 sh -ec '
  mkdir -p /w && cp -R /repo/rust /w/rust
  Xvfb :94 -screen 0 640x480x24 +extension GLX > /out/app-captures/xvfb.log 2>&1 &
  trap "kill $! 2>/dev/null || true" EXIT
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-appchecks --bin betterpod-rust -- --nocapture --test-threads=1
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-appchecks --bin betterpod-rust app::qa::controller_routes_and_real_screens -- --ignored --exact --nocapture --test-threads=1
 '
