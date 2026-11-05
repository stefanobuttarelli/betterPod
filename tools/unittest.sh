#!/bin/sh
# Run shared unit tests natively in Docker against Debian's SDL2.
# Tests stay outside src/ because the app is built from src/*.go.
#   tools/unittest.sh [go test flags, e.g. -run TestVersions]
set -eu
cd "$(dirname "$0")/.."
IMAGE=betterpod-build:1
docker image inspect "$IMAGE" > /dev/null 2>&1 || docker build -t "$IMAGE" -f tools/Dockerfile tools
exec docker run --rm -v "$PWD:/repo:ro" \
  -v betterpod-gomod:/go/pkg/mod -v betterpod-gocache:/root/.cache \
  "$IMAGE" sh -ec '
    cp -R /repo /w
    cp /repo/tools/unittest/*_test.go /w/src/
    cd /w/src
    CGO_CFLAGS="-I/usr/include/SDL2" go test -count=1 -v "$@" *.go' sh "$@"
