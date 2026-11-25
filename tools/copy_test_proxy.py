#!/usr/bin/env python3
"""A small TCP proxy that breaks one coming exchange on purpose.

A copy test (tools/copy_test.py) puts this proxy between the product and a copy of
existing software, so that a recipe can make a fault happen at a chosen request: the
connection refused, the request lost, the response lost after the software has acted, or
the response cut short.

It listens on 127.0.0.1 on a free port and forwards every connection to an upstream host
and port. It speaks plain HTTP/1.1 well enough to see where one request ends and its
response begins and ends: the head, then a body by `Content-Length` or chunked encoding
(a response with neither is read to the end of the upstream's connection). Keep-alive and
`Connection: close` both work; each request uses a new connection to the upstream.

Out of scope: TLS. The proxy does not decrypt anything, so an upstream that is TLS cannot
be proxied. It also does not answer `Expect: 100-continue` on the software's behalf and
does not relay upgraded connections.

It rewrites nothing, so the software sees what the product sent, with one exception: the
`Host` header is set to the upstream's host and port, so that the software's own address
checks pass. That rewrite is the constructor option `rewrite_host`, on by default.

A rule says: for the next `times` requests (default 1) whose method equals `method` and
whose path (without the query) matches the regular expression `path` (`re.search`), do one
`do`:

    pass           count it only
    delay          wait `seconds`, then forward
    refuse         close the client connection without reading the request's body and
                   without forwarding it (the head is read, because the rule needs the
                   method and path to match)
    drop_request   read the whole request, never forward it, close the client connection
    drop_response  forward the request, read the upstream's whole response so that the
                   software has certainly acted, then close the client connection
                   without sending one byte of it
    cut_response   forward, send the client only the first `bytes` bytes of the
                   response, and close

Rules are used in the order they were added; a request that matches no rule passes.

The proxy records every request it saw, in order: method, path without the query, the
action of the rule that applied (or None), and the upstream status if a response was read.
It records no headers and no bodies, because they can hold credentials.

    python3 tools/copy_test_proxy.py --upstream 127.0.0.1:3000 \\
        --rule "POST ^/api/ drop_response 1"

In a copy test the proxy runs in a container on the run's closed network
(decision 0310): `--listen 0.0.0.0:PORT` makes it reachable by the product's container,
`--control 127.0.0.1:PORT` opens a control port for one JSON line per connection
(`add_rule`, `record`, `reset`), reachable only inside that container, and
`--control ADDRESS --send JSON` is its client, which the runner runs with `docker exec`.

Standard library only; Python 3.11 or newer.
"""

import argparse
import asyncio
import json
import re
import socket
import threading

ACTIONS = ("pass", "delay", "refuse", "drop_request", "drop_response", "cut_response")
HEAD_LIMIT = 1 << 16
END = b"\r\n\r\n"


class _Rule:
    def __init__(self, method, path, do, times, seconds, nbytes):
        if do not in ACTIONS:
            raise ValueError(f"unknown fault {do!r}; one of {', '.join(ACTIONS)}")
        self.method = method.upper()
        self.path = re.compile(path)
        self.do = do
        self.left = int(times)
        self.seconds = float(seconds)
        self.nbytes = int(nbytes)


def _split_head(head):
    """The first line and the header fields (lowercase name, value) of a head."""
    lines = head[: -len(END)].decode("latin-1").split("\r\n")
    fields = []
    for line in lines[1:]:
        name, _, value = line.partition(":")
        fields.append((name.strip().lower(), value.strip()))
    return lines[0], fields


def _field(fields, name):
    for key, value in fields:
        if key == name:
            return value
    return None


async def _read_head(reader):
    return await reader.readuntil(END)


async def _read_chunked(reader):
    raw = b""
    while True:
        line = await reader.readline()
        if not line.endswith(b"\n"):
            raise asyncio.IncompleteReadError(line, None)
        raw += line
        size = int(line.split(b";")[0].strip(), 16)
        if size == 0:
            while True:
                trailer = await reader.readline()
                if not trailer.endswith(b"\n"):
                    raise asyncio.IncompleteReadError(trailer, None)
                raw += trailer
                if trailer in (b"\r\n", b"\n"):
                    return raw
        raw += await reader.readexactly(size + 2)


async def _read_body(reader, fields):
    """The raw body that follows a head whose body is framed by its fields, or None when
    only the end of the connection ends it."""
    if "chunked" in (_field(fields, "transfer-encoding") or "").lower():
        return await _read_chunked(reader)
    length = _field(fields, "content-length")
    if length is not None:
        return await reader.readexactly(int(length))
    return b""


