# Knulli cross compiler and native unit-test environment.
FROM golang:1.24-bookworm

ARG ZIG_VERSION=0.13.0
RUN dpkg --add-architecture arm64 \
 && apt-get update \
 && apt-get install -y --no-install-recommends xz-utils zip binutils libsdl2-dev libsdl2-mixer-dev lame flac vorbis-tools ffmpeg \
 && mkdir -p /tmp/arm64 /opt/sdl2/include/SDL2 /opt/sdl2/lib \
 && cd /tmp/arm64 \
 && apt-get download libsdl2-dev:arm64 libsdl2-2.0-0:arm64 \
 && for p in *.deb; do dpkg-deb -x "$p" root; done \
 && cp root/usr/include/SDL2/*.h /opt/sdl2/include/SDL2/ \
 && cp root/usr/include/aarch64-linux-gnu/SDL2/_real_SDL_config.h /opt/sdl2/include/SDL2/SDL_config.h \
 && cp /usr/include/SDL2/SDL_mixer.h /opt/sdl2/include/SDL2/ \
 && cp -L root/usr/lib/aarch64-linux-gnu/libSDL2-2.0.so.0 /opt/sdl2/lib/ \
 && ln -s libSDL2-2.0.so.0 /opt/sdl2/lib/libSDL2.so \
 && rm -rf /tmp/arm64 /var/lib/apt/lists/*

RUN arch=$(uname -m) \
 && curl -fsSL "https://ziglang.org/download/${ZIG_VERSION}/zig-linux-${arch}-${ZIG_VERSION}.tar.xz" | tar -xJ -C /opt \
 && ln -s "/opt/zig-linux-${arch}-${ZIG_VERSION}/zig" /usr/local/bin/zig

ENV GOFLAGS=-buildvcs=false
