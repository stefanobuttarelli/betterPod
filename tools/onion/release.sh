#!/bin/sh
# Make the zip built by tools/onion/build.sh the current Onion release, for
# Settings → Software Update on the Miyoo (src/ij_update.go):
#
#   tools/onion/release.sh "What's new" ["more notes" ...]
#
# It copies dist/betterpod-v<version>-onion.zip to release/ and writes
# release/latest-onion.json (version, path, raw URL, SHA-256, size, notes),
# like release/latest-knulli.json for Knulli. Commit and push both; consoles
# read the manifest through GitHub's API, then the zip at its path. Older
# Onion zips in release/ can then be removed.
#
# A beta (BETTERPOD_VERSION like "1.1-beta.1") goes to release/beta-onion.json
# instead: only consoles with Beta Versions on read it.
set -e
cd "$(dirname "$0")/../.."
[ $# -gt 0 ] || { echo "usage: $0 \"what's new\" [...]" >&2; exit 1; }

VERSION=$(sed -nE 's/^[[:space:]]*BETTERPOD_VERSION[[:space:]]*=[[:space:]]*"(.*)"/\1/p' src/ij_update.go)
[ -n "$VERSION" ] || { echo "BETTERPOD_VERSION not found in src/ij_update.go" >&2; exit 1; }
NAME=betterpod-v$VERSION-onion.zip
[ -f "dist/$NAME" ] || { echo "dist/$NAME is missing: run tools/onion/build.sh" >&2; exit 1; }

mkdir -p release
cp "dist/$NAME" "release/$NAME"
MANIFEST=release/latest-onion.json
case "$VERSION" in *-*) MANIFEST=release/beta-onion.json ;; esac
python3 - "$VERSION" "release/$NAME" "$MANIFEST" "$@" << 'PY'
import hashlib, json, sys
version, path, out, notes = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4:]
data = open(path, "rb").read()
manifest = {
    "version": version,
    "path": path,
    "url": "https://raw.githubusercontent.com/stefanobuttarelli/betterPod/main/" + path,
    "sha256": hashlib.sha256(data).hexdigest(),
    "size": len(data),
    "notes": "\n".join("- " + n for n in notes),
}
with open(out, "w") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
print("%s: v%s, %d bytes, sha256 %s" % (out, version, len(data), manifest["sha256"][:16]))
PY
echo "Next: git add release/$NAME $MANIFEST, commit and push"
