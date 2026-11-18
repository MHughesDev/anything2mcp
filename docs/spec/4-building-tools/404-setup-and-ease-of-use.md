# 404. Setup and ease of use

The ease-of-use target: from nothing to a first working read tool in under ten minutes, with no file written by hand. This section designs the path from install to a first tool. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

```text
anything2mcp init                 # choose a profile; creates config and the local store
anything2mcp add https://app...   # probe the app, detect sign-in, sign in once
anything2mcp learn                # watch a person or an agent use the app; draft files
anything2mcp test                 # run the publish gates
anything2mcp publish              # make an immutable version
anything2mcp connect [<profile>]  # print the MCP client block; a profile kept as data writes it into that client's file
anything2mcp serve                # run as a server in the configured profile
```

Principles behind the commands:

- **One install.** `pip install anything2mcp`, `npm install -g anything2mcp`, `cargo install anything2mcp`, a downloadable binary, or a container image all deliver the same binary.
- **Sensible defaults.** A new install is the Solo profile: stdio, a local store, the user's own sign-in, approvals on for writes. Nothing needs configuring to start.
- **`anything2mcp doctor`** checks the browser, key store, network reach to each target, session health, and clock, and prints a fix for each problem.
- **Every error names its fix.** Failures carry a code, a plain sentence, and the next command or tool to run.
- **Agent-ready project.** A project created by `init` ([section 220](../2-kernel/220-actions-and-the-authoring-interface.md)) includes an `AGENTS.md` that teaches any coding agent the build workflow, the file formats, and the rules, so the agent can extend a target without a human explaining Anything2MCP.
- **Starter templates** for common shapes: a JSON-API app, a form-post app, an app with an OpenAPI document, an app with its own MCP server, and a desktop app.
- **Clone-and-fit.** The repository can be cloned into an existing project; `anything2mcp init --embed` wires it in as a library or sidecar and adds the agent instructions.
