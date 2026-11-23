# Research: recording intent and reconciling after a crash

**Status:** completed on 2026-11-23 for the documents that could be reached, and **Partial** in one respect that matters. From this session only `raw.githubusercontent.com` resolved. The standards sites, the vendors' own documentation sites, and the payment provider's API reference did not. What was read instead is the text of each project's documentation as kept in its public repository, the draft's source file in the working group's repository, and two API guidelines held in public repositories. Nothing was installed, built, or run. Each claim carries a tag:

- **[read]**: the file was opened and read in this session. Its URL is given.
- **[memory]**: stated from knowledge, not opened in this session. Treat as unverified.

Answers brief 7 of [the open research list](11-open-research.md) and extends [note 13](13-ledger-leases-and-runner-links.md), whose engine claims came from search summaries. Feeds [spec section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md), [section 209](../spec/2-kernel/209-record-identity-and-dependent-records.md), the [ledger schema](../reference/ledger-schema.md), and roadmap packages 3.3 to 3.7. This note records facts and proposals; the lead decides.

## Question

When a program sends a change to a system it does not own, and stops before it knows the result, what do the established ways of doing this do? How do durable workflow engines record what they are about to do, find out after a crash whether it happened, and treat a side effect that has no idempotency key? What do public HTTP interfaces offer for retries? What does "exactly once" actually give? And where does rule 3 and the ledger of section 208 agree with that practice, differ from it, or fall short of it?

## Answer in short

- **No engine read here records intent before a side effect. Each records the outcome after it.** The engine's log holds the result of a step once the step returns. A crash after the effect and before that record means the step runs again. All three engines say so in their own documents (part 1).
- **For an outside system, all three engines hand the problem to the application.** Their answer is an idempotency key, made stable across retries and enforced by the system being called, not by the engine. None offers a way to read back, and none has a state for "may have happened". The ledger has both (part 1).
- **So rule 3 asks for more than the engines give, and the extra part is the part that matters.** The `intent` state, the `acted` state, and the read-back on resume let the ledger tell "not sent", "sent and landed", and "sent, answer lost". The engines can tell only "no result recorded" (part 4).
- **"Exactly once" has three meanings in the sources.** Exactly one recorded completion (Temporal). Exactly one commit, when the effect is in the same database as the record of it (DBOS). Exactly one execution of a handler for one idempotency key, within the key's lifetime (Restate, and payment-style interfaces). None means one send to a system with no key (part 3).
- **Idempotency keys in public interfaces are real but narrow.** They are optional, kept for a limited and sometimes undocumented time, tied to a fingerprint of the request in some designs, and answered with the first response or a conflict. Most of the systems this product latches onto accept none (part 2).
- **The existing design holds up.** Seven refinements are proposed at the end. The largest: say that a key protects only inside its window, so a resume after the window reads first; say that a replayed answer is not read-back evidence; and test the durability of an intent against power loss, not only against a killed process.

## 1. How durable workflow engines treat a side effect

### What each one records, and when

- **Temporal.** An activity is the code that touches the outside world. Its result is recorded in the event history only when it returns or fails. The document says so: "Activities won't record to the Event History until they return or produce an error. If an Activity fails to report to the server at all, it will be retried." **[read]** [activity definition](https://raw.githubusercontent.com/temporalio/documentation/main/docs/encyclopedia/activities/activity-definition.mdx) (source of the docs page).
- **Restate.** The handler's progress is kept in a journal. A block run with `ctx.run` has its result stored; "Restate replays the result instead of re-executing the operation on retries." The block is "retried infinitely on failure" unless a retry policy or a terminal error says otherwise. **[read]** [journaling results](https://raw.githubusercontent.com/restatedev/documentation/main/docs/develop/ts/journaling-results.mdx), [durable execution](https://raw.githubusercontent.com/restatedev/documentation/main/docs/concepts/durable_execution.mdx).
- **DBOS.** Each step's outcome is written to Postgres after the step: "one database write per step (to checkpoint the step's outcome)". On recovery a workflow runs again from the top and a step with a checkpoint returns the stored output; the first step with no checkpoint runs. **[read]** [architecture](https://raw.githubusercontent.com/dbos-inc/dbos-docs/main/docs/architecture.md).

