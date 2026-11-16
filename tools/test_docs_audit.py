#!/usr/bin/env python3
"""Prove that tools/docs_audit.py fails when it should.

Each test copies the repository to a temporary folder, plants one mistake, and checks
that the audit reports it. Run from the repository root:

    python3 tools/test_docs_audit.py

Standard library only; Python 3.11 or newer.
"""
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SKIP = shutil.ignore_patterns(".git", "target", "__pycache__", ".web2mcp")


class AuditCatches(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="w2m-audit-"))
        self.root = self.tmp / "repo"
        shutil.copytree(REPO, self.root, ignore=SKIP)

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def audit(self):
        run = subprocess.run(
            [sys.executable, str(REPO / "tools" / "docs_audit.py"), "--root", str(self.root)],
            capture_output=True, text=True, encoding="utf-8")
        return run.returncode, run.stdout + run.stderr

    def depend(self, crate, on):
        manifest = self.root / "crates" / crate / "Cargo.toml"
        text = manifest.read_text(encoding="utf-8")
        line = f'{on} = {{ path = "../{on}" }}\n'
        if "[dependencies]\n" in text:
            text = text.replace("[dependencies]\n", "[dependencies]\n" + line, 1)
        else:
            text += "\n[dependencies]\n" + line
        manifest.write_text(text, encoding="utf-8")

    def assert_finding(self, kind, *words):
        code, out = self.audit()
        self.assertEqual(code, 1, out)
        self.assertIn(f"[{kind}]", out)
        for word in words:
            self.assertIn(word, out)

    def test_an_unchanged_copy_passes(self):
        code, out = self.audit()
        self.assertEqual(code, 0, out)
        self.assertIn("0 finding(s)", out)

    def test_a_dependency_on_a_higher_layer_is_refused(self):
        self.depend("w2m-core", "w2m-engine")
        self.assert_finding("layering", "w2m-core (layer 1) depends on w2m-engine (layer 5)")

    def test_a_dependency_inside_a_layer_is_refused(self):
        self.depend("w2m-auth", "w2m-browser")
        self.assert_finding("layering", "w2m-auth (layer 3) depends on w2m-browser (layer 3)")

    def test_a_dependency_on_a_lower_layer_is_allowed(self):
        self.depend("w2m-engine", "w2m-core")
        code, out = self.audit()
        self.assertEqual(code, 0, out)

    def test_a_crate_missing_from_the_table_is_refused(self):
        extra = self.root / "crates" / "w2m-extra"
        extra.mkdir()
        (extra / "Cargo.toml").write_text('[package]\nname = "w2m-extra"\n', encoding="utf-8")
        self.assert_finding("layering", "crates/w2m-extra exists but is not in the layering table")

    def test_a_crate_missing_from_disk_is_refused(self):
        shutil.rmtree(self.root / "crates" / "w2m-probe")
        self.assert_finding("layering", "w2m-probe is in the layering table but crates/w2m-probe does not exist")

    def test_a_package_named_differently_from_its_folder_is_refused(self):
        manifest = self.root / "crates" / "w2m-core" / "Cargo.toml"
        text = manifest.read_text(encoding="utf-8")
        manifest.write_text(text.replace('name = "w2m-core"', 'name = "w2m-kernel"'), encoding="utf-8")
        self.assert_finding("layering", "crates/w2m-core holds a package named w2m-kernel")

    def test_a_broken_link_is_refused(self):
        page = self.root / "docs" / "README.md"
        page.write_text(page.read_text(encoding="utf-8") + "\n[nothing](no-such-file.md)\n", encoding="utf-8")
        self.assert_finding("links", "broken link to no-such-file.md")

    def test_a_replaced_statement_is_refused(self):
        page = self.root / "docs" / "spec" / "39-local-engine-and-clients.md"
        page.write_text(page.read_text(encoding="utf-8") + "\nRuns are marked interrupted.\n", encoding="utf-8")
        self.assert_finding("replaced", "39-local-engine-and-clients.md")


if __name__ == "__main__":
    unittest.main(verbosity=2)
