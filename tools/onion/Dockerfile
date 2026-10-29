# Build environment for betterPod on Onion OS (the Miyoo Mini: 32-bit ARM,
# hard float, glibc 2.28). Used by tools/onion/build.sh.
#
# - Go 1.24 (cgo);
# - zig cc as the C compiler and linker, targeting arm-linux-gnueabihf with
#   glibc 2.28, so the binary runs on the Miyoo firmware;
# - the SDL2 and SDL2_mixer headers (the libraries themselves are the
#   Miyoo builds in onion/betterpod/libs).
FROM golang:1.24-bookworm

ARG ZIG_VERSION=0.13.0
ARG SDL_VERSION=2.26.5
ARG MIXER_VERSION=2.6.3

RUN apt-get update \
 && apt-get install -y --no-install-recommends xz-utils zip \
 && rm -rf /var/lib/apt/lists/*

RUN arch=$(uname -m) \
 && curl -fsSL "https://ziglang.org/download/${ZIG_VERSION}/zig-linux-${arch}-${ZIG_VERSION}.tar.xz" | tar -xJ -C /opt \
 && ln -s "/opt/zig-linux-${arch}-${ZIG_VERSION}/zig" /usr/local/bin/zig

RUN mkdir -p /opt/sdl2/include/SDL2 && cd /tmp \
 && curl -fsSL "https://github.com/libsdl-org/SDL/releases/download/release-${SDL_VERSION}/SDL2-${SDL_VERSION}.tar.gz" | tar -xz \
 && cp SDL2-${SDL_VERSION}/include/*.h /opt/sdl2/include/SDL2/ \
 && curl -fsSL "https://github.com/libsdl-org/SDL_mixer/releases/download/release-${MIXER_VERSION}/SDL2_mixer-${MIXER_VERSION}.tar.gz" | tar -xz \
 && cp SDL2_mixer-${MIXER_VERSION}/include/SDL_mixer.h /opt/sdl2/include/SDL2/ \
 && rm -rf /tmp/SDL2*

ENV GOFLAGS=-buildvcs=false
