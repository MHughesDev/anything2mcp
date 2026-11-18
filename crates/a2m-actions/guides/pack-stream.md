name: pack-stream

# The stream pack, mode `stream`

**Latches onto.** A message broker: a queue, a topic, a log, or a retained value. Surface kind: `broker`, with `address` and `protocol`. The channel is fixed in the target's files. An input never chooses it.

**File kinds.** `channels/`: one file per channel. `schemas/`: the message shapes.

**Verbs.** `read`, `list`, `await`, `publish`, `take`.

**Steps.** `latest`, `browse`, `subscribe`, `await`, `publish`, `acknowledge`, `scan`. The steps `channels` and `describe` are sent by discovery only.

## A channel, from the `example` target

```yaml
channel: orders.events
address: example.orders.events
shape: log
role: events
schema: order_event
key: {field: order_id}
read_through: [browse, own_subscription]
consumers: outside
idempotency: {header: idempotency-token}
wait: {default: 2s, max: 10s}
limits: {browse_count: 2}
```

`shape` is `retained`, `log`, `queue`, or `topic`. `role` is `state` or `events`. A binding of mode `stream` names a `channel`. A wait names `wait` and `on_nothing`, `answer` or `fail`:

```yaml
bindings:
  - mode: stream
    verb: await
    channel: orders.events
    key: {order_id: input.order_id}
    wait: 1s
    on_nothing: fail
```

## Rules

- Every wait has a time limit, no longer than the channel's and the setting `wait_ceiling_seconds`.
- A call always ends. There is no subscription that never ends.
- Text that comes from a broker goes in an output of an untrusted field type, or a field the schema holds to an `enum`.
- A publish declares the effect and hazard its channel requires. A message that others read cannot be recalled: `irreversible`, with the hazard `sends_outside`. There is no automatic retry.
- A message is taken and acknowledged only on a queue with `ownership: runtime_owned`.
- A channel that is published to says who reads it, with `consumers`. The default is `unknown`.

## Refuses

- Taking from a queue others expect to read, unless the target says the runtime owns it.
- Choosing the channel from an input, or passing a message body through as free text.
- Starting a run because a message arrived. Triggers are not designed.

## Codes

`input`: `schema_invalid`, `value_out_of_range`, `message_too_large`, `protocol_unknown`, `credential_not_carried`, `payload_unreadable`. `policy`: `ack_not_owned`, `surface_not_declared`, `scan_limit`, `take_too_large`. `rejected`: `broker_refused`. `drift`: `channel_missing`, `shape_changed`, `response_schema_mismatch`, `position_out_of_range`. `unknown_variant`: `unexpected_reply`. `transient`: `upstream_unavailable`, `nothing_arrived`. `busy`: `subscription_in_use`.
