"""Finds the ``anything2mcp`` binary this wheel installed.

The wheel carries one copy of the binary, as the script ``anything2mcp``,
and a small script ``a2m`` beside it that starts it under the short name.

Only the distribution that installed this very module is trusted, and only
a binary in the scripts folder of the installation scheme it was installed
into: never the first ``anything2mcp`` distribution on ``sys.path``, which a
project folder can put there, never ``PATH``, and never an environment
variable (PR-2 of the third audit, decision 0194). A caller who wants
another program passes it explicitly.
"""

from __future__ import annotations

import sys
import sysconfig
from pathlib import Path
from typing import List, Optional

PROGRAM = "anything2mcp"


def _resolved(path: object) -> Optional[Path]:
    try:
        return Path(str(path)).resolve()
    except (OSError, RuntimeError):
        return None


def _own_distribution():  # type: ignore[no-untyped-def]
    """The installed distribution whose record holds this module's own file."""
    from importlib.metadata import distributions

    here = _resolved(__file__)
    if here is None:
        return None
    for dist in distributions(name=PROGRAM):
        for entry in dist.files or []:
            if entry.name != Path(__file__).name or entry.parent.name != PROGRAM:
                continue
            if _resolved(entry.locate()) == here:
                return dist
    return None


def _scripts_folders(dist) -> List[Path]:  # type: ignore[no-untyped-def]
    """The scripts folders of the schemes whose library folder is where
    ``dist`` was installed."""
    location = _resolved(dist.locate_file(""))
    folders: List[Path] = []
    if location is None:
        return folders
    for scheme in sysconfig.get_scheme_names():
        try:
            paths = sysconfig.get_paths(scheme)
        except KeyError:
            continue
        libraries = {_resolved(paths[key]) for key in ("purelib", "platlib") if key in paths}
        if location in libraries and "scripts" in paths:
            scripts = _resolved(paths["scripts"])
            if scripts is not None and scripts not in folders:
                folders.append(scripts)
    return folders


def binary_path() -> Optional[Path]:
    """The ``anything2mcp`` binary this package installed, or ``None``."""
    name = PROGRAM + (".exe" if sys.platform.startswith("win") else "")
    dist = _own_distribution()
    if dist is None:
        return None
    folders = _scripts_folders(dist)
    for entry in dist.files or []:
        if entry.name != name or entry.parent.name not in ("bin", "Scripts"):
            continue
        found = _resolved(entry.locate())
        if found is not None and found.parent in folders and found.is_file():
            return found
    return None
