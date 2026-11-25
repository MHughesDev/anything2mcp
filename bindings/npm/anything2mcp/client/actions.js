// GENERATED: do not edit. Written by scripts/generate-actions.js from the OpenAPI
// document the engine serves at /api/v1/openapi.json (engine version 0.1.0).
// A test fails when this file differs from what the generator writes. To change a
// method, change the action in the engine, save the new document, and run the
// generator.

"use strict";

// Every action this module has a method for, in the order of the document.
const ACTIONS = Object.freeze([
  "action.describe",
  "action.run",
  "action.search",
  "assertion.confirm",
  "audit.query",
  "auth.detect",
  "binding.promote",
  "capture.start",
  "capture.stop",
  "client.connect",
  "component.list",
  "config.show",
  "discover.run",
  "docs.get",
  "doctor.run",
  "draft.delete",
  "draft.lint",
  "draft.read",
  "draft.ready",
  "draft.try",
  "draft.write",
  "engine.logs",
  "engine.start",
  "engine.status",
  "engine.stop",
  "finding.describe",
  "finding.list",
  "fixture.record",
  "grant.add",
  "grant.list",
  "grant.remove",
  "killswitch.set",
  "net.current",
  "net.interfaces",
  "net.listeners",
  "net.neighbours",
  "net.proxy",
  "net.resolvers",
  "net.routes",
  "net.standing",
  "policy.explain",
  "policy.publish",
  "policy.test",
  "policy.tools_allowed",
  "policy.versions",
  "policy.who_can",
  "provider.describe",
  "provider.list",
  "publish.request",
  "reach.list",
  "reach.name",
  "reach.withdraw",
  "run.cancel",
  "run.get",
  "run.list",
  "run.resume",
  "run.trace",
  "scout.report",
  "scout.run",
  "secret.set",
  "self.update",
  "session.end",
  "session.list",
  "session.login",
  "session.revoke",
  "session.status",
  "shadow.report",
  "spec.import",
  "target.add",
  "target.list",
  "target.probe",
  "target.show",
  "target.status",
  "test.run",
  "tool.call",
  "tool.describe",
  "tool.search",
  "toolset.create",
  "toolset.list",
  "toolset.remove",
  "toolset.update",
  "trace.export",
  "trace.show",
  "unknown.list",
  "version.diff",
  "version.publish",
  "version.rollback",
  "watch.changes",
  "watch.confirm",
  "watch.create",
  "watch.end",
  "watch.get",
  "watch.list",
  "watch.pause",
  "watch.resume",
]);

// Each method names its action and passes its input to `call`, which the client
// supplies. It resolves to the engine's response and branches on nothing.
class Actions {
  /**
   * One action in full: its summary, input schema, output schema, examples, effect, and
   * role. The engine action is `action.describe`.
   */
  actionDescribe(input, options = {}) {
    return this.call("action.describe", input, options);
  }

  /**
   * Run any action by name with its input. The action's own role, schema, and policy
   * decision apply. The engine action is `action.run`.
   */
  actionRun(input, options = {}) {
    return this.call("action.run", input, options);
  }

  /**
   * Find actions by words. Returns names and one-line summaries only. The engine action
   * is `action.search`.
   */
  actionSearch(input = {}, options = {}) {
    return this.call("action.search", input, options);
  }

  /**
   * Confirm the reads a target's drafts assert, as the person, so that a try of the
   * drafts in an environment of kind `test` may let a page send them. A person's act,
   * carried out on the engine's page, never an agent's. The engine action is
   * `assertion.confirm`.
   */
  assertionConfirm(input, options = {}) {
    return this.call("assertion.confirm", input, options);
  }

  /**
   * Read the audit log, oldest first, and check its chain. The engine action is
   * `audit.query`.
   */
  auditQuery(input = {}, options = {}) {
    return this.call("audit.query", input, options);
  }

  /**
   * The sign-in method of a surface, with evidence, and a recommended identity kind.
   * Read only. The engine action is `auth.detect`.
   */
  authDetect(input, options = {}) {
    return this.call("auth.detect", input, options);
  }

  /**
   * Make the first binding of a drafted operation verified, on the evidence of its
   * offline tests and a trial, or a binding in shadow verified, on its run of exact
   * matches. For an operation that writes, and for a binding in shadow, a person's act.
   * The engine action is `binding.promote`.
   */
  bindingPromote(input, options = {}) {
    return this.call("binding.promote", input, options);
  }

