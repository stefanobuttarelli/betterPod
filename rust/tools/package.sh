#!/bin/sh
# Prepare a second Ports app, never a stable release/update package.
set -eu
cd "$(dirname "$0")/../.."
[ -x build/rust/betterpod-rust.knulli ] || { echo "Build first: rust/tools/build.sh knulli" >&2; exit 1; }
PKG=build/rust/package
rm -rf "$PKG"
mkdir -p "$PKG/betterpod-rust/conf" dist
cp 'rust/tools/betterPod Rust.sh' "$PKG/betterPod Rust.sh"
cp build/rust/betterpod-rust.knulli "$PKG/betterpod-rust/betterpod-rust"
cp -R port/betterpod/assets port/betterpod/libs "$PKG/betterpod-rust/"
for file in port/betterpod/LICENSE* port/betterpod/THIRD_PARTY_LICENSES.md; do
  [ ! -f "$file" ] || cp "$file" "$PKG/betterpod-rust/"
done
cp rust/README.md "$PKG/betterpod-rust/README.md"
cp rust/VALIDATION.md "$PKG/betterpod-rust/VALIDATION.md"
cp rust/PARITY.md "$PKG/betterpod-rust/PARITY.md"
cp rust/CONSOLE_QA.md "$PKG/betterpod-rust/CONSOLE_QA.md"
cp rust/THIRD_PARTY_NOTICES.txt "$PKG/betterpod-rust/"
printf '{"application":"betterpod-rust"}\n' > "$PKG/betterpod-rust/.rust-app.json"
cat > "$PKG/betterpod-rust/port.json" <<'JSON'
{
  "version": 4,
  "name": "betterpod-rust-experimental.zip",
  "items": ["betterPod Rust.sh", "betterpod-rust"],
  "items_opt": ["betterpod-rust/conf"],
  "attr": {
    "title": "betterPod Rust (local experiment)",
    "porter": ["betterPod"],
    "desc": "Independent Rust rewrite experiment. Separate data and launcher; audio first, measured UI cadence.",
    "inst": "Extract into /userdata/roms/ports. Keep the existing betterPod app. This experiment does not provide automatic updates.",
    "genres": ["other"], "image": {}, "rtr": true, "exp": true,
    "runtime": null, "reqs": [], "arch": ["aarch64"]
  }
}
JSON
chmod +x "$PKG/betterPod Rust.sh" "$PKG/betterpod-rust/betterpod-rust"
OUT="$PWD/dist/betterpod-rust-experimental.zip"
rm -f "$OUT"
(cd "$PKG" && zip -qr -X "$OUT" 'betterPod Rust.sh' betterpod-rust)
python3 - <<'PY'
import pathlib, zipfile, hashlib
p=pathlib.Path('dist/betterpod-rust-experimental.zip')
with zipfile.ZipFile(p) as z:
 assert z.testzip() is None
 assert all(x=='betterPod Rust.sh' or x.startswith('betterpod-rust/') for x in z.namelist())
 assert z.read('betterpod-rust/betterpod-rust')==pathlib.Path('build/rust/betterpod-rust.knulli').read_bytes()
print(p, p.stat().st_size, hashlib.sha256(p.read_bytes()).hexdigest())
PY
