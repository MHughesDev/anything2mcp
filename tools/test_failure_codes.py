#!/usr/bin/env python3
"""Prove that tools/failure_codes.py names a declared failure code that nothing raises, and only such a
code (decision 0455, package LS2).

A code is raised when, outside every `failure_codes!` table and every test, a crate's source names its
string or its constant, or another crate's manifest passes it on. The scratch trees here are made in a
temporary folder; one test holds today's tree to no finding. Run from the repository root:

    python3 tools/test_failure_codes.py

Standard library only; Python 3.11 or newer.
"""
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import failure_codes as fc  # noqa: E402

REPO = HERE.parent


def table(*rows):
    body = "".join(f'    {name} = Input, "{code}", meaning "A case.", next "Do a thing.";\n' for name, code in rows)
    return "a2m_core::failure_codes! {\n" + body + "}\n"


def write(root, path, text):
    p = root / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8")


def tree():
    """A tree of four crates, with a code of each kind the check must tell apart."""
    root = Path(tempfile.mkdtemp(prefix="a2m-failure-codes-"))
    CLEANUP.append(root)
    write(root, "crates/a2m-x/src/codes.rs", table(
        ("USED", "used_by_constant"),
        ("BY_STRING", "used_by_string"),
        ("PASSED", "passed_on"),
        ("ONLY_TESTS", "only_in_tests"),
        ("ELSEWHERE", "used_by_another_crate"),
        ("AFTER", "used_after_a_test_module"),
        ("SHARED", "named_alike_elsewhere"),
    ))
    write(root, "crates/a2m-x/src/lib.rs", "\n".join([
        "pub mod codes;",
        "#[cfg(test)]",
        "mod early;",
        "// ONLY_TESTS in a comment is not a use, nor \"only_in_tests\"",
        "/* nor ONLY_TESTS here */",
        "pub fn f() -> Failure { codes::USED.failure(\"m\", \"n\") }",
        "#[cfg(test)]",
        "mod tests { fn t() { let _ = super::codes::ONLY_TESTS; let _ = \"only_in_tests\"; } }",
        "pub fn g() -> Failure { codes::AFTER.failure(\"m\", \"n\") }",
        "",
    ]))
    write(root, "crates/a2m-x/src/tests.rs", "fn t() { let _ = ONLY_TESTS; }\n")
    write(root, "crates/a2m-x/tests/it.rs", "fn t() { let _ = \"only_in_tests\"; }\n")
    write(root, "crates/a2m-w/src/lib.rs", "\n".join([
        "use a2m_x::codes::ELSEWHERE;",
        "pub fn h() -> &'static str { \"used_by_string\" }",
        "",
    ]))
    # A constant of the same name in a crate that does not name a2m-x is that crate's own.
    write(root, "crates/a2m-v/src/lib.rs", "pub const SHARED: u8 = 1;\n")
    write(root, "crates/a2m-pack-y/anything2mcp-provider.toml", "\n".join([
        "[provider]", 'name = "y"',
        "[[failure_codes]]", 'class = "input"', 'code = "passed_on"',
        "[[failure_codes]]", 'class = "input"', 'code = "own_manifest_only"',
        "",
    ]))
    # A pack's own code listed in its own manifest is still raised by nothing.
    write(root, "crates/a2m-pack-y/src/codes.rs", table(("OWN", "own_manifest_only")))
    write(root, "crates/a2m-pack-y/tests/fixture/anything2mcp-provider.toml", "\n".join([
        "[[failure_codes]]", 'class = "input"', 'code = "named_alike_elsewhere"', "",
    ]))
    write(root, fc.REFERENCE, f"# Reference\n\n{fc.BEGIN}\n{fc.END}\n")
    rows, faults = fc.resolve(fc.scan(root), root)
    assert not faults, faults
    write(root, fc.REFERENCE, f"# Reference\n\n{fc.BEGIN}\n{fc.render(rows)}\n{fc.END}\n")
    return root


CLEANUP = []
UNRAISED = ["named_alike_elsewhere", "only_in_tests", "own_manifest_only"]


class Unraised(unittest.TestCase):
    @classmethod
    def tearDownClass(cls):
        for root in CLEANUP:
            shutil.rmtree(root, ignore_errors=True)

    def test_today_s_tree_has_no_code_nothing_raises(self):
        self.assertEqual(fc.unraised(fc.scan(REPO)), [])

    def test_each_code_nothing_raises_is_named_and_no_other(self):
        found = fc.unraised(fc.scan(tree()))
        self.assertEqual([p.split("`")[1] for p in found], UNRAISED, found)

    def test_the_check_fails_naming_each(self):
        root = tree()
        problems = fc.check(root)
        for code in UNRAISED:
            self.assertTrue(any(f"`{code}`" in p and "nothing raises it" in p for p in problems), problems)
        run = subprocess.run([sys.executable, str(HERE / "failure_codes.py"), "--check", "--root", str(root)],
                             capture_output=True, text=True)
        self.assertEqual(run.returncode, 1, run.stdout + run.stderr)
        self.assertIn("`only_in_tests`", run.stdout)

    def test_a_tree_whose_codes_are_all_raised_passes(self):
        root = tree()
        write(root, "crates/a2m-x/src/more.rs", "use a2m_x::codes::{ONLY_TESTS, SHARED};\n")
        write(root, "crates/a2m-pack-y/src/lib.rs", "fn f() { codes::OWN; }\n")
        self.assertEqual(fc.check(root), [])

    def test_a_test_item_is_cut_to_its_end_and_no_further(self):
        text = "\n".join([
            'const A: &[u8] = b"#[cfg(test)]";',
            "fn keep() { let c = '{'; let s = r#\"x \"#[cfg(test)]\" y\"#; }",
            "#[cfg(test)]",
            "#[allow(dead_code)]",
            "mod tests { fn f() { \"}\"; '}'; } }",
            "#[cfg(test)] mod other;",
            "fn after<'a>(x: &'a str) {}",
            "#[cfg(test)]",
            "pub(crate) fn only(x: [u8; 2]) -> u8 { 1 }",
            "fn last() {}",
        ])
        kept = fc.without_tests(text)
        for line in ("const A", "fn keep()", "fn after<'a>", "fn last()"):
            self.assertIn(line, kept)
        for line in ("mod tests", "mod other", "fn only"):
            self.assertNotIn(line, kept)
        self.assertEqual(fc.without_tests("#![cfg(test)]\nfn f() {}\n"), "")


if __name__ == "__main__":
    unittest.main()
