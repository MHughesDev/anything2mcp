"""The client: sends one action to the local engine and returns its answer.

It holds no engine logic. It says hello once on its connection, posts the
action's input to the action's REST route, and hands back the response the
engine wrote, without branching on what it means.
"""

from __future__ import annotations

import http.client
import json
import os
import re
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
#: The form of an action's name, ``noun.verb``. Nothing else goes into a
#: request's path: no input may change where a request goes.
_ACTION_NAME = re.compile(r"[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*\Z")


def _closed() -> ChannelError:
    """The connection is closed and nothing was sent."""
    return ChannelError(
        "connection_lost",
        "The connection to the engine is closed, so nothing was sent.",
        "Connect again with `anything2mcp.connect()`, then make the call again.",
    )


def _answer_lost(why: str) -> ChannelError:
    """The request may have reached the engine, and its answer is lost."""
    return ChannelError(
        "answer_lost",
        f"{why} The engine may have had the call: it may have started a run, and that run may have written.",
        "Do not make the call again: a second call may start a second run and write twice. "
        "Find this call's run first in your recent runs (`run.list`), or tell the person that the "
        "answer was lost so that they can check.",
        retry_safe=False,
    )


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
        body = self._read(answer, lost=lambda why: ChannelError("connection_lost", why, "Connect again."))
        if answer.status != 200:
            error = body.get("error") or {}
            raise ChannelError(
                str(error.get("code", "hello_invalid")),
                str(error.get("message", "The engine refused the hello.")),
                str(error.get("fix", "")),
            )
        return str(body.get("engine_version", ""))

    @staticmethod
    def _read(answer: http.client.HTTPResponse, lost: Any = _answer_lost) -> Dict[str, Any]:
        """The answer's body as one JSON object. ``lost`` makes the error
        for an answer that breaks off, runs too long, or is not one."""
        try:
            raw = answer.read(ANSWER_LIMIT + 1)
        except (OSError, http.client.HTTPException):
            raise lost("The engine's answer broke off.") from None
        if len(raw) > ANSWER_LIMIT:
            raise lost("The engine's answer was too long.")
        try:
            body = json.loads(raw)
        except ValueError:
            body = None
        if not isinstance(body, dict):
            raise lost("The engine's answer could not be read.")
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
        its answer could not be read. Once the request may have reached the
        engine, a lost answer is ``answer_lost`` with ``retry_safe`` false:
        the call may have started a run that writes.

        ``idempotency_key`` is sent as the ``Idempotency-Key`` header. The
        engine does not yet tie a second call with the same key to the
        first one's run, so it does not make a repeated call safe."""
        if not isinstance(action, str) or not _ACTION_NAME.match(action):
            raise ValueError("An action's name is `noun.verb`, in lowercase letters, digits, and underscores.")
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
        except (http.client.NotConnected, http.client.ImproperConnectionState):
            # Raised before a byte is written: the connection had closed,
            # or an earlier answer on it was lost.
            self._connection.close()
            raise _closed() from None
        except (OSError, http.client.HTTPException) as lost:
            # Some of the request may have been written.
            self._connection.close()
            raise _answer_lost("The connection to the engine was lost while the call was sent.") from lost
        try:
            answer = self._connection.getresponse()
            body = self._read(answer)
        except (OSError, http.client.HTTPException) as lost:
            self._connection.close()
            raise _answer_lost("The connection to the engine was lost before it answered.") from lost
        except ChannelError:
            self._connection.close()
            raise
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
