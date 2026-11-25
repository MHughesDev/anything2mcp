#!/usr/bin/env python3
"""Proves tools/notices.py refuses a stale or missing notices file, and a crate with no text it can carry.

Runs without cargo: the list of crates and their sources are given, not asked of cargo.
"""
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import notices  # noqa: E402

REPO = Path(__file__).resolve().parent.parent


class Fixture:
    """A copy of the repository's LICENSE and two unpacked crates, one with a text and one without."""

    def __init__(self, second_licence="MIT"):
        self.dir = Path(tempfile.mkdtemp())
        shutil.copy(REPO / "LICENSE", self.dir / "LICENSE")
        (self.dir / "bindings" / "python").mkdir(parents=True)
        with_text = self.dir / "src" / "alpha"
        with_text.mkdir(parents=True)
        (with_text / "Cargo.toml").write_text("")
        (with_text / "LICENSE-MIT").write_text("Alpha licence text  \r\nsecond line\r\n")
        (with_text / "README.md").write_text("not a licence")
        without = self.dir / "src" / "beta"
        without.mkdir(parents=True)
        (without / "Cargo.toml").write_text("")
        self.packages = {
            ("alpha", "1.0.0"): [{"name": "alpha", "version": "1.0.0", "license": "MIT OR Apache-2.0",
                                  "repository": "https://example.com/alpha", "source": "registry",
                                  "manifest_path": str(with_text / "Cargo.toml")}],
            ("beta", "2.0.0"): [{"name": "beta", "version": "2.0.0", "license": second_licence,
                                 "repository": "https://example.com/beta", "source": "registry",
                                 "manifest_path": str(without / "Cargo.toml")}],
        }

    def __enter__(self):
        self.saved = (notices.crates_in_binary, notices.packages)
        notices.crates_in_binary = lambda root: set(self.packages)
        notices.packages = lambda root: self.packages
        return self

    def __exit__(self, *exc):
        notices.crates_in_binary, notices.packages = self.saved
        shutil.rmtree(self.dir)

    def run(self, command):
        return notices.main(["notices.py", command, "--root", str(self.dir)])


class NoticesTest(unittest.TestCase):
    def test_write_then_check_passes_and_both_files_agree(self):
        with Fixture() as f:
            self.assertEqual(f.run("write"), 0)
            self.assertEqual(f.run("check"), 0)
            text = (f.dir / notices.NOTICES).read_text()
            self.assertEqual(text, (f.dir / notices.COPY).read_text())
            self.assertIn("alpha 1.0.0  |  MIT OR Apache-2.0", text)
            self.assertIn("Alpha licence text\nsecond line\n", text)
            self.assertNotIn("\r", text)
            self.assertNotIn("not a licence", text)
            self.assertIn("Copyright (c) the authors of beta (https://example.com/beta)", text)
            self.assertIn("tools/notices.py supplied the MIT text", text)

    def test_a_stale_file_fails(self):
        with Fixture() as f:
            f.run("write")
            path = f.dir / notices.NOTICES
            path.write_text(path.read_text() + "edited\n")
            self.assertEqual(f.run("check"), 1)

    def test_a_copy_that_differs_fails(self):
        with Fixture() as f:
            f.run("write")
            (f.dir / notices.COPY).write_text("old\n")
            self.assertEqual(f.run("check"), 1)

    def test_a_missing_file_fails(self):
        with Fixture() as f:
            self.assertEqual(f.run("check"), 1)

    def test_a_new_crate_makes_the_file_stale(self):
        with Fixture() as f:
            f.run("write")
            del f.packages[("beta", "2.0.0")]
            self.assertEqual(f.run("check"), 1)

    def test_a_crate_with_no_text_and_no_mit_choice_fails(self):
        with Fixture(second_licence="Apache-2.0") as f:
            self.assertEqual(f.run("write"), 1)
            self.assertFalse((f.dir / notices.NOTICES).exists())

    def test_mit_is_found_inside_an_expression_and_not_inside_a_word(self):
        self.assertIsNotNone(notices.supplied_mit(REPO, {"name": "x", "license": "(Zlib OR MIT)"}))
        self.assertIsNone(notices.supplied_mit(REPO, {"name": "x", "license": "MIT-0"}))
        self.assertIsNone(notices.supplied_mit(REPO, {"name": "x", "license": None}))


if __name__ == "__main__":
    unittest.main()