  /**
   * Ask for a capture of your person's own use of a surface. Nothing records until the
   * person begins it on the engine's page; then only the structure of what they use is
   * kept, never a value. The engine action is `capture.start`.
   */
  captureStart(input, options = {}) {
    return this.call("capture.start", input, options);
  }

  /**
   * End a capture you asked for, or read how it ended; store what it found as findings,
   * and scout from them as `scout.run` does from a discovery: draft by rule, try the
   * drafted reads, and report. Answers with the report's reference; never writes to the
   * system. The engine action is `capture.stop`.
   */
  captureStop(input, options = {}) {
    return this.call("capture.stop", input, options);
  }

  /**
   * Give the configuration an MCP client needs to start the stdio proxy for this
   * project: printed for the `generic` client profile, or set in the file a client
   * profile names, shown first and written by a local person. The engine action is
   * `client.connect`.
   */
  clientConnect(input = {}, options = {}) {
    return this.call("client.connect", input, options);
  }

  /**
   * The outside programs providers need: found or not, with path and digest, known or
   * unverified. Starts nothing. The engine action is `component.list`.
   */
  componentList(input = {}, options = {}) {
    return this.call("component.list", input, options);
  }

  /**
   * The effective settings, and for each the layer that set it. With a key, that key's
   * documentation. The engine action is `config.show`.
   */
  configShow(input = {}, options = {}) {
    return this.call("config.show", input, options);
  }

  /**
   * Run the discoverers that suit a target's surfaces, in the sandbox. Returns
   * findings; never writes. The engine action is `discover.run`.
   */
  discoverRun(input, options = {}) {
    return this.call("discover.run", input, options);
  }

  /**
   * A built-in reference page, by topic or action name. With no topic, the list of
   * topics. The engine action is `docs.get`.
   */
  docsGet(input = {}, options = {}) {
    return this.call("docs.get", input, options);
  }

  /**
   * Check the settings, the store, the key store, the providers, and the clock, and
   * give a fix for each problem. Makes no outside connection. The engine action is
   * `doctor.run`.
   */
  doctorRun(input = {}, options = {}) {
    return this.call("doctor.run", input, options);
  }

  /**
   * Remove draft files of a target from the project, one by `file` or several by
   * `files`, all or none; a published version keeps its own copy. The engine action is
   * `draft.delete`.
   */
  draftDelete(input, options = {}) {
    return this.call("draft.delete", input, options);
  }

  /**
   * Lint and compile a target's draft files and report every problem with its file and
   * line. Changes nothing. The engine action is `draft.lint`.
   */
  draftLint(input, options = {}) {
    return this.call("draft.lint", input, options);
  }

  /**
   * Read one draft file of a target; stands in for a file system. The engine action is
   * `draft.read`.
   */
  draftRead(input, options = {}) {
    return this.call("draft.read", input, options);
  }

  /**
   * Make every drafted read a trial in this project read a value through ready to
   * publish: record its offline test and fixture, promote its first binding, run the
   * publish gates, and ask a person to publish, each step by its own action. A write, a
   * pipeline, and a read with no trial are left alone, each with its reason. The engine
   * action is `draft.ready`.
   */
  draftReady(input, options = {}) {
    return this.call("draft.ready", input, options);
  }

  /**
   * Run one draft binding in the sandbox, where writes go only to declared test
   * records. The engine action is `draft.try`.
   */
  draftTry(input, options = {}) {
    return this.call("draft.try", input, options);
  }

  /**
   * Write one draft file of a target after validating and linting it. The engine action
   * is `draft.write`.
   */
  draftWrite(input, options = {}) {
    return this.call("draft.write", input, options);
  }

  /**
   * The local engine's recent log lines. The engine action is `engine.logs`.
   */
  engineLogs(input = {}, options = {}) {
    return this.call("engine.logs", input, options);
  }

  /**
   * Start the local engine. The engine action is `engine.start`.
   */
  engineStart(input = {}, options = {}) {
    return this.call("engine.start", input, options);
  }

  /**
   * Whether the local engine runs, its health state, and the cause of anything but
   * `ok`. The engine action is `engine.status`.
   */
  engineStatus(input = {}, options = {}) {
    return this.call("engine.status", input, options);
  }

