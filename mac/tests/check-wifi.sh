#!/bin/sh
# Actual Rust server <-> Mac Go client checks on disposable local audio only.
# The pairing code travels through an anonymous subprocess pipe, not arguments.
set -eu
cd "$(dirname "$0")/../.."

[ "$(uname -s)" = Darwin ] || { echo 'The interop audio check requires macOS.' >&2; exit 1; }
command -v cargo >/dev/null
command -v go >/dev/null
command -v afconvert >/dev/null
frameworks="$PWD/build/rust/macdeps/frameworks"
if [ ! -d "$frameworks/SDL2.framework" ] || [ ! -d "$frameworks/SDL2_mixer.framework" ]; then
    echo 'Prepare the local Rust Mac frameworks with rust/tools/build-mac.sh first.' >&2
    exit 1
fi

export RUSTFLAGS="${RUSTFLAGS:-} -L framework=$frameworks -C link-arg=-Wl,-rpath,@executable_path/../Frameworks -C link-arg=-Wl,-rpath,$frameworks"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
cargo build --offline --locked --manifest-path rust/Cargo.toml \
    --target-dir build/rust/target-mac --example wifi_library_fixture

BETTERPOD_LAN_FIXTURE="$PWD/build/rust/target-mac/debug/examples/wifi_library_fixture" \
    go test -race -count=1 -v ./mac/core -run '^TestLan'
