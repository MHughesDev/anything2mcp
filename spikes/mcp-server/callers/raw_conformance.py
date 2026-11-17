#!/usr/bin/env python3
"""Hand-written conformance caller. Standard library only; it uses no client library.

Usage: raw_conformance.py <server-binary> <tools.json> <report.json>

It starts the server several times over stdio, sends raw JSON-RPC lines, and checks
each answer against the protocol text. Each check has an id that the research note
and the checklist for the real server use.
"""
import json
import queue
import subprocess
import sys
import threading
import time

V_MODERN = "2026-07-28"
V_LEGACY = "2025-11-25"
TOOL = "example_orders_order_get"
PV = "io.modelcontextprotocol/protocolVersion"
CC = "io.modelcontextprotocol/clientCapabilities"
CI = "io.modelcontextprotocol/clientInfo"


class Proc:
    def __init__(self, binary, tools, revisions=None):
        argv = [binary, tools] + ([revisions] if revisions else [])
        self.p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, bufsize=1)
        self.q = queue.Queue()
        self.bad_lines = []
        self.log = []
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.p.stdout:
            try:
                msg = json.loads(line)
                if not isinstance(msg, dict):
                    raise ValueError("not an object")
            except ValueError:
                self.bad_lines.append(line)
                continue
            self.log.append({"dir": "server", "msg": msg})
            self.q.put(msg)

    def send_raw(self, text):
        self.log.append({"dir": "client", "raw": text})
        self.p.stdin.write(text + "\n")
        self.p.stdin.flush()

    def send(self, msg):
        self.send_raw(json.dumps(msg))

    def recv(self, timeout=3.0):
        try:
            return self.q.get(timeout=timeout)
        except queue.Empty:
            return None

    def call(self, id_, method, params=None, timeout=3.0):
        """Send a request; return (answer, notifications seen before it)."""
        msg = {"jsonrpc": "2.0", "id": id_, "method": method}
        if params is not None:
            msg["params"] = params
        self.send(msg)
        seen = []
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            got = self.recv(max(0.01, end - time.monotonic()))
            if got is None:
                break
            if got.get("id") == id_ and "method" not in got:
                return got, seen
            seen.append(got)
        return None, seen

    def close(self):
        self.p.stdin.close()
        try:
            return self.p.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.p.kill()
            return None


def meta(version=V_MODERN, **extra):
    m = {PV: version, CC: {}, CI: {"name": "raw-conformance", "version": "0"}}
    m.update(extra)
    return m


def init(p, version=V_LEGACY):
    r, _ = p.call(1, "initialize", {"protocolVersion": version, "capabilities": {},
                                    "clientInfo": {"name": "raw-conformance", "version": "0"}})
    p.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    return r


def code(r):
    return (r or {}).get("error", {}).get("code")


