"""Tests of the Windows named-pipe transport.

The calls into the operating system cannot run here. What can is tested on
every system: the pipe's name, the framing of the stream, the whole HTTP
exchange over a stream (the hello, a call, an answer), and what the client
does when the pipe is missing, busy, or served by someone else, with a
stand-in for the operating system. The last test opens a real pipe and runs
only on Windows.

Run: ``python -m unittest discover -s tests`` from ``bindings/python``.
"""

from __future__ import annotations

import hashlib
import io
import json
import os
import sys
import threading
import unittest
from pathlib import Path
from typing import Dict, List, Optional

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "src"))

import anything2mcp  # noqa: E402
from anything2mcp import _winpipe  # noqa: E402
from anything2mcp._client import Client  # noqa: E402

SID = "S-1-5-21-1111111111-2222222222-3333333333-1001"
RUNTIME = "C:\\Users\\pat\\AppData\\Local\\anything2mcp\\run"


class PlaceTests(unittest.TestCase):
    def test_the_pipe_is_named_as_the_engine_names_it(self) -> None:
        # The tag is the first 16 hex digits of the SHA-256 of the runtime directory's text.
        tag = hashlib.sha256(RUNTIME.encode()).hexdigest()[:16]
        self.assertEqual(_winpipe.pipe_name(RUNTIME, SID), f"\\\\.\\pipe\\anything2mcp.{SID}.{tag}")
        self.assertTrue(_winpipe.pipe_name(RUNTIME, SID).endswith(".8af282852451f074"))

    def test_the_runtime_directory_follows_the_engines_rules(self) -> None:
        local = {"LOCALAPPDATA": "C:\\Users\\pat\\AppData\\Local"}
        self.assertEqual(_winpipe.runtime_dir_text(local), RUNTIME)
        # A2M_HOME wins, and is joined as a Windows path is, not normalised.
        self.assertEqual(_winpipe.runtime_dir_text({"A2M_HOME": "D:\\a2m", **local}), "D:\\a2m\\run")
        self.assertEqual(_winpipe.runtime_dir_text({"A2M_HOME": "D:\\a2m\\"}), "D:\\a2m\\run")
        self.assertEqual(_winpipe.runtime_dir_text({"A2M_HOME": "D:/a2m"}), "D:/a2m\\run")
        for bad in ({"A2M_HOME": "relative"}, {"LOCALAPPDATA": "relative"}, {}):
            with self.assertRaises(anything2mcp.ChannelError) as raised:
                _winpipe.runtime_dir_text(bad)
            self.assertEqual(raised.exception.code, "path_not_allowed")


class StreamTests(unittest.TestCase):
    def test_a_write_is_finished_when_the_pipe_takes_it_in_pieces(self) -> None:
        taken: List[bytes] = []

        def write(data: bytes) -> int:
            taken.append(data[:3])
            return min(3, len(data))

        stream = _winpipe.PipeStream(lambda size: b"", write, lambda: None)
        self.assertEqual(stream.write(b"abcdefgh"), 8)
        self.assertEqual(b"".join(taken), b"abcdefgh")

    def test_a_pipe_that_takes_nothing_is_an_error_and_not_a_hang(self) -> None:
        stream = _winpipe.PipeStream(lambda size: b"", lambda data: 0, lambda: None)
        with self.assertRaises(OSError):
            stream.write(b"x")

    def test_closing_the_file_of_a_response_leaves_the_pipe_open(self) -> None:
        closed: List[bool] = []
        data = io.BytesIO(b"hello\r\nworld\r\n")
        stream = _winpipe.PipeStream(data.read, lambda b: len(b), lambda: closed.append(True))
        sock = _winpipe.PipeSocket(stream)
        first = sock.makefile("rb")
        self.assertEqual(first.readline(), b"hello\r\n")
        first.close()
        self.assertEqual(closed, [])
        sock.close()
        self.assertEqual(closed, [True])


