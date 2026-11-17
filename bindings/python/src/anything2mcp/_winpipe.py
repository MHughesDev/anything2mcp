"""The channel on Windows: a named pipe, and HTTP/1.1 over it.

What the engine does (``a2m-local``, the Windows part of ``os``): the pipe
is named ``\\\\.\\pipe\\anything2mcp.<SID>.<tag>``, where ``<SID>`` is the text
of the user's security identifier and ``<tag>`` is the first 16 hex digits of
the SHA-256 of the runtime directory's text. The engine makes the pipe with an
access list that names only the user and the first-instance flag. The client's
part, done here, is to open the pipe and then check that the process serving
it runs as the same user, before it sends a byte.

**Why ctypes and not ``_winapi``.** ``_winapi`` is private, and it offers
``CreateFile``, ``ReadFile``, ``WriteFile`` and ``WaitNamedPipe`` but not the
two things the check needs: the process that serves a pipe
(``GetNamedPipeServerProcessId``) and the user of a process token
(``OpenProcessToken``, ``GetTokenInformation``, ``ConvertSidToStringSidW``).
``ctypes`` is public, in the standard library on Windows, and offers all of
them, so one module does everything. It is imported only on Windows, and only
when a pipe is opened.

**What it offers on Windows, and what it does not.** The check is the same one
the engine's own client makes: the server's process token names the user's
identifier. A pipe made by another user, or by a process that cannot be opened
for query (such as a service), is refused with ``peer_not_allowed``. A read on
the pipe has no timeout: the ``timeout`` of ``connect`` is not honoured on
Windows. The pipe is opened at the *identification* impersonation level, so
the engine cannot act as the client.

Everything that does not call the operating system is here as plain Python and
is tested on every system: the pipe's name, the framing of the stream, and the
choice of what to do when the pipe is busy, missing, or not the user's. The
calls into the operating system sit behind :class:`Win32`, which a test
replaces.
"""

from __future__ import annotations

import hashlib
import http.client
import io
import ntpath
import re
from typing import Callable, Mapping, Optional

from ._channel import APP_FOLDER, HOME_VARIABLE, ChannelError

#: How many times the client waits for a free instance of a busy pipe, and
#: for how long each time. The operating system wakes the wait as soon as an
#: instance is free; the limit only bounds a pipe that never frees.
BUSY_TRIES = 40
BUSY_WAIT_MS = 100

#: Windows error numbers the client tells apart.
ERROR_FILE_NOT_FOUND = 2
ERROR_BROKEN_PIPE = 109
ERROR_PIPE_BUSY = 231

_ABSOLUTE = re.compile(r"^(?:[A-Za-z]:[\\/]|[\\/]{2}[^\\/]+[\\/]+[^\\/]+)")


class PipeOpenError(Exception):
    """The pipe could not be opened; ``code`` is the Windows error number."""

    def __init__(self, code: int) -> None:
        super().__init__(f"Windows error {code}")
        self.code = code


def _absolute_dir(env: Mapping[str, str], name: str) -> Optional[str]:
    value = env.get(name, "")
    return value if value and _ABSOLUTE.match(value) else None


def runtime_dir_text(env: Mapping[str, str]) -> str:
    """The engine's runtime directory as text, by ``Paths::resolve`` for
    Windows. The pipe's tag is a digest of this text exactly as the engine
    holds it, so it is joined with Windows rules and not normalised."""
    home = env.get(HOME_VARIABLE, "")
    if home:
        if not _ABSOLUTE.match(home):
            raise ChannelError(
                "path_not_allowed",
                f"`{HOME_VARIABLE}` is not an absolute path.",
                f"Set `{HOME_VARIABLE}` to the full path of a directory.",
            )
        return ntpath.join(home, "run")
    local = _absolute_dir(env, "LOCALAPPDATA")
    if local is None:
        raise ChannelError(
            "path_not_allowed",
            "The local application data directory is not known.",
            f"Set `{HOME_VARIABLE}` to the full path of a directory for the engine.",
        )
    return ntpath.join(local, APP_FOLDER, "run")


