#!/usr/bin/env python3
"""Proves how tools/gate.py weighs a test program when it shares programs out
over shards: a test target whose source is a `main.rs` weighs the tests of its
whole folder (decision 0446), as a library does, and any other test target
weighs the tests of its own file.

And proves the check that CI's shards are one run of the workspace's tests
(decision 0449): it reads cargo's lists, it names a test left out, run twice,
or not in the full list, and, on a small workspace built here with cargo, the
shards' records of a real run match the full list cargo gives."""
import os
import shutil
import subprocess
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


CARGO_LISTING = """\
   Compiling alpha v0.1.0 (/w/alpha)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.00s
     Running unittests src/lib.rs (target/debug/deps/alpha-0a1b2c3d)
tests::adds: test
tests::slow: test
     Running tests/one.rs (target\\debug\\deps\\one-99ffee00.exe)
\x1b[1mwrites: test\x1b[0m
   Doc-tests alpha
src/lib.rs - add (line 3): test
"""


class Records(unittest.TestCase):
    def test_cargos_listing_names_each_program_and_test(self):
        self.assertEqual(gate.parse_listing(CARGO_LISTING), [
            ("alpha-0a1b2c3d", "tests::adds"),
            ("alpha-0a1b2c3d", "tests::slow"),
            ("one-99ffee00", "writes"),
            ("doctests alpha", "src/lib.rs - add (line 3)"),
        ])

    def test_a_program_is_known_by_its_file_name_on_every_system(self):
        self.assertEqual(gate.program_key("C:\\a\\target\\debug\\deps\\one-99.exe"), "one-99")
        self.assertEqual(gate.program_key("/w/target/debug/deps/one-99"), "one-99")

    def test_shards_that_ran_every_test_once_match(self):
        full = [("p", "a"), ("p", "b"), ("q", "a")]
        self.assertEqual(gate.compare_tests(full, [[("p", "a"), ("q", "a")], [("p", "b")]]), [])

    def test_a_test_no_shard_ran_is_named(self):
        full = [("p", "a"), ("p", "b")]
        self.assertEqual(gate.compare_tests(full, [[("p", "a")]]), ["missing: p b"])

    def test_a_test_two_shards_ran_is_named(self):
        full = [("p", "a")]
        self.assertEqual(gate.compare_tests(full, [[("p", "a")], [("p", "a")]]), ["run 2 times: p a"])

    def test_a_test_the_full_list_has_not_got_is_named(self):
        full = [("p", "a")]
        self.assertEqual(gate.compare_tests(full, [[("p", "a"), ("r", "z")]]), ["not in the full list: r z"])

    def test_an_empty_full_list_never_matches(self):
        self.assertEqual(gate.compare_tests([], [[]]), ["the full list holds no test"])

    def test_a_missing_record_fails_the_comparison(self):
        root = folder_with({"full.txt": "p\ta\n"})
        self.assertEqual(gate.compare_main([str(root / "full.txt"), str(root / "shard-1.txt")]), 1)


WORKSPACE = {
    "Cargo.toml": '[workspace]\nmembers = ["alpha", "beta"]\nresolver = "2"\n',
    "alpha/Cargo.toml": '[package]\nname = "alpha"\nversion = "0.1.0"\nedition = "2021"\n',
    "alpha/src/lib.rs": "/// ```\n/// assert_eq!(alpha::add(1, 1), 2);\n/// ```\npub fn add(a: u8, b: u8) -> u8 { a + b }\n"
    "#[cfg(test)]\nmod tests {\n#[test]\nfn adds() { assert_eq!(super::add(2, 2), 4); }\n#[test]\nfn dir() { assert!(std::path::Path::new(\"Cargo.toml\").exists()); }\n}\n",
    "alpha/tests/whole/main.rs": "mod read;\n#[test]\nfn whole() {}\n",
    "alpha/tests/whole/read.rs": "#[test]\nfn reads() {}\n#[test]\n#[ignore]\nfn slow() {}\n",
    "alpha/tests/one.rs": "#[test]\nfn one_of_alpha() {}\n",
    "beta/Cargo.toml": '[package]\nname = "beta"\nversion = "0.1.0"\nedition = "2021"\n',
    "beta/src/lib.rs": "#[test]\nfn b1() {}\n#[test]\nfn b2() {}\n",
    "beta/tests/one.rs": "#[test]\nfn one_of_beta() {}\n",
}


class OneRun(unittest.TestCase):
    """The shards of a real build, run as CI runs them, are the one run cargo
    lists: two programs named `one` in two packages are told apart, the
    doctests are run once, and an ignored test is listed as cargo lists it."""

    def test_the_shards_of_a_real_workspace_match_its_full_list(self):
        if shutil.which("cargo") is None:
            self.fail("cargo is needed to prove the shards of a real build")
        root = folder_with(WORKSPACE)
        env = {**os.environ, "CARGO_TARGET_DIR": str(root / "target"), "CARGO_TERM_COLOR": "always"}
        env.pop("RUSTUP_TOOLCHAIN", None)
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=root, env=env, check=True, capture_output=True)
        here = os.getcwd()
        os.chdir(root)
        try:
            self.assertEqual(gate.list_tests(str(root / "full.txt"), env), 0)
            records = []
            for k in (1, 2):
                ok, _, output, failed, ran = gate.shard_tests(k, 2, env)
                self.assertTrue(ok, f"{failed}\n{output}")
                gate.write_record(str(root / f"shard-{k}.txt"), ran)
                records.append(str(root / f"shard-{k}.txt"))
        finally:
            os.chdir(here)
        self.addCleanup(shutil.rmtree, root, ignore_errors=True)
        full = gate.read_record(str(root / "full.txt"))
        self.assertEqual(len(full), 10, full)
        self.assertEqual(len({program for program, _ in full}), 6, full)
        self.assertEqual(gate.compare_main([str(root / "full.txt")] + records), 0)
        self.assertNotEqual(gate.compare_main([str(root / "full.txt"), records[0]]), 0)
        self.assertNotEqual(gate.compare_main([str(root / "full.txt")] + records + records[:1]), 0)


if __name__ == "__main__":
    unittest.main()
