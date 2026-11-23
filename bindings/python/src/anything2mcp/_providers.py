"""Where provider packages are looked for (package F3).

A provider package is a pip package that declares an entry in the group
``anything2mcp.providers``. The engine, not this module, finds them: it
reads the ``entry_points.txt`` of each installed package in the folders
below and the manifest each entry names, as text. It runs no package code
and imports nothing, and a package it finds is listed as present and not
enabled until a person enables it.

This module holds no engine logic. It only says which folders hold this
interpreter's installed packages, so that the search can be stated openly
and given to the engine as it is.
"""

from __future__ import annotations

import os
import sysconfig
from typing import List

ENTRY_POINT_GROUP = "anything2mcp.providers"


def provider_site_folders() -> List[str]:
    """The folders of this interpreter's installed packages, each once.

    Only the folders of this interpreter's own scheme (``purelib`` and
    ``platlib``) that exist: not the working folder, not ``PYTHONPATH``,
    and not the user's other Python installations.
    """
    folders: List[str] = []
    for key in ("purelib", "platlib"):
        folder = sysconfig.get_paths().get(key)
        if folder and os.path.isdir(folder) and folder not in folders:
            folders.append(folder)
    return folders
