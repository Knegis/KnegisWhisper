import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from release import validate_version, preflight, prepare


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.write("Cargo.toml", '[workspace]\nmembers = []\n[workspace.package]\nversion = "0.1.0"\n')
        self.write("Cargo.lock", "version = 4\npackage = []\n")
        self.write("apps/desktop/package.json", '{"version":"0.1.0"}')
        self.write("apps/desktop/package-lock.json", '{"version":"0.1.0","packages":{"":{"version":"0.1.0"}}}')
        self.write("apps/desktop/src-tauri/tauri.conf.json", '{"version":"0.1.0"}')
        self.write("tools/release/notes.md", "{{TAG}} {{SHA}}")
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "user.name", "Release test")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        self.git("tag", "v0.1.0")
        self.git("branch", "release-main")

    def write(self, path, value):
        dest = self.root / path
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(value, encoding="utf-8")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args], stderr=subprocess.DEVNULL, text=True).strip()

    def test_matching_tag_and_main_ancestry(self):
        self.assertEqual(preflight(self.root, "v0.1.0", "release-main"), self.git("rev-parse", "HEAD"))

    def test_unsafe_or_invalid_tag_rejected(self):
        for tag in ["main", "--help", "v01.0.0", "v0.1.0;echo bad", "v0.1.0-preview"]:
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                validate_version(self.root, tag)

    def test_manifest_and_lockfile_drift_rejected(self):
        for path in ["apps/desktop/package.json", "apps/desktop/src-tauri/tauri.conf.json", "apps/desktop/package-lock.json"]:
            original = (self.root / path).read_text()
            self.write(path, original.replace("0.1.0", "0.2.0", 1))
            with self.subTest(path=path), self.assertRaises(ValueError):
                validate_version(self.root, "v0.1.0")
            self.write(path, original)

    def test_missing_tag_rejected(self):
        self.git("tag", "-d", "v0.1.0")
        with self.assertRaises(subprocess.CalledProcessError):
            preflight(self.root, "v0.1.0", "release-main")

    def test_wrong_checkout_and_non_main_commit_rejected(self):
        self.write("later.txt", "later")
        self.git("add", ".")
        self.git("commit", "-qm", "later")
        with self.assertRaises(ValueError):
            preflight(self.root, "v0.1.0", "release-main")
        self.git("tag", "-f", "v0.1.0")
        with self.assertRaises(subprocess.CalledProcessError):
            preflight(self.root, "v0.1.0", "release-main")

    def test_assets_have_exact_name_hash_and_provenance(self):
        self.write("bundle/nsis/setup.exe", "installer bytes")
        out = self.root / "assets"
        target = prepare(self.root, "v0.1.0", self.root / "bundle", out)
        self.assertEqual(target.name, "KnegisWhisper_0.1.0_windows_x64_setup.exe")
        digest = hashlib.sha256(target.read_bytes()).hexdigest()
        self.assertEqual((out / "SHA256SUMS.txt").read_text(), f"{digest}  {target.name}\n")
        self.assertIn(self.git("rev-parse", "HEAD"), (out / "RELEASE_NOTES.md").read_text())
        with self.assertRaises(ValueError):
            prepare(self.root, "v0.1.0", self.root / "bundle", out)

    def test_missing_empty_and_ambiguous_installers_rejected(self):
        bundle = self.root / "bundle"
        out = self.root / "assets"
        with self.assertRaises(ValueError):
            prepare(self.root, "v0.1.0", bundle, out)
        self.write("bundle/nsis/a.exe", "")
        with self.assertRaises(ValueError):
            prepare(self.root, "v0.1.0", bundle, out)
        self.write("bundle/nsis/a.exe", "installer")
        self.write("bundle/nsis/b.exe", "another")
        with self.assertRaises(ValueError):
            prepare(self.root, "v0.1.0", bundle, out)


if __name__ == "__main__":
    unittest.main()
