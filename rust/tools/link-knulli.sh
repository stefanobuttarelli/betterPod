#!/bin/sh
# rustc's linker. The final binary uses Knulli's SDL2 and the patched mixer
# in its own app directory. Rust code does not link any Go app implementation.
set -eu
exec aarch64-linux-gnu-gcc --sysroot=/opt/knulli-glibc228 \
  -L/opt/knulli-glibc228/lib/aarch64-linux-gnu \
  -L/opt/knulli-glibc228/usr/lib/aarch64-linux-gnu \
  -L/opt/sdl2/lib -L/out/linklibs \
  -Wl,-rpath,'$ORIGIN/libs' -Wl,--allow-shlib-undefined "$@"
