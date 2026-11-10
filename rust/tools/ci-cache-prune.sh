#!/bin/sh
# CI only: keep compiled dependencies in the Actions cache and drop this
# crate's own artifacts, which change with every commit and would only make
# the cache larger. Cargo rebuilds them from the restored dependencies.
# Usage: rust/tools/ci-cache-prune.sh <RUST_CACHE_DIR>
set -eu
root=${1:?cache directory}
[ -d "$root" ] || exit 0
# The build container writes as root.
sudo chown -R "$(id -u):$(id -g)" "$root"
for dir in "$root/target-native/debug" "$root/target-knulli/release" \
  "$root/target-knulli/aarch64-unknown-linux-gnu/release"; do
  [ -d "$dir" ] || continue
  rm -rf "$dir/examples" "$dir/incremental"
  for sub in deps .fingerprint build; do
    [ -d "$dir/$sub" ] || continue
    find "$dir/$sub" -mindepth 1 -maxdepth 1 \( -name 'betterpod_rust*' \
      -o -name 'libbetterpod_rust*' -o -name 'betterpod-rust*' -o -name 'stream_proxy*' \) \
      -exec rm -rf {} +
  done
  rm -f "$dir/betterpod-rust" "$dir/betterpod-rust.d" "$dir"/libbetterpod_rust.*
done
du -sh "$root"/* 2>/dev/null || true
