"""The client: sends one action to the local engine and returns its answer.

It holds no engine logic. It says hello once on its connection, posts the
action's input to the action's REST route, and hands back the response the
engine wrote, without branching on what it means.
"""

from __future__ import annotations

import http.client
import json
import os
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Mapping, Optional, Union

from . import _channel
from ._actions import Actions
from ._channel import ChannelError

#: The local API version this client speaks; the engine refuses another.
LOCAL_API = 1
#: The REST prefix of every action route.
REST_PREFIX = "/api/v1"
#: The longest answer read, in bytes.
ANSWER_LIMIT = 16 * 1024 * 1024

_HELLO_PATH = "/local/hello"
_EXIT_CLASS_HEADER = "a2m-exit-class"
_PROJECT_HEADER = "a2m-project"
_EXIT_CLASSES = ("succeeded", "failed", "malformed", "person_needed")


def _version() -> str:
    try:
        from importlib.metadata import version

        return version("anything2mcp")
    except Exception:  # not installed: a source checkout under test
        return "0.0.0"


def _encode_path(path: Path) -> str:
    """A folder's path as the project header carries it: the same
    percent-encoding the command line uses."""
    plain = set(b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~/")
    out = []
    for byte in str(path).encode("utf-8"):
        out.append(chr(byte) if byte in plain else f"%{byte:02X}")
    return "".join(out)


@dataclass(frozen=True)
class Error:
    """A failure as the engine reports it: one class and one code."""

    failure_class: str
    code: str
    message: str
    fix: str
    retry_safe: bool


@dataclass(frozen=True)
class Response:
    """One action's answer, as the engine wrote it."""

    ok: bool
    action: str
    exit_class: str
    result: Any = None
    error: Optional[Error] = None
    warnings: List[Any] = field(default_factory=list)
    next: Any = None
    body: Dict[str, Any] = field(default_factory=dict)

    @classmethod
    def of(cls, body: Dict[str, Any], exit_header: Optional[str]) -> "Response":
        error = body.get("error")
        return cls(
            ok=bool(body.get("ok")),
            action=str(body.get("action", "")),
            # The header, as the command line reads it; an answer that
            # names no known class is not one to call a success.
            exit_class=exit_header if exit_header in _EXIT_CLASSES else "failed",
            result=body.get("result"),
            error=None
            if not isinstance(error, dict)
            else Error(
                failure_class=str(error.get("class", "")),
                code=str(error.get("code", "")),
                message=str(error.get("message", "")),
                fix=str(error.get("fix", "")),
                retry_safe=bool(error.get("retry_safe", False)),
            ),
            warnings=list(body.get("warnings") or []),
            next=body.get("next"),
            body=body,
        )


class Client(Actions):
    """A connection to the local engine. Not safe to share between threads;
    open one per thread. Use it as a context manager, or call :meth:`close`."""

    def __init__(
        self,
        connection: http.client.HTTPConnection,
        name: str = "python",
        project: Optional[Union[str, Path]] = None,
    ) -> None:
        self._connection = connection
        self._label = f"sdk:{name}"
        self._project = None if project is None else Path(project).resolve()
        self.engine_version = self._hello()

    def _hello(self) -> str:
        self._connection.request(
            "GET",
            _HELLO_PATH,
            headers={
                "a2m-client-version": _version(),
                "a2m-local-api": str(LOCAL_API),
                "a2m-client-label": self._label,
            },
        )
        answer = self._connection.getresponse()
        body = self._read(answer)
        if answer.status != 200:
            error = body.get("error") or {}
            raise ChannelError(
                str(error.get("code", "hello_invalid")),
                str(error.get("message", "The engine refused the hello.")),
                str(error.get("fix", "")),
            )
        return str(body.get("engine_version", ""))

    @staticmethod
    def _read(answer: http.client.HTTPResponse) -> Dict[str, Any]:
        raw = answer.read(ANSWER_LIMIT + 1)
        if len(raw) > ANSWER_LIMIT:
            raise ChannelError("connection_lost", "The engine's answer was too long.", "Run it again.")
        try:
            body = json.loads(raw)
        except ValueError:
            body = None
        if not isinstance(body, dict):
            raise ChannelError(
                "connection_lost", "The engine's answer could not be read.", "Run it again."
            )
        return body

    def call(
        self,
        action: str,
        input: Optional[Mapping[str, Any]] = None,
        *,
        idempotency_key: Optional[str] = None,
    ) -> Response:
        """Sends one action, such as ``"engine.status"`` or ``"tool.call"``,
        with its input, and returns the engine's response, success or not.
        Raises :class:`ChannelError` only when the engine was not reached or
        its answer could not be read."""
        headers = {"content-type": "application/json"}
        if self._project is not None:
            headers[_PROJECT_HEADER] = _encode_path(self._project)
        if idempotency_key is not None:
            headers["idempotency-key"] = idempotency_key
        payload = json.dumps(dict(input or {})).encode("utf-8")
        try:
            self._connection.request(
                "POST", f"{REST_PREFIX}/actions/{action}", body=payload, headers=headers
            )
            answer = self._connection.getresponse()
        except (OSError, http.client.HTTPException) as lost:
            raise ChannelError(
                "connection_lost", "The connection to the engine was lost.", "Run it again."
            ) from lost
        body = self._read(answer)
        return Response.of(body, answer.getheader(_EXIT_CLASS_HEADER))

    def close(self) -> None:
        self._connection.close()

    def __enter__(self) -> "Client":
        return self

    def __exit__(self, *_: object) -> None:
        self.close()


def connect(
    *,
    project: Optional[Union[str, Path]] = None,
    name: str = "python",
    timeout: Optional[float] = 660.0,
    env: Optional[Mapping[str, str]] = None,
) -> Client:
    """Connects to the local engine that runs as this user. It starts
    nothing: when no engine answers it raises :class:`ChannelError` with
    the code ``engine_unreachable``. ``project`` is the folder the engine
    reads target files from; ``name`` is how this program names itself in
    the engine's audit log."""
    environment = os.environ if env is None else env
    if sys.platform.startswith("win"):
        # A named pipe, checked by its server's user; see ``_winpipe``.
        from . import _winpipe

        connection: http.client.HTTPConnection = _winpipe.connect_connection(environment, timeout)
    else:
        path = _channel.socket_path(env)
        _channel.check_owner(path)
        connection = _channel.UnixConnection(path, timeout=timeout)
        try:
            connection.connect()
        except OSError:
            connection.close()
            raise ChannelError(
                "engine_unreachable",
                "No engine is running.",
                "Start one with `anything2mcp serve`, or run any `anything2mcp` command.",
            ) from None
    try:
        return Client(connection, name=name, project=project)
    except (OSError, http.client.HTTPException) as lost:
        connection.close()
        raise ChannelError(
            "connection_lost", "The engine closed the connection.", "Run it again."
        ) from lost
    except ChannelError:
        connection.close()
        raise
