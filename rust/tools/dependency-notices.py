#!/usr/bin/env python3
"""Collect pinned Rust dependency license texts from Cargo metadata."""
import json
import hashlib
import pathlib
import sys

root = pathlib.Path(__file__).resolve().parents[2]
metadata = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else root / "build/rust/dependency-metadata.json"
packages = json.loads(metadata.read_text())["packages"]
output = root / "rust/THIRD_PARTY_NOTICES.txt"
documents = {}
with output.open("w") as out:
    out.write("betterPod Rust dependency notices\n\n")
    out.write("Generated from Cargo.lock and cargo metadata for aarch64-unknown-linux-gnu.\n")
    out.write("Each dependency retains its own license. Native SDL/codec, puzzle and asset\nlicenses are included separately in the app package.\n\n")
    for package in sorted(packages, key=lambda p: (p["name"], p["version"])):
        if package["name"] == "betterpod-rust":
            continue
        base = pathlib.Path(package["manifest_path"]).parent
        candidates = list(base.iterdir())
        files = []
        for directory in [p for p in base.iterdir() if p.is_dir() and p.name.lower() in ("licenses", "license")]:
            if directory.is_dir():
                files.extend(p for p in directory.iterdir() if p.is_file())
        files.extend(p for p in candidates if p.is_file() and p.name.lower().startswith(("license", "licence", "unlicense", "copying", "copyright", "notice")))
        family = "symphonia" if package["name"].startswith("symphonia") else "lofty" if package["name"] in ("lofty", "lofty_attr", "ogg_pager") else "noise" if package["name"].startswith("noise-") else None
        if family and not files:
            files.extend(p for p in (root/"rust/licenses"/family).glob("*") if p.is_file() and p.name != "SOURCE.txt")
        if package["name"] == "sdl2-sys":
            sdl = next(p for p in packages if p["name"] == "sdl2")
            files.extend(pathlib.Path(sdl["manifest_path"]).parent.glob("LICENSE*"))
            files.append(base / "SDL/LICENSE.txt")
        if package.get("license_file"):
            files.append(base / package["license_file"])
        files = sorted(set(p.resolve() for p in files))
        out.write("=" * 72 + "\n")
        out.write(f"{package['name']} {package['version']}\nLicense: {package.get('license') or 'see included license'}\n")
        out.write(f"Source: https://crates.io/crates/{package['name']}/{package['version']}\n\n")
        for path in files:
            text = path.read_text(errors="replace")
            digest = hashlib.sha256(text.encode()).hexdigest()[:16]
            documents.setdefault(digest, (path.name, text))
            out.write(f"Included text {digest}: {path.name}\n")
        if not files:
            raise SystemExit(f"Missing license text: {package['name']} {package['version']}")
    out.write("\nFull license and notice texts (identical copies included once)\n\n")
    for digest, (name, text) in sorted(documents.items()):
        out.write("=" * 72 + f"\nText {digest}: {name}\n\n{text}\n\n")
print(output, output.stat().st_size, "bytes")
