# 14. Edge cases and large applications

Each common behavior of real applications has one built-in handling rule, so operation files declare the behavior instead of coding around it.

| Behavior | Built-in handling |
| --- | --- |
| Pagination: pages, cursors, "load more", infinite scroll | `list` verb with a pager per style; results paged by cursor to the caller |
| Long-running jobs inside the app (exports, reports) | `call` then poll a declared status until done or a time limit |
| File upload and download | Declared on the step; files pass through a scoped temporary store, never through the agent's context |
| Optimistic locking (ETag, version numbers, "changed by another user") | Read the version first, send it with the write; a conflict is `rejected` with both versions shown |
| Rotating anti-forgery tokens | A declared bootstrap request or page field supplies the token per request |
| Requests signed by front-end code | `ui` or `page_http` only; `http` is marked unsupported for that endpoint |
| WebSockets and server-sent events | Observed for confirmation in `ui` and `page_http`; not driven directly in the first release |
| GraphQL, including persisted queries | Endpoint catalog stores the operation name and query hash |
| Multi-step wizards and drafts | Pipeline steps with a checkpoint per page; resume returns to the last confirmed page |
| Workspaces and tenants | Workspace is part of the session; switching is an explicit operation |
| Humans editing the same record | Optimistic locking where available; otherwise read immediately before write and compare |
| Maintenance windows and outages | Surface health check; calls queue or fail with `unavailable` and a retry time |
| Locales, time zones, number formats | Fixed per target; field types normalize both sides |
| Feature flags and A/B variants | Ordered locators and variant-aware screen models; an unknown variant stops |
| Bulk volumes | Pipelines batch by record with rate budgets and per-record checkpoints |

**Large applications.** Big systems with many modules, services, and origins are handled as one target with many surfaces, organized by module:

- **Modules.** Files live under `targets/<target>/<module>/`, and tool names carry the module, such as `example_purchasing_order_create`. Agents find tools by module through search, so hundreds of operations stay usable.
- **Shared pieces.** Field types, auth, and environment are defined once per target and shared by all modules.
- **Several origins and services.** Micro-frontends, API gateways, and separate services are separate surfaces with a declared relationship, such as sharing one sign-in.
- **Cross-target pipelines.** A pipeline can read from one target and write to another, such as reading records from one application and entering them in another, with each target's session and policy applied separately.
- **Load on demand.** Only modules in use are loaded, so a large target does not slow the server.