def pipe_name(runtime_dir: str, sid: str) -> str:
    """The pipe's name, as ``Place::new`` and ``place`` in ``a2m-local`` make it."""
    tag = hashlib.sha256(runtime_dir.encode("utf-8")).hexdigest()[:16]
    return f"\\\\.\\pipe\\anything2mcp.{sid}.{tag}"


class PipeStream(io.RawIOBase):
    """A byte stream over three calls: read some bytes, write some bytes,
    close. ``read(size)`` returns ``b""`` at the end of the stream."""

    def __init__(
        self,
        read: Callable[[int], bytes],
        write: Callable[[bytes], int],
        close: Callable[[], None],
    ) -> None:
        super().__init__()
        self._read = read
        self._write = write
        self._close = close

    def readable(self) -> bool:
        return True

    def writable(self) -> bool:
        return True

    def readinto(self, buffer) -> int:  # type: ignore[no-untyped-def]
        data = self._read(len(buffer))
        buffer[: len(data)] = data
        return len(data)

    def write(self, data) -> int:  # type: ignore[no-untyped-def]
        view = memoryview(data).cast("B")
        sent = 0
        while sent < len(view):
            written = self._write(bytes(view[sent:]))
            if written <= 0:
                raise OSError("the pipe took no bytes")
            sent += written
        return sent

    def close(self) -> None:
        if not self.closed:
            try:
                self._close()
            finally:
                super().close()


class _ReadView(io.RawIOBase):
    """What ``makefile`` hands out: reads the stream, and closing it leaves
    the stream open, as closing a file made from a socket leaves the socket."""

    def __init__(self, stream: PipeStream) -> None:
        super().__init__()
        self._stream = stream

    def readable(self) -> bool:
        return True

    def readinto(self, buffer) -> int:  # type: ignore[no-untyped-def]
        return self._stream.readinto(buffer)


class PipeSocket:
    """The few calls of a socket that ``http.client`` makes."""

    def __init__(self, stream: PipeStream) -> None:
        self._stream = stream

    def sendall(self, data) -> None:  # type: ignore[no-untyped-def]
        self._stream.write(data)

    def makefile(self, mode: str = "rb", buffering: Optional[int] = None) -> io.BufferedReader:
        if "b" not in mode or "r" not in mode:
            raise ValueError("a pipe's file is read as bytes")
        return io.BufferedReader(_ReadView(self._stream))

    def settimeout(self, _seconds: Optional[float]) -> None:
        """Accepted and ignored: a read on the pipe has no timeout."""

    def shutdown(self, _how: int) -> None:
        pass

    def close(self) -> None:
        self._stream.close()


class PipeConnection(http.client.HTTPConnection):
    """HTTP/1.1 over a named pipe. The host name is only what the ``Host``
    header says; nothing is looked up."""

    def __init__(self, open_pipe: Callable[[], PipeSocket], timeout: Optional[float] = None) -> None:
        super().__init__("localhost", timeout=timeout)
        self._open_pipe = open_pipe

    def connect(self) -> None:
        self.sock = self._open_pipe()  # type: ignore[assignment]


