#!/bin/sh
# Run the native Rust binary under Xvfb/Mesa with its own scratch data.
# The result is a local integration check, not an H700 performance result.
set -eu
cd "$(dirname "$0")/../.."
[ -x build/rust/betterpod-rust.native ] || rust/tools/build.sh native
MUSIC=${1:-$PWD/build/desktop/music}
[ -d "$MUSIC" ] || { echo "Pass an existing music fixture directory" >&2; exit 2; }
MUSIC=$(cd "$MUSIC" && pwd)
DURATION=${BETTERPOD_RUST_SMOKE_SECONDS:-30}
case "$DURATION" in ''|*[!0-9]*) echo "Smoke seconds must be an integer" >&2; exit 2 ;; esac
[ "$DURATION" -ge 25 ] || { echo "Use at least 25 seconds for all benchmark phases" >&2; exit 2; }
TIMEOUT=$((DURATION+20))
OUT="$PWD/build/rust/smoke-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT/conf"
printf '%s\n' "$OUT" > build/rust/latest-smoke.txt
docker run --rm \
  -v "$PWD:/repo:ro" -v "$MUSIC:/music:ro" -v "$OUT:/out" \
  -e SDL_AUDIODRIVER=dummy -e LIBGL_ALWAYS_SOFTWARE=1 \
  -e BETTERPOD_RUST_MUSIC_DIR=/music -e BETTERPOD_RUST_DATA_DIR=/out/conf \
  -e BETTERPOD_RUST_ASSETS_DIR=/repo/port/betterpod/assets \
  -e BETTERPOD_RUST_EXIT_AFTER_SECONDS="$DURATION" -e SMOKE_TIMEOUT="$TIMEOUT" \
  -e BETTERPOD_RUST_SCREENSHOT=/out/screenshot.png \
  -e BETTERPOD_RUST_BENCHMARK=1 -e BETTERPOD_RUST_METRICS=/out/frames.csv \
  betterpod-rust-build:1 sh -ec '
    Xvfb :98 -screen 0 640x480x24 +extension GLX > /out/xvfb.log 2>&1 &
    display_pid=$!
    trap "kill $display_pid 2>/dev/null || true" EXIT
    export DISPLAY=:98
    if [ "$(uname -m)" = aarch64 ]; then export LD_LIBRARY_PATH=/repo/port/betterpod/libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}; fi
    n=0
    until xdpyinfo >/dev/null 2>&1; do
      n=$((n+1)); [ "$n" -lt 50 ] || exit 1
      sleep .1
    done
    timeout -k 3 "$SMOKE_TIMEOUT" /repo/build/rust/betterpod-rust.native > /out/stdout.log 2>&1
  '
python3 - "$OUT" <<'PYTEST'
import csv, json, pathlib, struct, sys
root = pathlib.Path(sys.argv[1])
cache = json.loads((root/'conf/library.json').read_text())
tracks = cache.get('library', {}).get('tracks', [])
assert tracks, 'Smoke did not load any tracks: do not report a music performance pass'
rows = list(csv.DictReader((root/'frames.csv').open()))
assert rows, 'No frame samples'
phases = {r['phase'] for r in rows}
assert {'home','coverflow','nowplaying','albums','visualizer'} <= phases, f'Missing benchmark phases: {phases}'
playing = [r for r in rows if r['playing'] == 'true']
assert playing, 'No sample shows active playback'
positions = [float(r['position_s']) for r in playing]
assert max(positions)-min(positions) > .05, 'Playback position did not advance'
png = (root/'screenshot.png').read_bytes()
assert png[:8] == b'\x89PNG\r\n\x1a\n'
assert struct.unpack('>II', png[16:24]) == (640,480)
print('PASS local integration:', len(tracks), 'tracks,', len(rows), 'frame samples,', len(playing), 'playing samples, all 5 phases')
PYTEST
echo "Smoke artifacts: $OUT"
