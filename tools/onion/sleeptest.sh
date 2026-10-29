#!/bin/sh
# The sleep timer (src/onion_sleep.go) on the emulated ARM binary, with a
# short timer: MIYOOPOD_SLEEP_MINUTES=1, a "minute" of 3 s.
#
#   tools/onion/sleeptest.sh [OUT]   -> OUT (default build/sleeptest)
#
# 1. left alone with nothing playing: the screen goes off, then betterPod
#    quits with /tmp/.offOrder (Onion's runtime.sh powers off);
# 2. the screen turned off with POWER: it quits after the timer and one
#    minute;
# 3. with MIYOOPOD_SLEEP_MINUTES=0 (Onion's "never") it stays up.
set -e
cd "$(dirname "$0")/../.."
OUT=${1:-build/sleeptest}
IMAGE=betterpod-onion-smoketest:1

[ -x dist/sdcard/App/betterpod/betterpod ] || { echo "run tools/onion/build.sh first" >&2; exit 1; }
docker image inspect "$IMAGE" > /dev/null 2>&1 || docker build --platform linux/arm/v7 -t "$IMAGE" tools/onion/smoketest

rm -rf "$OUT" && mkdir -p "$OUT/app" "$OUT/music" "$OUT/out"
cp -R dist/sdcard/App/betterpod/. "$OUT/app/"
python3 tools/onion/smoketest/music.py "$OUT/music"

docker run --rm --platform linux/arm/v7 \
  -v "$PWD/$OUT/app:/app" -v "$PWD/$OUT/music:/music" -v "$PWD/$OUT/out:/out" \
  -v "$PWD/tools/onion/smoketest/sleep.sh:/sleep.sh:ro" \
  "$IMAGE" sh /sleep.sh
cat "$OUT/out/result.txt"
grep -q FAIL "$OUT/out/result.txt" && exit 1 || exit 0
