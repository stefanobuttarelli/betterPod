#!/bin/bash
# Compiler for unchanged upstream puzzle engines, targeting Knulli's libc.
set -eu
args=()
for arg in "$@"; do
  # cc-rs detects Clang and adds Rust's vendor-qualified target spelling.
  # The equivalent Zig target is already selected explicitly below.
  case "$arg" in --target=aarch64-unknown-linux-gnu) ;; *) args+=("$arg") ;; esac
done
exec zig cc -target aarch64-linux-gnu.2.28 "${args[@]}"
