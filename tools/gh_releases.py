#!/usr/bin/env python3
"""Publish a GitHub release for every version in CHANGELOG.md that has none.

Run by .github/workflows/releases.yml. A version's release is tagged at the
commit whose subject starts with "vX.Y:" (the last such commit), gets its
CHANGELOG section as notes, and the zips from release/ (Knulli's and
Onion's) when they are still there: the title says which systems it is for
("Knulli + Miyoo", "Knulli only", "Miyoo only") and an install table ends the
notes. The newest version with Knulli's zip is marked as the latest release.
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

def systems(knulli, onion):
    """The release title's systems, from the zips it carries."""
    if knulli and onion:
        return " (Knulli + Miyoo)"
    if knulli:
        return " (Knulli only)"
    if onion:
        return " (Miyoo only)"
    return ""


def release_notes(v, body, knulli, onion):
    notes = body + "\n\n---\n\n"
    if knulli or onion:
        notes += "### Install\n\n| Console | File | How |\n|---|---|---|\n"
        if knulli:
            notes += f"| Anbernic (Knulli) | `betterpod-v{v}-knulli.zip` | unzip into `/userdata/roms/ports/`, or Settings → Software Update |\n"
        if onion:
            notes += f"| Miyoo Mini family (Onion OS) | `betterpod-v{v}-onion.zip` | unzip at the root of the SD card, or Settings → Software Update |\n"
        if not (knulli and onion):
            other = "Miyoo" if knulli else "Anbernic"
            notes += f"\nNothing new for the {other} in this version: it stays on its latest release.\n"
    else:
        notes += "**Install:** update from Settings → Software Update, or get the newest release's zip.\n"
    notes += "\n**On a Mac?** [betterPod Share for Mac](https://betterpod.xyz/mac/) sends songs and albums to a betterPod with an 8-digit code.\n"
    return notes


for v, body in reversed(versions):  # oldest first, so the newest ends up on top
    if key(v) < key(only):
        continue
    zipf = f"release/betterpod-v{v}-knulli.zip"
    onionf = f"release/betterpod-v{v}-onion.zip"
    if "v" + v in have:
        # releases from 0.73 (both systems) get their title and install table
        # once, if they were made before these existed
        if key(v) >= key("0.73"):
            cur = json.loads(gh("api", f"repos/{repo}/releases/tags/v{v}").stdout or "{}")
            names = [a["name"] for a in cur.get("assets", [])]
            k, o = f"betterpod-v{v}-knulli.zip" in names, f"betterpod-v{v}-onion.zip" in names
            if cur and "(" not in cur.get("name", ""):
                open("/tmp/notes.md", "w").write(release_notes(v, body, k, o))
                r = gh("release", "edit", f"v{v}", "--title", f"betterPod {v}{systems(k, o)}", "--notes-file", "/tmp/notes.md")
                print(f"v{v} updated:", r.stdout.strip(), r.stderr.strip())
        continue
    commits = [l.split()[0] for l in log if l.split(" ", 1)[1].startswith(f"v{v}:")]
    if not commits:
        print(f"v{v}: no commit, skipped")
        continue
    k, o = os.path.exists(zipf), os.path.exists(onionf)
    open("/tmp/notes.md", "w").write(release_notes(v, body, k, o))
    # "Latest" stays on the newest release that carries Knulli's zip when
    # this one is for the Miyoo only (Anbernic users follow that link too)
    args = ["release", "create", f"v{v}", "--target", commits[0], "--title", f"betterPod {v}{systems(k, o)}",
            "--notes-file", "/tmp/notes.md", f"--latest={'true' if v == newest and k else 'false'}"]
    args += [f for f in (zipf, onionf) if os.path.exists(f)]
    r = gh(*args)
    print(f"v{v}:", r.stdout.strip(), r.stderr.strip())
    if r.returncode:
        sys.exit(r.returncode)
