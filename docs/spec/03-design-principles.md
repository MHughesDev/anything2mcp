# 3. Design principles

1. **The caller decides what; the system decides how.** Agents and applications name an operation and its inputs. They never handle clicks, selectors, endpoints, or tokens.
2. **One operation, many bindings.** An operation is defined once. Each mode that can carry it is a binding, verified to produce the same result. The runtime chooses among verified bindings by declared rules.
3. **Build with AI, run without it.** Exploration, inference, and repair happen at build time and end in reviewed files. Run time is a fixed replay.
4. **Bring your own agent.** The building agent is whatever the user already uses. The system supplies tools and evidence, not a model.
5. **Verify, never assume.** A write is done when the application shows the value back, not when a request returns.
6. **Fail early, loudly, and resumably.** Validate before acting. Stop at the first surprise, name it, keep the checkpoint.
7. **Most stable channel first, but proven.** Prefer an official API, then site tools, then direct HTTP, then in-page HTTP, then the UI tree. A faster binding is used only after it has matched the UI's behavior.
8. **Knowledge lives in one place.** Locators only in screen models, endpoints only in the endpoint catalog, units only in field types, business steps only in operations and pipelines.
9. **The user's rights, no more.** The system acts with the user's existing permissions or a dedicated account's, never around them. It identifies itself as automation and stops when blocked.
10. **Secrets never reach a caller.** Sessions, tokens, and passwords are used inside drivers and never appear in inputs, outputs, logs, or traces.
11. **Unknown means stop.** A control, platform, response shape, or page variant with no rule is a named failure, never a guess.
12. **Small core, plug-in edges.** Drivers, auth providers, readers, and widget drivers are plug-ins behind stable interfaces, so new kinds of applications are added without touching the core.

**Priority order.** When principles pull against each other: safe first, then correct, then fast, then easy. Inside that order, every path has a speed budget, and every feature must work with its defaults before it offers a setting.
