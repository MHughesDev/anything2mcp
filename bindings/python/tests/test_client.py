"""Tests of the Python client against a fake engine of the test's own.

The fake listens on a Unix socket where the engine would, answers the hello
as the engine's channel does, and records each request. The last test runs
the real binary, and is skipped unless ``A2M_TEST_BINARY`` names one.

Run: ``python -m unittest discover -s tests`` from ``bindings/python``.
"""

from __future__ import annotations

import json
import os
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler
from pathlib import Path, PurePosixPath

# The unix rules are written with "/" paths; on a Windows host they are PurePosixPath.
UnixPath = PurePosixPath if os.name == "nt" else Path
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "src"))

import anything2mcp  # noqa: E402
from anything2mcp import _channel  # noqa: E402

UNIX = hasattr(__import__("socket"), "AF_UNIX") and not sys.platform.startswith("win")


class FakeEngine:
    """Answers the hello, then each action with what ``answers`` holds."""

    def __init__(self, home: Path, local_api: str = "1") -> None:
        self.requests: list = []
        self.answers: dict = {}
        #: Actions whose request the fake takes and then drops the
        #: connection without an answer, as an engine that stops does.
        self.drops: set = set()
        self.connections = 0
        run = home / "run"
        run.mkdir(mode=0o700, parents=True, exist_ok=True)
        os.chmod(run, 0o700)
        self.path = run / "engine.sock"
        fake = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"
            said_hello = False

            def setup(self) -> None:
                fake.connections += 1
                super().setup()

            def log_message(self, *_: object) -> None:
                pass

            def _send(self, status: int, body: dict, exit_class: str = "") -> None:
                data = json.dumps(body).encode()
                self.send_response(status)
                self.send_header("content-type", "application/json")
                self.send_header("content-length", str(len(data)))
                if exit_class:
                    self.send_header("a2m-exit-class", exit_class)
                self.end_headers()
                self.wfile.write(data)

            def do_GET(self) -> None:
                fake.requests.append(("GET", self.path, dict(self.headers), b""))
                if self.path != "/local/hello":
                    self._send(400, {"error": {"code": "hello_invalid"}})
                    return
                if self.headers.get("a2m-local-api") != local_api:
                    self._send(
                        400,
                        {"ok": False, "error": {"code": "local_api_mismatch", "message": "No.", "fix": "Upgrade."}},
                    )
                    return
                type(self).said_hello = True
                self._send(200, {"engine_version": "9.9.9", "local_api": 1})

            def do_POST(self) -> None:
                length = int(self.headers.get("content-length", "0"))
                body = self.rfile.read(length)
                fake.requests.append(("POST", self.path, dict(self.headers), body))
                if not type(self).said_hello:
                    self._send(400, {"error": {"code": "hello_invalid"}})
                    return
                action = self.path.rsplit("/", 1)[-1]
                if action in fake.drops:
                    self.close_connection = True
                    return
                status, answer, exit_class = fake.answers.get(
                    action, (200, {"ok": True, "action": action, "result": {"echo": json.loads(body)}}, "succeeded")
                )
                self._send(status, answer, exit_class)

        self.server = socketserver.UnixStreamServer(str(self.path), Handler)  # type: ignore[attr-defined]
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def stop(self) -> None:
        self.server.shutdown()
        self.server.server_close()


