"""Finds the ``anything2mcp`` binary this wheel installed.

The wheel carries one copy of the binary, as the script ``anything2mcp``,
and a small script ``a2m`` beside it that starts it under the short name.
"""

from __future__ import annotations

import sys
import sysconfig
from pathlib import Path
from typing import Optional

PROGRAM = "anything2mcp"


def binary_path() -> Optional[Path]:
    """The ``anything2mcp`` binary this package installed, or ``None``."""
    name = PROGRAM + (".exe" if sys.platform.startswith("win") else "")
    from importlib.metadata import PackageNotFoundError, distribution

    try:
        files = distribution(PROGRAM).files or []
    except PackageNotFoundError:
        files = []
    for entry in files:
        if entry.name == name and entry.parent.name in ("bin", "Scripts"):
            found = Path(str(entry.locate())).resolve()
            if found.is_file():
                return found
    scripts = sysconfig.get_path("scripts")
    if scripts and (Path(scripts) / name).is_file():
        return Path(scripts) / name
    return None
