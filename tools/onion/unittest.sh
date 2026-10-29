#!/bin/sh
# Unit tests of the Onion layer and Software Update (tools/onion/unittest/),
# run natively in Docker against Debian's SDL2. They stay out of src/ because
# the app is built from the file list src/*.go.
#
#   tools/onion/unittest.sh
set -e
cd "$(dirname "$0")/../.."
exec docker run --rm -v "$PWD:/repo:ro" -v betterpod-onion-gomod:/go/pkg/mod -v betterpod-onion-gocache:/root/.cache \
  golang:1.24-bookworm sh -c '
    apt-get update -qq > /dev/null && apt-get install -y -qq libsdl2-dev libsdl2-mixer-dev lame flac vorbis-tools > /dev/null 2>&1
    cp -R /repo /w && cp /repo/tools/onion/unittest/*_test.go /w/src/ && cd /w/src
    CGO_CFLAGS="-I/usr/include/SDL2" go test -count=1 -v *.go'
