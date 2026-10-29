#!/bin/sh
# Smoke test of the Onion build without a Miyoo: the ARM binary from
# tools/onion/build.sh runs emulated (Docker, linux/arm/v7) with Debian's
# SDL2, in Xvfb. Keys are pressed with xdotool (the Miyoo buttons are
# keyboard keys) and through MIYOOPOD_ONION_INPUT (POWER, volume keys);
# every step's frame is saved as a PNG.
#
#   tools/onion/smoketest.sh [OUT]   -> OUT (default build/smoketest)
#   GODEBUG=gctrace=1 tools/onion/smoketest.sh   (the GC's trace in OUT/frames/stdout.txt)
#
# It checks start-up, the library scan, navigation, playback, the Onion keys
# and quitting. It can't check the Miyoo's own SDL2 driver (video, MI_AO).
set -e
cd "$(dirname "$0")/../.."
OUT=${1:-build/smoketest}
IMAGE=betterpod-onion-smoketest:1

[ -x dist/sdcard/App/betterpod/betterpod ] || { echo "run tools/onion/build.sh first" >&2; exit 1; }
docker image inspect "$IMAGE" > /dev/null 2>&1 || docker build --platform linux/arm/v7 -t "$IMAGE" tools/onion/smoketest

rm -rf "$OUT" && mkdir -p "$OUT/app" "$OUT/music" "$OUT/frames"
cp -R dist/sdcard/App/betterpod/. "$OUT/app/"
python3 tools/onion/smoketest/music.py "$OUT/music"
# a podcast with an episode betterPod downloaded (noted in podcasts.json),
# to delete offline
mkdir -p "$OUT/music/Podcasts/Smoke Cast" "$OUT/app/conf"
head -c 4096 /dev/zero > "$OUT/music/Podcasts/Smoke Cast/2026-10-27 Hello.mp3"
printf '[{"name":"Smoke Cast","author":"betterPod","feed":"http://127.0.0.1:9/feed","art":"","art600":"","files":{"smoke:hello":"2026-10-27 Hello.mp3"}}]' > "$OUT/app/conf/podcasts.json"

docker run --rm --platform linux/arm/v7 -e GODEBUG="${GODEBUG:-}" \
  -v "$PWD/$OUT/app:/app" -v "$PWD/$OUT/music:/music" -v "$PWD/$OUT/frames:/out" \
  -v "$PWD/tools/onion/smoketest/run.sh:/run.sh:ro" \
  "$IMAGE" sh /run.sh

python3 tools/onion/smoketest/raw2png.py "$OUT"/frames/*.raw
rm -f "$OUT"/frames/*.raw
ls "$OUT/frames"
