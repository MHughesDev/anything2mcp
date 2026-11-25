# 404. Setup and ease of use

The ease-of-use target: from nothing to a first working read tool in under ten minutes, with no file written by hand. This section designs the path from install to a first tool. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

The path an agent with no knowledge of the project takes, as measured ([H-18's result](../../hypotheses/H-18-result.md), about a minute and a quarter):

```text
anything2mcp init                                   # the project folder and its guidance file
anything2mcp add <name> --address <url> --kind <k>  # the target, the scout, and its report: reads drafted and tried
anything2mcp ready <name>                           # each proven read's test, fixture, promotion, gates, and the publish asked for
                                                    # the person reads the page and presses, past the computer's own prompt
anything2mcp tool search                            # the published tools
anything2mcp tool call <tool> --input '{...}'       # one of them
anything2mcp connect [<profile>]                    # print the MCP client block; a client profile kept as data writes it into that client's file
```

A system with no description that the scout can read is learned from the person's own use: `anything2mcp learn <name>` asks for a capture, the person begins and ends it on the engine's page, and `anything2mcp capture stop <name>` scouts what it saw. `anything2mcp serve` runs the engine as a server in the configured deployment shape, which is after the first release.

Principles behind the commands:

- **One install.** `pip install anything2mcp`, `npm install -g anything2mcp`, a downloadable binary, or a container image all deliver the same binary.
- **Sensible defaults.** A new install is the Solo deployment shape: stdio, a local store, the user's own sign-in, approvals on for writes. Nothing needs configuring to start.
- **`anything2mcp doctor`** checks the browser, key store, network reach to each target, session health, and clock, and prints a fix for each problem.
- **Every error names its fix.** Failures carry a code, a plain sentence, and the next command or tool to run.
- **Agent-ready project.** A project created by `init` ([section 220](../2-kernel/220-actions-and-the-authoring-interface.md)) includes an `AGENTS.md` that teaches any coding agent the build workflow, the file formats, and the rules, so the agent can extend a target without a human explaining Anything2MCP.
- **Not in the first release: starter templates and clone-and-fit.** Templates for common shapes (a JSON-API app, a form-post app, an app with an OpenAPI document, an app with its own MCP server, a desktop app) and an `init --embed` that wires the product into an existing project as a library or sidecar are designed and not built. The measured path needs neither, since the scout drafts from the system itself, and each would be a second way to start a project that the first-hour test does not hold ([decision 0456](../../decisions/0456-the-first-release-leaves-out-what-its-first-hour-never-meets-and-a-shortcut-is-offered-only-when-built.md), point 5).