@unittest.skipUnless(UNIX, "the client reaches the engine on macOS and Linux only")
class ClientTests(unittest.TestCase):
    def setUp(self) -> None:
        # A short path: a socket's path has a length limit.
        self.home = Path(tempfile.mkdtemp(prefix="a2m"))
        self.env = {"A2M_HOME": str(self.home)}

    def tearDown(self) -> None:
        shutil.rmtree(self.home, ignore_errors=True)

    def test_says_hello_first_then_posts_the_action_and_returns_the_engines_answer(self) -> None:
        engine = FakeEngine(self.home)
        try:
            project = self.home / "my work"
            project.mkdir()
            with anything2mcp.connect(project=project, name="tests", env=self.env) as client:
                self.assertEqual(client.engine_version, "9.9.9")
                answer = client.call("tool.call", {"tool": "example_orders_order_get"}, idempotency_key="k-1")
            self.assertTrue(answer.ok)
            self.assertEqual(answer.exit_class, "succeeded")
            self.assertEqual(answer.result, {"echo": {"tool": "example_orders_order_get"}})
            (m1, p1, h1, _), (m2, p2, h2, b2) = engine.requests
            self.assertEqual((m1, p1), ("GET", "/local/hello"))
            self.assertEqual(h1["a2m-client-label"], "sdk:tests")
            self.assertEqual(h1["a2m-local-api"], "1")
            self.assertEqual((m2, p2), ("POST", "/api/v1/actions/tool.call"))
            self.assertEqual(json.loads(b2), {"tool": "example_orders_order_get"})
            self.assertEqual(h2["a2m-project"], str(project.resolve()).replace(" ", "%20"))
            self.assertEqual(h2["idempotency-key"], "k-1")
        finally:
            engine.stop()

    def test_a_failure_is_returned_as_the_engine_wrote_it_and_not_raised(self) -> None:
        engine = FakeEngine(self.home)
        refused = {
            "ok": False,
            "action": "draft.lint",
            "error": {"class": "input", "code": "schema_invalid", "message": "m", "fix": "f", "retry_safe": True},
        }
        engine.answers["draft.lint"] = (400, refused, "malformed")
        try:
            with anything2mcp.connect(env=self.env) as client:
                answer = client.call("draft.lint")
            self.assertFalse(answer.ok)
            self.assertEqual(answer.exit_class, "malformed")
            assert answer.error is not None
            self.assertEqual((answer.error.failure_class, answer.error.code), ("input", "schema_invalid"))
            self.assertEqual(answer.body, refused)
        finally:
            engine.stop()

    def test_a_keyed_call_whose_answer_is_lost_is_told_to_make_the_same_call_again(self) -> None:
        engine = FakeEngine(self.home)
        engine.drops.add("tool.call")
        try:
            client = anything2mcp.connect(env=self.env)
            with self.assertRaises(anything2mcp.ChannelError) as raised:
                client.call("tool.call", {"tool": "example_orders_order_update"}, idempotency_key="example-key-1")
            lost = raised.exception
            self.assertEqual(lost.code, "answer_lost")
            self.assertFalse(lost.retry_safe)
            self.assertEqual(
                lost.fix,
                "Make the same call again with the same idempotency key and nothing changed: it gives back "
                "the run the first call started, finished or not, and starts no other.",
            )
            client.close()
        finally:
            engine.stop()

    def test_a_call_whose_answer_is_lost_after_the_engine_took_it_is_not_safe_to_make_again(self) -> None:
        engine = FakeEngine(self.home)
        engine.drops.add("tool.call")
        try:
            client = anything2mcp.connect(env=self.env)
            with self.assertRaises(anything2mcp.ChannelError) as raised:
                client.call("tool.call", {"tool": "example_orders_order_update"})
            lost = raised.exception
            self.assertEqual(lost.code, "answer_lost")
            self.assertFalse(lost.retry_safe)
            self.assertTrue(lost.fix.startswith("Do not make the call again"), lost.fix)
            self.assertNotIn("Run it again", lost.fix)
            self.assertRegex(lost.fix, r"`run_list\(since=\d+, tool='example_orders_order_update'\)`")
            self.assertIn("one minute before the call", lost.fix)
            self.assertIn("what that run's `next` says", lost.fix)
            self.assertIn("pass an `idempotency_key` on a call that may write", lost.fix)
            # The engine had it.
            self.assertEqual([r[1] for r in engine.requests][-1], "/api/v1/actions/tool.call")
            # The client does not open the channel again behind the caller's
            # back, with no check and no hello: nothing more is sent.
            sent, connections = len(engine.requests), engine.connections
            with self.assertRaises(anything2mcp.ChannelError) as raised:
                client.call("engine.status")
            self.assertEqual(raised.exception.code, "connection_lost")
            self.assertTrue(raised.exception.retry_safe)
            self.assertEqual((len(engine.requests), engine.connections), (sent, connections))
            client.close()
        finally:
            engine.stop()

    def test_an_action_name_that_is_not_noun_dot_verb_is_refused_before_anything_is_sent(self) -> None:
        engine = FakeEngine(self.home)
        try:
            with anything2mcp.connect(env=self.env) as client:
                sent = len(engine.requests)
                for name in ("../local/hello", "tool.call?x=1", "tool.call#x", "tool%2Ecall", "Tool.Call", "tool"):
                    with self.assertRaises(ValueError):
                        client.call(name)
                self.assertEqual(len(engine.requests), sent)
        finally:
            engine.stop()

    def test_no_engine_is_a_named_failure(self) -> None:
        # Asked to start nothing, the client says so by name.
        with self.assertRaises(anything2mcp.ChannelError) as raised:
            anything2mcp.connect(env=self.env, start=False)
        self.assertEqual(raised.exception.code, "engine_unreachable")
        # Asked to start one with a program that cannot be run, the same.
        missing = self.home / "no-such-program"
        with self.assertRaises(anything2mcp.ChannelError) as raised:
            anything2mcp.connect(env=self.env, binary=missing)
        self.assertEqual(raised.exception.code, "engine_unreachable")

    def test_a_socket_in_a_directory_others_can_change_is_not_trusted(self) -> None:
        engine = FakeEngine(self.home)
        try:
            os.chmod(engine.path.parent, 0o755)
            with self.assertRaises(anything2mcp.ChannelError) as raised:
                anything2mcp.connect(env=self.env)
            self.assertEqual(raised.exception.code, "peer_not_allowed")
            self.assertEqual(engine.requests, [], "nothing is sent before the check")
        finally:
            engine.stop()

    def test_a_refused_hello_is_a_named_failure(self) -> None:
        engine = FakeEngine(self.home, local_api="2")
        try:
            with self.assertRaises(anything2mcp.ChannelError) as raised:
                anything2mcp.connect(env=self.env)
            self.assertEqual(raised.exception.code, "local_api_mismatch")
        finally:
            engine.stop()


