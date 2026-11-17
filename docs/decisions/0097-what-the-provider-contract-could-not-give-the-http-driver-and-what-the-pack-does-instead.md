# 0097. What the provider contract could not give the HTTP driver, and what the pack does instead

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** four places where the contract as built, in `a2m-provider` and `a2m-auth`, differs from what [spec section 303](../spec/3-packs/303-the-http-pack.md) assumed. Each is a measurement for hypothesis H-26. No kernel crate was changed.

## Context

The HTTP pack is the first driver built against the provider contract. Its spec section was written before the contract existed.

## Decisions

1. **Credentials are placed once, when the driver prepares, and held by the prepared client for one pass.** The driver's `act` is not handed the lease, and a lease cannot be copied, so a credential cannot be injected for each request as section 303 said. The pack applies the lease in `prepare` into a sink of its own, which keeps each value in the core's secret wrapper, puts it only into a request for an endpoint that takes the session's credential, and is wiped when the client is released. The alternative, asking the kernel for the lease in `act`, is a change to a public contract and is left to the owner of that crate.
2. **The driver asks the lease about a sign of expiry before it sees one.** For the same reason, the lease's judgment of a refusing status is asked in `prepare`, for the statuses 401 and 403, and remembered. A redirect is a sign of expiry when it leads to a host the target's auth file declares under `expiry_signals`, which the driver reads from the plan. A status the target declares with a path is a sign as well. The provider is never asked about a host the driver has not yet seen.
3. **What a driver did travels as a capture.** A step's result has no place for the two lists of the step trace, what was done and what was measured. The `request` step returns them as a capture named `acting`, in the form of the telemetry crate's own type, and the exchange, with headers passed through the allowlist, as a capture named `exchange`.
4. **A system's own validation message is not passed on.** Spec section 502 says such a message is a field marked untrusted. A step's failure has one sentence and a next step, and no such field, so the pack reports the status and that the answer was in the endpoint's declared shape for a refusal, and leaves the system's text out.

## What follows

- Section 303's sentence that a credential is never held beyond one request now says one pass.
- If the contract gains the lease in `act`, a field for what a driver did, or a field for untrusted text in a failure, the pack uses it and this record is superseded.

## What would reopen it

A session whose credential changes within one pass; or a second pack that needs the same four things, which would show they belong in the contract.