None of the three pages describes a record written before the effect that says "about to send". The gap is therefore the same in each: the effect happens, the process dies, no outcome is recorded.

### What happens after the crash

- **Temporal** names the case. A worker "completes successfully, but the Worker crashes just before it notifies the Temporal Service. In this case, the Event History won't reflect the successful completion of the Task, so the Activity will be retried." **[read]** activity definition, above. It also says an activity with a retry policy "will be observed as completed exactly once", yet "may be executed multiple times and may even partially complete more than once". **[read]** same page. The service cannot tell a dead worker from a slow one; it waits for the start-to-close timeout. **[read]** same page, and [detecting activity failures](https://raw.githubusercontent.com/temporalio/documentation/main/docs/encyclopedia/detecting-activity-failures.mdx) for the heartbeat timeout.
- **DBOS** is plain: "Steps are tried _at least once_ but are never re-executed after they complete." **[read]** [workflow tutorial](https://raw.githubusercontent.com/dbos-inc/dbos-docs/main/docs/python/tutorials/workflow-tutorial.md). And: "Steps should be **idempotent**, meaning it should be safe to retry them multiple times." **[read]** architecture, above.
- **Restate** replays the journal and injects stored results; an action with no stored result runs again. **[read]** durable execution, above.

### What each one says to do with a side effect that has no key

None of the pages read gives a method for an effect that cannot be made idempotent. Temporal's advice is to make activities idempotent and to split a multi-step activity into several, "only the step that failed will be executed again". **[read]** activity definition. A step that cannot be keyed is left to the application. The compensation pattern (the saga) is the usual answer for the case where the first effect cannot be taken back by retrying; it comes from a 1987 paper on long-lived transactions. **[memory]** Not read here; the brief names it and its first-hand text could not be reached.

### The idempotency key as the engines use it

- Temporal: "You can achieve idempotency … through the use of unique identifiers, known as idempotency keys … These are enforced by the service you are calling from your Activity, not by the Activity itself." It suggests the workflow run id plus the activity id as the key, because it is "consistent across retry attempts but unique among Workflow Executions". **[read]** activity definition.
- Restate: the SDK seeds its random generator with the invocation id, so "it always returns the same value on retries", and the page says to use the UUIDs "to generate stable idempotency keys". **[read]** journaling results. Restate also takes an `idempotency-key` header on an invocation: "Duplicate requests will get the same response as the first request, or will latch on to the first invocation if it's still running." **[read]** [invocations](https://raw.githubusercontent.com/restatedev/documentation/main/docs/concepts/invocations.mdx).
- DBOS: "An assigned workflow ID acts as an idempotency key: if a workflow is called multiple times with the same ID, it executes only once." **[read]** workflow tutorial. This protects the start of the workflow, not the effects inside it.

The ledger's `idempotency_token` is "stable for the run and key" ([ledger schema](../reference/ledger-schema.md), `run_records`). That is the same construction Temporal and Restate recommend.

### Where the engines are stronger than a plain log

DBOS can make a database effect exactly once. A datasource transaction inside a workflow "records the outcome atomically in the same database transaction", and on replay "DBOS detects the existing record and returns the stored result without re-executing the function". **[read]** [transaction tutorial](https://raw.githubusercontent.com/dbos-inc/dbos-docs/main/docs/python/tutorials/transaction-tutorial.md). The condition is that the effect and its record share one database. For a system that is not that database, the page's guarantee does not apply.

## 2. Idempotency keys in public HTTP practice

### The standard method rule

RFC 9110 says PUT and DELETE are idempotent and POST is not. A client "SHOULD NOT automatically retry a request with a non-idempotent method unless it has some means to know that the request semantics are actually idempotent, regardless of the method, or some means to detect that the original request was never applied." A proxy "MUST NOT automatically retry non-idempotent requests." **[read]** [RFC 9110 § 9.2.2, from the working group's rendered copy](https://raw.githubusercontent.com/httpwg/httpwg.github.io/main/specs/rfc9110.html). The second clause is the read-back rule of rule 3, stated for HTTP.

### The Idempotency-Key header draft

The working group's draft (the file in its repository, main branch; revision number not read) says **[read]** [draft source](https://raw.githubusercontent.com/ietf-wg-httpapi/idempotency/main/draft-ietf-httpapi-idempotency-key-header.md):

- The key is "a unique value generated by the client", a structured-field string; a random UUID is recommended.
- It "MUST NOT be reused with another request with a different request payload".
- The server MAY keep time-limited keys and "SHOULD define such expiration policy and publish it".
- The server MAY store a fingerprint of the payload. With a fingerprint, a reused key with a different payload is refused and the original result is not returned. Without one, a changed payload "handles such a request as a duplicate".
- A duplicate after the first completed gets "a response consistent with the request being completed". A duplicate while the first is running gets `409`. A reused key with another payload gets `422`. A missing required key gets `400`.
- A client "cannot assume the server will respect" the header without knowing the server's documentation.

Not verified: whether the draft has become an RFC, and its current revision.

### Other published designs

- **Google's request identification rule.** `request_id` is optional; "This request is only idempotent if a `request_id` is provided"; the server "should return the response for the previously successful request"; "APIs may choose any reasonable timeframe for honoring request IDs". It also allows a duplicate to get the current state instead of the old answer ("Stale success responses"). **[read]** [AIP-155](https://raw.githubusercontent.com/aip-dev/google.aip.dev/master/aip/general/0155.md).
- **Microsoft's Azure guidelines.** Every operation, POST included, "must be idempotent", and actions that need it support the `Repeatability-Request-ID` and `Repeatability-First-Sent` headers. **[read]** [Azure guidelines](https://raw.githubusercontent.com/microsoft/api-guidelines/vNext/azure/Guidelines.md). The second header gives the server a time to judge whether a key is still inside its window. **[memory]** The page names the headers; the use of the time is from knowledge of the specification it points to.
- **A cloud compute interface.** `ClientToken`: "Unique, case-sensitive identifier … to ensure the idempotency of the request. If you do not specify a client token, a randomly generated token is used"; at most 64 ASCII characters. **[read]** [botocore's service model](https://raw.githubusercontent.com/boto/botocore/develop/botocore/data/ec2/2016-11-15/service-2.json). So the client library, not the caller, may supply the key on retry.
- **A payment interface.** Its client library adds keys on automatic retries "where appropriate to prevent duplication". **[read]** [library README](https://raw.githubusercontent.com/stripe/stripe-node/master/README.md). The provider's own reference page (key length, how long it keeps a key, how it treats a changed payload) did not load and is **not verified**. From memory the key is kept for about a day and a changed payload is refused **[memory]**.

### What the practice has in common

1. The key is chosen by the client and must be the same on every retry.
2. The key is optional, and without it nothing is promised.
3. The system keeps the key for a limited time, sometimes undocumented.
4. The system answers a duplicate with the first result, a conflict, or a refusal of a changed payload; one design allows a changed answer.
5. A key is checked for scope: who sent it, which operation, and sometimes which payload.

The systems this product latches onto are mostly web applications, desktop applications, files, and services that offer none of this. That matches the spec's own premise in section 208 and does not change.

## 3. What "exactly once" gives

| Source | What is promised | What is not |
| --- | --- | --- |
| Temporal | One completion is observed by the workflow | One execution of the activity; partial runs may repeat |
| DBOS steps | Never re-run after the checkpoint is written | One execution before it; steps are "at least once" |
| DBOS transactions | One commit, with the record of it, in one database | Anything outside that database |
| Restate handler | One execution of a handler per idempotency key, duplicates get the first answer | A side effect in `ctx.run` with no key at the other end |
| HTTP key | One effect per key, inside the window and the scope, if the server honours it | Any effect after the key expired; any server that ignores the header |
| Azure guidance | An aim: "exactly once" is named as "client retries and service idempotency" together | A property of the network |

The last row is from the Azure guidelines **[read]**, where "exactly once behavior = client retries & service idempotency". Together the rows say the same thing: exactly-once is an effect on state, got by repeating a send until the receiver can show it has the effect, and it needs the receiver to take part. Two-generals reasoning shows no sender can learn by messages alone whether its last message arrived. **[memory]**

## 4. Comparison with rule 3 and section 208

### Where the ledger agrees with the engines

- Identity before action: the token is stable for the run and record, as Temporal and Restate advise. Agrees.
- Replay skips what is done: records in `verified` or `unchanged` are skipped on resume. Agrees with all three engines.
- A lease and a lease number guard the writer, as note 13 found. Not re-checked here.
- A claim that no engine does better: borne out. For the outside system each engine hands the key and the check to the application. The sentence in section 208, "durable-execution engines give no stronger guarantee for writes to an outside system than this design does", is true as far as the pages read here go, and this note now carries primary text for it.

### Where the ledger goes further

| Gap in the engines | What the ledger has |
| --- | --- |
| No record that a send may have happened | `intent` stored first, `acted` after, and both treated as doubt on resume (rule 5) |
| After a crash the answer is "run it again" | After a crash the answer is "read it, compare, then decide" (resume table) |
| An effect with no key and no read-back is retried | The same effect is `irreversible`, approved per run, never resumed automatically, never called `verified` |
| Success is the function returning | Success is a value read back that equals the intended value after normalization (rule 2) |

The first row is the main finding. The engines' journals have the crash window between effect and record. The ledger's `intent` record is written before the window opens, so a resume knows the window may have been entered. RFC 9110's phrase "some means to detect that the original request was never applied" is what the read-back supplies.

### Where the ledger meets the same limits

- **The write sent, the answer lost, no key, no read-back.** Every design is stuck here, the ledger included. The ledger's answer, `possible_commit` with `cannot_reconcile`, is stop and ask a person. That is the correct limit, and it is the same as the engines' advice to "compensate", except that the ledger does not pretend a compensation exists. Decision 0185 already states it.
- **The system applied part of a write.** The third row of the resume table detects it. No engine page read here addresses it.
- **A change by someone else after the read-back.** Out of reach for any design; section 208 already says so.

### Points where the practice shows a gap in the text

1. **A key protects inside a window only.** Public interfaces keep a key for a limited and sometimes unstated time (part 2). Section 208 says the binding sends the token "when a surface documents an idempotency key" and that the system then "refuses a duplicate", with no mention of the window. A resume after an hour or a day may meet a system that has forgotten the key and will take the same send again. The read-back before any write on resume covers this for records that can be read back, but the text should say the key is an addition to the read-back, never a replacement for it, and the binding should state the window it relies on.
2. **A replayed answer is the first answer, or a newer state.** The draft says a duplicate gets "a response consistent with the request being completed"; Google's rule allows a different, current answer. Section 208's `response` evidence says the save's own answer stands as proof "where it carries every intended field". A replayed answer is not the answer to this send and may reflect later state. It should not be used as `response` evidence unless the provider's binding states that its answers are the original.
3. **Same key, different payload.** The draft refuses it (`422`). The ledger's token is "stable for the run and key". If a record's intended values change between attempts, a new preview and approval make a new run, and so a new token. This is implied but not written. It should be, because a system with no fingerprint treats the changed payload as a duplicate and drops it silently, and the run would then read back a mismatch it cannot explain.
4. **A conflict (`409`) on a retry is information.** The draft says a retry during the first request's processing gets a conflict, and "no correction is required" before retrying. A binding that sees a conflict on a retry has learned the first send is still running or has landed. It should read, not resend. Section 208 has no row for this.
5. **Durability of an intent against power loss.** Section 208 says an intent is durable on commit, and hypothesis H-15 asks about cost. In SQLite's write-ahead mode with the commit setting below full, a committed transaction can be lost after a power failure though not after a process kill. **[memory]** Not read here. If so, a write sent after such a commit could leave no intent on disk, and the record would come back as `planned` and be sent again with no read. Rule 1 is written for "durably stored". The test for it kills the process; it should also cover a lost tail. The note's check is a measurement and a test, not a claim: see the proposals.
6. **A transactional target can do better than the read-back.** DBOS shows that where the effect and the proof share one database transaction, the record of the effect is atomic with it. The `stamped` strategy already writes a token into a field. When the target is a database the data pack connects to, the stamp can be written in the same transaction as the write, which closes the window for that target. Section 209 does not say this.
7. **The word "duplicates".** The first sentence of section 208 says the ledger makes a stopped run "resumable without duplicates". Part 3 shows no design promises that for a send to a system with no key and no read-back. The promise that holds is narrower: no second send while the first may have landed, unless a person has approved it.

### What could not be checked

The brief also asks for a recommended ledger schema for SQLite and Postgres. The schema in the reference was read only for the `run_records` and `caller_idempotency` tables. Nothing in this note found a missing column. The proposals below add one declared field to a manifest, not to the schema. A fuller comparison of the migrations was not done.

## What could not be verified

- The payment provider's reference page on idempotent requests (host did not resolve). Its key length, retention, and treatment of a changed payload are from memory.
- The current revision and status of the Idempotency-Key draft.
- The workflow engines' own articles on idempotency (hosts did not resolve); only their documentation source files were read. Temporal's blog post on idempotency and durable execution, named in its page, was not opened.
- The saga pattern and compensating transactions: no primary text reached.
- SQLite's durability settings and what they lose on power failure: from memory.
- Behaviour. Nothing was run against any engine or interface; every statement is about what the documents say.
- Whether the engines record a "started" event before the effect. The pages read do not say, and a start event would not change the finding, since it says the activity began, not that a given send was made.

## What this changes in the design

These are proposals. The lead applies them; each needs a decision record where it changes behaviour.

1. **Section 208, "The problem it solves", first sentence and intro line.** Replace "resumable without duplicates" with: "resumable with no second send while the first may have landed, unless a person has approved it". Reason: part 4, point 7. Wording only; no decision record.

2. **Section 208, "The problem it solves", last paragraph.** Replace "The same research found that durable-execution engines give no stronger guarantee for writes to an outside system than this design does." with: "Research notes 13 and 38 found that the durable-execution engines read record the result of a step after it ran, never an intent before it, and leave a side effect with no key to the application. The ledger records the intent first and reads back on resume, which the engines do not." Reason: part 1 and part 4.

3. **Section 208, "Idempotency tokens where the system takes them", add after the first paragraph:** "A system keeps a key for a limited time. A binding that sends the token states in its manifest the window it relies on, or that the window is not documented. The token is an addition to the read-back and never a replacement for it: a resume reads before it writes, whether or not the token was sent. When the window has passed since the intent was recorded, or is not documented, the system is taken to have forgotten the token. A conflict answer to a retry, where the system gives one, means the first send is running or has landed: the binding reads, and does not send again." Reason: part 2, points 1 and 4. Needs a decision record and one manifest field, `idempotency_window`.

4. **Section 208, "What a binding offers as evidence", row `response`, add:** "An answer given to a repeated send is the first answer or a newer state, and is never evidence under `response`, unless the provider's binding states that its answers to a repeated send are the original. It is read back instead." Reason: part 4, point 2.

5. **Section 208, "Idempotency tokens where the system takes them", add:** "A token is never reused for a send whose intended values differ from the values it was first sent with. A changed value is a new preview, a new approval, a new run, and a new token." Reason: part 4, point 3. Wording only if the engine already behaves so; add a unit test in `a2m-ledger` that a record with a changed `intended_hash` cannot be given the old token.

6. **Rule 1's test and hypothesis H-15.** Add to the H-15 experiment: "measure the cost of an intent commit with the setting that survives power loss on each operating system, and run a test that discards the last committed transaction before restart, to see what the resume does." Add to section 208, "One schema, two databases", row "Durability of an intent": "A commit that survives power loss, not only a process kill." Reason: part 4, point 5. The fix, if the test shows a loss, is a decision record on the setting. This belongs with roadmap package 3.7's random-kill proof: add "and a lost tail of the ledger file".

7. **Section 209, "Rules for `stamped`", add:** "Where the target is a database the data pack connects to with a transaction, the stamp is written in the same transaction as the write. The write and its proof then commit together, and a stopped run finds both or neither." Reason: part 1, DBOS transactions; part 4, point 6. Needs a decision record when the data pack's write path is built.

Package notes: 3.3 needs only proposal 5's test; 3.7 needs proposal 6; proposals 3 and 4 wait for the first binding that sends a documented key.
