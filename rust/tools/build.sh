#!/bin/sh
# Build/test the separate Rust app. No push, release or console writes.
# Usage: rust/tools/build.sh [native|knulli|test|all]
set -eu
cd "$(dirname "$0")/../.."
MODE=${1:-native}
IMAGE=betterpod-rust-build:1
case "$MODE" in native|knulli|test|all) ;; *) echo "Expected native, knulli, test or all" >&2; exit 2 ;; esac
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
  docker image inspect betterpod-build:1 >/dev/null 2>&1 || docker build -t betterpod-build:1 -f tools/Dockerfile tools
  docker build -t "$IMAGE" -f rust/tools/Dockerfile .
fi
mkdir -p build/rust
# Source and the production package are read-only. Cargo's target and registry
# are separate caches, so test runs cannot alter the installed or stable app.
docker run --rm \
  -v "$PWD:/repo:ro" -v "$PWD/build/rust:/out" \
  -v betterpod-rust-registry:/opt/cargo/registry \
  -v betterpod-rust-git:/opt/cargo/git \
  -v betterpod-rust-target-native:/target-native -v betterpod-rust-target-knulli:/target-knulli \
  -e BUILD_MODE="$MODE" -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" "$IMAGE" sh -ec '
    mkdir -p /w
    # Host Cargo caches are not source and can dwarf the entire checkout.
    tar -C /repo --exclude=rust/target -cf - rust | tar -C /w -xf -
    cd /w/rust
    if [ "$BUILD_MODE" = test ] || [ "$BUILD_MODE" = all ]; then
      cargo test --locked --all-targets --target-dir /target-native
    fi
    if [ "$BUILD_MODE" = native ] || [ "$BUILD_MODE" = all ]; then
      cargo build --locked --release --target-dir /target-native
      cp /target-native/release/betterpod-rust /out/betterpod-rust.native
    fi
    if [ "$BUILD_MODE" = knulli ] || [ "$BUILD_MODE" = all ]; then
      mkdir -p /out/linklibs
      ln -sf /repo/port/betterpod/libs/libSDL2_mixer-2.0.so.0 /out/linklibs/libSDL2_mixer.so
      export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=/w/rust/tools/link-knulli.sh
      export CC_aarch64_unknown_linux_gnu=/w/rust/tools/cc-knulli.sh
      cargo build --locked --release --target aarch64-unknown-linux-gnu --target-dir /target-knulli
      cp /target-knulli/aarch64-unknown-linux-gnu/release/betterpod-rust /out/betterpod-rust.knulli
      b=/out/betterpod-rust.knulli
      readelf -h "$b" | grep -Eq "Machine:[[:space:]]+AArch64"
      glibc=$(readelf --dyn-syms -W "$b" | sed -n "s/.*@GLIBC_\([0-9.]*\).*/\1/p" | sort -uV | tail -n 1)
      if [ -n "$glibc" ] && [ "$(printf "%s\n2.28\n" "$glibc" | sort -V | tail -n 1)" != 2.28 ]; then
        echo "Knulli supports glibc <=2.28; app requires $glibc" >&2; exit 1
      fi
      echo "Verified AArch64 / glibc ${glibc:-unknown}"
    fi'
