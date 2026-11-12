import tempfile
import unittest
from pathlib import Path
from generate import render, checked_sources

class NoticesTests(unittest.TestCase):
    def test_requires_original_notice_for_unresolved_package(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = dict(name="fixture", version="1", source="registry", repository="https://example.invalid", license="MIT",
                     manifest_path=str(Path(tmp) / "Cargo.toml"))
            with self.assertRaisesRegex(ValueError, "Missing original notice"):
                render(dict(crates=[dict(package=p)], licenses=[]), [])

    def test_preserves_copyright_and_is_deterministic(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            (folder / "LICENSE").write_text("Copyright Fixture Owner\nPermission granted.\n")
            p = dict(name="fixture", version="1", source="registry", repository="https://example.invalid", license="MIT",
                     manifest_path=str(folder / "Cargo.toml"))
            data = dict(crates=[dict(package=p)], licenses=[])
            first = render(data, [])
            self.assertIn("Copyright Fixture Owner", first)
            self.assertNotIn(tmp, first)
            self.assertEqual(first, render(data, []))

    def test_missing_frontend_notice_is_an_error(self):
        with self.assertRaisesRegex(ValueError, "Missing frontend"):
            render(dict(crates=[], licenses=[]), [dict(name="fixture", version="1", texts=[])])

    def test_source_tampering_is_an_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            folder = Path(tmp)
            (folder / "sources.json").write_text('[{"name":"fixture","file":"LICENSE","sha256":"wrong"}]')
            (folder / "LICENSE").write_text("Modified notice")
            with self.assertRaisesRegex(ValueError, "Changed notice source"):
                checked_sources(folder)

if __name__ == "__main__":
    unittest.main()