def _closes(version, fields):
    token = (_field(fields, "connection") or "").lower()
    if "close" in token:
        return True
    return version.upper().endswith("/1.0") and "keep-alive" not in token


class FaultProxy:
    def __init__(self, upstream_host, upstream_port, rewrite_host=True, listen_host="127.0.0.1"):
        self.listen_host = listen_host
        self.upstream_host = upstream_host
        self.upstream_port = int(upstream_port)
        self.rewrite_host = rewrite_host
        self.port = None
        self._rules = []
        self._record = []
        self._lock = threading.Lock()
        self._server = None
        self._tasks = set()
        self._writers = set()

    # Rules and record: safe to call from any thread.

    def add_rule(self, method, path, do="pass", times=1, seconds=1.0, nbytes=0):
        rule = _Rule(method, path, do, times, seconds, nbytes)
        with self._lock:
            self._rules.append(rule)

    def record(self):
        """Every request seen, in order, as dicts: method, path, rule, status."""
        with self._lock:
            return [dict(entry) for entry in self._record]

    def reset(self):
        """Clear the rules and the record."""
        with self._lock:
            self._rules.clear()
            self._record.clear()

    def _take(self, method, path):
        with self._lock:
            rule = next(
                (r for r in self._rules if r.left > 0 and r.method == method and r.path.search(path)),
                None,
            )
            if rule is not None:
                rule.left -= 1
                if rule.left == 0:
                    self._rules.remove(rule)
            entry = {"method": method, "path": path, "rule": rule.do if rule else None, "status": None}
            self._record.append(entry)
            return rule, entry

    # Lifecycle.

    async def start(self, port=0):
        self._server = await asyncio.start_server(self._handle, self.listen_host, port, limit=HEAD_LIMIT)
        self.port = self._server.sockets[0].getsockname()[1]
        return self.port

    async def close(self):
        """End every connection and free the port."""
        if self._server is None:
            return
        self._server.close()
        if hasattr(self._server, "close_clients"):  # Python 3.13: also drops connections not yet handed to a task
            self._server.close_clients()
        tasks = list(self._tasks)
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        for writer in list(self._writers):
            writer.close()
        await self._server.wait_closed()
        self._server = None

    # One client connection.

    async def _handle(self, reader, writer):
        task = asyncio.current_task()
        self._tasks.add(task)
        self._writers.add(writer)
        try:
            await self._serve(reader, writer)
        except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, ConnectionError, OSError, ValueError):
            pass
        finally:
            self._tasks.discard(task)
            self._writers.discard(writer)
            writer.close()

    async def _serve(self, reader, writer):
        while True:
            head = await _read_head(reader)
            line, fields = _split_head(head)
            parts = line.split(" ")
            if len(parts) != 3:
                return
            method, target, version = parts
            path = target.split("#")[0].split("?")[0]
            rule, entry = self._take(method.upper(), path)
            action = rule.do if rule else "pass"
            if action == "refuse":
                return
            body = await _read_body(reader, fields)
            if action == "drop_request":
                return
            if action == "delay":
                await asyncio.sleep(rule.seconds)
            try:
                answer, status, answer_closes = await self._forward(head, body, method)
            except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, OSError, ValueError):
                writer.write(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                await writer.drain()
                return
            entry["status"] = status
            if action == "drop_response":
                return
            if action == "cut_response":
                writer.write(answer[: rule.nbytes])
                await writer.drain()
                return
            writer.write(answer)
            await writer.drain()
            if answer_closes or _closes(version, fields):
                return

    async def _forward(self, head, body, method):
        """Send one request upstream; return the raw response, its status, and whether the
        connection must close after it."""
        if self.rewrite_host:
            head = self._with_host(head)
        up_reader, up_writer = await asyncio.open_connection(
            self.upstream_host, self.upstream_port, limit=HEAD_LIMIT
        )
        try:
            up_writer.write(head + body)
            await up_writer.drain()
            raw = b""
            while True:
                answer_head = await _read_head(up_reader)
                raw += answer_head
                line, fields = _split_head(answer_head)
                version, _, rest = line.partition(" ")
                status = int(rest.split(" ")[0])
                if 100 <= status < 200 and status != 101:
                    continue
                break
            closes = _closes(version, fields)
            if method.upper() == "HEAD" or status in (101, 204, 304) or 100 <= status < 200:
                pass
            elif (
                "chunked" in (_field(fields, "transfer-encoding") or "").lower()
                or _field(fields, "content-length") is not None
            ):
                raw += await _read_body(up_reader, fields)
            else:
                raw += await up_reader.read()
                closes = True
            return raw, status, closes
        finally:
            up_writer.close()

    def _with_host(self, head):
        host = f"{self.upstream_host}:{self.upstream_port}".encode("latin-1")
        lines = head[: -len(END)].split(b"\r\n")
        for number, line in enumerate(lines[1:], 1):
            if line.split(b":", 1)[0].strip().lower() == b"host":
                lines[number] = b"Host: " + host
        return b"\r\n".join(lines) + END


def render_record(record):
    """The record as one line per request: METHOD path -> rule-or-none -> status-or-none."""
    return "\n".join(
        f"{e['method']} {e['path']} -> {e['rule'] or 'none'} -> {e['status'] if e['status'] is not None else 'none'}"
        for e in record
    )


def apply_control(proxy, message):
    """One control message, as a dict, for a proxy; the answer is a dict. `op` is one of
    add_rule (method, path, do, times, seconds, bytes), record, or reset."""
    op = message.get("op")
    if op == "add_rule":
        proxy.add_rule(
            message.get("method", "GET"),
            message["path"],
            message.get("do", "pass"),
            times=message.get("times", 1),
            seconds=message.get("seconds", 1),
            nbytes=message.get("bytes", 0),
        )
        return {"ok": True}
    if op == "record":
        return {"ok": True, "record": proxy.record()}
    if op == "reset":
        proxy.reset()
        return {"ok": True}
    return {"ok": False, "error": f"unknown op {op!r}"}


async def _control_line(proxy, reader, writer):
    """One line of JSON in, one line of JSON out."""
    try:
        line = await reader.readline()
        try:
            answer = apply_control(proxy, json.loads(line))
        except (ValueError, KeyError, TypeError, AttributeError) as bad:
            answer = {"ok": False, "error": str(bad)}
        writer.write(json.dumps(answer).encode() + b"\n")
        await writer.drain()
    except (ConnectionError, OSError):
        pass
    finally:
        writer.close()


def send_control(address, message, seconds=10):
    """Send one control message to a proxy's control address, `host:port`, and return its
    answer. This is the client of `--control`."""
    host, _, port = address.rpartition(":")
    with socket.create_connection((host, int(port)), timeout=seconds) as link:
        link.sendall(json.dumps(message).encode() + b"\n")
        data = b""
        while not data.endswith(b"\n"):
            more = link.recv(65536)
            if not more:
                break
            data += more
    return json.loads(data)


async def _serve_by_hand(args):
    host, _, port = args.upstream.rpartition(":")
    listen_host, _, listen_port = args.listen.rpartition(":")
    proxy = FaultProxy(host or "127.0.0.1", int(port), rewrite_host=not args.keep_host, listen_host=listen_host)
    for text in args.rule:
        words = text.split()
        method, path, do = words[:3]
        times = int(words[3]) if len(words) > 3 else 1
        proxy.add_rule(method, path, do, times)
    listening = await proxy.start(int(listen_port))
    control = None
    if args.control:
        control_host, _, control_port = args.control.rpartition(":")
        control = await asyncio.start_server(
            lambda r, w: _control_line(proxy, r, w), control_host, int(control_port)
        )
    print(f"listening on {listen_host}:{listening}, forwarding to {args.upstream}", flush=True)
    try:
        await asyncio.Event().wait()
    finally:
        if control is not None:
            control.close()
        await proxy.close()
        print(render_record(proxy.record()))


def main():
    parser = argparse.ArgumentParser(description="A TCP proxy that breaks one coming HTTP exchange on purpose.")
    parser.add_argument("--upstream", help="host:port to forward to")
    parser.add_argument("--listen", default="127.0.0.1:0", help="host:port to listen on (default: 127.0.0.1, any port)")
    parser.add_argument("--rule", action="append", default=[], help='"METHOD PATH-REGEX ACTION [TIMES]"')
    parser.add_argument("--keep-host", action="store_true", help="do not rewrite the Host header")
    parser.add_argument(
        "--control",
        help="host:port. With --upstream: also listen there for one JSON line per connection that adds a rule, "
        "reads the record, or resets (used by tools/copy_test.py inside the proxy's container). "
        "With --send: the address of a running proxy's control port",
    )
    parser.add_argument("--send", help="send this JSON message to --control, print the answer, and exit")
    args = parser.parse_args()
    if args.send:
        if not args.control:
            parser.error("--send needs --control")
        print(json.dumps(send_control(args.control, json.loads(args.send))))
        return
    if not args.upstream:
        parser.error("--upstream is required")
    try:
        asyncio.run(_serve_by_hand(args))
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