  /**
   * Stop the local engine after saving its sessions. The engine action is
   * `engine.stop`.
   */
  engineStop(input = {}, options = {}) {
    return this.call("engine.stop", input, options);
  }

  /**
   * One finding in full, with its evidence references. The engine action is
   * `finding.describe`.
   */
  findingDescribe(input, options = {}) {
    return this.call("finding.describe", input, options);
  }

  /**
   * Findings for a target, by level, kind, or source. The engine action is
   * `finding.list`.
   */
  findingList(input, options = {}) {
    return this.call("finding.list", input, options);
  }

  /**
   * Record an offline test's fixture from one real read, every value scrubbed by rule,
   * and the test that replays it. The engine action is `fixture.record`.
   */
  fixtureRecord(input, options = {}) {
    return this.call("fixture.record", input, options);
  }

  /**
   * Grant a principal or a group the use of tools up to an effect class. A person's
   * act, published as a new version of the policy set. The engine action is
   * `grant.add`.
   */
  grantAdd(input, options = {}) {
    return this.call("grant.add", input, options);
  }

  /**
   * The grants, the grants of roles, and who holds which role, in the set in force. The
   * engine action is `grant.list`.
   */
  grantList(input = {}, options = {}) {
    return this.call("grant.list", input, options);
  }

  /**
   * Remove a grant. A person's act, published as a new version of the policy set. The
   * engine action is `grant.remove`.
   */
  grantRemove(input, options = {}) {
    return this.call("grant.remove", input, options);
  }

  /**
   * Stop, or release, a target, a workspace, or the whole system. Releasing needs a
   * person present. The engine action is `killswitch.set`.
   */
  killswitchSet(input, options = {}) {
    return this.call("killswitch.set", input, options);
  }

  /**
   * Which facts of the network this machine is on were read, to recognize it again, and
   * which were left out and why. Shows no digest and no address. Sends nothing. The
   * engine action is `net.current`.
   */
  netCurrent(input = {}, options = {}) {
    return this.call("net.current", input, options);
  }

  /**
   * This machine's network interfaces and their addresses, read from the operating
   * system. Hardware addresses are shown as their length only. Sends nothing. The
   * engine action is `net.interfaces`.
   */
  netInterfaces(input = {}, options = {}) {
    return this.call("net.interfaces", input, options);
  }

  /**
   * The sockets listening on this machine, each with the kind of place its address is,
   * and the program that holds it where this user may see it. Sends nothing. The engine
   * action is `net.listeners`.
   */
  netListeners(input = {}, options = {}) {
    return this.call("net.listeners", input, options);
  }

  /**
   * The neighbour table this machine already holds, never refreshed by asking. Hardware
   * addresses are shown as their length only. Sends nothing. The engine action is
   * `net.neighbours`.
   */
  netNeighbours(input = {}, options = {}) {
    return this.call("net.neighbours", input, options);
  }

  /**
   * The proxy setting this engine reads, with any credentials removed: on Linux this
   * engine's own environment, not the caller's; on Windows the user's system setting.
   * Read to be shown, never applied. Sends nothing. The engine action is `net.proxy`.
   */
  netProxy(input = {}, options = {}) {
    return this.call("net.proxy", input, options);
  }

  /**
   * The name servers and search domains this machine uses, as the system states them.
   * Sends nothing. The engine action is `net.resolvers`.
   */
  netResolvers(input = {}, options = {}) {
    return this.call("net.resolvers", input, options);
  }

  /**
   * This machine's routes, and each family's default routes, lowest metric first. Sends
   * nothing. The engine action is `net.routes`.
   */
  netRoutes(input = {}, options = {}) {
    return this.call("net.routes", input, options);
  }

  /**
   * Where this machine stands on its networks: for each interface, the kind of place
   * each of its addresses is, and which interface carries each family's preferred
   * default route. Sends nothing. The engine action is `net.standing`.
   */
  netStanding(input = {}, options = {}) {
    return this.call("net.standing", input, options);
  }

  /**
   * Why a past call was allowed or denied: the decision, the policy version, and the
   * rules that made it. For an auditor or an administrator. The engine action is
   * `policy.explain`.
   */
  policyExplain(input, options = {}) {
    return this.call("policy.explain", input, options);
  }

  /**
   * Publish a policy file after its own tests pass. A person's act; where the set in
   * force asks for a second administrator, the reviewer proves presence of their own.
   * The engine action is `policy.publish`.
   */
  policyPublish(input, options = {}) {
    return this.call("policy.publish", input, options);
  }

