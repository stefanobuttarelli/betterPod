#!/bin/sh
# Compile the Go package in DIR for linux/arm64 (cgo via zig) and print the
# path of the resulting archive in the Go build cache (used to compare the
# reconstructed sources against the v0.20 object code).
set -e
DIR=${1:-/home/claude/knullipod/src}
cd "$DIR"
export GOSUMDB=off GOPROXY=off GOFLAGS=-mod=mod
export CGO_ENABLED=1 GOARCH=arm64 CC=/home/claude/tc/zcc
export CGO_CFLAGS="-I/home/claude/sdlinc/SDL2 -I/home/claude/sdlinc -I$DIR"
go list -export -f '{{.Export}}' *.go
