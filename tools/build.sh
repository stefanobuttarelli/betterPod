#!/bin/sh
# Build betterPod for Knulli (Anbernic H700, aarch64, glibc >= 2.28) and pack
# the PortMaster-style release zips.
#
#   tools/build.sh   -> dist/betterpod-knulli.zip         (betterPod.sh + betterpod/)
#                       dist/betterpod-knulli-bridge.zip  (for installations
#                         still called KnulliPod: KnulliPod.sh + knullipod/, the
#                         layout their updater installs; its KnulliPod.sh moves
#                         them to betterpod/ the next time they start)
#
# Needs: Go 1.24, a C cross compiler for aarch64-linux-gnu (CC, default: zig
# cc through tools/zcc.sh), the SDL2 headers and libSDL2-2.0.so for aarch64
# (SDL2_SYSROOT, with include/SDL2 and lib/). SDL2_mixer is linked against the
# copy shipped in port/betterpod/libs.
set -e
cd "$(dirname "$0")/.."
ROOT=$PWD
SYSROOT=${SDL2_SYSROOT:-/home/claude/sysroot}
CC=${CC:-$ROOT/tools/zcc.sh}

mkdir -p build/linklibs dist
ln -sf "$ROOT/port/betterpod/libs/libSDL2_mixer-2.0.so.0" build/linklibs/libSDL2_mixer.so

GOOS=linux GOARCH=arm64 CGO_ENABLED=1 CC="$CC" \
CGO_CFLAGS="-I$SYSROOT/include/SDL2 -D_REENTRANT -O2" \
CGO_LDFLAGS="-L$SYSROOT/lib -L$ROOT/build/linklibs" \
  go build -ldflags="-s -w" -o build/betterpod src/*.go

# the release
rm -rf build/pkg && mkdir -p build/pkg
cp -a port/betterPod.sh port/betterpod build/pkg/
cp build/betterpod build/pkg/betterpod/betterpod
chmod +x build/pkg/betterPod.sh build/pkg/betterpod/betterpod
rm -f dist/betterpod-knulli.zip
(cd build/pkg && zip -qr -X "$ROOT/dist/betterpod-knulli.zip" betterPod.sh betterpod)

# the bridge from KnulliPod
rm -rf build/bridge && mkdir -p build/bridge
cp -a port/betterpod build/bridge/knullipod
cp build/betterpod build/bridge/knullipod/MiyooPod
cp port/betterPod.sh build/bridge/knullipod/betterPod.sh
cp tools/bridge/KnulliPod.sh build/bridge/KnulliPod.sh
chmod +x build/bridge/KnulliPod.sh build/bridge/knullipod/MiyooPod build/bridge/knullipod/betterPod.sh
rm -f dist/betterpod-knulli-bridge.zip
(cd build/bridge && zip -qr -X "$ROOT/dist/betterpod-knulli-bridge.zip" KnulliPod.sh knullipod)
echo "dist/betterpod-knulli.zip dist/betterpod-knulli-bridge.zip"
