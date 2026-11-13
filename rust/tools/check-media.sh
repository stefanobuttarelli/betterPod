#!/bin/sh
# Real local codec fixtures; dummy audio is not a listening or Bluetooth test.
set -eu
cd "$(dirname "$0")/../.."
mkdir -p build/rust/media-check
docker run --rm -v "$PWD:/repo:ro" -v "$PWD/build/rust/media-check:/out" \
 -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
 -v betterpod-rust-target-integration:/target -e CARGO_BUILD_JOBS=2 \
 -e SDL_AUDIODRIVER=dummy -e LIBGL_ALWAYS_SOFTWARE=1 -e DISPLAY=:93 \
 -e BETTERPOD_RUST_ASSETS_DIR=/repo/port/betterpod/assets \
 -e BETTERPOD_RUST_SCREENSHOT=/out/renderer.png betterpod-rust-build:1 sh -ec '
  mkdir -p /w; cp -R /repo/rust /w/rust
  cargo build --locked --manifest-path /w/rust/Cargo.toml --target-dir /target --example audio-check --example video-check --example renderer-check
  ffmpeg -v error -nostdin -y -f lavfi -i sine=frequency=997:sample_rate=44100:duration=6 -ac 2 -c:a pcm_s16le /out/source.wav
  for pair in aac.m4a:aac alac.m4a:alac mp3.mp3:libmp3lame vorbis.ogg:libvorbis opus.opus:libopus aiff.aiff:pcm_s24be; do
    ffmpeg -v error -nostdin -y -i /out/source.wav -c:a "${pair#*:}" "/out/${pair%%:*}"
  done
  ffmpeg -v error -nostdin -y -f lavfi -i "aevalsrc=0.17*sin(2*PI*997*t)+0.000001*sin(2*PI*5711*t):s=192000:d=55" -ac 2 -c:a pcm_s24le /out/large24.wav
  ffmpeg -v error -nostdin -y -i /out/large24.wav -t 6 -ar 96000 -c:a flac /out/hires24.flac
  ffmpeg -v error -nostdin -y -i /out/large24.wav -t 6 -c:a alac /out/hires24.m4a
  ffmpeg -v error -nostdin -y -f lavfi -i testsrc2=size=320x240:rate=30:duration=6 -i /out/source.wav -c:v mpeg4 -q:v 5 -c:a aac -shortest /out/video.mp4
  ffmpeg -v error -nostdin -y -f lavfi -i testsrc2=size=320x240:rate=30:duration=2 -c:v mpeg4 -q:v 5 -an /out/silent.mp4
  if [ "$(uname -m)" = aarch64 ]; then export LD_LIBRARY_PATH=/repo/port/betterpod/libs; fi
  /target/debug/examples/audio-check /out/source.wav /out/aac.m4a /out/alac.m4a /out/mp3.mp3 /out/vorbis.ogg /out/opus.opus /out/aiff.aiff /out/hires24.flac /out/hires24.m4a /out/large24.wav
  /target/debug/examples/video-check /out/video.mp4 /out/silent.mp4
  Xvfb :93 -screen 0 640x480x24 +extension GLX > /out/xvfb.log 2>&1 &
  trap "kill $! 2>/dev/null || true" EXIT
  n=0; until xdpyinfo >/dev/null 2>&1; do n=$((n+1)); [ "$n" -lt 50 ] || exit 1; sleep .1; done
  /target/debug/examples/renderer-check
 '
echo 'PASS local codec/transport/renderer fixtures: build/rust/media-check'