class FakeWin32:
    """What the operating system would answer. ``opens`` is what each try
    to open the pipe does: an error number, or ``None`` for success."""

    def __init__(self, opens: List[Optional[int]], server: str = SID, own: str = SID) -> None:
        self.opens = list(opens)
        self.server = server
        self.own = own
        self.calls: List[str] = []
        self.sent = b""

    def current_sid(self) -> str:
        return self.own

    def open(self, name: str) -> int:
        self.calls.append("open")
        self.name = name
        outcome = self.opens.pop(0)
        if outcome is not None:
            raise _winpipe.PipeOpenError(outcome)
        return 7

    def wait(self, name: str, milliseconds: int) -> bool:
        self.calls.append("wait")
        return True

    def server_sid(self, handle: int) -> str:
        self.calls.append("check")
        return self.server

    def close(self, handle: int) -> None:
        self.calls.append("close")

    def read(self, handle: int, size: int) -> bytes:
        return b""

    def write(self, handle: int, data: bytes) -> int:
        self.sent += data
        return len(data)


class OpenTests(unittest.TestCase):
    def test_no_pipe_means_no_engine(self) -> None:
        api = FakeWin32([_winpipe.ERROR_FILE_NOT_FOUND])
        with self.assertRaises(anything2mcp.ChannelError) as raised:
            _winpipe.open_pipe(RUNTIME, api)
        self.assertEqual(raised.exception.code, "engine_unreachable")

    def test_a_busy_pipe_is_waited_for_on_the_systems_own_signal_and_tried_again(self) -> None:
        api = FakeWin32([_winpipe.ERROR_PIPE_BUSY, _winpipe.ERROR_PIPE_BUSY, None])
        sock = _winpipe.open_pipe(RUNTIME, api)
        self.assertEqual(api.calls, ["open", "wait", "open", "wait", "open", "check"])
        self.assertTrue(api.name.startswith("\\\\.\\pipe\\anything2mcp." + SID + "."))
        sock.close()

    def test_a_pipe_that_stays_busy_gives_up_with_a_named_failure(self) -> None:
        api = FakeWin32([_winpipe.ERROR_PIPE_BUSY] * (_winpipe.BUSY_TRIES + 1))
        with self.assertRaises(anything2mcp.ChannelError) as raised:
            _winpipe.open_pipe(RUNTIME, api)
        self.assertEqual(raised.exception.code, "connection_lost")

    def test_a_pipe_served_by_another_user_is_refused_before_a_byte_is_sent(self) -> None:
        api = FakeWin32([None], server="S-1-5-21-1111111111-2222222222-3333333333-1002")
        with self.assertRaises(anything2mcp.ChannelError) as raised:
            _winpipe.open_pipe(RUNTIME, api)
        self.assertEqual(raised.exception.code, "peer_not_allowed")
        self.assertEqual(api.calls, ["open", "check", "close"])
        self.assertEqual(api.sent, b"")

    def test_a_server_whose_user_cannot_be_read_is_not_trusted(self) -> None:
        api = FakeWin32([None])

        def unreadable(handle: int) -> str:
            raise OSError("access denied")

        api.server_sid = unreadable  # type: ignore[method-assign]
        with self.assertRaises(anything2mcp.ChannelError) as raised:
            _winpipe.open_pipe(RUNTIME, api)
        self.assertEqual(raised.exception.code, "peer_not_allowed")