class PlaceTests(unittest.TestCase):
    """The place of the socket, by the engine's rules in ``Paths``."""

    def test_each_system_keeps_the_socket_where_the_engine_puts_it(self) -> None:
        linux = {"HOME": "/home/pat"}
        self.assertEqual(
            _channel.socket_path(linux, "linux"),
            UnixPath("/home/pat/.local/share/anything2mcp/run/engine.sock"),
        )
        self.assertEqual(
            _channel.socket_path({**linux, "XDG_RUNTIME_DIR": "/run/user/1000"}, "linux"),
            UnixPath("/run/user/1000/anything2mcp/engine.sock"),
        )
        self.assertEqual(
            _channel.socket_path({**linux, "XDG_RUNTIME_DIR": "relative"}, "linux"),
            UnixPath("/home/pat/.local/share/anything2mcp/run/engine.sock"),
        )
        self.assertEqual(
            _channel.socket_path({"HOME": "/Users/pat", "TMPDIR": "/var/folders/x/T"}, "darwin"),
            UnixPath("/var/folders/x/T/anything2mcp/engine.sock"),
        )
        self.assertEqual(
            _channel.socket_path({"A2M_HOME": "/srv/a2m", "HOME": "/ignored"}, "linux"),
            UnixPath("/srv/a2m/run/engine.sock"),
        )
        with self.assertRaises(anything2mcp.ChannelError):
            _channel.socket_path({"A2M_HOME": "relative"}, "linux")
        # Windows has a pipe and no socket file: its place is in test_winpipe.py.
        self.assertEqual(
            _channel.runtime_dir({"LOCALAPPDATA": "C:\\Users\\pat\\AppData\\Local"}, "win32").as_posix(),
            "C:/Users/pat/AppData/Local/anything2mcp/run",
        )


