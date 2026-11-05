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
# Use tools/docker-build.sh for the bundled Docker environment.
# Needs: Go 1.24, a C cross compiler for aarch64-linux-gnu (CC, default: zig
# cc through tools/zcc.sh), the SDL2 headers and libSDL2-2.0.so for aarch64
# (SDL2_SYSROOT, with include/SDL2 and lib/). SDL2_mixer is linked against the
# copy shipped in port/betterpod/libs.
set -e
cd "$(dirname "$0")/.."
ROOT=$PWD
SYSROOT=${SDL2_SYSROOT:-/opt/sdl2}
CC=${CC:-$ROOT/tools/zcc.sh}

mkdir -p build/linklibs dist
ln -sf "$ROOT/port/betterpod/libs/libSDL2_mixer-2.0.so.0" build/linklibs/libSDL2_mixer.so

# main.go and pz_*.go #include C files: Go's build cache doesn't see them
# change (the binary is built from a file list), so their hash goes in the
# flags.
CSUM=$(
  for cfile in src/*.c src/*.h src/puzzles/*.c src/puzzles/*.h; do
    [ ! -f "$cfile" ] || cat "$cfile"
  done | sha256sum | cut -c1-16
)
GOOS=linux GOARCH=arm64 CGO_ENABLED=1 CC="$CC" \
CGO_CFLAGS="-I$SYSROOT/include/SDL2 -D_REENTRANT -O2 -DKNULLI_CSUM=$CSUM" \
CGO_LDFLAGS="-L$SYSROOT/lib -L$ROOT/build/linklibs" \
  go build -ldflags="-s -w" -o build/betterpod src/*.go

# Refuse a binary that the Knulli firmware cannot load. Docker includes
# readelf; native builds run these checks when binutils is installed.
if command -v readelf > /dev/null 2>&1; then
  B=build/betterpod
  readelf -h "$B" | grep -Eq 'Class:[[:space:]]+ELF64' &&
    readelf -h "$B" | grep -Eq 'Machine:[[:space:]]+AArch64' ||
    { echo "Knulli requires an AArch64 ELF64 executable" >&2; exit 1; }
  needed=$(readelf -d "$B" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')
  extra=$(printf '%s\n' "$needed" |
    grep -vxE 'libSDL2-2\.0\.so\.0|libSDL2_mixer-2\.0\.so\.0|libc\.so\.6|libm\.so\.6|libpthread\.so\.0|libdl\.so\.2|librt\.so\.1|libresolv\.so\.2' || true)
  [ -z "$extra" ] || { echo "Unexpected Knulli dependencies: $extra" >&2; exit 1; }
  for lib in libSDL2-2.0.so.0 libSDL2_mixer-2.0.so.0 libc.so.6; do
    printf '%s\n' "$needed" | grep -Fxq "$lib" ||
      { echo "Missing Knulli dependency: $lib" >&2; exit 1; }
  done
  glibc=$(readelf --dyn-syms -W "$B" | sed -n 's/.*@GLIBC_\([0-9.]*\).*/\1/p' | sort -uV | tail -n 1)
  if [ -n "$glibc" ] && [ "$(printf '%s\n2.28\n' "$glibc" | sort -V | tail -n 1)" != 2.28 ]; then
    echo "Knulli requires glibc <= 2.28; the binary needs $glibc" >&2
    exit 1
  fi
  echo "Verified AArch64, Knulli libraries and glibc ${glibc:-unknown}"
fi

# port/betterpod without what local test runs leave there (the library and
# settings in conf/, logs, a test music folder, an old binary)
pkgdir() {
  cp -a port/betterpod "$1"
  find "$1/conf" -mindepth 1 ! -name .gitkeep -exec rm -rf {} + 2> /dev/null || true
  find "$1" -name '*.log' -delete
  rm -rf "$1/music" "$1/log.txt" "$1/betterpod" "$1/betterpod.prev" "$1/MiyooPod"
  find "$1" -name .DS_Store -delete
}

# the release
rm -rf build/pkg && mkdir -p build/pkg
cp -a port/betterPod.sh build/pkg/
pkgdir build/pkg/betterpod
cp build/betterpod build/pkg/betterpod/betterpod
chmod +x build/pkg/betterPod.sh build/pkg/betterpod/betterpod
rm -f dist/betterpod-knulli.zip
(cd build/pkg && zip -qr -X "$ROOT/dist/betterpod-knulli.zip" betterPod.sh betterpod)

# Historical KnulliPod updaters require their original executable name.
# This bridge migrates those Knulli installations to betterPod.
rm -rf build/bridge && mkdir -p build/bridge
pkgdir build/bridge/knullipod
cp build/betterpod build/bridge/knullipod/MiyooPod
cp port/betterPod.sh build/bridge/knullipod/betterPod.sh
cp tools/bridge/KnulliPod.sh build/bridge/KnulliPod.sh
chmod +x build/bridge/KnulliPod.sh build/bridge/knullipod/MiyooPod build/bridge/knullipod/betterPod.sh
rm -f dist/betterpod-knulli-bridge.zip
(cd build/bridge && zip -qr -X "$ROOT/dist/betterpod-knulli-bridge.zip" KnulliPod.sh knullipod)
echo "dist/betterpod-knulli.zip dist/betterpod-knulli-bridge.zip"
