"""Tests of the generated methods.

The first test is the one that stops drift: the committed module must be
exactly what the generator writes from the saved OpenAPI document. The
others hold the shape of a method: it names its action, passes its input,
leaves out what was not given, and decides nothing.

Run: ``python -m unittest discover -s tests`` from ``bindings/python``.
"""

from __future__ import annotations

import importlib.util
import inspect
import json
import sys
import unittest
from pathlib import Path
from typing import Any, Dict, Optional

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(ROOT / "src"))

from anything2mcp import Client, _actions  # noqa: E402

_spec = importlib.util.spec_from_file_location("generate_actions", ROOT / "tools" / "generate_actions.py")
assert _spec is not None and _spec.loader is not None
generator = importlib.util.module_from_spec(_spec)
sys.modules["generate_actions"] = generator
_spec.loader.exec_module(generator)

DOCUMENT = json.loads((ROOT / "tools" / "openapi.json").read_text(encoding="utf-8"))


class Recorder(_actions.Actions):
    """Stands in for a client: records what a method sent to ``call``."""

    def call(self, action: str, input: Optional[Dict[str, Any]] = None, *, idempotency_key: Optional[str] = None) -> Any:  # type: ignore[override]
        self.sent = (action, input, idempotency_key)
        return "answer"


class GeneratedModuleTests(unittest.TestCase):
    def test_the_committed_module_is_what_the_generator_writes(self) -> None:
        written = generator.render(DOCUMENT)
        committed = (ROOT / "src" / "anything2mcp" / "_actions.py").read_text(encoding="utf-8")
        self.assertEqual(
            committed,
            written,
            "run `python3 tools/generate_actions.py` from bindings/python and commit the result",
        )

    def test_every_action_of_the_document_has_a_method_on_the_client(self) -> None:
        actions = [name for name, _ in generator.actions_of(DOCUMENT)]
        self.assertEqual(list(_actions.ACTIONS), actions)
        self.assertGreater(len(actions), 80)
        for action in actions:
            self.assertTrue(callable(getattr(Client, action.replace(".", "_"))), action)

    def test_the_generated_methods_are_keyword_only(self) -> None:
        for action in _actions.ACTIONS:
            method = getattr(Client, action.replace(".", "_"))
            for name, parameter in list(inspect.signature(method).parameters.items())[1:]:
                self.assertEqual(parameter.kind, inspect.Parameter.KEYWORD_ONLY, f"{action}.{name}")


class MethodShapeTests(unittest.TestCase):
    def test_a_method_names_its_action_and_passes_only_what_was_given(self) -> None:
        recorder = Recorder()
        answer = recorder.tool_call(tool="example_order_get", input={"order_id": "A-100"})
        self.assertEqual(answer, "answer")
        self.assertEqual(
            recorder.sent,
            ("tool.call", {"tool": "example_order_get", "input": {"order_id": "A-100"}}, None),
        )

    def test_an_idempotency_key_goes_with_the_call(self) -> None:
        recorder = Recorder()
        recorder.tool_call(tool="example_order_get", idempotency_key="example-key-1")
        self.assertEqual(recorder.sent[2], "example-key-1")

    def test_noun_and_verb_are_joined_as_the_action_names_them(self) -> None:
        recorder = Recorder()
        recorder.run_get(run_id="r1")
        self.assertEqual(recorder.sent[0], "run.get")
        recorder.engine_status()
        self.assertEqual(recorder.sent, ("engine.status", {}, None))

    def test_a_required_input_cannot_be_left_out(self) -> None:
        with self.assertRaises(TypeError):
            Recorder().tool_call()  # type: ignore[call-arg]

    def test_a_reserved_word_gets_a_trailing_underscore_and_is_sent_by_its_own_name(self) -> None:
        withs = [
            (a, op)
            for a, op in generator.actions_of(DOCUMENT)
            if "from" in dict(generator.request_schema(op)[0])
        ]
        self.assertTrue(withs, "the document has an action with a field named `from`")
        action, _ = withs[0]
        recorder = Recorder()
        required = {
            name: "x"
            for name in generator.request_schema(withs[0][1])[1]
        }
        required["from"] = "x"
        kwargs = {("from_" if k == "from" else k): v for k, v in required.items()}
        getattr(recorder, action.replace(".", "_"))(**kwargs)
        self.assertEqual(recorder.sent[0], action)
        self.assertIn("from", recorder.sent[1])


