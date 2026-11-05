#!/usr/bin/env python3
"""Offline checks for release selection and artifact validation."""
import hashlib
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest import mock

import gh_releases

from gh_releases import release_section, verify_package, release_notes


class ReleaseTests(unittest.TestCase):
    def test_newest_section_only(self):
        self.assertEqual(release_section("# Changelog\n\n## v1.1 (2026-11-05)\n\nStable\n\n## v1.1-beta.8\n\nOld"), ("1.1", "Stable"))
        with self.assertRaises(ValueError):
            release_section("# Changelog\n")

    def test_manifest_matches_package(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src/ij_update.go").write_text('const BETTERPOD_VERSION = "1.1"')
            release = root / "release"
            release.mkdir()
            package = release / "betterpod-v1.1-knulli.zip"
            package.write_bytes(b"release content")
            manifest = {"version": "1.1", "path": "release/" + package.name,
                        "size": package.stat().st_size,
                        "sha256": hashlib.sha256(package.read_bytes()).hexdigest()}
            pointer = release / "latest-knulli.json"
            pointer.write_text(json.dumps(manifest))
            self.assertEqual(verify_package(root, "1.1"), package)
            (root / "src/ij_update.go").write_text('const BETTERPOD_VERSION = "1.1-beta.8"')
            with self.assertRaises(ValueError):
                verify_package(root, "1.1")
            (root / "src/ij_update.go").write_text('const BETTERPOD_VERSION = "1.1"')
            package.write_bytes(b"changed content")
            with self.assertRaises(ValueError):
                verify_package(root, "1.1")
            manifest["version"] = "1.0.1"
            pointer.write_text(json.dumps(manifest))
            with self.assertRaises(ValueError):
                verify_package(root, "1.1")

    def test_beta_channel_and_notes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "src").mkdir()
            (root / "src/ij_update.go").write_text('const BETTERPOD_VERSION = "1.2-beta.1"')
            (root / "release").mkdir()
            package = root / "release/betterpod-v1.2-beta.1-knulli.zip"
            package.write_bytes(b"beta")
            (root / "release/beta-knulli.json").write_text(json.dumps({
                "version": "1.2-beta.1", "path": str(package.relative_to(root)),
                "size": 4, "sha256": hashlib.sha256(b"beta").hexdigest()}))
            self.assertEqual(verify_package(root, "1.2-beta.1"), package)
        self.assertIn("`betterpod-v1.1-knulli.zip`", release_notes("Stable", "1.1"))


class PublicationTests(unittest.TestCase):
    target = "a" * 40
    repo = "example/betterPod"

    def release_tree(self, root, version):
        (root / "src").mkdir()
        (root / "src/ij_update.go").write_text(
            f'const BETTERPOD_VERSION = "{version}"')
        (root / "CHANGELOG.md").write_text(
            f"# Changelog\n\n## v{version} (2026-11-05)\n\nNew release\n"
            "\n## v1.0\n\nHistorical notes\n")
        (root / "release").mkdir()
        package = root / f"release/betterpod-v{version}-knulli.zip"
        content = b"verified release package"
        package.write_bytes(content)
        channel = "beta" if "-" in version else "latest"
        (root / f"release/{channel}-knulli.json").write_text(json.dumps({
            "version": version, "path": str(package.relative_to(root)),
            "size": len(content), "sha256": hashlib.sha256(content).hexdigest()}))
        return package

    def invoke_main(self, root, version, tag_result):
        calls = []
        notes = []

        def fake_run(*args):
            calls.append(args)
            if args == ("gh", "api", f"repos/{self.repo}/releases", "--paginate", "--slurp"):
                return '[[{"tag_name": "v1.0"}]]'
            if args == ("git", "log", "--format=%H %s"):
                return f"{'b' * 40} Merge release PR\n{self.target} v{version}: release\n"
            if args == ("git", "diff", self.target, "HEAD", "--", "src", "port", "go.mod", "go.sum", "release"):
                return ""
            if args[:3] == ("gh", "release", "create"):
                notes.append(Path(args[args.index("--notes-file") + 1]).read_text())
                return "https://example.com/release\n"
            self.fail(f"Unexpected command: {args}")

        with mock.patch.object(gh_releases, "__file__", str(root / "tools/gh_releases.py")), \
             mock.patch.object(gh_releases.os, "chdir"), \
             mock.patch.dict(gh_releases.os.environ, {"GITHUB_REPOSITORY": self.repo}), \
             mock.patch.object(gh_releases, "run", side_effect=fake_run), \
             mock.patch.object(gh_releases.subprocess, "run", return_value=tag_result) as tag_check, \
             mock.patch("builtins.print"):
            try:
                gh_releases.main()
            finally:
                self.command_calls = calls
                self.release_notes = notes
            tag_check.assert_called_once_with(
                ["git", "rev-parse", "--verify", "--quiet", f"refs/tags/v{version}^{{commit}}"],
                capture_output=True, text=True)
        return calls, notes

    def test_stable_and_beta_publication(self):
        for version, latest, prerelease in [("1.1", "--latest=true", False),
                                           ("1.2-beta.1", "--latest=false", True)]:
            with self.subTest(version=version), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                package = self.release_tree(root, version)
                # A retry may retain the correct tag before a release exists.
                if prerelease:
                    tag = SimpleNamespace(returncode=0, stdout=self.target + "\n")
                else:
                    tag = SimpleNamespace(returncode=1, stdout="")
                calls, notes = self.invoke_main(root, version, tag)
                create = [args for args in calls if args[:3] == ("gh", "release", "create")]
                self.assertEqual(len(create), 1)
                args = create[0]
                self.assertEqual(args[3], "v" + version)
                self.assertEqual(args[args.index("--repo") + 1], self.repo)
                self.assertEqual(args[args.index("--target") + 1], self.target)
                self.assertEqual([arg for arg in args if arg.startswith("--latest=")], [latest])
                self.assertEqual("--prerelease" in args, prerelease)
                self.assertEqual(Path(args[-1]).resolve(), package.resolve())
                self.assertIn("New release", notes[0])
                self.assertNotIn("Historical notes", notes[0])

    def test_mismatched_existing_tag_aborts_before_publication(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.release_tree(root, "1.1")
            wrong_tag = SimpleNamespace(returncode=0, stdout="c" * 40 + "\n")
            with self.assertRaisesRegex(ValueError, "Existing tag v1.1 points at a different commit"):
                self.invoke_main(root, "1.1", wrong_tag)
            self.assertFalse(any(args[:3] == ("gh", "release", "create") for args in self.command_calls))
            self.assertFalse(any(args[:2] == ("git", "diff") for args in self.command_calls))
            self.assertEqual(self.release_notes, [])


if __name__ == "__main__":
    unittest.main()
