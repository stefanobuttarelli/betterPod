#!/usr/bin/env python3
"""Publish the newest CHANGELOG version and its verified Knulli package.

Run by .github/workflows/releases.yml. Historical releases stay unchanged.
The tag targets the last commit whose subject starts with "vX.Y:".
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def run(*args):
    return subprocess.run(args, capture_output=True, text=True, check=True).stdout


def release_section(changelog):
    sections = re.split(r"^## v", changelog, flags=re.M)[1:]
    if not sections:
        raise ValueError("CHANGELOG has no version section")
    head, body = sections[0].split("\n", 1)
    version = head.split()[0]
    if not re.fullmatch(r"\d+(?:\.\d+)+(?:-beta\.\d+)?", version):
        raise ValueError(f"Invalid version: {version}")
    return version, body.strip()


def verify_package(root, version):
    source = (root / "src/ij_update.go").read_text()
    match = re.search(r'BETTERPOD_VERSION\s*=\s*"([^"]+)"', source)
    if match is None or match[1] != version:
        raise ValueError("Source version does not match the CHANGELOG version")
    channel = "beta" if "-" in version else "latest"
    manifest = json.loads((root / f"release/{channel}-knulli.json").read_text())
    package = f"release/betterpod-v{version}-knulli.zip"
    if manifest["version"] != version or manifest["path"] != package:
        raise ValueError("Release manifest does not match the CHANGELOG version")
    path = root / package
    if path.stat().st_size != manifest["size"]:
        raise ValueError("Release package size does not match the manifest")
    if hashlib.sha256(path.read_bytes()).hexdigest() != manifest["sha256"]:
        raise ValueError("Release package SHA-256 does not match the manifest")
    return path


def release_notes(body, version):
    return (body + "\n\n---\n\n### Install\n\n"
            f"Download `betterpod-v{version}-knulli.zip` and unzip it into "
            "`/userdata/roms/ports/`, or use Settings → Software Update. "
            "Your library and settings are kept.\n\n"
            "**On a Mac?** [betterPod Share for Mac](https://betterpod.xyz/mac/) "
            "sends songs and albums to betterPod with an 8-digit code.\n")


def main():
    root = Path(__file__).resolve().parent.parent
    os.chdir(root)
    repo = os.environ["GITHUB_REPOSITORY"]
    version, body = release_section((root / "CHANGELOG.md").read_text())
    tag = "v" + version
    existing = json.loads(run("gh", "api", f"repos/{repo}/releases", "--paginate", "--slurp"))
    if any(r["tag_name"] == tag for page in existing for r in page):
        print(f"{tag}: already published")
        return
    package = verify_package(root, version)
    log = run("git", "log", "--format=%H %s").splitlines()
    target = next((line.split(" ", 1)[0] for line in log
                   if line.split(" ", 1)[1].startswith(f"v{version}:")), None)
    if target is None:
        raise ValueError(f"No release commit starts with v{version}:")
    # A tag left by an interrupted run must not point at different source.
    existing_tag = subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", f"refs/tags/{tag}^{{commit}}"],
        capture_output=True, text=True)
    if existing_tag.returncode == 0 and existing_tag.stdout.strip() != target:
        raise ValueError(f"Existing tag {tag} points at a different commit")
    # Tag the source that actually carries this exact package and manifest.
    diff = run("git", "diff", target, "HEAD", "--", "src", "port", "go.mod", "go.sum", "release")
    if diff:
        raise ValueError("Source or package changed after the release commit")
    with tempfile.TemporaryDirectory(prefix="betterpod-release-") as tmp:
        notes = Path(tmp) / "notes.md"
        notes.write_text(release_notes(body, version))
        args = ["gh", "release", "create", tag, "--repo", repo, "--target", target,
                "--title", f"betterPod {version}", "--notes-file", str(notes),
                "--latest=false" if "-" in version else "--latest=true"]
        if "-" in version:
            args.append("--prerelease")
        args.append(str(package))
        print(run(*args).strip())


if __name__ == "__main__":
    main()
