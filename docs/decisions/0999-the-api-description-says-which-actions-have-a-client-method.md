# 0999. The API description says which actions have a client method

**Status:** Accepted. Design decision, made by the design architect. The owner had not read this record when it was merged. Built in `a2m-actions` (`RestRouteFace::sdk`, and the registry's check), `a2m-api` (`x-sdk` in the OpenAPI document), and both generators in `bindings/`.

**Date:** 2026-11-24

**Changes:** what the two client generators of [decision 0156](0156-the-clients-are-generated-from-the-served-document-and-reach-windows-by-the-pipe.md) and [decision 0405](0405-a-method-name-is-the-actions-words-joined-in-each-languages-way.md) read to decide whether an action has a method.

## Context

An action's faces are four flags in the registry: `command`, `mcp`, `rest`, and `sdk`. The generators of the Python and Node clients read the engine's OpenAPI document, which is made from the REST routes alone, so an action whose `sdk` flag was false still had a method in both clients. Today every action has all four faces, so nothing was wrong yet; the first action without an SDK face would have been given a method anyway.

## Decision

1. **Every operation of an action carries `x-sdk`,** a boolean taken from the action's `faces.sdk`, beside its `x-action`. The two routes that dispatch no action carry neither.
2. **A generator writes a method only for an action whose own route says `x-sdk: true`.** A route that says false gets no method and no input type; a route of an action that does not carry `x-sdk` as a boolean stops the generator, naming the route. Unknown means stop.
3. **An action with an SDK face and no REST face is refused** when the registry is built, because a client's method calls the action's REST route and the clients are generated from the routes' description.
4. What a method is named, and the check of the saved document against the engine, are as [decision 0405](0405-a-method-name-is-the-actions-words-joined-in-each-languages-way.md) says.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Carry `x-sdk` only on an action that has no SDK method | A document from an engine that does not carry the field would read as "every action has a method", which is the fault this record fixes; carried always, a missing field is a stop |
| Generate the clients from `sdk_methods` in a recorded catalog the build writes, not from the OpenAPI document | The design names that catalog ([a2m-actions](../architecture/crates/a2m-actions.md), "Recorded catalog"), but it is not built; it is a new file, a new writer, and a second description to keep true, for one flag the served document can carry |
| Leave the routes of an action without an SDK face out of the document | The REST face and the SDK face are separate flags; a REST caller would lose the description of a route it can call |

## Revisit if

The recorded catalog of the actions is built: the generators may then read it instead, and `x-sdk` can go.
