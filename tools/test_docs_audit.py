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
SKIP = shutil.ignore_patterns(".git", "target", "__pycache__", ".anything2mcp", "dist", ".venv")


class AuditCatches(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="a2m-audit-"))
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
        self.depend("a2m-core", "a2m-engine")
        self.assert_finding("layering", "a2m-core (layer 1) depends on a2m-engine (layer 4)")

    def test_a_dependency_inside_a_layer_is_refused(self):
        self.depend("a2m-auth", "a2m-ledger")
        self.assert_finding("layering", "a2m-auth (layer 3) depends on a2m-ledger (layer 3)")

    def test_a_kernel_crate_that_depends_on_a_pack_is_refused(self):
        self.depend("a2m-engine", "a2m-pack-http")
        self.assert_finding("layering", "a2m-engine (layer 4) depends on a2m-pack-http (layer 8)",
                            "a kernel crate never depends on a pack crate")

    def test_a_pack_that_depends_on_the_engine_is_refused(self):
        self.depend("a2m-pack-http", "a2m-engine")
        self.assert_finding("layering", "a2m-pack-http (layer 8) depends on a2m-engine (layer 4)",
                            "allows layers 1 to 3")

    def test_a_pack_that_depends_on_another_pack_is_refused(self):
        self.depend("a2m-pack-http", "a2m-pack-web-browser")
        self.assert_finding("layering", "a2m-pack-http (layer 8) depends on a2m-pack-web-browser (layer 7)",
                            "only when that crate is the base of its own pack")

    def test_a_pack_that_depends_on_its_own_base_and_the_kernel_is_allowed(self):
        self.depend("a2m-pack-web-ui", "a2m-pack-web-browser")
        self.depend("a2m-pack-web-ui", "a2m-provider")
        code, out = self.audit()
        self.assertEqual(code, 0, out)

    def test_the_product_crate_may_depend_on_everything(self):
        self.depend("anything2mcp", "a2m-engine")
        self.depend("anything2mcp", "a2m-pack-web-ui")
        code, out = self.audit()
        self.assertEqual(code, 0, out)

    def test_a_dependency_on_a_lower_layer_is_allowed(self):
        self.depend("a2m-engine", "a2m-core")
        code, out = self.audit()
        self.assertEqual(code, 0, out)

    def test_the_test_application_in_a_product_crate_is_refused(self):
        self.depend("a2m-engine", "example-app")
        self.assert_finding("layering", "a2m-engine depends on example-app outside its tests")

    def test_the_test_application_in_a_crate_s_tests_is_allowed(self):
        manifest = self.root / "crates" / "a2m-engine" / "Cargo.toml"
        text = manifest.read_text(encoding="utf-8")
        text += '\n[dev-dependencies]\nexample-app = { path = "../../targets/example-app" }\n'
        manifest.write_text(text, encoding="utf-8")
        code, out = self.audit()
        self.assertEqual(code, 0, out)

    def test_a_crate_missing_from_the_table_is_refused(self):
        extra = self.root / "crates" / "a2m-extra"
        extra.mkdir()
        (extra / "Cargo.toml").write_text('[package]\nname = "a2m-extra"\n', encoding="utf-8")
        self.assert_finding("layering", "crates/a2m-extra exists but is not in the layering table")

    def test_a_crate_missing_from_disk_is_refused(self):
        shutil.rmtree(self.root / "crates" / "a2m-pack-web-probe")
        self.assert_finding("layering", "a2m-pack-web-probe is in the layering table but crates/a2m-pack-web-probe does not exist")

    def test_a_package_named_differently_from_its_folder_is_refused(self):
        manifest = self.root / "crates" / "a2m-core" / "Cargo.toml"
        text = manifest.read_text(encoding="utf-8")
        manifest.write_text(text.replace('name = "a2m-core"', 'name = "a2m-kernel"'), encoding="utf-8")
        self.assert_finding("layering", "crates/a2m-core holds a package named a2m-kernel")

    def test_a_pip_package_on_another_version_is_refused(self):
        project = self.root / "bindings" / "python" / "pyproject.toml"
        text = project.read_text(encoding="utf-8")
        changed = text.replace('version = "', 'version = "9.', 1)
        self.assertNotEqual(text, changed)
        project.write_text(changed, encoding="utf-8")
        self.assert_finding("release", "every package carries the same version")

    def test_a_changed_copy_of_the_licence_is_refused(self):
        copy = self.root / "bindings" / "python" / "LICENSE"
        copy.write_text(copy.read_text(encoding="utf-8") + "One more line.\n", encoding="utf-8")
        self.assert_finding("release", "bindings/python/LICENSE must be an exact copy of LICENSE")

    def test_an_earlier_name_in_a_living_file_is_refused(self):
        earlier = "Web" + "2MCP"
        page = self.root / "docs" / "spec" / "01-summary.md"
        page.write_text(page.read_text(encoding="utf-8") + f"\n{earlier} does this.\n", encoding="utf-8")
        source = self.root / "crates" / "a2m-core" / "src" / "lib.rs"
        source.write_text(source.read_text(encoding="utf-8") + f"// {earlier.lower()}\n", encoding="utf-8")
        self.assert_finding("names", "docs/spec/01-summary.md", "crates/a2m-core/src/lib.rs")

    def test_an_earlier_name_in_a_decision_record_is_allowed(self):
        record = sorted((self.root / "docs" / "decisions").glob("0001-*.md"))[0]
        record.write_text(record.read_text(encoding="utf-8") + "\nWeb" + "2MCP was the first name.\n", encoding="utf-8")
        code, out = self.audit()
        self.assertEqual(code, 0, out)

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
