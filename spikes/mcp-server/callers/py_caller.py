#!/usr/bin/env python3
"""Caller built on the reference client library for Python (the `mcp` package on PyPI).

Usage: py_caller.py <mode> <report.json> <server command... | http URL>
  mode: auto | legacy | 2026-07-28
"""
import asyncio
import json
import sys
from importlib.metadata import version

import anyio
from mcp import Client, StdioServerParameters
from mcp.shared.exceptions import MCPError

TOOL = "example_orders_order_get"


async def main():
    mode, report_path, command = sys.argv[1], sys.argv[2], sys.argv[3:]
    report = {"client": "reference client library for Python", "package": "mcp " + version("mcp"),
              "mode": mode, "steps": {}}
    steps = report["steps"]

    async def step(name, coro_fn):
        try:
            steps[name] = {"ok": True, "got": await coro_fn()}
        except MCPError as e:
            steps[name] = {"ok": False, "error": {"code": e.code, "message": str(e)}}
        except Exception as e:  # the report must say what the library raised
            steps[name] = {"ok": False, "error": {"type": type(e).__name__, "message": str(e)[:300]}}

    # A server command, or the URL of a Streamable HTTP endpoint that is already running.
    params = command[0] if command[0].startswith("http") else StdioServerParameters(command=command[0], args=command[1:])
    try:
        async with Client(params, mode=mode, read_timeout_seconds=10) as client:
            report["negotiated"] = client.protocol_version
            info = client.server_info
            report["server_info"] = info.model_dump(exclude_none=True) if info else None
            report["server_capabilities"] = client.server_capabilities.model_dump(exclude_none=True)

            async def list_tools():
                r = await client.list_tools()
                t = r.tools[0]
                return {"count": len(r.tools), "name": t.name, "has_output_schema": t.output_schema is not None,
                        "annotations": t.annotations.model_dump(exclude_none=True, by_alias=True) if t.annotations else None}

            async def call(args, **kw):
                r = await client.call_tool(TOOL, args, **kw)
                return {"is_error": r.is_error, "structured": r.structured_content,
                        "text": [c.text for c in r.content if getattr(c, "text", None)]}

            async def unknown():
                r = await client.call_tool("example_no_such_tool", {})
                return {"is_error": r.is_error}

            async def with_progress():
                seen = []

                async def on_progress(progress, total, message):
                    seen.append(progress)

                r = await call({"order_id": "A-1002", "wait_ms": 200}, progress_callback=on_progress)
                r["progress_seen"] = seen
                return r

            async def cancelled():
                with anyio.move_on_after(0.2) as scope:
                    await call({"order_id": "A-1002", "wait_ms": 2000})
                await anyio.sleep(0.3)
                return {"client_gave_up": scope.cancelled_caught}

            await step("tools_list", list_tools)
            await step("call_ok", lambda: call({"order_id": "A-1001"}))
            await step("call_not_found", lambda: call({"order_id": "Z-0"}))
            await step("call_bad_arguments", lambda: call({}))
            await step("call_unknown_tool", unknown)
            await step("call_with_progress", with_progress)
            await step("call_then_cancel", cancelled)
            await step("call_after_cancel", lambda: call({"order_id": "A-1002"}))
            await step("ping", lambda: client.send_ping())
            if isinstance(steps["ping"].get("got"), object) and steps["ping"]["ok"]:
                steps["ping"]["got"] = "empty result"
    except BaseException as e:  # a failed connect is a result too
        report["connect_error"] = {"type": type(e).__name__, "message": str(e)[:400]}
    with open(report_path, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=1, default=str)
    print(json.dumps(report, default=str))


asyncio.run(main())
