#!/bin/sh
# Build a local Mac app without installing SDL or changing global settings.
set -eu
cd "$(dirname "$0")/../.."
[ "$(uname -s)" = Darwin ] || { echo 'This command needs macOS' >&2; exit 1; }
ROOT=$PWD
DEPS="$ROOT/build/rust/macdeps"
FRAMEWORKS="$DEPS/frameworks"
mkdir -p "$FRAMEWORKS"
fetch() {
  name=$1; url=$2; expected=$3
  if [ ! -f "$DEPS/$name" ]; then curl -fL --retry 2 -o "$DEPS/$name.partial" "$url"; mv "$DEPS/$name.partial" "$DEPS/$name"; fi
  actual=$(shasum -a 256 "$DEPS/$name" | cut -d ' ' -f1)
  [ "$actual" = "$expected" ] || { echo "Checksum mismatch: $name" >&2; exit 1; }
}
fetch SDL2-2.32.10.dmg https://github.com/libsdl-org/SDL/releases/download/release-2.32.10/SDL2-2.32.10.dmg 4a7ac31640d70214e848f994be8a12849c0f97918a7e6c2e27a40036166d1a7f
fetch SDL2_mixer-2.8.1.dmg https://github.com/libsdl-org/SDL_mixer/releases/download/release-2.8.1/SDL2_mixer-2.8.1.dmg d74052391ee4d91836bf1072a060f1d821710f3498a54996c66b9a17c79a72d1
for pair in SDL2:SDL2-2.32.10.dmg SDL2_mixer:SDL2_mixer-2.8.1.dmg; do
  framework=${pair%%:*}; image=${pair#*:}
  if [ ! -d "$FRAMEWORKS/$framework.framework" ]; then
    mount="$DEPS/mount-$framework"
    mkdir -p "$mount"
    hdiutil attach -quiet -readonly -nobrowse -mountpoint "$mount" "$DEPS/$image"
    trap 'hdiutil detach -quiet "$mount" 2>/dev/null || true' EXIT HUP INT TERM
    cp -R "$mount/$framework.framework" "$FRAMEWORKS/"
    hdiutil detach -quiet "$mount"
    trap - EXIT HUP INT TERM
  fi
done
export RUSTFLAGS="${RUSTFLAGS:-} -L framework=$FRAMEWORKS -C link-arg=-Wl,-rpath,@executable_path/../Frameworks -C link-arg=-Wl,-rpath,$FRAMEWORKS"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
cargo build --locked --release --manifest-path rust/Cargo.toml --target-dir build/rust/target-mac
APP="$ROOT/dist/betterPod Rust.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp build/rust/target-mac/release/betterpod-rust "$APP/Contents/MacOS/betterpod-rust"
cp -R "$FRAMEWORKS/SDL2.framework" "$FRAMEWORKS/SDL2_mixer.framework" "$APP/Contents/Frameworks/"
cp -R port/betterpod/assets "$APP/Contents/Resources/"
cp LICENSE port/betterpod/THIRD_PARTY_LICENSES.md "$APP/Contents/Resources/"
cp rust/README.md "$APP/Contents/Resources/README.md"
cp rust/VALIDATION.md "$APP/Contents/Resources/VALIDATION.md"
cp rust/PARITY.md "$APP/Contents/Resources/PARITY.md"
cp rust/THIRD_PARTY_NOTICES.txt "$APP/Contents/Resources/"
cat > "$APP/Contents/MacOS/launch" <<'SH'
#!/bin/sh
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
RESOURCES="$HERE/../Resources"
export BETTERPOD_RUST_ASSETS_DIR=${BETTERPOD_RUST_ASSETS_DIR:-$RESOURCES/assets}
export BETTERPOD_RUST_DATA_DIR=${BETTERPOD_RUST_DATA_DIR:-$HOME/Library/Application Support/betterPod Rust}
export BETTERPOD_RUST_MUSIC_DIR=${BETTERPOD_RUST_MUSIC_DIR:-$HOME/Music}
mkdir -p "$BETTERPOD_RUST_DATA_DIR"
exec "$HERE/betterpod-rust" >> "$BETTERPOD_RUST_DATA_DIR/betterpod-rust.log" 2>&1
SH
chmod +x "$APP/Contents/MacOS/launch" "$APP/Contents/MacOS/betterpod-rust"
cat > "$APP/Contents/Info.plist" <<'XML'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>betterPod Rust</string>
<key>CFBundleIdentifier</key><string>xyz.betterpod.rust.local</string>
<key>CFBundleExecutable</key><string>launch</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
XML
codesign --force --deep --sign - "$APP"
echo "$APP"
