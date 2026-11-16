# 25. Setup and ease of use

The ease-of-use target: from nothing to a first working read tool in under ten minutes, with no file written by hand.

```text
web2mcp init                 # choose a profile; creates config and the local store
web2mcp add https://app...   # probe the app, detect sign-in, sign in once
web2mcp learn                # watch a person or an agent use the app; draft files
web2mcp test                 # run the publish gates
web2mcp publish              # make an immutable version
web2mcp connect claude       # write the MCP client config; also: vscode, cursor, copilot-studio
web2mcp serve                # run as a server in the configured profile
```

Principles behind the commands:

- **One install.** `pip install web2mcp`, `npm install -g w2mcp`, `cargo install web2mcp`, a downloadable binary, or a container image all deliver the same binary.
- **Sensible defaults.** A new install is the Solo profile: stdio, a local store, the user's own sign-in, approvals on for writes. Nothing needs configuring to start.
- **`web2mcp doctor`** checks the browser, key store, network reach to each target, session health, and clock, and prints a fix for each problem.
- **Every error names its fix.** Failures carry a code, a plain sentence, and the next command or tool to run.
- **Agent-ready project.** A project created by `init` ([section 40](40-authoring-interface.md)) includes an `AGENTS.md` that teaches any coding agent the build workflow, the file formats, and the rules, so the agent can extend a target without a human explaining Web2MCP.
- **Starter templates** for common shapes: a JSON-API app, a form-post app, an app with an OpenAPI document, an app with its own MCP server, and a desktop app.
- **Clone-and-fit.** The repository can be cloned into an existing project; `web2mcp init --embed` wires it in as a library or sidecar and adds the agent instructions.