  /**
   * Run a policy file's decision table against its own layers and report each failure.
   * Publishes nothing. The engine action is `policy.test`.
   */
  policyTest(input, options = {}) {
    return this.call("policy.test", input, options);
  }

  /**
   * Which published tools a principal may use, through a toolset where one is named.
   * Records no decision. The engine action is `policy.tools_allowed`.
   */
  policyToolsAllowed(input, options = {}) {
    return this.call("policy.tools_allowed", input, options);
  }

  /**
   * The history of published policy versions, oldest first, and which is in force. The
   * engine action is `policy.versions`.
   */
  policyVersions(input = {}, options = {}) {
    return this.call("policy.versions", input, options);
  }

  /**
   * Which of the principals named may use one published tool, through a toolset where
   * one is named. Records no decision. The engine action is `policy.who_can`.
   */
  policyWhoCan(input, options = {}) {
    return this.call("policy.who_can", input, options);
  }

  /**
   * One provider's manifest: what it supplies, its effects, its requirements, and its
   * settings. The engine action is `provider.describe`.
   */
  providerDescribe(input, options = {}) {
    return this.call("provider.describe", input, options);
  }

  /**
   * Every pack and provider: installed, enabled, ready, or what it lacks and how to get
   * it. The engine action is `provider.list`.
   */
  providerList(input = {}, options = {}) {
    return this.call("provider.list", input, options);
  }

  /**
   * Submit a target's drafts for a person's review. Publishing itself is a person's
   * act. The engine action is `publish.request`.
   */
  publishRequest(input, options = {}) {
    return this.call("publish.request", input, options);
  }

  /**
   * The reaches a person has named: each label, the names of the facts it states,
   * whether it is in force on the network this machine is on now, and the entries the
   * person wrote. Shows no digest. The engine action is `reach.list`.
   */
  reachList(input = {}, options = {}) {
    return this.call("reach.list", input, options);
  }

  /**
   * A person names the network this machine is on now, with the destinations that may
   * be looked at and sent to, and the gateway names the person chooses. The network is
   * read from the machine, never taken from the input. A person's act, never an
   * agent's. The engine action is `reach.name`.
   */
  reachName(input, options = {}) {
    return this.call("reach.name", input, options);
  }

  /**
   * A person forgets one reach by its label. The engine action is `reach.withdraw`.
   */
  reachWithdraw(input, options = {}) {
    return this.call("reach.withdraw", input, options);
  }

  /**
   * Stop a running or waiting run. Says what was completed before it stopped. The
   * engine action is `run.cancel`.
   */
  runCancel(input, options = {}) {
    return this.call("run.cancel", input, options);
  }

  /**
   * The status, progress, result, and trace reference of one run. The engine action is
   * `run.get`.
   */
  runGet(input, options = {}) {
    return this.call("run.get", input, options);
  }

  /**
   * The caller's recent runs, newest first: each run's state, whether a record of it is
   * in doubt, whether it waits for a person, and the next step. After a lost answer,
   * list the runs since the call was sent to find its run before calling again. The
   * engine action is `run.list`.
   */
  runList(input = {}, options = {}) {
    return this.call("run.list", input, options);
  }

  /**
   * Continue a run that rests or stopped. The engine reconciles every doubtful record
   * first; an approved write goes on from its checkpoint. The engine action is
   * `run.resume`.
   */
  runResume(input, options = {}) {
    return this.call("run.resume", input, options);
  }

  /**
   * A summary of one of the caller's own runs' trace: each step's kind, outcome, and
   * time, with references to its kept captures, and never a value. The engine action is
   * `run.trace`.
   */
  runTrace(input, options = {}) {
    return this.call("run.trace", input, options);
  }

  /**
   * The last scout's report on a target: found, drafted, proven, open, and what to look
   * at first. Each list is counted and shows its first entries; `part` reads one whole,
   * in pages. The engine action is `scout.report`.
   */
  scoutReport(input, options = {}) {
    return this.call("scout.report", input, options);
  }

  /**
   * Identify a target, run the discoverers that suit it, draft what follows by rule,
   * and try the drafted reads. Resumable; never writes to the system. The engine action
   * is `scout.run`.
   */
  scoutRun(input, options = {}) {
    return this.call("scout.run", input, options);
  }

