#!/usr/bin/env python3
"""Proves tools/contracts.py catches each kind of break against a kept release, passes a break that is
listed with its record, and passes an addition (decision 0437, package YL4).

The kept folders here are made in a scratch folder from today's tree, as `write` makes them, with a
stand-in for the compiled target: the comparison is of the files, and `a2m-core`'s test
`kept_contracts` is what proves a kept compiled target loads.
"""
import copy
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import contracts as c  # noqa: E402

TODAY = c.tree(HERE.parent)
ACTION = "action.describe"


def keep(root, version, contract, breaks=""):
    folder = root / "contracts" / version
    (folder / "compiled").mkdir(parents=True)
    for name, data in contract.items():
        (folder / name).write_text(json.dumps(data), encoding="utf-8")
    target = b"anything2mcp-plan 1\n{}"
    (folder / "compiled" / "example.format-1.target").write_bytes(target)
    (folder / "compiled-targets.json").write_text(json.dumps(
        [{"file": "compiled/example.format-1.target", "format": 1, "hash": hashlib.sha256(target).hexdigest()}]))
    (folder / "breaks").write_text(breaks, encoding="utf-8")
    return folder


def scratch():
    root = Path(tempfile.mkdtemp(prefix="a2m-contracts-"))
    (root / "docs" / "decisions").mkdir(parents=True)
    (root / "docs" / "decisions" / "0437-a-record.md").write_text("# 0437\n")
    return root


def operation(contract, name):
    for path, operations in contract["openapi.json"]["paths"].items():
        if path.endswith("/actions/" + name):
            return operations["post"]
    raise KeyError(name)


def broken():
    """Today's contract with one break of each kind the proof names."""
    now = copy.deepcopy(TODAY)
    paths = now["openapi.json"]["paths"]
    gone = next(p for p in paths if p.endswith("/actions/engine.status"))
    del paths[gone]
    schema = operation(now, "docs.get")["requestBody"]["content"]["application/json"]["schema"]
    schema["required"] = schema.get("required", []) + ["topic"]
    codes = now["failure-codes.json"]
    codes["action_not_built_again"] = codes.pop("action_not_built")
    codes["settings_invalid"]["class"] = "policy"
    now["vocabularies.json"]["run_state"].append("paused")
    del now["settings.json"]["profile.name"]
    return now


EXPECTED = [
    "action.removed:engine.status",
    "input.required:docs.get.topic",
    "code.removed:action_not_built",
    "code.class:settings_invalid",
    "vocabulary.added:run_state.paused",
    "setting.removed:profile.name",
]


