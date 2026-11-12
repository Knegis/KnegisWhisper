import contextlib
import io
import os
from unittest.mock import patch

import hashlib
import json
import subprocess
import tempfile
import unittest
from pathlib import Path

from release import validate_version, preflight, prepare, calendar_clients, calendar_report


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
        self.write("tools/release/notes.md", "{{TAG}} {{SHA}} {{CALENDAR_CLIENTS}}")
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
        self.assertEqual((out / "SHA256SUMS.txt").read_bytes(), f"{digest}  {target.name}\n".encode("utf-8"))
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

    def test_calendar_presence_and_required_clients_never_print_values(self):
        names = ["KW_GOOGLE_CALENDAR_CLIENT_ID", "KW_GOOGLE_CALENDAR_CLIENT_SECRET", "KW_MICROSOFT_CALENDAR_CLIENT_ID"]
        values = ["dummy-google-id", "dummy-google-secret", "dummy-microsoft-id"]
        for supplied in [{}, dict(zip(names, values)), {names[0]: values[0]},
                         {names[1]: values[1]}, {names[2]: values[2]},
                         {names[0]: " ", names[1]: values[1], names[2]: " "}]:
            with self.subTest(supplied=list(supplied)):
                stderr = io.StringIO()
                stdout = io.StringIO()
                with contextlib.redirect_stderr(stderr), contextlib.redirect_stdout(stdout):
                    sha = preflight(self.root, "v0.1.0", "release-main", environ=supplied)
                    self.assertEqual(sha, self.git("rev-parse", "HEAD"))
                    if all(calendar_clients(supplied).values()):
                        preflight(self.root, "v0.1.0", "release-main", True, supplied)
                    else:
                        with self.assertRaises(ValueError):
                            preflight(self.root, "v0.1.0", "release-main", True, supplied)
                self.assertEqual(stdout.getvalue(), "")
                self.assertIn(calendar_report(calendar_clients(supplied)), stderr.getvalue())
                for value in values:
                    self.assertNotIn(value, stderr.getvalue())

    def test_preflight_cli_keeps_sha_alone_on_stdout_and_accepts_required_flag(self):
        env = dict(os.environ, KW_GOOGLE_CALENDAR_CLIENT_ID="dummy-google-id",
                   KW_GOOGLE_CALENDAR_CLIENT_SECRET="dummy-google-secret",
                   KW_MICROSOFT_CALENDAR_CLIENT_ID="dummy-microsoft-id")
        result = subprocess.run([os.sys.executable, str(Path(__file__).with_name("release.py")),
                                 "preflight", "--root", str(self.root), "--tag", "v0.1.0",
                                 "--main-ref", "release-main", "--require-calendar-clients"],
                                capture_output=True, text=True, env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, self.git("rev-parse", "HEAD") + "\n")
        self.assertIn("calendar: google=included microsoft=included", result.stderr)
        for value in ["dummy-google-id", "dummy-google-secret", "dummy-microsoft-id"]:
            self.assertNotIn(value, result.stdout + result.stderr)

    def test_notes_include_calendar_presence_without_values(self):
        self.write("bundle/nsis/setup.exe", "installer bytes")
        for index, env in enumerate([{}, {"KW_GOOGLE_CALENDAR_CLIENT_ID": "dummy-google-id",
                                          "KW_GOOGLE_CALENDAR_CLIENT_SECRET": "dummy-google-secret",
                                          "KW_MICROSOFT_CALENDAR_CLIENT_ID": "dummy-microsoft-id"}]):
            out = self.root / f"calendar-assets-{index}"
            with patch.dict(os.environ, {**{name: "" for name in ["KW_GOOGLE_CALENDAR_CLIENT_ID", "KW_GOOGLE_CALENDAR_CLIENT_SECRET", "KW_MICROSOFT_CALENDAR_CLIENT_ID"]}, **env}):
                prepare(self.root, "v0.1.0", self.root / "bundle", out)
            notes = (out / "RELEASE_NOTES.md").read_text()
            self.assertNotIn("{{CALENDAR_CLIENTS}}", notes)
            self.assertIn("One-click Google calendars: " + ("included" if index else "not included"), notes)
            self.assertIn("One-click Outlook calendars: " + ("included" if index else "not included"), notes)
            for value in ["dummy-google-id", "dummy-google-secret", "dummy-microsoft-id"]:
                self.assertNotIn(value, notes)


if __name__ == "__main__":
    unittest.main()