  /**
   * Store a secret under a name, taken from a prompt or standard input. Nothing reads
   * it back. The engine action is `secret.set`.
   */
  secretSet(input, options = {}) {
    return this.call("secret.set", input, options);
  }

  /**
   * Update a standalone install to the newest release of its channel. The engine action
   * is `self.update`.
   */
  selfUpdate(input = {}, options = {}) {
    return this.call("self.update", input, options);
  }

  /**
   * End someone's session. It cannot be used to enter one. The engine action is
   * `session.end`.
   */
  sessionEnd(input, options = {}) {
    return this.call("session.end", input, options);
  }

  /**
   * The sessions of a workspace, without their contents. The engine action is
   * `session.list`.
   */
  sessionList(input = {}, options = {}) {
    return this.call("session.list", input, options);
  }

  /**
   * Ask the caller's person to sign in to one surface of a target. A credential is
   * never an input: where one is not stored, the person types it on the engine's own
   * page, and carries the sign-in out there. Returns that a session exists, for whom,
   * where, until when, and at which level; never its contents. The engine action is
   * `session.login`.
   */
  sessionLogin(input, options = {}) {
    return this.call("session.login", input, options);
  }

  /**
   * End one of the caller's own sessions. The engine action is `session.revoke`.
   */
  sessionRevoke(input, options = {}) {
    return this.call("session.revoke", input, options);
  }

  /**
   * Whether each target is signed in for the caller. Shows states and times, never a
   * credential. The engine action is `session.status`.
   */
  sessionStatus(input = {}, options = {}) {
    return this.call("session.status", input, options);
  }

  /**
   * How often a shadow binding matched the verified one, and where it differed. The
   * engine action is `shadow.report`.
   */
  shadowReport(input, options = {}) {
    return this.call("shadow.report", input, options);
  }

  /**
   * A new scout of a target, handed a description the system publishes about itself as
   * a file of the project's `inputs` folder: exactly `scout.run` with `files` naming it
   * `description`. Drafts by rule; never writes to the system. The engine action is
   * `spec.import`.
   */
  specImport(input, options = {}) {
    return this.call("spec.import", input, options);
  }

  /**
   * Register a target in the project: makes its folder and a first target file, with
   * one surface where an address and its kind are given. The engine action is
   * `target.add`.
   */
  targetAdd(input, options = {}) {
    return this.call("target.add", input, options);
  }

  /**
   * The targets in the project and the versions published from them. The engine action
   * is `target.list`.
   */
  targetList(input = {}, options = {}) {
    return this.call("target.list", input, options);
  }

  /**
   * The platform, framework, support tier, and surfaces at an address or in a running
   * application. Read only. The engine action is `target.probe`.
   */
  targetProbe(input, options = {}) {
    return this.call("target.probe", input, options);
  }

  /**
   * Print a target's compiled target as text: its tools, steps, surfaces, and
   * permissions. The engine action is `target.show`.
   */
  targetShow(input, options = {}) {
    return this.call("target.show", input, options);
  }

  /**
   * The target's build state and the next action to take. The engine action is
   * `target.status`.
   */
  targetStatus(input, options = {}) {
    return this.call("target.status", input, options);
  }

  /**
   * Run the publish gates on a target's drafts and return the evidence. The engine
   * action is `test.run`.
   */
  testRun(input, options = {}) {
    return this.call("test.run", input, options);
  }

  /**
   * Call one published operation or pipeline by name with its input. Returns the result
   * of a read, or the run's resting state (such as needs_approval) with the next step.
   * A write never proceeds without a person's approval. The engine action is
   * `tool.call`.
   */
  toolCall(input, options = {}) {
    return this.call("tool.call", input, options);
  }

  /**
   * The full description, input schema, and output schema of one published tool, by its
   * name, and its digest, which changes when and only when its definition does. The
   * engine action is `tool.describe`.
   */
  toolDescribe(input, options = {}) {
    return this.call("tool.describe", input, options);
  }

  /**
   * Find operations and pipelines of the published targets by words or module. Returns
   * names, one-line descriptions, effects, and whether each can be called now; call
   * tool.describe for a schema. The engine action is `tool.search`.
   */
  toolSearch(input = {}, options = {}) {
    return this.call("tool.search", input, options);
  }

  /**
   * Create a toolset: who may connect, which tools it selects, the highest effect class
   * and the identity kind of its runs. A toolset only narrows what grants allow. A
   * person's act, published as a new version of the policy set. The engine action is
   * `toolset.create`.
   */
  toolsetCreate(input, options = {}) {
    return this.call("toolset.create", input, options);
  }

