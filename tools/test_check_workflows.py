#!/usr/bin/env python3
"""Proves tools/check_workflows.py fails on a workflow file that does not read as YAML, and reads the
repository's workflows as PyYAML does, where PyYAML is installed."""
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import check_workflows as cw  # noqa: E402

try:
    import yaml
except ImportError:
    yaml = None

GOOD = "name: t\non:\n  push:\njobs:\n  t:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo test 'tests::memory::' --\n"
BROKEN = GOOD.replace("'tests::memory::' --", "tests::memory:: --")


def root_with(text):
    root = Path(tempfile.mkdtemp(prefix="a2m-workflows-"))
    (root / ".github" / "workflows").mkdir(parents=True)
    (root / ".github" / "workflows" / "ci.yml").write_text(text, encoding="utf-8")
    return root


class Workflows(unittest.TestCase):
    def test_the_self_check_passes(self):
        self.assertEqual(cw.self_check(), [])

    def test_a_good_file_reads(self):
        self.assertEqual(cw.check(root_with(GOOD)), ([], 1))

    def test_the_file_that_failed_every_run_is_caught_by_its_line(self):
        problems, _ = cw.check(root_with(BROKEN))
        self.assertEqual(len(problems), 1, problems)
        self.assertIn(".github/workflows/ci.yml: line 8:", problems[0])

    def test_a_file_that_is_not_a_workflow_is_caught(self):
        problems, _ = cw.check(root_with("name: t\n"))
        self.assertEqual(len(problems), 1, problems)

    def test_the_repository_reads(self):
        problems, count = cw.check(HERE.parent)
        self.assertEqual(problems, [])
        self.assertGreater(count, 0)

    @unittest.skipIf(yaml is None, "PyYAML is not installed")
    def test_what_it_reads_is_what_pyyaml_reads(self):
        for path in sorted((HERE.parent / ".github" / "workflows").glob("*.yml")):
            text = path.read_text(encoding="utf-8")
            self.assertEqual(cw.read(text), yaml.load(text, Loader=yaml.BaseLoader), path.name)
        self.assertEqual(cw.read(cw.MUST_READ), yaml.load(cw.MUST_READ, Loader=yaml.BaseLoader))
        # Every sample the check refuses, PyYAML refuses too, but a key given twice, which GitHub refuses.
        for name, sample in cw.MUST_FAIL.items():
            if name == "a key given twice":
                continue
            with self.assertRaises(yaml.YAMLError, msg=name):
                yaml.load(sample, Loader=yaml.BaseLoader)


if __name__ == "__main__":
    unittest.main()
