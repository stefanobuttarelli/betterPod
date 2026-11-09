#!/bin/sh
# Native-size source references and actual input routes, using private fixtures.
set -eu
cd "$(dirname "$0")/../.."
ROOT=$PWD
OUT="$ROOT/build/rust/console-followup"
mkdir -p "$OUT"
python3 rust/tools/visual-parity.py fixtures --root "$OUT"
docker run --rm --network none \
 -v "$ROOT:/repo:ro" -v "$OUT:/out" \
 -v betterpod-gomod:/go/pkg/mod -v betterpod-visual-gocache:/root/.cache/go-build \
 -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
 -v betterpod-rust-target-native:/target-native \
 -e CARGO_BUILD_JOBS=2 -e SDL_AUDIODRIVER=dummy -e DISPLAY=:96 -e LIBGL_ALWAYS_SOFTWARE=1 \
 -e BETTERPOD_VISUAL_MANIFEST=/out/scenes.json -e BETTERPOD_VISUAL_FIXTURES=/out/fixtures \
 -e BETTERPOD_RUST_ASSETS_DIR=/repo/port/betterpod/assets \
 -e BETTERPOD_AUTO_UPDATE_CHECK=0 -e BETTERPOD_SOFT_VOLUME=1 \
 betterpod-rust-build:1 sh -ec '
  mkdir -p /w
  export LD_LIBRARY_PATH=/repo/port/betterpod/libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
  python3 /repo/rust/tools/visual-parity.py prepare-go --source /repo --target /w/go --root /out
  cp /repo/rust/tools/share-followup-go-test.go /w/go/src/share_followup_test.go
  ln -s /repo/port/betterpod/assets /w/go/src/assets
  cd /w/go/src
  export BETTERPOD_VISUAL_OUTPUT=/out/go BETTERPOD_DATA_DIR=/out/go/private-data
  CGO_CFLAGS=-I/usr/include/SDL2 go test -count=1 -run "^TestShareFollowupReference$" *.go
  tar -C /repo --exclude=rust/target -cf - rust | tar -C /w -xf -
  Xvfb :96 -screen 0 640x480x24 +extension GLX -nolisten tcp >/out/xvfb.log 2>&1 &
  trap "kill $! 2>/dev/null || true" EXIT
  export BETTERPOD_RUST_QA_OUTPUT=/out/rust
  mkdir -p /out/rust
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-native --bin betterpod-rust \
   app::app_services::followup_qa::console_followup_input_and_captures -- --ignored --exact --nocapture --test-threads=1
 '