class Contracts(unittest.TestCase):
    def test_the_tree_reads_whole(self):
        for key in ("failure_class", "run_state", "effect_class", "evidence_level", "coverage"):
            self.assertTrue(TODAY["vocabularies.json"][key], key)
        self.assertIn("action_not_built", TODAY["failure-codes.json"])
        self.assertIn("profile.name", TODAY["settings.json"])
        self.assertTrue(any(k.startswith("packs.") for k in TODAY["settings.json"]))
        actions = c.actions_of(TODAY["openapi.json"])
        self.assertIn(ACTION, actions)
        self.assertIn("engine.start", actions, "an action the client carries out is built")
        self.assertNotIn("version.migrate", actions, "an action with no handler is not kept")

    def test_the_description_says_x_sdk_for_exactly_the_built_actions(self):
        root = HERE.parent
        self.assertEqual(c.sdk_problems(root), [])
        document = json.loads((root / c.OPENAPI).read_text(encoding="utf-8"))
        built = c.built_actions(root)
        for path, operations in document["paths"].items():
            for op in operations.values():
                if op.get("x-action") == "version.migrate":
                    op["x-sdk"] = True
                if op.get("x-action") == ACTION:
                    op["x-sdk"] = False
        problems = c.sdk_problems(root, document, built)
        self.assertEqual(len(problems), 2, problems)
        self.assertTrue(any("`version.migrate` has no handler" in p for p in problems), problems)
        self.assertTrue(any(f"`{ACTION}` is built" in p for p in problems), problems)

    def test_a_folder_kept_from_today_passes(self):
        root = scratch()
        keep(root, "0.1.0", TODAY)
        self.assertEqual(c.check(root, TODAY), ([], 1))

    def test_each_break_fails_once_by_name(self):
        root = scratch()
        keep(root, "0.1.0", TODAY)
        problems, _ = c.check(root, broken())
        self.assertEqual(sorted(problems), sorted(f"against 0.1.0: {b}" for b in EXPECTED))

    def test_more_breaks_are_named(self):
        now = copy.deepcopy(TODAY)
        op = operation(now, ACTION)
        op["requestBody"]["content"]["application/json"]["schema"]["properties"]["action"] = {"type": "integer"}
        op["x-sdk"] = False
        now["failure-codes.json"]["settings_invalid"]["passes_by_itself"] = True
        now["vocabularies.json"]["hazard"].remove("physical")
        now["settings.json"]["profile.name"] = "int"
        del operation(now, "docs.get")["responses"]["200"]["content"]["application/json"]["schema"]["properties"]["result"]["properties"]["topics"]
        schema = operation(now, ACTION)["requestBody"]["content"]["application/json"]["schema"]
        schema["properties"]["detail"] = {"type": "string"}
        schema["required"] = schema["required"] + ["detail"]
        found = c.breaks(TODAY, now)
        for name in ("output.removed:docs.get.topics", f"input.required:{ACTION}.detail",f"input.type:{ACTION}.action", f"sdk.removed:{ACTION}", "code.passes:settings_invalid",
                     "vocabulary.removed:hazard.physical", "setting.type:profile.name"):
            self.assertIn(name, found)

    def test_the_same_breaks_listed_with_their_record_pass(self):
        root = scratch()
        keep(root, "0.1.0", TODAY)
        (root / "contracts" / "next-breaks").write_text("# why\n" + "".join(f"{b} 0437\n" for b in EXPECTED))
        self.assertEqual(c.check(root, broken()), ([], 1))

    def test_a_break_listed_by_a_later_release_passes_against_an_earlier_one(self):
        root = scratch()
        keep(root, "0.1.0", TODAY)
        keep(root, "0.2.0", broken(), "".join(f"{b} 0437\n" for b in EXPECTED))
        self.assertEqual(c.check(root, broken()), ([], 2))

    def test_a_listing_without_a_record_or_that_does_not_happen_is_a_problem(self):
        root = scratch()
        keep(root, "0.1.0", TODAY)
        (root / "contracts" / "next-breaks").write_text("code.removed:action_not_built 0001\nsetting.removed:x\n")
        problems, _ = c.check(root, TODAY)
        self.assertEqual(len(problems), 3, problems)
        self.assertTrue(any("no decision record 0001" in p for p in problems))
        self.assertTrue(any("`<break> <record>`" in p for p in problems))
        self.assertTrue(any("is listed and is not a break" in p for p in problems))

    def test_additions_pass(self):
        root = scratch()
        keep(root, "0.1.0", TODAY)
        now = copy.deepcopy(TODAY)
        paths = now["openapi.json"]["paths"]
        source = next(p for p in paths if p.endswith("/actions/" + ACTION))
        added = copy.deepcopy(paths[source])
        added["post"]["x-action"] = "action.new"
        paths["/api/v1/actions/action.new"] = added
        schema = operation(now, ACTION)["requestBody"]["content"]["application/json"]["schema"]
        schema["properties"]["more"] = {"type": "string"}
        now["failure-codes.json"]["a_new_code"] = {"class": "input", "passes_by_itself": False}
        now["vocabularies.json"]["hazard"].append("another")
        now["settings.json"]["profile.another"] = "text"
        self.assertEqual(c.check(root, now), ([], 1))

    def test_a_kept_folder_that_is_not_whole_is_a_problem(self):
        root = scratch()
        folder = keep(root, "0.1.0", TODAY)
        (folder / "compiled" / "example.format-1.target").write_bytes(b"other bytes")
        problems, _ = c.check(root, TODAY)
        self.assertEqual(problems, ["contracts/0.1.0/compiled/example.format-1.target is not the bytes its hash was kept for"])
        self.assertEqual(len(c.kept_problems(root, "0.2.0")), 1, "a version with no kept folder is refused")

    def test_write_refuses_a_kept_version_and_a_break_a_patch_may_not_make(self):
        root = scratch()
        keep(root, "0.1.0", TODAY)
        with self.assertRaises(c.Fault):
            c.write(root, "0.1.0")
        with self.assertRaises(c.Fault):
            c.write(root, "next")
        (root / "contracts" / "next-breaks").write_text("setting.removed:profile.name 0437\n")
        # The tree of this scratch folder is not a repository, so check would fault on reading it;
        # the version rule is reached through a tree that matches what was kept.
        original = c.tree
        c.tree = lambda _root: (lambda t: (t["settings.json"].pop("profile.name"), t)[1])(copy.deepcopy(TODAY))
        try:
            with self.assertRaisesRegex(c.Fault, "may not break"):
                c.write(root, "0.1.1")
        finally:
            c.tree = original


if __name__ == "__main__":
    unittest.main()
