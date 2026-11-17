#!/usr/bin/env python3
"""Saves the OpenAPI document of a running engine as ``tools/openapi.json``.

Start an engine first (``anything2mcp serve``, with ``A2M_HOME`` set to a
scratch directory if you do not want your own), then, from
``bindings/python``::

    python3 tools/fetch_openapi.py

The document is written with sorted keys and one-space indentation so that
a change in the engine shows as a small difference. Run the generators
afterwards: ``tools/generate_actions.py`` here, and
``../npm/client/tools/generate.js``. Standard library only.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "src"))

from anything2mcp import _client  # noqa: E402

OUT = HERE / "openapi.json"


def main() -> int:
    client = _client.connect(name="fetch-openapi")
    try:
        connection = client._connection  # the same connection the client says hello on
        connection.request("GET", f"{_client.REST_PREFIX}/openapi.json")
        answer = connection.getresponse()
        raw = answer.read()
        if answer.status != 200:
            sys.stderr.write(f"The engine answered {answer.status}.\n")
            return 1
    finally:
        client.close()
    document = json.loads(raw)
    OUT.write_text(
        json.dumps(document, indent=1, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
