#!/usr/bin/env python3
"""Publish a GitHub release for every version in CHANGELOG.md that has none.

Run by .github/workflows/releases.yml. A version's release is tagged at the
commit whose subject starts with "vX.Y:" (the last such commit), gets its
CHANGELOG section as notes, and the zips from release/ (Knulli's and
Onion's) when they are still there. The newest version is marked as the latest release.
"""
import json, os, re, subprocess, sys

def sh(*a):
    return subprocess.run(a, capture_output=True, text=True, check=True).stdout

def gh(*a):
    return subprocess.run(["gh", *a], capture_output=True, text=True)

repo = os.environ["GITHUB_REPOSITORY"]
log = sh("git", "log", "--format=%H %s").splitlines()
have = set(json.loads(gh("api", f"repos/{repo}/releases", "--paginate", "--jq", "[.[].tag_name]").stdout.replace("][", ",") or "[]"))
parts = re.split(r"^## v", open("CHANGELOG.md").read(), flags=re.M)[1:]
versions = []
for p in parts:
    head, body = p.split("\n", 1)
    versions.append((head.split()[0], body.strip()))
newest = versions[0][0]
only = os.environ.get("ONLY_SINCE", "0.65")

def key(v):
    return [int(x) for x in v.split(".")]

for v, body in reversed(versions):  # oldest first, so the newest ends up on top
    if key(v) < key(only) or "v" + v in have:
        continue
    commits = [l.split()[0] for l in log if l.split(" ", 1)[1].startswith(f"v{v}:")]
    if not commits:
        print(f"v{v}: no commit, skipped")
        continue
    zipf = f"release/betterpod-v{v}-knulli.zip"
    onionf = f"release/betterpod-v{v}-onion.zip"
    notes = body + "\n\n---\n"
    if os.path.exists(zipf):
        notes += f"**Install on Knulli:** unzip `betterpod-v{v}-knulli.zip` into `/userdata/roms/ports/` on the SD card, or update from Settings → Software Update.\n"
    if os.path.exists(onionf):
        notes += f"**Install on Onion (Miyoo):** unzip `betterpod-v{v}-onion.zip` at the root of the SD card, or update from Settings → Software Update.\n"
    if not os.path.exists(zipf) and not os.path.exists(onionf):
        notes += "**Install:** update from Settings → Software Update, or get the newest release's zip.\n"
    open("/tmp/notes.md", "w").write(notes)
    args = ["release", "create", f"v{v}", "--target", commits[0], "--title", f"betterPod {v}",
            "--notes-file", "/tmp/notes.md", f"--latest={'true' if v == newest else 'false'}"]
    args += [f for f in (zipf, onionf) if os.path.exists(f)]
    r = gh(*args)
    print(f"v{v}:", r.stdout.strip(), r.stderr.strip())
    if r.returncode:
        sys.exit(r.returncode)
