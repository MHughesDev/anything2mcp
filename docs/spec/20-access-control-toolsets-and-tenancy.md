# 20. Access control, toolsets, and tenancy

Administrators decide which tools exist for whom. Three pieces do this: principals, roles and grants, and toolsets.

**Principals.** Everyone and everything that calls Anything2MCP is a principal: a person, a group, an AI agent with its own identity, or an application with an API key. On a company server, people and groups come from the identity provider; nothing is managed twice.

**Roles.**

| Role | Can do |
| --- | --- |
| Viewer | Call `read` operations they are granted |
| Operator | Viewer, plus start `write` runs they are granted |
| Approver | Approve write runs for the targets they are assigned |
| Builder | Use the authoring tools, create drafts, run tests; cannot publish alone |
| Publisher | Review and publish versions; cannot approve their own drafts |
| Auditor | Read audit logs and traces; no calls |
| Admin | Manage targets, grants, toolsets, sessions, settings, and kill switches |

**Grants** bind a principal to a scope: a target, a module, an operation pattern such as `example_order_*`, a maximum effect class, and an identity mode. Everything is denied unless granted. Grants compile to policies evaluated by the policy engine ([section 45](45-policy-engine.md)).

**Toolsets** are how created tasks and workflows are exposed to a given set of users. A toolset is a named, versioned selection of pipelines and operations with its own MCP endpoint, such as `/mcp/estimating`.

- Each toolset lists who may connect, which identity mode its runs use, and its approval policy.
- One server hosts many toolsets: one for a team's hosted agent, one read-only set for reporting, one for a developer's build agent.
- Under the 2026-07-28 MCP specification, tool lists no longer vary per connection ([changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog)), so a toolset's list is the same for everyone connected to it, and per-user rights are enforced when a tool is called. A caller without the grant gets a clear `policy` result, not a hidden failure.

**Workspaces** separate departments or customers on one installation. A workspace has its own targets, sessions, vault namespace, toolsets, audit stream, and admins. Nothing crosses a workspace boundary without an explicit cross-workspace grant.

**Separation of duties**, enforced by the system:

- The person who drafts a version cannot be its only publisher.
- An agent's identity can never hold the Approver role.
- Admin actions are themselves audited and can require a second admin.
