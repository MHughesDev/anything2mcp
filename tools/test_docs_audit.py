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
import tomllib
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SKIP = shutil.ignore_patterns(".git", "target", "__pycache__", ".anything2mcp", "dist", ".venv", "venv", "node_modules")


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

    def depend(self, crate, on, table="dependencies"):
        manifest = self.root / "crates" / crate / "Cargo.toml"
        text = manifest.read_text(encoding="utf-8")
        if on in tomllib.loads(text).get(table, {}):
            return
        line = f'{on} = {{ path = "../{on}" }}\n'
        head = f"[{table}]\n"
        if head in text:
            text = text.replace(head, head + line, 1)
        else:
            text += "\n" + head + line
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
        self.assert_finding("layering", "a2m-core (layer 1) depends on a2m-engine (layer 6)")

    def test_a_dependency_inside_a_layer_is_refused(self):
        self.depend("a2m-auth", "a2m-policy")
        self.assert_finding("layering", "a2m-auth (layer 4) depends on a2m-policy (layer 4)")

    def test_a_kernel_crate_that_depends_on_a_pack_is_refused(self):
        self.depend("a2m-engine", "a2m-pack-http")
        self.assert_finding("layering", "a2m-engine (layer 6) depends on a2m-pack-http (layer 10)",
                            "a kernel crate never depends on a pack crate")

    def test_a_pack_that_depends_on_the_engine_is_refused(self):
        self.depend("a2m-pack-http", "a2m-engine")
        self.assert_finding("layering", "a2m-pack-http (layer 10) depends on a2m-engine (layer 6)",
                            "allows layers 1 to 5")

    def test_a_pack_that_depends_on_another_pack_is_refused(self):
        self.depend("a2m-pack-http", "a2m-pack-web-browser")
        self.assert_finding("layering", "a2m-pack-http (layer 10) depends on a2m-pack-web-browser (layer 9)",
                            "only when that crate is the base of its own pack")

    def test_a_pack_that_depends_on_its_own_base_and_the_kernel_is_allowed(self):
        self.depend("a2m-pack-web-ui", "a2m-pack-web-browser")
        self.depend("a2m-pack-web-ui", "a2m-provider")
        code, out = self.audit()
        self.assertEqual(code, 0, out)

    def test_the_edges_the_design_needs_are_allowed(self):
        for crate, on in [("a2m-files", "a2m-expr"), ("a2m-policy", "a2m-ledger"), ("a2m-auth", "a2m-ledger"),
                          ("a2m-provider", "a2m-auth"), ("a2m-provider", "a2m-sources"),
                          ("a2m-engine", "a2m-provider"), ("a2m-sched", "a2m-ledger"),
                          ("a2m-pack-http", "a2m-provider"), ("a2m-pack-web-browser", "a2m-auth")]:
            self.depend(crate, on)
        code, out = self.audit()
        self.assertEqual(code, 0, out)

    def test_the_engine_and_the_scheduler_may_not_depend_on_each_other(self):
        self.depend("a2m-engine", "a2m-sched")
        self.assert_finding("layering", "a2m-engine (layer 6) depends on a2m-sched (layer 6)")

    def test_the_engine_may_not_depend_on_the_compiler_or_on_policy(self):
        self.depend("a2m-engine", "a2m-files")
        self.depend("a2m-engine", "a2m-policy")
        self.assert_finding("layering", "a2m-engine depends on a2m-files; the engine runs only compiled plans",
                            "a2m-engine depends on a2m-policy; only the control plane decides")

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
        self.depend("a2m-ledger", "example-db")
        self.assert_finding("layering", "a2m-engine depends on example-app outside its tests",
                            "a2m-ledger depends on example-db outside its tests")

    def test_the_test_application_in_a_crate_s_tests_is_allowed(self):
        self.depend("a2m-engine", "example-app", table="dev-dependencies")
        self.depend("a2m-engine", "example-db", table="dev-dependencies")
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

    def test_an_npm_launcher_on_another_version_is_refused(self):
        launcher = self.root / "bindings" / "npm" / "anything2mcp" / "package.json"
        text = launcher.read_text(encoding="utf-8")
        changed = text.replace('"version": "', '"version": "9.', 1)
        self.assertNotEqual(text, changed)
        launcher.write_text(changed, encoding="utf-8")
        self.assert_finding("release", "the npm launcher is 9.", "every package carries the same version")

    def test_an_npm_platform_package_on_another_version_is_refused(self):
        launcher = self.root / "bindings" / "npm" / "anything2mcp" / "package.json"
        text = launcher.read_text(encoding="utf-8")
        changed = text.replace('"@anything2mcp/linux-x64": "', '"@anything2mcp/linux-x64": "9.', 1)
        self.assertNotEqual(text, changed)
        launcher.write_text(changed, encoding="utf-8")
        self.assert_finding("release", "its optional dependency @anything2mcp/linux-x64 is 9.")

    def test_a_wrong_count_of_journal_entries_is_refused(self):
        index = self.root / "docs" / "README.md"
        text = index.read_text(encoding="utf-8")
        changed = text.replace(" entries |", "0 entries |", 1)
        self.assertNotEqual(text, changed)
        index.write_text(changed, encoding="utf-8")
        self.assert_finding("counts", "for journal entries")

    def test_a_plan_document_missing_from_the_plan_index_is_refused(self):
        (self.root / "docs" / "plan" / "a-new-plan.md").write_text("# A new plan\n", encoding="utf-8")
        self.assert_finding("plans", "docs/plan/a-new-plan.md is not listed in docs/plan/README.md")

    def test_a_review_document_missing_from_the_reviews_index_is_refused(self):
        (self.root / "docs" / "reviews" / "a-new-review.md").write_text("# A new review\n", encoding="utf-8")
        self.assert_finding("reviews", "docs/reviews/a-new-review.md is not listed in docs/reviews/README.md")

    def test_a_wrong_count_of_review_documents_is_refused(self):
        index = self.root / "docs" / "README.md"
        text = index.read_text(encoding="utf-8")
        changed = text.replace("lists them | 4 |", "lists them | 40 |", 1)
        self.assertNotEqual(text, changed)
        index.write_text(changed, encoding="utf-8")
        self.assert_finding("counts", "for review documents")

    def test_a_changed_copy_of_the_licence_is_refused(self):
        copy = self.root / "bindings" / "python" / "LICENSE"
        copy.write_text(copy.read_text(encoding="utf-8") + "One more line.\n", encoding="utf-8")
        self.assert_finding("release", "bindings/python/LICENSE must be an exact copy of LICENSE")

    def test_an_earlier_name_in_a_living_file_is_refused(self):
        earlier = "Web" + "2MCP"
        page = self.root / "docs" / "spec" / "1-product" / "101-summary.md"
        page.write_text(page.read_text(encoding="utf-8") + f"\n{earlier} does this.\n", encoding="utf-8")
        source = self.root / "crates" / "a2m-core" / "src" / "lib.rs"
        source.write_text(source.read_text(encoding="utf-8") + f"// {earlier.lower()}\n", encoding="utf-8")
        self.assert_finding("names", "docs/spec/1-product/101-summary.md", "crates/a2m-core/src/lib.rs")

    def test_an_earlier_name_in_a_decision_record_is_allowed(self):
        record = sorted((self.root / "docs" / "decisions").glob("0001-*.md"))[0]
        record.write_text(record.read_text(encoding="utf-8") + "\nWeb" + "2MCP was the first name.\n", encoding="utf-8")
        code, out = self.audit()
        self.assertEqual(code, 0, out)

    def test_a_crate_without_a_design_is_refused(self):
        (self.root / "docs" / "architecture" / "crates" / "a2m-expr.md").unlink()
        self.assert_finding("designs", "crates/a2m-expr has no design")

    def test_a_design_without_a_crate_is_refused(self):
        folder = self.root / "docs" / "architecture" / "crates"
        (folder / "a2m-extra.md").write_text("# `a2m-extra`\n", encoding="utf-8")
        self.assert_finding("designs", "a2m-extra.md describes a crate that does not exist")

    def test_a_design_that_states_the_wrong_layer_is_refused(self):
        design = self.root / "docs" / "architecture" / "crates" / "a2m-core.md"
        text = design.read_text(encoding="utf-8")
        changed = text.replace("| Layer | 1", "| Layer | 2", 1)
        self.assertNotEqual(text, changed)
        design.write_text(changed, encoding="utf-8")
        self.assert_finding("designs", "a2m-core.md must state layer 1")

    def test_an_old_section_number_in_a_living_document_is_refused(self):
        page = self.root / "docs" / "plan" / "roadmap.md"
        page.write_text(page.read_text(encoding="utf-8") + "\nSee section 42.\n", encoding="utf-8")
        self.assert_finding("refs", "docs/plan/roadmap.md: section 42 does not exist")

    def test_an_old_section_number_in_a_decision_record_is_allowed(self):
        record = sorted((self.root / "docs" / "decisions").glob("0001-*.md"))[0]
        record.write_text(record.read_text(encoding="utf-8") + "\nSee section 42.\n", encoding="utf-8")
        code, out = self.audit()
        self.assertEqual(code, 0, out)

    def test_a_section_in_the_wrong_part_is_refused(self):
        spec = self.root / "docs" / "spec"
        (spec / "2-kernel" / "225-performance.md").rename(spec / "3-packs" / "225-performance.md")
        self.assert_finding("structure", "section 225 is not in the part its first digit names")

    def test_a_file_loose_in_the_spec_folder_is_refused(self):
        (self.root / "docs" / "spec" / "99-loose.md").write_text("# 99. Loose\n", encoding="utf-8")
        self.assert_finding("structure", "docs/spec/99-loose.md: not a section")

    def test_a_broken_link_is_refused(self):
        page = self.root / "docs" / "README.md"
        page.write_text(page.read_text(encoding="utf-8") + "\n[nothing](no-such-file.md)\n", encoding="utf-8")
        self.assert_finding("links", "broken link to no-such-file.md")

    def test_a_replaced_statement_is_refused(self):
        page = self.root / "docs" / "spec" / "2-kernel" / "207-the-engine-loop.md"
        page.write_text(page.read_text(encoding="utf-8") + "\nRuns are marked interrupted.\n", encoding="utf-8")
        self.assert_finding("replaced", "207-the-engine-loop.md")

    def test_a_code_missing_from_the_failure_code_table_is_refused(self):
        page = self.root / "docs" / "reference" / "technical-reference.md"
        text = page.read_text(encoding="utf-8")
        lines = [l for l in text.splitlines(keepends=True) if "| `not_granted` |" not in l]
        self.assertNotEqual(len(lines), len(text.splitlines()))
        page.write_text("".join(lines), encoding="utf-8")
        self.assert_finding("codes", "`not_granted` is declared in", "and has no row in the reference")

    def test_a_row_with_no_code_behind_it_is_refused(self):
        page = self.root / "docs" / "reference" / "technical-reference.md"
        text = page.read_text(encoding="utf-8")
        row = "| `busy` | `no_such_code_anywhere` | `a2m-policy` | Planted. |\n"
        marker = "| `busy` | `acknowledgement_not_kept` |"
        self.assertIn(marker, text)
        page.write_text(text.replace(marker, row + marker, 1), encoding="utf-8")
        self.assert_finding("codes", "a row for `no_such_code_anywhere` and no crate declares it")

    def test_a_code_declared_with_two_classes_is_refused(self):
        table = self.root / "crates" / "a2m-engine" / "src" / "codes.rs"
        text = table.read_text(encoding="utf-8")
        # `engine_degraded` is declared by the scheduler too, as `busy`.
        planted = text.replace('ENGINE_DEGRADED = Busy, "engine_degraded",', 'ENGINE_DEGRADED = Input, "engine_degraded",', 1)
        self.assertNotEqual(text, planted)
        table.write_text(planted, encoding="utf-8")
        self.assert_finding("codes", "`engine_degraded` is declared with more than one class")

    def test_a_macro_row_with_no_next_is_a_fault(self):
        table = self.root / "crates" / "a2m-expr" / "src" / "codes.rs"
        text = table.read_text(encoding="utf-8")
        planted = text.replace('next "Compile the target again with this engine.";', 'next "  ";', 1)
        self.assertNotEqual(text, planted)
        table.write_text(planted, encoding="utf-8")
        self.assert_finding("codes", "`plan_expression_unloadable` has an empty next step")

    def test_a_migrated_crate_with_a_loose_code_is_a_fault(self):
        source = self.root / "crates" / "a2m-expr" / "src" / "error.rs"
        text = source.read_text(encoding="utf-8")
        cut = text.index("/// A place inside an expression")
        loose = '/// Planted.\npub const PLANTED_LOOSE: FailureCode = FailureCode::fixed("planted_loose");\n\n'
        source.write_text(text[:cut] + loose + text[cut:], encoding="utf-8")
        self.assert_finding("codes", "a2m-expr has moved to the `failure_codes!` table form, and declares `planted_loose` outside it")


if __name__ == "__main__":
    unittest.main(verbosity=2)
