#!/bin/sh
# Profiles the interface without a Miyoo: builds it natively in Docker, runs
# it in Onion mode on one core in Xvfb, walks through the screens that
# animate and reports the drawing time per scenario, plus a CPU profile.
#
#   tools/onion/perf.sh [OUT]   -> OUT/report.txt, OUT/cpu.prof (default build/perf)
#   PERF_EFFECTS=full tools/onion/perf.sh OUT   (Settings → Effects: Full)
#
# The host is several times faster than a Cortex-A7: compare scenarios and
# versions with each other, not with the Miyoo's 16.7 ms frame.
set -e
cd "$(dirname "$0")/../.."
OUT=${1:-build/perf}
IMAGE=betterpod-onion-perf:1
docker image inspect "$IMAGE" > /dev/null 2>&1 || docker build -t "$IMAGE" tools/onion/perf
rm -rf "$OUT" && mkdir -p "$OUT/music"
python3 tools/onion/perf/music.py "$OUT/music"
docker run --rm -e PERF_EFFECTS="${PERF_EFFECTS:-}" -v "$PWD:/w:ro" -v "$PWD/$OUT/music:/music:ro" -v "$PWD/$OUT:/out" \
  -v betterpod-onion-perf-gocache:/root/.cache -v betterpod-onion-gomod:/go/pkg/mod "$IMAGE" sh /w/tools/onion/perf/run.sh
python3 tools/onion/perf/report.py "$OUT/miyoopod.log" | tee "$OUT/report.txt"
