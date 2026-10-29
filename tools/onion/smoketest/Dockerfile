# Smoke test for betterPod's Onion ARM binary (tools/onion/smoketest.sh): the
# real linux/arm build, emulated, with Debian's SDL2 in place of the Miyoo
# one, drawing into Xvfb and driven with xdotool.
FROM --platform=linux/arm/v7 debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends libsdl2-2.0-0 libsdl2-mixer-2.0-0 xvfb xauth xdotool \
 && rm -rf /var/lib/apt/lists/*

# the binary asks for the Miyoo SONAMEs (libSDL2.so, libSDL2_mixer.so)
RUN mkdir -p /opt/sdl \
 && ln -s /usr/lib/arm-linux-gnueabihf/libSDL2-2.0.so.0 /opt/sdl/libSDL2.so \
 && ln -s /usr/lib/arm-linux-gnueabihf/libSDL2_mixer-2.0.so.0 /opt/sdl/libSDL2_mixer.so
