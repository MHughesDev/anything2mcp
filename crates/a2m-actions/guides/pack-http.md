name: pack-http

# The HTTP pack, mode `http`

**Latches onto.** An HTTP interface: a service with a JSON interface, with or without a published description. Surface kind: `http`, with `base`.

**File kind.** `endpoints/`: the endpoint catalog. Endpoints live nowhere else.

**Verbs.** `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `submit`, `call`.

**Steps.** `request`, `validate`, `extract`, `probe`.

Not built: the `api` mode, the token step, GraphQL and OData rules. TLS is built behind the feature `tls` and is off by default: without it, a surface reached over HTTPS is refused with `tls_not_enabled`; with it, certificate and handshake failures are named below.

## An endpoint, from the `example` target

```yaml
endpoint: order_get
surface: api
method: GET
path: "/api/orders/{order_id}"
auth: none
effect: read
headers: {accept: "application/json"}
responses: {absent: [404]}
response_schema:
  type: object
  required: [order_id, customer, status, items]
  properties:
    order_id: {type: string}
    customer: {type: string}
    status: {type: string, enum: [draft, submitted]}
    items: {type: array, items: {type: object}}
```

A binding of mode `http` names an `endpoint`, and may give `path_params`, `query`, `body`, and `extract` (an output name to a dotted path in the answer, with one `*` for a list: `order_ids: "*.order_id"`).

## Rules

- A placeholder in `path` is one whole segment, such as `{order_id}`. An input holding a separator or `..` is refused.
- `response_schema` uses only `type`, `nullable`, `properties`, `required`, `additionalProperties`, `items`, `enum`.
- `responses` declares `ok`, `absent`, and, as lists of a `status` with an optional `when`, `refused`, `conflict`, `unavailable`. An answer that matches nothing declared stops the step.
- A write that is sent is settled only by the system's answer and the read-back. An unclear answer is `possible_commit`.
- With a `version` read, a write sends the version as a precondition. A conflict is `stale`, `version_conflict`.
- `auth` is `session`, `token`, or `none`. The session keeper supplies it.

## Refuses

- Any host, scheme, or port that comes from an input. They come from the surface.
- A redirect. It is never followed.
- A secret in a template. The lint `http.secret_literal` is an error.
- An endpoint the front end computes or signs (`signed: true`): no `http` binding compiles on it.
- Renewing a token, retrying inside a step, using a proxy.
- Imitating another application's client, or getting past an edge's bot defense.

## Codes

`rejected`: `app_validation_error`. `stale`: `version_conflict`, `version_not_read`. `drift`: `response_schema_mismatch`. `unknown_variant`: `unexpected_status`, `response_too_large`, `certificate_untrusted`, `certificate_name_mismatch`, `certificate_expired`, `certificate_not_verified`, `tls_handshake_failed`. `transient`: `upstream_unavailable`. `policy`: `surface_not_declared`. `input`: `path_value_refused`, `step_not_usable`, `tls_not_enabled`, `auth_scheme_not_built`, `description_unreadable`, `description_too_large`.
