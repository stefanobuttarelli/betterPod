#!/bin/sh
# Build the Knulli executable and release zips without installing Go or zig.
#   tools/docker-build.sh -> build/betterpod, dist/betterpod-knulli*.zip
set -eu
cd "$(dirname "$0")/.."
IMAGE=betterpod-build:1
docker image inspect "$IMAGE" > /dev/null 2>&1 || docker build -t "$IMAGE" -f tools/Dockerfile tools
mkdir -p build/docker dist
docker run --rm -v "$PWD:/repo:ro" -v "$PWD/build/docker:/out" \
  -v betterpod-gocache:/root/.cache -v betterpod-gomod:/go/pkg/mod \
  "$IMAGE" sh -ec '
    cp -R /repo /w && cd /w && rm -rf build dist
    SDL2_SYSROOT=/opt/sdl2 CC=/w/tools/zcc.sh sh tools/build.sh
    cp build/betterpod /out/
    cp dist/betterpod-knulli*.zip /out/'
cp build/docker/betterpod build/betterpod
cp build/docker/betterpod-knulli*.zip dist/
