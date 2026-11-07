#!/bin/sh
# Paired static UI proof only. Offline fixtures, no console or user library.
set -eu
cd "$(dirname "$0")/../.."
ROOT=$PWD
OUT="$ROOT/build/rust/youtube-design-checks"
mkdir -p "$OUT"
python3 rust/tools/youtube-design-checks.py fixtures --root "$OUT"
docker run --rm --network none -v "$ROOT:/repo:ro" -v "$OUT:/out" \
 -v betterpod-gomod:/go/pkg/mod -v betterpod-visual-gocache:/root/.cache/go-build \
 -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
 -v betterpod-rust-target-checks:/target-checks \
 -e CARGO_BUILD_JOBS=2 -e SDL_AUDIODRIVER=dummy -e LIBGL_ALWAYS_SOFTWARE=1 -e DISPLAY=:96 \
 -e BETTERPOD_VISUAL_MANIFEST=/out/scenes.json -e BETTERPOD_VISUAL_FIXTURES=/out/fixtures \
 -e BETTERPOD_RUST_ASSETS_DIR=/repo/port/betterpod/assets -e BETTERPOD_AUTO_UPDATE_CHECK=0 -e BETTERPOD_SOFT_VOLUME=1 \
 betterpod-rust-build:1 sh -ec '
 mkdir -p /w
 export LD_LIBRARY_PATH=/repo/port/betterpod/libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
 python3 /repo/rust/tools/youtube-design-checks.py prepare-go --source /repo --target /w/go --root /out
 ln -s /repo/port/betterpod/assets /w/go/src/assets
 cd /w/go/src
 for pass in 1 2; do
  export BETTERPOD_VISUAL_OUTPUT=/out/reference-$pass BETTERPOD_DATA_DIR=/out/reference-$pass/private-data
  CGO_CFLAGS=-I/usr/include/SDL2 go test -v -count=1 -run "^TestVisualParitySnapshots$" *.go
 done
 cp -R /repo/rust /w/rust
 Xvfb :96 -screen 0 640x480x24 +extension GLX -nolisten tcp >/out/xvfb.log 2>&1 &
 display_pid=$!;trap "kill $display_pid 2>/dev/null || true" EXIT
 n=0;until xdpyinfo >/dev/null 2>&1;do n=$((n+1));[ "$n" -lt 100 ]||exit 1;sleep .05;done
 for pass in 1 2;do
  export BETTERPOD_VISUAL_OUTPUT=/out/candidate-$pass
  cargo test --locked --offline --manifest-path /w/rust/Cargo.toml --target-dir /target-checks \
   --bin betterpod-rust app::visual_qa::capture_static_scenes -- --exact --ignored --nocapture --test-threads=1
 done
 '
python3 rust/tools/youtube-design-checks.py compare --root "$OUT"
