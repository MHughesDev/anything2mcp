"""Where the local engine's channel is, and an HTTP connection over it.

The rules for the place are the engine's own (``Paths`` in the
``a2m-actions`` crate and the channel of ``a2m-local``), restated here
because a client must find the socket before it can ask anything. A test
in ``tests/`` holds them to the engine's answer when a built binary is
given.

On macOS and Linux the channel is a Unix socket, found and checked here. On
Windows it is a named pipe, found and checked in :mod:`_winpipe`.
"""

from __future__ import annotations

import http.client
import os
import socket
import stat
import sys
from pathlib import Path, PureWindowsPath
from typing import Mapping, Optional

APP_FOLDER = "anything2mcp"
HOME_VARIABLE = "A2M_HOME"
SOCKET_NAME = "engine.sock"


class ChannelError(Exception):
    """The engine could not be reached. ``code`` is the engine's failure code."""

    def __init__(self, code: str, message: str, fix: str) -> None:
        super().__init__(f"{message} {fix}")
        self.code = code
        self.message = message
        self.fix = fix


def _dir(env: Mapping[str, str], name: str) -> Optional[Path]:
    value = env.get(name, "")
    if not value:
        return None
    path = Path(value)
    return path if path.is_absolute() else None


def runtime_dir(env: Optional[Mapping[str, str]] = None, platform: Optional[str] = None) -> Path:
    """The engine's runtime directory, by the engine's rules."""
    env = os.environ if env is None else env
    platform = sys.platform if platform is None else platform
    home = env.get(HOME_VARIABLE, "")
    if home:
        if not Path(home).is_absolute():
            raise ChannelError(
                "path_not_allowed",
                f"`{HOME_VARIABLE}` is not an absolute path.",
                f"Set `{HOME_VARIABLE}` to the full path of a directory.",
            )
        return Path(home) / "run"
    if platform.startswith("win"):
        # Shown for the user's information; the pipe's name is made from the
        # text of this path, in `_winpipe`, and not from this object.
        from . import _winpipe

        return PureWindowsPath(_winpipe.runtime_dir_text(env))  # type: ignore[return-value]
    user_home = _dir(env, "HOME")
    if user_home is None:
        raise ChannelError(
            "path_not_allowed",
            "The home directory is not known.",
            f"Set `{HOME_VARIABLE}` to the full path of a directory for the engine.",
        )
    if platform == "darwin":
        tmp = _dir(env, "TMPDIR")
        if tmp is not None:
            return tmp / APP_FOLDER
        return user_home / "Library" / "Application Support" / APP_FOLDER / "run"
    run = _dir(env, "XDG_RUNTIME_DIR")
    if run is not None:
        return run / APP_FOLDER
    data = _dir(env, "XDG_DATA_HOME") or (user_home / ".local" / "share")
    return data / APP_FOLDER / "run"


def socket_path(env: Optional[Mapping[str, str]] = None, platform: Optional[str] = None) -> Path:
    """The engine's socket."""
    return runtime_dir(env, platform) / SOCKET_NAME


def check_owner(path: Path) -> None:
    """The client's check of the engine: the socket and its directory are
    this user's, and nobody else can change the directory. Without it
    another program could make the socket first and answer in the
    engine's place."""
    not_running = ChannelError(
        "engine_unreachable",
        "No engine is running.",
        "Start one with `anything2mcp serve`, or run any `anything2mcp` command.",
    )
    try:
        folder = os.lstat(path.parent)
        found = os.lstat(path)
    except FileNotFoundError:
        raise not_running from None
    me = os.getuid()  # type: ignore[attr-defined, unused-ignore]
    ours = (
        stat.S_ISDIR(folder.st_mode)
        and folder.st_uid == me
        and folder.st_mode & 0o077 == 0
        and stat.S_ISSOCK(found.st_mode)
        and found.st_uid == me
    )
    if not ours:
        raise ChannelError(
            "peer_not_allowed",
            "What listens at the engine's place does not belong to you.",
            "Check who owns the engine's runtime directory, remove what is not yours to trust, "
            "and start the engine again.",
        )


class UnixConnection(http.client.HTTPConnection):
    """HTTP/1.1 over a Unix domain socket. The host name is only what the
    ``Host`` header says; nothing is looked up."""

    def __init__(self, path: Path, timeout: Optional[float] = None) -> None:
        super().__init__("localhost", timeout=timeout)
        self._path = str(path)

    def connect(self) -> None:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)  # type: ignore[attr-defined, unused-ignore]
        try:
            if self.timeout is not None:
                sock.settimeout(self.timeout)
            sock.connect(self._path)
        except OSError:
            sock.close()
            raise
        self.sock = sock