class ExchangeTests(unittest.TestCase):
    """The hello and a call, over a stream made of two operating-system pipes
    and an engine of the test's own on the other end."""

    def test_the_client_says_hello_then_posts_the_action_and_reads_the_answer(self) -> None:
        to_engine_r, to_engine_w = os.pipe()
        to_client_r, to_client_w = os.pipe()
        seen: List[bytes] = []

        def engine() -> None:
            def request() -> bytes:
                head = b""
                while not head.endswith(b"\r\n\r\n"):
                    head += os.read(to_engine_r, 1)
                length = 0
                for line in head.split(b"\r\n"):
                    if line.lower().startswith(b"content-length:"):
                        length = int(line.split(b":")[1])
                body = b""
                while len(body) < length:
                    body += os.read(to_engine_r, length - len(body))
                return head + body

            def answer(status: str, body: Dict[str, object], extra: str = "") -> None:
                data = json.dumps(body).encode()
                head = f"HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {len(data)}\r\n{extra}\r\n"
                os.write(to_client_w, head.encode() + data)

            seen.append(request())
            answer("200 OK", {"engine_version": "9.9.9", "local_api": 1})
            seen.append(request())
            answer("200 OK", {"ok": True, "action": "engine.status", "result": {"up": True}}, "a2m-exit-class: succeeded\r\n")

        thread = threading.Thread(target=engine)
        thread.start()

        def read(size: int) -> bytes:
            return os.read(to_client_r, size)

        def write(data: bytes) -> int:
            return os.write(to_engine_w, data)

        stream = _winpipe.PipeStream(read, write, lambda: None)
        connection = _winpipe.PipeConnection(lambda: _winpipe.PipeSocket(stream))
        client = Client(connection, name="test")
        self.assertEqual(client.engine_version, "9.9.9")
        response = client.engine_status()
        thread.join(timeout=10)
        for descriptor in (to_engine_r, to_engine_w, to_client_r, to_client_w):
            os.close(descriptor)
        self.assertTrue(response.ok)
        self.assertEqual(response.exit_class, "succeeded")
        self.assertEqual(response.result, {"up": True})
        self.assertTrue(seen[0].startswith(b"GET /local/hello HTTP/1.1\r\n"))
        self.assertIn(b"a2m-local-api: 1", seen[0])
        self.assertIn(b"a2m-client-label: sdk:test", seen[0])
        self.assertTrue(seen[1].startswith(b"POST /api/v1/actions/engine.status HTTP/1.1\r\n"))


@unittest.skipUnless(sys.platform.startswith("win"), "opens a real pipe: Windows only")
class RealPipeTests(unittest.TestCase):
    def test_the_real_calls_read_this_users_identifier_and_find_no_engine(self) -> None:
        api = _winpipe.Win32()
        sid = api.current_sid()
        self.assertTrue(sid.startswith("S-1-"), sid)
        # No engine runs in a scratch home: the pipe is not there.
        import tempfile

        with tempfile.TemporaryDirectory() as home:
            with self.assertRaises(anything2mcp.ChannelError) as raised:
                anything2mcp.connect(env={"A2M_HOME": home}, start=False)
            self.assertEqual(raised.exception.code, "engine_unreachable")

    BINARY = os.environ.get("A2M_TEST_BINARY", "")

    @unittest.skipUnless(BINARY, "set A2M_TEST_BINARY to a built anything2mcp to run it")
    def test_the_client_reaches_a_real_engine_over_its_pipe(self) -> None:
        import subprocess
        import tempfile

        with tempfile.TemporaryDirectory() as home:
            env = {**os.environ, "A2M_HOME": home, "A2M_VAULT_PASSPHRASE": "secret-for-tests-only"}
            engine = subprocess.Popen([self.BINARY, "serve"], env=env, stderr=subprocess.PIPE)
            try:
                client = None
                for _ in range(100):
                    try:
                        client = anything2mcp.connect(env=env, start=False)
                        break
                    except anything2mcp.ChannelError:
                        engine.poll()
                        if engine.returncode is not None:
                            break
                        import time

                        time.sleep(0.1)
                self.assertIsNotNone(client)
                assert client is not None
                self.assertTrue(client.engine_status().ok)
                client.close()
            finally:
                engine.terminate()
                engine.wait(timeout=30)
                if engine.stderr is not None:
                    engine.stderr.close()


if __name__ == "__main__":
    unittest.main()
