name: pack-mcp

# The MCP pack, mode `mcp`

**Latches onto.** An existing MCP server, remote or a local program. Surface kind: `mcp`, with `address`. A local server also names its `program` and `digest`, and may give `environment`. A local server is not built on Windows. Remote: a bearer token over plain HTTP, or over HTTPS in a build with the feature `tls`, which is off by default.

**File kind.** `mappings/`. A mapping names the only tools that can be reached. Nothing is reachable until a person maps it.

**Verbs.** `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `download`, `submit`, `call`.

**Steps.** `call_tool`, `list_tools`.

## A mapping, from the `example` target

```yaml
surface: orders
server: {name: example-mcp, version: "1.0.0"}
purpose: tools
tools:
  orders_get:
    digest: "sha256:..."        # the digest of the tool's definition, as discovery reported it
    effect: read
    key_args: [order_id]
    untrusted_fields: [customer, note, reference]
    free_text: false
  orders_create:
    digest: "sha256:..."
    effect: write
    read_back: orders_get
    key: [order_id]
    stamp: reference
```

Write the whole digest. A binding of mode `mcp` names a `tool` and its `args`.

## Rules

- Every mapped tool states the digest of its definition. A changed definition is `drift`, `upstream_tool_changed`.
- The effect class is your judgment, recorded in the mapping. The server's own read-only marking is a claim, not proof.
- A tool mapped as a write names a mapped read tool as its `read_back`, or is mapped as `irreversible`.
- A local server program states its digest.
- A mapping with `purpose: discovery` has tools that no binding may use.
- Members of a result that the mapping does not list are left out. Text members go in `untrusted_fields`.
- Each write compares against a fresh tool list.

## Refuses

- A tool the mapping does not name.
- A tool that runs a free-text string as a command or a query. A mapping entry that says `free_text: true` is never called, and no binding may use it.
- Handing the server any credential beyond the one its mapping declares.
- Passing text from the server on as anything but untrusted data.
- Sharing one connection between two identities.
- Retrying inside a step. Using a server's resources or prompts: tools only.

## Codes

`drift`: `upstream_tool_changed`, `response_schema_mismatch`. `rejected`: `app_validation_error`. `unknown_variant`: `unexpected_status`, `certificate_untrusted`, `certificate_name_mismatch`, `certificate_expired`, `certificate_not_verified`, `tls_handshake_failed`. `possible_commit`: `ambiguous_match`. `transient`: `timeout`, `connection_lost`, `upstream_unavailable`. `policy`: `not_granted`. `input`: `arguments_invalid`, `tls_not_enabled`, `not_built`, `payload_unreadable`.
