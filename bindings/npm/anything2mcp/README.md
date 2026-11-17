# anything2mcp

Anything2MCP latches onto existing software and systems and exposes them to AI agents as deterministic, verified MCP tools. It is being built in the open at <https://github.com/MHughesDev/anything2mcp>, and is not ready for use yet.

`npm install anything2mcp` gives the `anything2mcp` program, with `a2m` as a short name for it. The program itself comes in a separate package for each platform, such as `@anything2mcp/linux-x64`, which npm installs as an optional dependency. Nothing is downloaded by a script, and no install script runs.

If you install with optional dependencies left out, the program is missing, and `anything2mcp` says which package to install.

Licensed under the MIT licence.
