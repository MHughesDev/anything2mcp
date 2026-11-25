# anything2mcp

Anything2MCP latches onto existing software and systems and exposes them to AI agents as deterministic, verified MCP tools. It is being built in the open at <https://github.com/MHughesDev/anything2mcp>, and is not ready for use yet.

`npm install anything2mcp` gives the `anything2mcp` program, with `a2m` as a short name for it. The program itself comes in a separate package for each platform, such as `@anything2mcp/linux-x64`, which npm installs as an optional dependency. Nothing is downloaded by a script, and no install script runs.

The package also has a thin client of the local engine, with no dependency:

```js
const { connect } = require("anything2mcp");

const client = await connect({ project: "." });
const status = await client.engineStatus();     // a typed method per action
console.log(status.ok, status.exitClass, status.result);
await client.toolCall({ tool: "example_order_get", input: { order_id: "A-100" } });
client.close();
```

The methods and their types (`.d.ts`, written by a generator, no compile step) come from the OpenAPI document the engine serves. When no engine runs, `connect` has the program of the platform package start one, by running `anything2mcp engine start` as any command would, and then connects to it; `connect({ start: false })` starts nothing and rejects with `engine_unreachable`. It reaches it on macOS and Linux by its socket, after checking that the socket belongs to you. On Windows Node cannot tell who serves a pipe, so `connect` runs the program's `anything2mcp channel` command, which opens the engine's pipe after checking that it is served as you, as every command does, and then carries the client's bytes without reading them. `binary` names the program; it is the platform package's by default.

If you install with optional dependencies left out, the program is missing, and `anything2mcp` says which package to install.

Licensed under the MIT licence.