class Win32:
    """The calls into the operating system. Windows only; every method is
    imported and run lazily so that this module loads everywhere."""

    def __init__(self) -> None:
        import ctypes
        from ctypes import wintypes

        self._ctypes = ctypes
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)  # type: ignore[attr-defined]
        advapi = ctypes.WinDLL("advapi32", use_last_error=True)  # type: ignore[attr-defined]
        handle, void = ctypes.c_void_p, ctypes.c_void_p
        self._invalid = (1 << (8 * ctypes.sizeof(void))) - 1

        def sign(function, result, *arguments):  # type: ignore[no-untyped-def]
            function.restype = result
            function.argtypes = list(arguments)
            return function

        dword_p = ctypes.POINTER(wintypes.DWORD)
        self._create = sign(
            kernel.CreateFileW, handle, wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD,
            void, wintypes.DWORD, wintypes.DWORD, handle,
        )
        self._read = sign(kernel.ReadFile, wintypes.BOOL, handle, void, wintypes.DWORD, dword_p, void)
        self._write = sign(kernel.WriteFile, wintypes.BOOL, handle, void, wintypes.DWORD, dword_p, void)
        self._close = sign(kernel.CloseHandle, wintypes.BOOL, handle)
        self._wait = sign(kernel.WaitNamedPipeW, wintypes.BOOL, wintypes.LPCWSTR, wintypes.DWORD)
        self._server_pid = sign(kernel.GetNamedPipeServerProcessId, wintypes.BOOL, handle, dword_p)
        self._open_process = sign(kernel.OpenProcess, handle, wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
        self._current_process = sign(kernel.GetCurrentProcess, handle)
        self._free = sign(kernel.LocalFree, handle, handle)
        self._open_token = sign(
            advapi.OpenProcessToken, wintypes.BOOL, handle, wintypes.DWORD, ctypes.POINTER(handle)
        )
        self._token_info = sign(
            advapi.GetTokenInformation, wintypes.BOOL, handle, ctypes.c_int, void, wintypes.DWORD, dword_p
        )
        self._sid_text = sign(
            advapi.ConvertSidToStringSidW, wintypes.BOOL, void, ctypes.POINTER(void)
        )

    # Values from the Windows headers.
    _GENERIC_READ_WRITE = 0x80000000 | 0x40000000
    _OPEN_EXISTING = 3
    # Identification level: the engine can learn who the client is and
    # cannot act as the client.
    _SQOS_IDENTIFICATION = 0x00100000 | 0x00010000
    _TOKEN_QUERY = 0x0008
    _TOKEN_USER = 1
    _PROCESS_QUERY_LIMITED_INFORMATION = 0x1000

    def _fail(self) -> OSError:
        return OSError(f"Windows error {self._ctypes.get_last_error()}")

    def open(self, name: str) -> int:
        handle = self._create(
            name, self._GENERIC_READ_WRITE, 0, None, self._OPEN_EXISTING, self._SQOS_IDENTIFICATION, None
        )
        if handle is None or handle == self._invalid:
            raise PipeOpenError(self._ctypes.get_last_error())
        return handle

    def wait(self, name: str, milliseconds: int) -> bool:
        return bool(self._wait(name, milliseconds))

    def read(self, handle: int, size: int) -> bytes:
        ctypes = self._ctypes
        buffer = ctypes.create_string_buffer(size)
        got = ctypes.c_ulong(0)
        if not self._read(handle, buffer, size, ctypes.byref(got), None):
            if ctypes.get_last_error() == ERROR_BROKEN_PIPE:
                return b""
            raise self._fail()
        return buffer.raw[: got.value]

    def write(self, handle: int, data: bytes) -> int:
        ctypes = self._ctypes
        put = ctypes.c_ulong(0)
        if not self._write(handle, data, len(data), ctypes.byref(put), None):
            raise self._fail()
        return int(put.value)

    def close(self, handle: int) -> None:
        self._close(handle)

    def _sid_of_token(self, token: int) -> str:
        ctypes = self._ctypes
        needed = ctypes.c_ulong(0)
        self._token_info(token, self._TOKEN_USER, None, 0, ctypes.byref(needed))
        if needed.value == 0:
            raise self._fail()
        buffer = ctypes.create_string_buffer(needed.value)
        if not self._token_info(token, self._TOKEN_USER, buffer, needed.value, ctypes.byref(needed)):
            raise self._fail()
        # The buffer starts with a TOKEN_USER, whose first field is the SID's address.
        sid = ctypes.cast(buffer, ctypes.POINTER(ctypes.c_void_p))[0]
        text = ctypes.c_void_p()
        if not self._sid_text(sid, ctypes.byref(text)) or not text.value:
            raise self._fail()
        try:
            return ctypes.wstring_at(text.value)
        finally:
            self._free(text)

    def _sid_of_process(self, process: int) -> str:
        ctypes = self._ctypes
        token = ctypes.c_void_p()
        if not self._open_token(process, self._TOKEN_QUERY, ctypes.byref(token)) or not token.value:
            raise self._fail()
        try:
            return self._sid_of_token(token.value)
        finally:
            self._close(token)

    def current_sid(self) -> str:
        """The text of this process's user's security identifier."""
        return self._sid_of_process(self._current_process())

    def server_sid(self, handle: int) -> str:
        """The text of the security identifier of the user the pipe's server
        process runs as. Raises ``OSError`` when it cannot be read."""
        ctypes = self._ctypes
        pid = ctypes.c_ulong(0)
        if not self._server_pid(handle, ctypes.byref(pid)):
            raise self._fail()
        process = self._open_process(self._PROCESS_QUERY_LIMITED_INFORMATION, 0, pid.value)
        if not process:
            raise self._fail()
        try:
            return self._sid_of_process(process)
        finally:
            self._close(process)


def _not_running() -> ChannelError:
    return ChannelError(
        "engine_unreachable",
        "No engine is running.",
        "Start one with `anything2mcp serve`, or run any `anything2mcp` command.",
    )


def open_pipe(runtime_dir: str, api: object, sid: Optional[str] = None) -> PipeSocket:
    """Opens the engine's pipe and checks its server, as the engine's own
    client does: the process that serves the pipe must run as this user.
    Nothing is sent before the check passes. ``api`` is :class:`Win32`, or a
    stand-in with the same methods in a test."""
    own = sid if sid is not None else api.current_sid()  # type: ignore[attr-defined]
    name = pipe_name(runtime_dir, own)
    tries = 0
    while True:
        try:
            handle = api.open(name)  # type: ignore[attr-defined]
            break
        except PipeOpenError as refused:
            if refused.code == ERROR_FILE_NOT_FOUND:
                raise _not_running() from None
            if refused.code == ERROR_PIPE_BUSY and tries < BUSY_TRIES:
                # Every instance is taken until the engine reaches its next
                # accept: wait on the system's own wake-up, and try again.
                tries += 1
                api.wait(name, BUSY_WAIT_MS)  # type: ignore[attr-defined]
                continue
            raise ChannelError(
                "connection_lost", "The engine's pipe could not be opened.", "Run it again."
            ) from None
    try:
        theirs = api.server_sid(handle)  # type: ignore[attr-defined]
    except OSError:
        theirs = None
    if theirs != own:
        api.close(handle)  # type: ignore[attr-defined]
        raise ChannelError(
            "peer_not_allowed",
            "What serves the engine's pipe does not belong to you.",
            "Stop the program that made the pipe, and start the engine again.",
        )

    def read(size: int) -> bytes:
        return api.read(handle, size)  # type: ignore[attr-defined, no-any-return]

    def write(data: bytes) -> int:
        return api.write(handle, data)  # type: ignore[attr-defined, no-any-return]

    return PipeSocket(PipeStream(read, write, lambda: api.close(handle)))  # type: ignore[attr-defined]


def connect_connection(
    env: Mapping[str, str], timeout: Optional[float] = None, api: Optional[object] = None
) -> PipeConnection:
    """The HTTP connection to this user's engine, opened. Raises
    :class:`ChannelError` the way the Unix path does."""
    runtime = runtime_dir_text(env)
    win32 = api if api is not None else Win32()
    connection = PipeConnection(lambda: open_pipe(runtime, win32), timeout=timeout)
    connection.connect()
    return connection
