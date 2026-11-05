#!/bin/sh
# C compiler for Knulli: aarch64 with glibc 2.28.
# The Docker environment provides zig; native setups can use pip's ziglang.
if command -v zig > /dev/null 2>&1; then
  exec zig cc -target aarch64-linux-gnu.2.28 "$@"
fi
exec python3 -m ziglang cc -target aarch64-linux-gnu.2.28 "$@"
