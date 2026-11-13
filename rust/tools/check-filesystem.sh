#!/bin/sh
# Filesystem fixtures and lifecycle barriers; no user music, real updates or console.
set -eu
cd "$(dirname "$0")/../.."
mkdir -p build/rust
docker run --rm -v "$PWD:/repo:ro" -v "$PWD/build/rust:/out" \
  -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
  -v betterpod-rust-target-checks:/target-checks \
  -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" betterpod-rust-build:1 sh -ec '
    mkdir -p /w; cp -R /repo/rust /w/rust
    for module in maintenance updater; do grep -q "pub mod $module;" /w/rust/src/lib.rs || printf "\npub mod %s;\n" "$module" >> /w/rust/src/lib.rs; done
    cargo test --locked --lib maintenance:: --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
    cargo test --locked --lib updater:: --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
    cargo test --locked --lib weather:: --manifest-path /w/rust/Cargo.toml --target-dir /target-checks
  '