class MethodNameTests(unittest.TestCase):
    """The rule both generators apply (decision 0405)."""

    def test_an_underscore_in_a_noun_or_a_verb_stays_and_the_dot_becomes_one(self) -> None:
        self.assertEqual(generator.method_name("tool.call"), "tool_call")
        self.assertEqual(generator.method_name("policy.tools_allowed"), "policy_tools_allowed")
        self.assertEqual(generator.method_name("spec.import"), "spec_import")

    def test_a_name_of_another_form_stops_the_generator(self) -> None:
        for action in ("tool", "a.b.c", "Tool.call", "tool.Call", "a._b", "a.b_", "a.b__c", "a-b.c", "1a.b", ""):
            with self.assertRaises(generator.GenerateError, msg=action):
                generator.method_name(action)


class GeneratorTests(unittest.TestCase):
    def document(self, schema: Dict[str, Any], summary: str = "Does a thing.") -> Dict[str, Any]:
        return {
            "info": {"version": "9.9.9"},
            "paths": {
                "/api/v1/actions/thing.do": {
                    "post": {
                        "x-action": "thing.do",
                        "summary": summary,
                        "requestBody": {"content": {"application/json": {"schema": schema}}},
                    }
                }
            },
        }

    def test_an_input_field_named_like_the_per_call_keyword_is_not_generated_twice(self) -> None:
        text = generator.render(
            self.document(
                {
                    "type": "object",
                    "additionalProperties": False,
                    "required": ["idempotency_key", "name"],
                    "properties": {"idempotency_key": {"type": "string"}, "name": {"type": "string"}},
                }
            )
        )
        method = text[text.index("    def thing_do("):]
        self.assertEqual(method.count("idempotency_key: Optional[str] = None"), 1)
        self.assertNotIn("idempotency_key: str", text)
        self.assertNotIn('"idempotency_key":', text)
        compile(text, "_actions.py", "exec")

    def test_types_come_from_the_schema(self) -> None:
        text = generator.render(
            self.document(
                {
                    "type": "object",
                    "additionalProperties": False,
                    "required": ["mode"],
                    "properties": {
                        "mode": {"enum": ["a", "b"], "type": "string"},
                        "count": {"type": "integer", "description": "How many."},
                        "ratio": {"type": "number"},
                        "flag": {"type": "boolean"},
                        "names": {"type": "array", "items": {"type": "string"}},
                        "extra": {"type": "object"},
                        "any": {},
                    },
                }
            )
        )
        for expected in (
            'mode: Literal["a", "b"],',
            "count: Optional[int] = None,",
            "ratio: Optional[float] = None,",
            "flag: Optional[bool] = None,",
            "names: Optional[List[str]] = None,",
            "extra: Optional[Dict[str, Any]] = None,",
            "any: Optional[Any] = None,",
            "count: How many.",
        ):
            self.assertIn(expected, text)
        compile(text, "<generated>", "exec")

    def test_a_document_it_does_not_understand_stops_the_generator(self) -> None:
        free = {"type": "object", "additionalProperties": {}, "properties": {}}
        with self.assertRaises(generator.GenerateError):
            generator.render(self.document(free))
        with self.assertRaises(generator.GenerateError):
            generator.render(self.document({"type": "object", "additionalProperties": False}, summary=""))
        with self.assertRaises(generator.GenerateError):
            generator.render({"info": {}, "paths": {}})


if __name__ == "__main__":
    unittest.main()
