#!/bin/sh
# C compiler for the aarch64 Knulli target: zig cc (pip install ziglang),
# glibc 2.28 like the H700 firmware.
exec python3 -m ziglang cc -target aarch64-linux-gnu.2.28 "$@"
