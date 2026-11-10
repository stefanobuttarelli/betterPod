#!/bin/sh
# Opt-in actual SDL/GLES controller routes. Separate scratch data and target cache.
set -eu
cd "$(dirname "$0")/../.."
MUSIC=${1:-build/desktop/music}
[ -d "$MUSIC" ] || { echo "Supply the existing local audio fixture as argument" >&2; exit 2; }
MUSIC=$(CDPATH= cd -- "$MUSIC" && pwd)
mkdir -p build/rust/app-captures
ROOT=$PWD
AUDIO_QA_IMAGE=betterpod-rust-audio-qa:1
if ! docker image inspect "$AUDIO_QA_IMAGE" >/dev/null 2>&1; then
 docker build -f rust/tools/audio-qa.Dockerfile -t "$AUDIO_QA_IMAGE" .
fi
docker run --rm --network none -v "$ROOT:/repo:ro" -v "$MUSIC:/music:ro" -v "$ROOT/build/rust:/out" \
 -v betterpod-rust-registry:/opt/cargo/registry -v betterpod-rust-git:/opt/cargo/git \
 -v betterpod-rust-target-appchecks:/target-appchecks \
 -e CARGO_BUILD_JOBS=2 -e SDL_AUDIODRIVER=dummy -e DISPLAY=:94 -e LIBGL_ALWAYS_SOFTWARE=1 \
 -e BETTERPOD_RUST_QA_MUSIC=/music -e BETTERPOD_RUST_ASSETS_DIR=/repo/port/betterpod/assets \
 -e BETTERPOD_RUST_QA_OUTPUT=/out/app-captures \
 "$AUDIO_QA_IMAGE" sh -ec '
  mkdir -p /w
  tar -C /repo --exclude=rust/target -cf - rust | tar -C /w -xf -
  if [ "$(uname -m)" = aarch64 ]; then export LD_LIBRARY_PATH=/repo/port/betterpod/libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}; fi
  Xvfb :94 -screen 0 640x480x24 +extension GLX > /out/app-captures/xvfb.log 2>&1 &
  trap "kill $! 2>/dev/null || true" EXIT
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-appchecks --bin betterpod-rust -- --nocapture --test-threads=1
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-appchecks --bin betterpod-rust app::qa::controller_routes_and_real_screens -- --ignored --exact --nocapture --test-threads=1
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-appchecks --bin betterpod-rust app::qa::custom_menu_entries_and_back_restore_visible_panels -- --ignored --exact --nocapture --test-threads=1
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-appchecks --bin betterpod-rust app::qa::playlist_full_page_draws_its_reserved_mini_player -- --ignored --exact --nocapture --test-threads=1
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-appchecks --bin betterpod-rust app::visual_player::regression_qa::playback_saver_cache_and_navigation_regressions -- --ignored --exact --nocapture --test-threads=1
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-appchecks --bin betterpod-rust app::album_read::tests::initial_album_info_input_retains_source_and_cancels_before_late_results -- --ignored --exact --nocapture --test-threads=1
  # SDL dummy supports one output. A private null sink allows the retained,
  # paused music mixer plus the independent Listen/video output, without devices.
  export SDL_AUDIODRIVER=pulseaudio PULSE_SERVER=unix:/tmp/bp-qa-pulse PULSE_SINK=qa
  pulseaudio -n --daemonize=yes --exit-idle-time=-1 --log-target=file:/out/app-captures/pulseaudio.log \
   -L "module-native-protocol-unix socket=/tmp/bp-qa-pulse auth-anonymous=1" \
   -L "module-null-sink sink_name=qa rate=48000 channels=2"
  pactl set-default-sink qa
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-appchecks --bin betterpod-rust app::qa::inline_preview_remote_pause_unplug_and_restore_use_real_decoder -- --ignored --exact --nocapture --test-threads=1
  cargo test --locked --manifest-path /w/rust/Cargo.toml --target-dir /target-appchecks --bin betterpod-rust app::visual_video::qa::fullscreen_video_source_controls_and_opening_cancel_use_real_decoder -- --ignored --exact --nocapture --test-threads=1
 '
