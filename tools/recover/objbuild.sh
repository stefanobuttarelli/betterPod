#!/bin/sh
# Compile src/ for linux/arm64 exactly like v0.20 and disassemble the Go object.
# usage: tools/recover/objbuild.sh OUTDIR
set -e
OUT=${1:-/tmp/kpobj}
mkdir -p "$OUT"
cd "$(dirname "$0")/../.."
EXPORTS=$PWD/docs/recovery/exports.txt; export EXPORTS; (cd tools/recover/genstubs && go run . "${DBG:-/home/claude/recover/dbg.elf}" ../../../src) > src/zz_stubs.go
# nothing missing any more: don't leave an empty stub file behind
grep -q '^\(const\|var\|type\|func\) ' src/zz_stubs.go || rm -f src/zz_stubs.go
A=$(GOSUMDB=off GOPROXY=off GOFLAGS=-mod=mod GOARCH=arm64 CGO_ENABLED=1 CC=/home/claude/tc/zcc \
  CGO_CFLAGS="-O2 -g -I/home/claude/sysroot/include/SDL2" CGO_LDFLAGS=-L/home/claude/sysroot/lib \
  go list -export -f '{{.Export}}' src/*.go)
rm -rf "$OUT/ar" && mkdir -p "$OUT/ar" && (cd "$OUT/ar" && ar x "$A")
go tool objdump "$OUT/ar/_go_.o" > "$OUT/obj.dis"
echo "$A"
