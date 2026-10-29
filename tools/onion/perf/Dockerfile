# Native profiling of the interface (tools/onion/perf.sh): the same code in
# Onion mode, built for the host CPU, drawing into Xvfb with Debian's SDL2.
FROM golang:1.24-bookworm
RUN apt-get update \
 && apt-get install -y --no-install-recommends libsdl2-dev libsdl2-mixer-dev xvfb xauth xdotool \
 && rm -rf /var/lib/apt/lists/*
ENV GOFLAGS=-buildvcs=false
