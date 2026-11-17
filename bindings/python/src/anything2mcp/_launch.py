"""Finds the binary this wheel installed, and starts it under the short name.

The wheel carries one copy of the binary, as the script ``anything2mcp``.
The short name ``a2m`` is a console script that runs this module, which
replaces itself with that binary (on Windows, where a process cannot
replace itself, it runs the binary and passes its exit code on).
"""

from __future__ import annotations

import os
import subprocess
import sys
import sysconfig
from pathlib import Path
from typing import List, Optional

PROGRAM = "anything2mcp"


def binary_path() -> Optional[Path]:
    """The ``anything2mcp`` binary this package installed, or ``None``."""
    name = PROGRAM + (".exe" if sys.platform.startswith("win") else "")
    try:
        from importlib.metadata import PackageNotFoundError, distribution

        try:
            files = distribution(PROGRAM).files or []
        except PackageNotFoundError:
            files = []
        for entry in files:
            if entry.name == name and entry.parent.name in ("bin", "Scripts"):
                found = Path(str(entry.locate()))
                if found.is_file():
                    return found
    except ImportError:  # pragma: no cover - the standard library has it from 3.8
        pass
    scripts = sysconfig.get_path("scripts")
    if scripts:
        found = Path(scripts) / name
        if found.is_file():
            return found
    return None


def main(argv: Optional[List[str]] = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    binary = binary_path()
    if binary is None:
        sys.stderr.write(
            f"error: the `{PROGRAM}` program that this package installs was not found.\n"
            f"fix: reinstall it with `pip install --force-reinstall {PROGRAM}`.\n"
        )
        return 4
    if sys.platform.startswith("win"):
        return subprocess.call([str(binary), *args])
    os.execv(str(binary), [str(binary), *args])
    return 1  # not reached: execv does not return


if __name__ == "__main__":
    sys.exit(main())
