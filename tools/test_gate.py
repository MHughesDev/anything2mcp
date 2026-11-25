#!/usr/bin/env python3
"""Proves how tools/gate.py weighs a test program when it shares programs out
over shards: a test target whose source is a `main.rs` weighs the tests of its
whole folder (decision 0446), as a library does, and any other test target
weighs the tests of its own file."""
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import gate  # noqa: E402


def folder_with(files):
    root = Path(tempfile.mkdtemp(prefix="a2m-gate-"))
    for name, text in files.items():
        (root / name).parent.mkdir(parents=True, exist_ok=True)
        (root / name).write_text(text, encoding="utf-8")
    return root


TWO = "#[test]\nfn a() {}\n#[tokio::test]\nasync fn b() {}\n"
ONE = "#[test]\nfn c() {}\n"


class Weights(unittest.TestCase):
    def test_a_main_rs_test_target_weighs_its_whole_folder(self):
        root = folder_with({"tests/main.rs": "mod write;\nmod read;\n", "tests/write.rs": TWO, "tests/read.rs": ONE})
        self.assertEqual(gate.program_weight(["test"], str(root / "tests" / "main.rs")), 3)

    def test_a_test_target_of_one_file_weighs_that_file_alone(self):
        root = folder_with({"tests/write.rs": TWO, "tests/read.rs": ONE})
        self.assertEqual(gate.program_weight(["test"], str(root / "tests" / "read.rs")), 1)

    def test_a_library_weighs_its_whole_folder(self):
        root = folder_with({"src/lib.rs": ONE, "src/store.rs": TWO})
        self.assertEqual(gate.program_weight(["lib"], str(root / "src" / "lib.rs")), 3)

    def test_a_source_that_cannot_be_read_weighs_nothing(self):
        root = folder_with({})
        self.assertEqual(gate.program_weight(["test"], str(root / "gone.rs")), 0)


if __name__ == "__main__":
    unittest.main()