BINARY = os.environ.get("A2M_TEST_BINARY", "")


@unittest.skipUnless(UNIX and BINARY, "set A2M_TEST_BINARY to a built anything2mcp to run it")
class RealEngineTests(unittest.TestCase):
    def test_the_client_reaches_a_real_engine_and_the_engine_answers(self) -> None:
        home = Path(tempfile.mkdtemp(prefix="a2m"))
        env = {
            **os.environ,
            "A2M_HOME": str(home),
            "A2M_VAULT_PASSPHRASE": "secret-for-tests-only",
        }
        engine = subprocess.Popen(
            [BINARY, "serve"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE
        )
        try:
            socket_path = home / "run" / "engine.sock"
            # Waits on the named condition, the socket existing, up to a deadline.
            deadline = time.monotonic() + 60
            while not socket_path.exists():
                if engine.poll() is not None:
                    self.fail(f"the engine stopped: {engine.stderr.read().decode() if engine.stderr else ''}")
                if time.monotonic() > deadline:
                    self.fail("the engine did not open its channel within a minute")
                time.sleep(0.05)
            with anything2mcp.connect(env={"A2M_HOME": str(home)}, name="tests") as client:
                status = client.call("engine.status")
                self.assertTrue(status.ok, status.body)
                self.assertEqual(status.exit_class, "succeeded")
                self.assertEqual(status.action, "engine.status")
                typed = client.engine_status()  # the generated method says the same
                self.assertEqual(typed.action, "engine.status")
                self.assertTrue(typed.ok, typed.body)
                unknown = client.call("no.such")
                self.assertFalse(unknown.ok)
        finally:
            engine.terminate()
            engine.wait(timeout=60)
            if engine.stderr is not None:
                engine.stderr.close()
            shutil.rmtree(home, ignore_errors=True)

    def test_a_client_that_finds_no_engine_has_the_program_start_one_and_is_answered(self) -> None:
        home = Path(tempfile.mkdtemp(prefix="a2m"))
        # The passphrase is given as a person's shell gives it: a machine with
        # no key store (a Linux runner) has the vault take its root key from it.
        env = {"A2M_HOME": str(home), "A2M_VAULT_PASSPHRASE": "secret-for-tests-only"}
        try:
            # Asked to start nothing, it starts nothing.
            with self.assertRaises(anything2mcp.ChannelError) as raised:
                anything2mcp.connect(env=env, start=False)
            self.assertEqual(raised.exception.code, "engine_unreachable")
            self.assertFalse((home / "run" / "engine.sock").exists())
            # Otherwise the program starts one, as a command would, and the
            # client is answered by it.
            with anything2mcp.connect(env=env, name="tests", binary=BINARY) as client:
                status = client.call("engine.status")
                self.assertEqual(status.action, "engine.status")
                self.assertEqual(status.exit_class, "succeeded", status.body)
            # The engine runs on: a client that starts nothing reaches it.
            with anything2mcp.connect(env=env, start=False) as again:
                self.assertTrue(again.call("engine.status").ok)
            log = (home / "data" / "logs" / "engine-output.log").read_text()
            self.assertEqual(log.count("a client started the engine on demand"), 1, log)
        finally:
            subprocess.run(
                [BINARY, "engine", "stop"],
                env={**os.environ, **env},
                stdin=subprocess.DEVNULL,
                capture_output=True,
                timeout=60,
                check=False,
            )
            shutil.rmtree(home, ignore_errors=True)


if __name__ == "__main__":
    unittest.main()
