#!/usr/bin/env python3
"""Prove that tools/progress.py writes the plan's counts from the rows, and that --check fails
on a count that is stale or on rows that disagree.

Each test copies the two plan files to a temporary folder, changes one thing, and runs the
tool there. Run from the repository root:

    python3 tools/test_progress.py

Standard library only; Python 3.11 or newer.
"""
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PLAN = ("docs/plan/roadmap.md", "docs/plan/build-order.md")

ROADMAP = """# Roadmap

| State | `done`, `ready`, `held`, `owner`, or `withdrawn` |

| # | Milestone | What | Version | State |
| --- | --- | --- | --- | --- |
| 1 | One | Something | | <!-- progress:begin milestone-1 --><!-- progress:end --> |

## Milestone 1: one

| # | Package | Crates | Needs | Proof | State |
| --- | --- | --- | --- | --- | --- |
| A1 | First | `a2m-core` | | A test | done |
| A2 | Second | `a2m-core` | A1 | A test | ready |
| A3 | Third | `a2m-core` | A1 | A test | held |
| A4 | Fourth | `a2m-core` | | A test | owner |
| A5 | Fifth | `a2m-core` | | A test | withdrawn ([decision 0001](x.md)) |

<!-- progress:begin ready --><!-- progress:end -->
"""

BUILD_ORDER = """# Build order

**Progress:** <!-- progress:begin build-order --><!-- progress:end -->

## The line: every package left, and whether a worker can take it

| Row | Line | What is open | Affects done work | Crates it holds |
| --- | --- | --- | --- | --- |
| ~~A1~~ | Done | | | |
| A2 | Build-ready | | | |
| A3 | After the release | | | |
| A4 | Owner | | | |

## After the line
"""


class Progress(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="a2m-progress-"))
        (self.tmp / "docs" / "plan").mkdir(parents=True)

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def copy_repository_plan(self):
        for name in PLAN:
            shutil.copy(REPO / name, self.tmp / name)

    def synthetic(self, roadmap=ROADMAP, build_order=BUILD_ORDER):
        (self.tmp / PLAN[0]).write_text(roadmap, encoding="utf-8")
        (self.tmp / PLAN[1]).write_text(build_order, encoding="utf-8")

    def text(self, name):
        return (self.tmp / name).read_text(encoding="utf-8")

    def edit(self, name, old, new):
        text = self.text(name)
        self.assertIn(old, text)
        (self.tmp / name).write_text(text.replace(old, new, 1), encoding="utf-8")

    def run_tool(self, flag):
        run = subprocess.run([sys.executable, str(REPO / "tools" / "progress.py"), flag, "--root", str(self.tmp)],
                             capture_output=True, text=True, encoding="utf-8")
        return run.returncode, run.stdout + run.stderr

    def test_the_repository_s_counts_are_true(self):
        self.copy_repository_plan()
        code, out = self.run_tool("--check")
        self.assertEqual(code, 0, out)

    def test_write_gives_each_count_from_the_rows(self):
        self.synthetic()
        code, out = self.run_tool("--write")
        self.assertEqual(code, 0, out)
        build_order, roadmap = self.text(PLAN[1]), self.text(PLAN[0])
        self.assertIn("1 of 4 packages done, 25%, counted from the roadmap's rows. Of the 3 left, "
                      "1 waits on the owner (A4), 1 on the first release (A3), and 1 is an agent's "
                      "to finish before it (A2);", build_order)
        self.assertIn("Right now, one package is marked `ready`: A2.", roadmap)
        self.assertIn("1 of 4 done; ready: A2; held: A3; owner: A4; withdrawn: A5", roadmap)
        self.assertEqual(self.run_tool("--check")[0], 0)

    def test_the_ready_sentence_names_every_ready_row_and_its_number_word(self):
        self.synthetic(ROADMAP.replace("| A test | held |", "| A test | ready |"))
        self.assertEqual(self.run_tool("--write")[0], 0)
        self.assertIn("Right now, the packages marked `ready` are A2 and A3: two.", self.text(PLAN[0]))

    def test_a_package_left_that_the_line_does_not_name_is_refused(self):
        self.copy_repository_plan()
        self.edit(PLAN[0], "| 5.1 |", "| 5.1x |")  # a row the line does not name
        code, out = self.run_tool("--check")
        self.assertEqual(code, 1, out)
        self.assertIn("5.1x is left in the roadmap and has no row in the line", out)

    def test_a_count_typed_by_hand_is_stale(self):
        self.synthetic()
        self.run_tool("--write")
        self.edit(PLAN[1], "1 of 4 packages done", "2 of 4 packages done")
        code, out = self.run_tool("--check")
        self.assertEqual(code, 1, out)
        self.assertIn("build-order.md: the count build-order is stale", out)
        self.assertEqual(self.run_tool("--write")[0], 0)
        self.assertIn("1 of 4 packages done", self.text(PLAN[1]))

    def test_a_package_done_in_the_roadmap_moves_every_count(self):
        self.synthetic()
        self.run_tool("--write")
        (self.tmp / PLAN[0]).write_text(self.text(PLAN[0]).replace("| A test | ready |", "| A test | done |"),
                                        encoding="utf-8")
        self.edit(PLAN[1], "| A2 | Build-ready |", "| ~~A2~~ | ~~Build-ready~~, done |")
        code, out = self.run_tool("--check")
        self.assertEqual(code, 1, out)
        self.assertIn("the count build-order is stale", out)
        self.assertIn("the count ready is stale", out)
        self.assertIn("the count milestone-1 is stale", out)
        self.assertEqual(self.run_tool("--write")[0], 0)
        self.assertIn("2 of 4 packages done, 50%", self.text(PLAN[1]))
        self.assertIn("and 0 are an agent's to finish before it;", self.text(PLAN[1]))
        self.assertIn("Right now, no package is marked `ready`.", self.text(PLAN[0]))

    def test_the_line_and_the_roadmap_must_agree_and_write_refuses_while_they_do_not(self):
        self.synthetic(ROADMAP.replace("| A test | ready |", "| A test | done |"))
        before = self.text(PLAN[1])
        code, out = self.run_tool("--write")
        self.assertEqual(code, 1, out)
        self.assertIn("the line still gives A2 as agent; the roadmap marks it done", out)
        self.assertEqual(self.text(PLAN[1]), before)

    def test_a_row_the_line_strikes_and_the_roadmap_leaves_is_refused(self):
        self.synthetic(build_order=BUILD_ORDER.replace("| A3 | After the release |", "| ~~A3~~ | Done |"))
        code, out = self.run_tool("--check")
        self.assertEqual(code, 1, out)
        self.assertIn("the line marks A3 done; the roadmap marks it held", out)

    def test_a_count_without_its_markers_is_refused(self):
        self.synthetic(ROADMAP.replace("<!-- progress:begin ready --><!-- progress:end -->", ""))
        code, out = self.run_tool("--check")
        self.assertEqual(code, 1, out)
        self.assertIn("the count ready has no place between progress markers", out)

    def test_a_marker_no_rows_make_is_refused(self):
        self.synthetic(ROADMAP + "<!-- progress:begin milestone-7 -->7 of 7<!-- progress:end -->\n")
        code, out = self.run_tool("--check")
        self.assertEqual(code, 1, out)
        self.assertIn("no rows make the count milestone-7", out)


if __name__ == "__main__":
    unittest.main()
