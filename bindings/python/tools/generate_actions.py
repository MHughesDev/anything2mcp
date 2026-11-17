#!/usr/bin/env python3
"""Writes the Python client's typed methods from the engine's OpenAPI document.

The engine serves the document at ``GET /api/v1/openapi.json``; it is made
from the action registry, so a method written from it cannot name an action
the engine lacks. ``tools/openapi.json`` is a saved copy (see
``tools/fetch_openapi.py``). This script reads it and writes
``src/anything2mcp/_actions.py``: one method per action, named as the action
names it with the dot as an underscore (``tool.call`` is ``tool_call``),
with keyword-only, typed inputs taken from the request schema and a
docstring taken from the operation's summary.

A generated method names its action and passes its input. It decides
nothing and branches on nothing the engine answers.

Standard library only. From ``bindings/python``::

    python3 tools/generate_actions.py            # write the module
    python3 tools/generate_actions.py --check    # fail if the module differs
"""

from __future__ import annotations

import argparse
import json
import keyword
import sys
import textwrap
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

HERE = Path(__file__).resolve().parent
DEFAULT_DOCUMENT = HERE / "openapi.json"
DEFAULT_OUTPUT = HERE.parent / "src" / "anything2mcp" / "_actions.py"

ACTION_ROUTE = "/api/v1/actions/"
WIDTH = 88


class GenerateError(Exception):
    """The document is not one this generator understands. It stops rather
    than guess what an input is."""


def method_name(action: str) -> str:
    """``tool.call`` is ``tool_call``: noun and verb, joined by an underscore."""
    name = action.replace(".", "_")
    if not name.isidentifier() or keyword.iskeyword(name):
        raise GenerateError(f"action `{action}` has no valid method name")
    return name


def parameter_name(field: str) -> str:
    """The Python name of an input field; a reserved word gets a trailing underscore."""
    if not field.isidentifier():
        raise GenerateError(f"input field `{field}` is not a valid name")
    return field + "_" if keyword.iskeyword(field) else field


def py_type(schema: Any) -> str:
    """The annotation of one schema. Anything this does not know is ``Any``,
    which is honest: the engine, not the client, checks a value."""
    if not isinstance(schema, dict):
        return "Any"
    if "enum" in schema:
        values = schema["enum"]
        if not values or not all(isinstance(v, (str, int, bool)) for v in values):
            raise GenerateError(f"unsupported enum {values!r}")
        return "Literal[" + ", ".join(json.dumps(v) for v in values) + "]"
    kind = schema.get("type")
    if isinstance(kind, list):
        parts = [py_type({**schema, "type": k}) for k in kind if k != "null"]
        text = parts[0] if len(parts) == 1 else "Union[" + ", ".join(parts) + "]"
        return f"Optional[{text}]" if "null" in kind else text
    if kind == "string":
        return "str"
    if kind == "integer":
        return "int"
    if kind == "number":
        return "float"
    if kind == "boolean":
        return "bool"
    if kind == "array":
        return f"List[{py_type(schema.get('items'))}]"
    if kind == "object":
        return "Dict[str, Any]"
    return "Any"


def request_schema(operation: Dict[str, Any]) -> Tuple[List[Tuple[str, Any]], List[str]]:
    """The input fields of an operation (sorted by name) and the required ones."""
    body = operation.get("requestBody")
    if not body:
        return [], []
    schema = body["content"]["application/json"]["schema"]
    if schema.get("type") != "object":
        raise GenerateError("a request body that is not an object")
    if schema.get("additionalProperties", False) not in (False,):
        raise GenerateError("a request body that takes fields it does not name")
    properties = schema.get("properties", {})
    return sorted(properties.items()), list(schema.get("required", []))


def actions_of(document: Dict[str, Any]) -> List[Tuple[str, Dict[str, Any]]]:
    """Every action with its POST operation on ``/api/v1/actions/<action>``,
    sorted by name. That route is the one the client's ``call`` uses."""
    found: Dict[str, Dict[str, Any]] = {}
    for path, item in document["paths"].items():
        if not path.startswith(ACTION_ROUTE) or "post" not in item:
            continue
        operation = item["post"]
        action = operation.get("x-action")
        if action != path[len(ACTION_ROUTE):]:
            raise GenerateError(f"route {path} names action {action!r}")
        found[action] = operation
    if not found:
        raise GenerateError("the document has no action routes")
    names = [method_name(action) for action in found]
    if len(set(names)) != len(names):
        raise GenerateError("two actions have the same method name")
    return sorted(found.items())


def escape_doc(text: str) -> str:
    return text.replace("\\", "\\\\").replace('"""', '\\"\\"\\"')