  /**
   * The toolsets in force, each with the published tools it selects. The engine action
   * is `toolset.list`.
   */
  toolsetList(input = {}, options = {}) {
    return this.call("toolset.list", input, options);
  }

  /**
   * Remove a toolset. A person's act, published as a new version of the policy set. The
   * engine action is `toolset.remove`.
   */
  toolsetRemove(input, options = {}) {
    return this.call("toolset.remove", input, options);
  }

  /**
   * Change the fields given of a toolset. A person's act, published as a new version of
   * the policy set. The engine action is `toolset.update`.
   */
  toolsetUpdate(input, options = {}) {
    return this.call("toolset.update", input, options);
  }

  /**
   * Export one run's evidence to one file in the project's working folder,
   * `.anything2mcp/exports/<run>.json`: its events, its step trace, and its text
   * captures, with a manifest that names each piece and what was left out and why. The
   * engine action is `trace.export`.
   */
  traceExport(input, options = {}) {
    return this.call("trace.export", input, options);
  }

  /**
   * View one run's evidence: its trace lines and capture references. The engine action
   * is `trace.show`.
   */
  traceShow(input, options = {}) {
    return this.call("trace.show", input, options);
  }

  /**
   * The open questions about a target. The engine action is `unknown.list`.
   */
  unknownList(input, options = {}) {
    return this.call("unknown.list", input, options);
  }

  /**
   * Compare two versions of a target and classify each change. The engine action is
   * `version.diff`.
   */
  versionDiff(input, options = {}) {
    return this.call("version.diff", input, options);
  }

  /**
   * Publish a target's drafts, compiled, as an immutable version. A person's act, never
   * an agent's. The engine action is `version.publish`.
   */
  versionPublish(input, options = {}) {
    return this.call("version.publish", input, options);
  }

  /**
   * Make another published version of a target the one that runs now: an earlier one,
   * or a later one again after a rollback. A person's act, never an agent's. The engine
   * action is `version.rollback`.
   */
  versionRollback(input, options = {}) {
    return this.call("version.rollback", input, options);
  }

  /**
   * The changes a watch recorded after a number: added, changed, unseen, gaps, pauses,
   * and its end, each with the runs of its checks. Waits up to `wait_seconds` for the
   * first. Holds no value; read a check's run with run.get for that. The engine action
   * is `watch.changes`.
   */
  watchChanges(input, options = {}) {
    return this.call("watch.changes", input, options);
  }

  /**
   * Confirm a watch the caller's person was asked for, so that its checks begin. A
   * person's act, carried out on the engine's page, never an agent's. The engine action
   * is `watch.confirm`.
   */
  watchConfirm(input, options = {}) {
    return this.call("watch.confirm", input, options);
  }

  /**
   * Ask for a watch: one published read, called with one fixed input at a fixed
   * interval until an end, that records what was added, changed, or went out of view.
   * Nothing runs until the caller's person confirms it on the engine's page. The engine
   * action is `watch.create`.
   */
  watchCreate(input, options = {}) {
    return this.call("watch.create", input, options);
  }

  /**
   * End a watch, by its person or by the caller that asked for it. Its checks and
   * changes are kept. The engine action is `watch.end`.
   */
  watchEnd(input, options = {}) {
    return this.call("watch.end", input, options);
  }

  /**
   * One watch of the caller's person: its read, interval, end, state, and how many
   * checks it made. Never a value the read returned. The engine action is `watch.get`.
   */
  watchGet(input, options = {}) {
    return this.call("watch.get", input, options);
  }

  /**
   * The watches of the caller's person, oldest first. The engine action is
   * `watch.list`.
   */
  watchList(input = {}, options = {}) {
    return this.call("watch.list", input, options);
  }

  /**
   * Pause a confirmed watch of the caller's person: no check runs until it is resumed.
   * The engine action is `watch.pause`.
   */
  watchPause(input, options = {}) {
    return this.call("watch.pause", input, options);
  }

  /**
   * Let a paused watch of the caller's person go on. One paused because its read
   * changed or drifted never goes on. The engine action is `watch.resume`.
   */
  watchResume(input, options = {}) {
    return this.call("watch.resume", input, options);
  }
}

module.exports = { ACTIONS, Actions };
