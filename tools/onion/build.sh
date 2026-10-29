#!/bin/sh
# Build betterPod for Onion OS (Miyoo Mini, Mini Plus, Mini v4, Mini Flip)
# and pack it for the SD card.
#
#   tools/onion/build.sh   -> dist/sdcard/App/betterpod/        (copy App/ to the card)
#                             dist/betterpod-v<version>-onion.zip
#                             (unzip at the root of the card)
#
# Everything runs in Docker (tools/onion/Dockerfile): Go 1.24, zig cc for
# 32-bit ARM with glibc 2.28, the SDL2 headers. Go links with -rdynamic, so
# onion.c's MI_AO_SendFrame is exported and libSDL2.so uses it (see there).
# The Miyoo SDL2 libraries (steward-fu's SDL2 with the mmiyoo drivers, as
# shipped by MiyooPod) are in onion/betterpod/libs and the binary links
# against them directly. The interface assets are Knulli's, from
# port/betterpod/assets: one set for both.
set -e
cd "$(dirname "$0")/../.."
ROOT=$PWD
IMAGE=betterpod-onion-build:1

if [ -z "$IN_DOCKER" ]; then
  docker image inspect "$IMAGE" > /dev/null 2>&1 || docker build -t "$IMAGE" tools/onion
  exec docker run --rm -e IN_DOCKER=1 -v "$ROOT:/src" -w /src \
    -v betterpod-onion-gocache:/root/.cache -v betterpod-onion-gomod:/go/pkg/mod \
    "$IMAGE" sh tools/onion/build.sh "$@"
fi

VERSION=$(sed -nE 's/^[[:space:]]*BETTERPOD_VERSION[[:space:]]*=[[:space:]]*"(.*)"/\1/p' src/ij_update.go)
[ -n "$VERSION" ] || { echo "BETTERPOD_VERSION not found in src/ij_update.go" >&2; exit 1; }

mkdir -p build dist
echo "Building betterPod $VERSION for Onion (linux/arm, Cortex-A7, glibc 2.28)..."
# main.go and pz_*.go #include C files: Go's build cache doesn't see them
# change (the binary is built from a file list), so their hash goes in the
# flags
CSUM=$(cat src/*.c src/puzzles/*.c src/puzzles/*.h | sha256sum | cut -c1-16)
CGO_ENABLED=1 GOOS=linux GOARCH=arm GOARM=7 CC="$ROOT/tools/onion/zcc.sh" \
CGO_CFLAGS="-I/opt/sdl2/include/SDL2 -D_REENTRANT -O2 -DONION_CSUM=$CSUM" \
CGO_LDFLAGS="-L$ROOT/onion/betterpod/libs -Wl,--allow-shlib-undefined" \
  go build -tags netgo,osusergo -trimpath \
    -ldflags="-s -w" \
    -o build/betterpod-onion src/*.go

# the folder for the SD card: App/betterpod
APP=dist/sdcard/App/betterpod
rm -rf dist/sdcard
mkdir -p "$APP"
cp -R onion/betterpod/. "$APP/"
cp -R port/betterpod/assets "$APP/assets"
cp LICENSE "$APP/LICENSE"
cp port/betterpod/LICENSE.ipod.js port/betterpod/LICENSE.MiyooPod port/betterpod/LICENSE.puzzles port/betterpod/LICENSE.scowl "$APP/"
cp build/betterpod-onion "$APP/betterpod"
find "$APP" -name .DS_Store -delete
rm -rf "$APP/conf"
chmod 755 "$APP/betterpod" "$APP/launch.sh"

ZIP="dist/betterpod-v$VERSION-onion.zip"
rm -f "$ZIP"
(cd dist/sdcard && zip -qr -X "$ROOT/$ZIP" App)
echo "$APP"
echo "$ZIP ($(du -h "$ZIP" | cut -f1))"