def render_method(action: str, operation: Dict[str, Any]) -> str:
    fields, required = request_schema(operation)
    unknown = [r for r in required if r not in dict(fields)]
    if unknown:
        raise GenerateError(f"`{action}` requires fields it does not define: {unknown}")
    summary = " ".join(str(operation.get("summary", "")).split())
    if not summary:
        raise GenerateError(f"`{action}` has no summary")
    params: List[str] = []
    pairs: List[str] = []
    notes: List[str] = []
    ordered = [f for f in fields if f[0] in required] + [f for f in fields if f[0] not in required]
    for field, schema in ordered:
        name = parameter_name(field)
        annotation = py_type(schema)
        if field in required:
            params.append(f"{name}: {annotation}")
        else:
            params.append(f"{name}: Optional[{annotation}] = None")
        pairs.append(f'"{field}": {name}')
        described = " ".join(str(schema.get("description", "")).split()) if isinstance(schema, dict) else ""
        if described:
            notes.append(f"{name}: {described}")
    params.append("idempotency_key: Optional[str] = None")
    head = f"    def {method_name(action)}(self, *, " + ", ".join(params) + ') -> "Response":'
    if len(head) > WIDTH:
        head = (
            f"    def {method_name(action)}(\n        self,\n        *,\n"
            + "".join(f"        {p},\n" for p in params)
            + '    ) -> "Response":'
        )
    doc = textwrap.fill(
        escape_doc(summary), width=WIDTH - 8, initial_indent="        ", subsequent_indent="        "
    )
    lines = [head, '        """' + doc.strip()]
    lines.append("")
    lines.append(f"        The engine action is ``{action}``.")
    if notes:
        lines.append("")
        lines.append("        Inputs:")
        for note in notes:
            lines.append(
                textwrap.fill(
                    escape_doc(note),
                    width=WIDTH - 8,
                    initial_indent="            ",
                    subsequent_indent="                ",
                )
            )
    lines.append('        """')
    call = f'        return self._action("{action}", {{{", ".join(pairs)}}}, idempotency_key)'
    if len(call) > WIDTH:
        call = (
            f'        return self._action(\n            "{action}",\n            {{\n'
            + "".join(f"                {pair},\n" for pair in pairs)
            + "            },\n            idempotency_key,\n        )"
        )
    lines.append(call)
    return "\n".join(lines)


HEADER = '''"""Typed methods, one per action of the engine. GENERATED: do not edit.

Written by ``tools/generate_actions.py`` from ``tools/openapi.json``, the
OpenAPI document the engine serves at ``/api/v1/openapi.json`` (engine
version {version}). A test fails when this file differs from what the
generator writes from that document. To change a method, change the action
in the engine, save the new document, and run the generator.

Each method names its action and passes its input. It returns the engine's
:class:`Response` as :meth:`Client.call` does, and branches on nothing.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, Dict, List, Literal, Mapping, Optional, Union

if TYPE_CHECKING:
    from ._client import Response

__all__ = ["Actions", "ACTIONS"]

#: Every action this module has a method for, in the order of the document.
ACTIONS = ({names})


class Actions:
    """The typed methods. :class:`Client` inherits them."""

    def call(
        self,
        action: str,
        input: Optional[Mapping[str, Any]] = None,
        *,
        idempotency_key: Optional[str] = None,
    ) -> "Response":  # pragma: no cover - the client supplies it
        raise NotImplementedError

    def _action(
        self, action: str, fields: Dict[str, Any], idempotency_key: Optional[str]
    ) -> "Response":
        # An input left out is left out of the request; nothing else is decided here.
        given = {{key: value for key, value in fields.items() if value is not None}}
        return self.call(action, given, idempotency_key=idempotency_key)
'''


def render(document: Dict[str, Any]) -> str:
    actions = actions_of(document)
    names = "\n    " + ",\n    ".join(json.dumps(a) for a, _ in actions) + ",\n"
    head = HEADER.format(version=document.get("info", {}).get("version", "unknown"), names=names)
    body = "\n\n".join(render_method(a, op) for a, op in actions)
    return head + "\n" + body + "\n"


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--document", type=Path, default=DEFAULT_DOCUMENT)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--check", action="store_true", help="fail when the file differs")
    args = parser.parse_args(argv)
    text = render(json.loads(args.document.read_text(encoding="utf-8")))
    if args.check:
        current = args.out.read_text(encoding="utf-8") if args.out.exists() else None
        if current != text:
            sys.stderr.write(f"{args.out} differs from what the generator writes; run it.\n")
            return 1
        return 0
    args.out.write_text(text, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
