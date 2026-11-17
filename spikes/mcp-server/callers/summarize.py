#!/usr/bin/env python3
"""Turns the reports and wire logs in results/ into one table. Standard library only."""
import glob
import json
import os
import sys

root = sys.argv[1]
STEPS = ["tools_list", "call_ok", "call_not_found", "call_bad_arguments", "call_unknown_tool",
         "call_with_progress", "call_then_cancel", "call_after_cancel", "ping"]
PV = "io.modelcontextprotocol/protocolVersion"
CC = "io.modelcontextprotocol/clientCapabilities"


def wire(path):
    """What the client asked for, read from the recorded lines."""
    opened, asked, caps, cancelled, answered = [], set(), None, False, None
    if not os.path.exists(path):
        return "no wire log"
    for line in open(path, encoding="utf-8"):
        d = json.loads(line)
        m = d["msg"]
        if d["dir"] == "client":
            if m.get("method") == "initialize":
                opened.append("initialize")
                asked.add(m["params"].get("protocolVersion"))
                caps = m["params"].get("capabilities")
            meta = (m.get("params") or {}).get("_meta") or {}
            if PV in meta:
                if m.get("method") == "server/discover":
                    opened.append("server/discover")
                asked.add(meta[PV])
                caps = meta.get(CC) if caps is None else caps
            if m.get("method") == "notifications/cancelled":
                cancelled = True
        elif m.get("id") is not None and "result" in m and "protocolVersion" in m["result"]:
            answered = m["result"]["protocolVersion"]
    first = " then ".join(dict.fromkeys(opened)) or "nothing"
    return (f"opened with {first}; asked {', '.join(sorted(a for a in asked if a))}; capabilities {json.dumps(caps)}"
            + (f"; handshake answered {answered}" if answered else "")
            + ("; sent notifications/cancelled" if cancelled else ""))


def cell(step):
    if step is None:
        return "-"
    if step["ok"]:
        got = step["got"]
        if isinstance(got, dict) and "progress_seen" in got:
            return "ok " + str(len(got["progress_seen"])) + " of 4"
        if isinstance(got, dict) and got.get("is_error"):
            return "ok, isError"
        return "ok"
    e = step["error"]
    return "error " + str(e.get("code") or e.get("type"))


rows = []
for path in sorted(glob.glob(os.path.join(root, "*.json"))):
    name = os.path.basename(path)[:-5]
    if name.startswith("raw-"):
        continue
    r = json.load(open(path, encoding="utf-8"))
    w = wire(os.path.join(root, "wire-" + name + ".jsonl"))
    connected = "connect_error" not in r
    rows.append((name, r, w, connected))

print("| Run (caller, revisions served, server) | Package | Connected | Negotiated | "
      + " | ".join(STEPS) + " |")
print("| --- | --- | --- | --- |" + " --- |" * len(STEPS))
for name, r, w, connected in rows:
    steps = r.get("steps", {})
    neg = r.get("negotiated") or ""
    if "wire log" in str(neg) or not neg:
        neg = w.split("handshake answered ")[1].split(";")[0] if "handshake answered" in w else ""
    print(f"| {name} | {r['package']} | {'yes' if connected else 'no: ' + r['connect_error']['message'][:60].replace('|', '/')} | {neg} | "
          + " | ".join(cell(steps.get(s)) for s in STEPS) + " |")
print()
print("| Run | What the client sent |")
print("| --- | --- |")
for name, r, w, connected in rows:
    print(f"| {name} | {w} |")
