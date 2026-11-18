# 403. Testing and publish gates

A version is published only when every gate passes, and the evidence is stored with the version. The determinism and equivalence gates compare trace signatures ([section 217](../2-kernel/217-traces-evidence-and-repair.md)); the schema and lint gate is the compiler ([section 206](../2-kernel/206-the-compiler-and-the-plan.md)). Two gates are built, schema and lint and offline replay: a publish is refused unless both pass, and the evidence stored with the version, and shown on the page a person publishes from, names each gate that did not run ([decision 0180](../../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)). The offline replay passes for a write only by a test that sent its save and whose recorded request matched what was sent, and it tests the bytes the version is compiled from: the evidence names the plan and the digest of its sources ([decision 0196](../../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)). Each offline test runs one binding of its operation, the one its file names or else the one that would act live, and its evidence names that binding and the digest of what it does; the offline replay passes only when every binding that may act in the version has a passing test that ran it, and, for a write, one whose save was sent through it and compared ([decision 0200](../../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md)).

| Gate | What it checks | Applies to |
| --- | --- | --- |
| Schema and lint | Every file validates; names are unique; no fixed sleeps; no undeclared surfaces | All |
| Offline replay | Operations run against recorded pages, responses, and traffic in `fixtures/` and produce the expected results | All |
| Contract | Every locator resolves uniquely and matches its fingerprint; every endpoint response matches its schema; every upstream tool still exists | All bindings |
| Determinism | Five live runs on the same input produce the same step trace | Reads, pipelines |
| Binding equivalence | For the same input, each verified binding returns the same normalized result; an HTTP write binding reproduces the request the UI sent, field for field, ignoring declared volatile values | Operations with more than one binding |
| Write round trip | On a test record: write, read back, compare, then restore the original values | Write operations |
| Idempotency | The same pipeline run twice; the second run changes nothing | Write pipelines |
| Resume | A run killed at a random step, then resumed, finishes with no duplicates | Write pipelines |
| Fault injection | Expired session, slow network, blocked surface, bad input, stale approval, and edge block each stop with the right class | All |
| Performance | Each operation meets its budget; no regression over 10% against the last version | All |
| Agent usability | A test agent with no prior knowledge completes scripted tasks using only the generated names, descriptions, and results | Each toolset |
| Human review | A person reads the generated description, schema, effect class, and permissions | All |

**Where writes are tested.** In order of preference: the system owner's sandbox or a test environment; a dedicated test record in production that the tests restore after use; otherwise the write operation stays unpublished. Anything2MCP never proves a write by performing it on live business data.

**After publishing.** Scheduled read-only canaries re-run the contract gate, more often for platforms that ship interface changes frequently. A failure marks the affected binding unhealthy; if no verified binding remains, the tool is marked unavailable before anyone runs a write.

**Creates.** For an operation that creates records: the same create run twice leaves one record, and a run stopped between sending a create and recording it finds that record on resume ([section 209](../2-kernel/209-record-identity-and-dependent-records.md)).

**Testing Anything2MCP itself.** The [test application plan](../../plan/test-application.md) lists every case and the driver conformance suite. The repository ships a test application that imitates the hard cases of web and desktop interfaces: virtualized grids, postback forms, single-page apps, anti-forgery tokens, optimistic locking, and a desktop app. Each further pack brings its own test stand, which imitates the part of its kind of system that makes the kind hard. Every driver and widget driver is tested against its stand on Windows, macOS, and Linux in CI.
