#!/usr/bin/env python3
"""Prove that tools/field_proxy.py does what its docstring says.

The tests start a tiny HTTP server of their own on a free port, put a FaultProxy in front
of it, and talk to the proxy over sockets. Run from the repository root:

    python3 tools/test_field_proxy.py

Standard library only; Python 3.11 or newer.
"""
import asyncio
import http.client
import socket
import sys
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from field_proxy import FaultProxy, render_record  # noqa: E402


class Upstream(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def _body(self):
        if "chunked" in (self.headers.get("Transfer-Encoding") or "").lower():
            data = b""
            while True:
                size = int(self.rfile.readline().split(b";")[0], 16)
                if size == 0:
                    self.rfile.readline()
                    return data
                data += self.rfile.read(size)
                self.rfile.read(2)
        return self.rfile.read(int(self.headers.get("Content-Length") or 0))

    def _answer(self):
        body = self._body()
        with self.server.lock:
            self.server.seen.append((self.command, self.path, self.headers.get("Host"), body))
        if self.path.startswith("/chunked"):
            self.send_response(200)
            self.send_header("Transfer-Encoding", "chunked")
            self.end_headers()
            for chunk in (b"abc", b"defgh", b"i" * 40):
                self.wfile.write(b"%x\r\n%s\r\n" % (len(chunk), chunk))
            self.wfile.write(b"0\r\n\r\n")
            return
        out = b"you sent %d bytes: " % len(body) + body + b" " + b"x" * 200
        self.send_response(200)
        self.send_header("Content-Length", str(len(out)))
        if self.path.startswith("/close"):
            self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(out)

    do_GET = do_POST = do_PUT = _answer


class ProxyCase(unittest.TestCase):
    def setUp(self):
        self.upstream = ThreadingHTTPServer(("127.0.0.1", 0), Upstream)
        self.upstream.daemon_threads = True
        self.upstream.seen = []
        self.upstream.lock = threading.Lock()
        threading.Thread(target=self.upstream.serve_forever, kwargs={"poll_interval": 0.01}, daemon=True).start()
        self.up_port = self.upstream.server_address[1]
        self.loop = asyncio.new_event_loop()
        self.thread = threading.Thread(target=self.loop.run_forever, daemon=True)
        self.thread.start()
        self.proxy = FaultProxy("127.0.0.1", self.up_port)
        self.port = self.call(self.proxy.start())
        self.closed = False

    def tearDown(self):
        if not self.closed:
            self.call(self.proxy.close())
        self.loop.call_soon_threadsafe(self.loop.stop)
        self.thread.join()
        self.loop.close()
        self.upstream.shutdown()
        self.upstream.server_close()

    def call(self, coroutine):
        return asyncio.run_coroutine_threadsafe(coroutine, self.loop).result(10)

    def seen(self):
        with self.upstream.lock:
            return list(self.upstream.seen)

    def raw(self, data, port=None):
        """Send bytes, read until the other side closes; return what came back."""
        with socket.create_connection(("127.0.0.1", port or self.port), timeout=10) as sock:
            sock.sendall(data)
            out = b""
            try:
                while True:
                    part = sock.recv(65536)
                    if not part:
                        return out
                    out += part
            except ConnectionResetError:
                return out

    def post(self, path="/api/make", body=b"hello", extra=b""):
        return (
            b"POST " + path.encode() + b" HTTP/1.1\r\nHost: wrong.invalid\r\nConnection: close\r\n"
            + extra + b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body
        )


class Passing(ProxyCase):
    def test_a_passed_request_is_identical_upstream_apart_from_host(self):
        out = self.raw(self.post(extra=b"X-Plain: 1\r\n"))
        self.assertTrue(out.startswith(b"HTTP/1.1 200"))
        self.assertIn(b"you sent 5 bytes: hello", out)
        self.assertEqual(self.seen(), [("POST", "/api/make", f"127.0.0.1:{self.up_port}", b"hello")])

    def test_host_is_kept_when_the_option_is_off(self):
        self.call(self.proxy.close())
        self.closed = True
        self.proxy = FaultProxy("127.0.0.1", self.up_port, rewrite_host=False)
        self.call_start = self.call(self.proxy.start())
        self.raw(self.post(), port=self.call_start)
        self.assertEqual(self.seen()[0][2], "wrong.invalid")
        self.call(self.proxy.close())

    def test_a_request_that_matches_no_rule_passes(self):
        self.proxy.add_rule("POST", "^/other$", "refuse")
        self.proxy.add_rule("GET", "^/api/make$", "refuse")
        out = self.raw(self.post())
        self.assertIn(b"you sent 5 bytes", out)
        self.assertEqual(len(self.seen()), 1)

    def test_chunked_request_and_response_bodies_pass_intact(self):
        request = (
            b"POST /api/chunk HTTP/1.1\r\nHost: x\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n"
            b"4\r\nwiki\r\n5\r\npedia\r\n0\r\n\r\n"
        )
        out = self.raw(request)
        self.assertEqual(self.seen()[0][3], b"wikipedia")
        self.assertIn(b"you sent 9 bytes: wikipedia", out)
        out = self.raw(b"GET /chunked HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        self.assertTrue(out.endswith(b"3\r\nabc\r\n5\r\ndefgh\r\n28\r\n" + b"i" * 40 + b"\r\n0\r\n\r\n"))

    def test_keep_alive_applies_the_rule_to_the_right_request(self):
        self.proxy.add_rule("GET", "^/second$", "drop_response")
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        conn.request("GET", "/first")
        first = conn.getresponse()
        self.assertEqual(first.status, 200)
        first.read()
        conn.request("GET", "/second")
        with self.assertRaises((http.client.RemoteDisconnected, ConnectionError)):
            conn.getresponse()
        conn.close()
        self.assertEqual([s[1] for s in self.seen()], ["/first", "/second"])
        self.assertEqual([(e["path"], e["rule"]) for e in self.proxy.record()], [("/first", None), ("/second", "drop_response")])

    def test_keep_alive_serves_two_requests_on_one_connection(self):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        for path in ("/a", "/b"):
            conn.request("GET", path)
            answer = conn.getresponse()
            self.assertEqual(answer.status, 200)
            answer.read()
        conn.close()
        self.assertEqual([s[1] for s in self.seen()], ["/a", "/b"])

    def test_a_response_that_closes_the_connection_is_relayed(self):
        out = self.raw(b"GET /close HTTP/1.1\r\nHost: x\r\n\r\n")
        self.assertIn(b"you sent 0 bytes", out)


class Faulting(ProxyCase):
    def test_pass_rule_only_counts(self):
        self.proxy.add_rule("POST", "^/api/", "pass")
        self.assertIn(b"200", self.raw(self.post()))
        self.assertEqual(self.proxy.record()[0]["rule"], "pass")

    def test_delay_waits_then_forwards(self):
        self.proxy.add_rule("POST", "^/api/", "delay", seconds=0.3)
        began = time.monotonic()
        out = self.raw(self.post())
        self.assertGreaterEqual(time.monotonic() - began, 0.29)
        self.assertIn(b"you sent 5 bytes", out)
        self.assertEqual(len(self.seen()), 1)

    def test_refuse_closes_and_upstream_never_sees_it(self):
        self.proxy.add_rule("POST", "^/api/", "refuse")
        self.assertEqual(self.raw(self.post()), b"")
        self.assertEqual(self.seen(), [])
        self.assertEqual(self.proxy.record()[0]["status"], None)

    def test_drop_request_reads_it_all_and_never_forwards(self):
        self.proxy.add_rule("POST", "^/api/", "drop_request")
        self.assertEqual(self.raw(self.post(body=b"b" * 5000)), b"")
        self.assertEqual(self.seen(), [])

    def test_drop_response_after_the_upstream_acted(self):
        self.proxy.add_rule("POST", "^/api/", "drop_response")
        self.assertEqual(self.raw(self.post()), b"")
        self.assertEqual(len(self.seen()), 1)
        self.assertEqual(self.proxy.record()[0]["status"], 200)

    def test_cut_response_sends_only_the_first_bytes(self):
        self.proxy.add_rule("POST", "^/api/", "cut_response", nbytes=10)
        out = self.raw(self.post())
        self.assertEqual(len(out), 10)
        self.assertEqual(out, b"HTTP/1.1 2"[:10])
        self.assertEqual(len(self.seen()), 1)

    def test_a_rule_is_consumed_after_its_times(self):
        self.proxy.add_rule("POST", "^/api/", "drop_response", times=2)
        self.assertEqual(self.raw(self.post()), b"")
        self.assertEqual(self.raw(self.post()), b"")
        self.assertIn(b"200", self.raw(self.post()))
        self.assertEqual(len(self.seen()), 3)
        self.assertEqual([e["rule"] for e in self.proxy.record()], ["drop_response", "drop_response", None])

    def test_rules_are_used_in_the_order_added(self):
        self.proxy.add_rule("POST", "^/api/", "refuse")
        self.proxy.add_rule("POST", "^/api/", "drop_response")
        self.raw(self.post())
        self.raw(self.post())
        self.assertEqual([e["rule"] for e in self.proxy.record()], ["refuse", "drop_response"])

    def test_reset_clears_rules_and_record(self):
        self.proxy.add_rule("POST", "^/api/", "refuse")
        self.raw(self.post())
        self.proxy.reset()
        self.assertEqual(self.proxy.record(), [])
        self.assertIn(b"200", self.raw(self.post()))

    def test_an_unknown_action_is_refused_at_once(self):
        with self.assertRaises(ValueError):
            self.proxy.add_rule("GET", "/", "explode")


class Recording(ProxyCase):
    def test_the_record_holds_no_header_or_body_text(self):
        self.raw(self.post(path="/api/make?token=querysecret", body=b"BODYMARKER", extra=b"Authorization: Bearer HEADERMARKER\r\n"))
        record = self.proxy.record()
        self.assertEqual(record, [{"method": "POST", "path": "/api/make", "rule": None, "status": 200}])
        text = repr(record) + render_record(record)
        for marker in ("BODYMARKER", "HEADERMARKER", "querysecret", "wrong.invalid"):
            self.assertNotIn(marker, text)
        self.assertEqual(render_record(record), "POST /api/make -> none -> 200")


class Closing(ProxyCase):
    def test_close_frees_the_port_and_ends_connections(self):
        idle = socket.create_connection(("127.0.0.1", self.port), timeout=10)
        until = time.monotonic() + 10
        while not self.proxy._writers and time.monotonic() < until:  # wait for the proxy to take it up
            time.sleep(0.01)
        self.assertTrue(self.proxy._writers)
        self.call(self.proxy.close())
        self.closed = True
        self.assertEqual(idle.recv(10), b"")
        idle.close()
        with self.assertRaises(ConnectionRefusedError):
            socket.create_connection(("127.0.0.1", self.port), timeout=10)
        with socket.socket() as again:
            if sys.platform != "win32":  # the closed connections may sit in TIME_WAIT
                again.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            again.bind(("127.0.0.1", self.port))
        self.assertEqual(self.proxy._tasks, set())


if __name__ == "__main__":
    unittest.main()
