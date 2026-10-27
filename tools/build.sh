#!/bin/sh
# Build KnulliPod for Knulli (Anbernic H700, aarch64, glibc >= 2.28) and pack
# the PortMaster-style release zip.
#
#   tools/build.sh            -> dist/knullipod.zip
#
# Needs: Go 1.24, a C cross compiler for aarch64-linux-gnu (CC, default: zig
# cc through tools/zcc.sh), the SDL2 headers and libSDL2-2.0.so for aarch64
# (SDL2_SYSROOT, with include/SDL2 and lib/). SDL2_mixer is linked against the
# copy shipped in port/knullipod/libs.
set -e
cd "$(dirname "$0")/.."
ROOT=$PWD
SYSROOT=${SDL2_SYSROOT:-/home/claude/sysroot}
CC=${CC:-$ROOT/tools/zcc.sh}

mkdir -p build/linklibs dist
ln -sf "$ROOT/port/knullipod/libs/libSDL2_mixer-2.0.so.0" build/linklibs/libSDL2_mixer.so

GOOS=linux GOARCH=arm64 CGO_ENABLED=1 CC="$CC" \
CGO_CFLAGS="-I$SYSROOT/include/SDL2 -D_REENTRANT -O2" \
CGO_LDFLAGS="-L$SYSROOT/lib -L$ROOT/build/linklibs" \
  go build -ldflags="-s -w" -o build/MiyooPod src/*.go

rm -rf build/pkg && mkdir -p build/pkg
cp -a port/KnulliPod.sh port/knullipod build/pkg/
cp build/MiyooPod build/pkg/knullipod/MiyooPod
chmod +x build/pkg/KnulliPod.sh build/pkg/knullipod/MiyooPod
rm -f dist/knullipod.zip
(cd build/pkg && zip -qr -X "$ROOT/dist/knullipod.zip" KnulliPod.sh knullipod)
echo "dist/knullipod.zip"