def run(binary, tools):
    results = []
    transcripts = {}

    def check(cid, what, ok, detail=""):
        results.append({"id": cid, "what": what, "pass": bool(ok), "detail": str(detail)[:300]})

    # ---- Handshake era, one process.
    p = Proc(binary, tools)
    r, _ = p.call("pre", "ping")
    check("L01", "ping before initialize is answered with an empty result", r and r.get("result") == {}, r)
    r = init(p)
    res = (r or {}).get("result", {})
    check("L02", "initialize 2025-11-25 is answered with protocolVersion 2025-11-25",
          res.get("protocolVersion") == V_LEGACY, r)
    check("L03", "initialize result has capabilities.tools and serverInfo name and version",
          "tools" in res.get("capabilities", {}) and res.get("serverInfo", {}).get("name")
          and res.get("serverInfo", {}).get("version") is not None, res)
    check("L04", "initialize result carries no session-era surprise: no resultType needed", True, "")
    check("L05", "notifications/initialized gets no answer", p.recv(0.3) is None)
    r, _ = p.call(2, "ping")
    check("L06", "ping is answered with an empty result", r and r.get("result") == {}, r)
    r, _ = p.call("three", "tools/list")
    check("L07", "a string id comes back as the same string", r and r.get("id") == "three", r)
    tl = (r or {}).get("result", {})
    t0 = (tl.get("tools") or [{}])[0]
    check("L08", "tools/list has the tool with name, description, inputSchema of type object",
          t0.get("name") == TOOL and t0.get("description") and t0.get("inputSchema", {}).get("type") == "object", t0)
    check("L09", "the tool carries outputSchema of type object", t0.get("outputSchema", {}).get("type") == "object")
    check("L10", "the tool carries annotations with readOnlyHint true",
          t0.get("annotations", {}).get("readOnlyHint") is True, t0.get("annotations"))
    check("L11", "a single page has no nextCursor", "nextCursor" not in tl)
    r2, _ = p.call(4, "tools/list", {})
    check("L12", "two tools/list answers list the same tools in the same order",
          r2 and json.dumps(r2["result"].get("tools")) == json.dumps(tl.get("tools")))
    r, _ = p.call(5, "tools/call", {"name": TOOL, "arguments": {"order_id": "A-1001"}})
    cr = (r or {}).get("result", {})
    sc = cr.get("structuredContent", {})
    check("L13", "tools/call returns structuredContent that fits the output schema",
          sc == {"order_id": "A-1001", "status": "open", "total": "118.40", "currency": "EUR", "lines": 3}, cr)
    check("L14", "tools/call returns a text content item too",
          cr.get("content") and cr["content"][0].get("type") == "text" and cr["content"][0].get("text"))
    check("L15", "a successful call is not marked isError", cr.get("isError") in (False, None))
    r, _ = p.call(6, "tools/call", {"name": "example_no_such_tool", "arguments": {}})
    check("L16", "an unknown tool is the protocol error -32602", code(r) == -32602, r)
    r, _ = p.call(7, "tools/call", {"name": TOOL, "arguments": {}})
    check("L17", "bad arguments are a tool result with isError true, not a protocol error",
          (r or {}).get("result", {}).get("isError") is True, r)
    r, _ = p.call(8, "tools/call", {"name": TOOL, "arguments": {"order_id": "Z-0"}})
    check("L18", "a failed call is a tool result with isError true and text content",
          (r or {}).get("result", {}).get("isError") is True and (r["result"].get("content") or [{}])[0].get("text"), r)
    r, _ = p.call(9, "example/no_such_method")
    check("L19", "an unknown method is -32601", code(r) == -32601, r)
    p.send_raw("{not json")
    r = p.recv()
    check("L20", "a line that is not JSON is -32700 with a null id",
          code(r) == -32700 and r.get("id", "absent") is None, r)
    p.send({"jsonrpc": "2.0", "id": 10})
    r = p.recv()
    check("L21", "a request with no method is -32600", code(r) == -32600, r)
    p.send({"jsonrpc": "2.0", "method": "notifications/example_unknown"})
    check("L22", "an unknown notification gets no answer", p.recv(0.3) is None)
    r, seen = p.call(11, "tools/call", {"name": TOOL, "arguments": {"order_id": "A-1002", "wait_ms": 200},
                                         "_meta": {"progressToken": "tok-1"}})
    prog = [m for m in seen if m.get("method") == "notifications/progress"]
    vals = [m["params"].get("progress") for m in prog]
    check("L23", "progress notifications carry the caller's token and rise, before the result",
          r and "result" in r and prog and all(m["params"].get("progressToken") == "tok-1" for m in prog)
          and vals == sorted(vals) and len(set(vals)) == len(vals), vals)
    r, seen = p.call(12, "tools/call", {"name": TOOL, "arguments": {"order_id": "A-1002", "wait_ms": 200}})
    check("L24", "no progress is sent to a caller that did not ask",
          r and not [m for m in seen if m.get("method") == "notifications/progress"], seen)
    p.send({"jsonrpc": "2.0", "id": 13, "method": "tools/call",
            "params": {"name": TOOL, "arguments": {"order_id": "A-1002", "wait_ms": 1200}}})
    time.sleep(0.1)
    p.send({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 13, "reason": "test"}})
    r14, seen = p.call(14, "ping", timeout=3)
    late = seen + [m for m in iter(lambda: p.recv(1.5), None)]
    answered = [m for m in late if m.get("id") == 13]
    check("L25", "the server answers other requests while a call is in flight", r14 and r14.get("result") == {}, r14)
    check("L26", "a cancelled call sends no result afterwards (an error answer is tolerated by the handshake revisions)",
          not [m for m in answered if "result" in m], answered)
    transcripts["cancel_legacy_answer"] = answered
    rc = p.close()
    check("F01", "every line on standard output was one JSON object", not p.bad_lines, p.bad_lines[:2])
    check("F02", "the server exits with status 0 when its input is closed", rc == 0, rc)
    transcripts["legacy"] = p.log

    # ---- Handshake negotiation, fresh processes.
    for cid, asked in (("L27", "2024-11-05"), ("L28", "2099-01-01")):
        p = Proc(binary, tools)
        r = init(p, asked)
        got = (r or {}).get("result", {}).get("protocolVersion")
        check(cid, f"initialize asking {asked} is answered with a handshake revision the server serves",
              got == V_LEGACY, r)
        p.close()

    # ---- Modern era, one fresh process, no handshake at all.
    p = Proc(binary, tools)
    r, _ = p.call(1, "server/discover", {"_meta": meta()})
    d = (r or {}).get("result", {})
    check("M01", "server/discover lists supportedVersions with both revisions",
          set(d.get("supportedVersions", [])) == {V_LEGACY, V_MODERN}, d)
    check("M02", "server/discover carries capabilities.tools, resultType complete, ttlMs and cacheScope",
          "tools" in d.get("capabilities", {}) and d.get("resultType") == "complete"
          and isinstance(d.get("ttlMs"), int) and d.get("cacheScope") in ("public", "private"), d)
    r, _ = p.call(2, "tools/list", {"_meta": meta()})
    tl = (r or {}).get("result", {})
    check("M03", "tools/list with per-request metadata works with no handshake",
          (tl.get("tools") or [{}])[0].get("name") == TOOL, r)
    check("M04", "the modern tools/list carries resultType complete, ttlMs and cacheScope",
          tl.get("resultType") == "complete" and isinstance(tl.get("ttlMs"), int)
          and tl.get("cacheScope") in ("public", "private"), {k: tl.get(k) for k in ("resultType", "ttlMs", "cacheScope")})
    r, _ = p.call(3, "tools/call", {"name": TOOL, "arguments": {"order_id": "A-1001"}, "_meta": meta()})
    cr = (r or {}).get("result", {})
    check("M05", "the modern tools/call returns structuredContent and resultType complete",
          cr.get("structuredContent", {}).get("total") == "118.40" and cr.get("resultType") == "complete", cr)
    check("M06", "modern results name the server in _meta (a SHOULD)",
          "io.modelcontextprotocol/serverInfo" in (cr.get("_meta") or {}), cr.get("_meta"))
    r, _ = p.call(4, "tools/list", {"_meta": meta("1900-01-01")})
    e = (r or {}).get("error", {})
    check("M07", "an unserved version is -32022 with data.supported and data.requested",
          e.get("code") == -32022 and V_MODERN in e.get("data", {}).get("supported", [])
          and e.get("data", {}).get("requested") == "1900-01-01", r)
    r, _ = p.call(5, "tools/list", {"_meta": {PV: V_MODERN}})
    check("M08", "a modern request without clientCapabilities is -32602", code(r) == -32602, r)
    r, _ = p.call(6, "ping", {"_meta": meta()})
    check("M09", "ping does not exist in the modern revision: -32601", code(r) == -32601, r)
    r, _ = p.call(7, "tools/list")
    check("M10", "a request with neither a handshake nor a version is an error, never a result",
          r and "error" in r, r)
    r, _ = p.call(8, "tools/call", {"name": "example_no_such_tool", "arguments": {}, "_meta": meta()})
    check("M11", "an unknown tool is -32602 in the modern revision too", code(r) == -32602, r)
    r, seen = p.call(9, "tools/call", {"name": TOOL, "arguments": {"order_id": "A-1002", "wait_ms": 200},
                                        "_meta": meta(progressToken=7)})
    prog = [m for m in seen if m.get("method") == "notifications/progress"]
    check("M12", "progress works in the modern revision, with a number token",
          r and "result" in r and prog and all(m["params"].get("progressToken") == 7 for m in prog), prog[:1])
    p.send({"jsonrpc": "2.0", "id": 10, "method": "tools/call",
            "params": {"name": TOOL, "arguments": {"order_id": "A-1002", "wait_ms": 1200}, "_meta": meta()}})
    time.sleep(0.1)
    p.send({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 10}})
    r11, seen = p.call(11, "tools/list", {"_meta": meta()}, timeout=3)
    late = seen + [m for m in iter(lambda: p.recv(1.5), None)]
    answered = [m for m in late if m.get("id") == 10]
    check("M13", "a cancelled call sends nothing further (the modern revision says MUST NOT)", not answered, answered)
    transcripts["cancel_modern_answer"] = answered
    rc = p.close()
    check("F03", "modern process: clean lines and exit status 0", not p.bad_lines and rc == 0, (p.bad_lines[:1], rc))
    transcripts["modern"] = p.log

    # ---- Both eras in one process.
    p = Proc(binary, tools)
    init(p)
    r, _ = p.call(2, "tools/list", {"_meta": meta()})
    check("D01", "after a handshake, the same process still answers a modern request (optional in the text)",
          (r or {}).get("result", {}).get("resultType") == "complete", r)
    p.close()
    p = Proc(binary, tools)
    p.call(1, "server/discover", {"_meta": meta()})
    r = init(p)
    check("D02", "after a server/discover probe, initialize still works (the dual-era client's fallback path)",
          (r or {}).get("result", {}).get("protocolVersion") == V_LEGACY, r)
    r, _ = p.call(3, "tools/list")
    check("D03", "and the handshake session then lists tools", (r or {}).get("result", {}).get("tools"), r)
    p.close()

    # ---- A server set to one revision only.
    p = Proc(binary, tools, V_MODERN)
    r = init(p)
    e = (r or {}).get("error", {})
    check("S01", "modern only: initialize is an error that names the versions served",
          r and "error" in r and V_MODERN in json.dumps(e), r)
    transcripts["modern_only_initialize"] = r
    p.close()
    p = Proc(binary, tools, V_LEGACY)
    r, _ = p.call(1, "server/discover", {"_meta": meta()})
    check("S02", "handshake only: a modern probe gets an error that is not a modern one (-32020 to -32022), so a dual-era client falls back",
          r and "error" in r and code(r) not in (-32020, -32021, -32022), r)
    transcripts["legacy_only_discover"] = r
    r = init(p)
    check("S03", "handshake only: initialize then works in the same process",
          (r or {}).get("result", {}).get("protocolVersion") == V_LEGACY, r)
    p.close()
    return results, transcripts


def main():
    binary, tools, report = sys.argv[1:4]
    results, transcripts = run(binary, tools)
    failed = [r for r in results if not r["pass"]]
    for r in results:
        print(("pass " if r["pass"] else "FAIL ") + r["id"] + "  " + r["what"] + ("" if r["pass"] else "   <- " + r["detail"]))
    print(f"{len(results) - len(failed)} of {len(results)} passed")
    with open(report, "w", encoding="utf-8") as f:
        json.dump({"server": binary, "results": results, "transcripts": transcripts}, f, indent=1)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
