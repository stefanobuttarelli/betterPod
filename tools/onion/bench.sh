#!/bin/sh
# Benchmarks of the interface's pixel loops, Go against cdraw.c, built for
# the Miyoo (32-bit ARM, zig/clang like the app) and run emulated in the
# smoke test image. Emulated time follows the instruction count, which is
# what an in-order Cortex-A7 pays: compare the pairs, not the absolute ns.
#
#   tools/onion/bench.sh [regexp]   (default: all)
set -e
cd "$(dirname "$0")/../.."
mkdir -p build/bench
docker run --rm -v "$PWD:/repo:ro" -v "$PWD/build/bench:/out" -v betterpod-onion-gocache:/root/.cache -v betterpod-onion-gomod:/go/pkg/mod \
  betterpod-onion-build:1 sh -c '
    cp -R /repo /w && cp /repo/tools/onion/unittest/*_test.go /w/src/ && cd /w/src
    CGO_ENABLED=1 GOOS=linux GOARCH=arm GOARM=7 CC=/w/tools/onion/zcc.sh \
    CGO_CFLAGS="-I/opt/sdl2/include/SDL2 -O2" \
    CGO_LDFLAGS="-L/w/onion/betterpod/libs -Wl,--allow-shlib-undefined" \
      go test -c -tags netgo,osusergo -o /out/betterpod.test *.go'
docker image inspect betterpod-onion-smoketest:1 > /dev/null 2>&1 || docker build --platform linux/arm/v7 -t betterpod-onion-smoketest:1 tools/onion/smoketest
docker run --rm --platform linux/arm/v7 -v "$PWD/build/bench:/out" -w /out -e LD_LIBRARY_PATH=/opt/sdl \
  betterpod-onion-smoketest:1 ./betterpod.test -test.run '^$' -test.bench "${1:-.}" -test.benchtime 20x
