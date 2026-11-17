# anything2mcp

Anything2MCP latches onto existing software and systems and exposes them to AI agents as deterministic, verified MCP tools. It is being built in the open at <https://github.com/MHughesDev/anything2mcp>, and is not ready for use yet.

`pip install anything2mcp` gives two things:

- The `anything2mcp` program, with `a2m` as a short name for it. The wheel carries the prebuilt binary for your platform; no Rust toolchain is needed.
- `import anything2mcp`: a thin client of the local engine, using only the standard library.

```python
import anything2mcp

with anything2mcp.connect(project=".") as client:
    status = client.engine_status()          # a typed method per action
    print(status.ok, status.exit_class, status.result)
    run = client.tool_call(tool="example_order_get", input={"order_id": "A-100"})
    other = client.call("engine.status")     # or any action by its name
```

The methods are generated from the OpenAPI document the engine serves, one per action, named as the action names it with the dot as an underscore. The client does not start the engine; start it with `anything2mcp serve`. It reaches the engine on macOS and Linux by its socket, and on Windows by its named pipe. On Windows it checks that the process serving the pipe runs as you before it sends anything, and a read has no timeout there.

Licensed under the MIT licence.
