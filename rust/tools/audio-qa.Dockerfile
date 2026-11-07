# Local-only decoder/controller tests: a private PulseAudio null sink permits
# the paused SDL_mixer music output and the independent preview/video output.
# The production/runtime image and physical audio devices are not changed.
FROM betterpod-rust-build:1
RUN apt-get update \
 && apt-get install -y --no-install-recommends pulseaudio \
 && rm -rf /var/lib/apt/lists/*
