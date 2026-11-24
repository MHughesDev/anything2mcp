#!/usr/bin/env python3
"""Saves the OpenAPI document of an engine as ``tools/openapi.json``, or
checks that the saved copy is what the engine serves.

From ``bindings/python``::

    python3 tools/fetch_openapi.py --binary PROGRAM          # save the document
    python3 tools/fetch_openapi.py --binary PROGRAM --check  # fail if it differs

With ``--binary`` the script starts an engine of that program in a scratch
home of its own (``A2M_HOME`` set to a new temporary folder), asks it for
the document, stops it, and removes the folder, so it neither needs nor
touches your own engine. Without ``--binary`` it asks the engine already
running as you, and starts one with the installed program when none runs.

The document is written with sorted keys and one-space indentation so that
a change in the engine shows as a small difference. ``--check`` writes
nothing and fails when the saved copy differs from the one the engine
serves; CI runs it against the program it built, beside the generators'
own ``--check``, so the saved copy cannot drift from the product. After
saving, run the generators: ``tools/generate_actions.py`` here, and
``../npm/scripts/generate-actions.js``. Standard library only.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import List, Mapping, Optional

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "src"))

from anything2mcp import _client  # noqa: E402

OUT = HERE / "openapi.json"
#: The passphrase the scratch engine's vault takes its root key from, on a
#: machine with no key store. The scratch home holds no secret.
SCRATCH_PASSPHRASE = "scratch-for-the-openapi-document-only"


def served(env: Optional[Mapping[str, str]], binary: Optional[Path]) -> str:
    """The document the engine serves, as the saved copy is written."""
    client = _client.connect(name="fetch-openapi", env=env, binary=binary)
    try:
        connection = client._connection  # the same connection the client says hello on
        connection.request("GET", f"{_client.REST_PREFIX}/openapi.json")
        answer = connection.getresponse()
        raw = answer.read()
        if answer.status != 200:
            raise SystemExit(f"The engine answered {answer.status}.")
    finally:
        client.close()
    document = json.loads(raw)
    return json.dumps(document, indent=1, sort_keys=True, ensure_ascii=False) + "\n"


def served_by_scratch_engine(binary: Path) -> str:
    """Starts an engine of ``binary`` in a scratch home, takes its document,
    and stops it."""
    home = Path(tempfile.mkdtemp(prefix="a2m-openapi-"))
    env = {
        **os.environ,
        "A2M_HOME": str(home),
        "A2M_VAULT_PASSPHRASE": SCRATCH_PASSPHRASE,
    }
    try:
        return served(env, binary)
    finally:
        subprocess.run(
            [str(binary), "engine", "stop"],
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        shutil.rmtree(home, ignore_errors=True)


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--binary",
        type=Path,
        help="start an engine of this program in a scratch home, and ask it",
    )
    parser.add_argument("--out", type=Path, default=OUT)
    parser.add_argument(
        "--check", action="store_true", help="fail when the saved copy differs from the served one"
    )
    args = parser.parse_args(argv)
    if args.binary is not None:
        text = served_by_scratch_engine(args.binary.resolve())
    else:
        text = served(None, None)
    if args.check:
        current = args.out.read_text(encoding="utf-8") if args.out.exists() else None
        if current != text:
            sys.stderr.write(
                f"{args.out} differs from the document the engine serves. Run "
                "`python3 tools/fetch_openapi.py --binary <the built anything2mcp>` from "
                "bindings/python, then both generators, and commit the three.\n"
            )
            return 1
        return 0
    args.out.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
