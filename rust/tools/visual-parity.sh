#!/bin/sh
# Fixed visual states only: no console access, physical inputs or performance run.
set -eu
cd "$(dirname "$0")/../.."
MODE=${1:-all}
case "$MODE" in all|go|rust|compare) ;; *) echo 'Usage: visual-parity.sh [all|go|rust|compare]' >&2; exit 2 ;; esac
ROOT=$PWD
OUT="$ROOT/build/rust/visual-parity"
mkdir -p "$OUT"
python3 rust/tools/visual-parity.py fixtures --root "$OUT"
if [ "$MODE" = compare ]; then python3 rust/tools/visual-parity.py compare --root "$OUT"; exit; fi
docker run --rm --network none -v "$ROOT:/repo:ro" -v "$OUT:/out" \
 -v betterpod-gomod:/go/pkg/mod -v betterpod-visual-gocache:/root/.cache/go-build \
 -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
 -v betterpod-rust-target-checks:/target-checks \
 -e VISUAL_MODE="$MODE" -e CARGO_BUILD_JOBS=2 -e SDL_AUDIODRIVER=dummy \
 -e LIBGL_ALWAYS_SOFTWARE=1 -e DISPLAY=:96 \
 -e BETTERPOD_VISUAL_MANIFEST=/out/scenes.json -e BETTERPOD_VISUAL_FIXTURES=/out/fixtures \
 -e BETTERPOD_RUST_ASSETS_DIR=/repo/port/betterpod/assets \
 -e BETTERPOD_AUTO_UPDATE_CHECK=0 -e BETTERPOD_SOFT_VOLUME=1 \
 betterpod-rust-build:1 sh -ec '
  mkdir -p /w
  export LD_LIBRARY_PATH=/repo/port/betterpod/libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
  if [ "$VISUAL_MODE" = all ] || [ "$VISUAL_MODE" = go ]; then
   python3 /repo/rust/tools/visual-parity.py prepare-go --source /repo --target /w/go --root /out
   ln -s /repo/port/betterpod/assets /w/go/src/assets
   cd /w/go/src
   for pass in 1 2; do
    export BETTERPOD_VISUAL_OUTPUT=/out/reference-$pass BETTERPOD_DATA_DIR=/out/reference-$pass/private-data
    CGO_CFLAGS=-I/usr/include/SDL2 go test -v -count=1 -run "^TestVisualParitySnapshots$" *.go
   done
  fi
  if [ "$VISUAL_MODE" = all ] || [ "$VISUAL_MODE" = rust ]; then
   cp -R /repo/rust /w/rust
   python3 - <<PY
from pathlib import Path
p=Path("/w/rust/src/app.rs")
s=p.read_text()
if "mod visual_qa;" not in s:p.write_text(s+"\n#[cfg(test)]\n#[path=\"app_visual_qa.rs\"]\nmod visual_qa;\n")
PY
   Xvfb :96 -screen 0 640x480x24 +extension GLX -nolisten tcp >/out/xvfb.log 2>&1 &
   display_pid=$!;trap "kill $display_pid 2>/dev/null || true" EXIT
   n=0;until xdpyinfo >/dev/null 2>&1;do n=$((n+1));[ "$n" -lt 100 ]||exit 1;sleep .05;done
   for pass in 1 2;do
    export BETTERPOD_VISUAL_OUTPUT=/out/candidate-$pass
    cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-checks \
     --bin betterpod-rust app::visual_qa::capture_static_scenes -- --exact --ignored --nocapture --test-threads=1
   done
  fi
 '
if [ "$MODE" = all ]; then python3 rust/tools/visual-parity.py compare --root "$OUT"; fi
