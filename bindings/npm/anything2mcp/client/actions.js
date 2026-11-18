// GENERATED: do not edit. Written by scripts/generate-actions.js from the OpenAPI
// document the engine serves at /api/v1/openapi.json (engine version 0.0.1).
// A test fails when this file differs from what the generator writes. To change a
// method, change the action in the engine, save the new document, and run the
// generator.

"use strict";

// Every action this module has a method for, in the order of the document.
const ACTIONS = Object.freeze([
  "action.describe",
  "action.run",
  "action.search",
  "audit.query",
  "auth.detect",
  "binding.promote",
  "browser.install",
  "capture.start",
  "capture.stop",
  "client.connect",
  "component.list",
  "config.set",
  "config.show",
  "delegation.grant",
  "delegation.list",
  "delegation.revoke",
  "discover.run",
  "docs.get",
  "doctor.run",
  "draft.delete",
  "draft.lint",
  "draft.read",
  "draft.try",
  "draft.write",
  "endpoints.infer",
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
  "net.interfaces",
  "net.listeners",
  "net.neighbours",
  "net.proxy",
  "net.resolvers",
  "net.routes",
  "net.standing",
  "plan.show",
  "plugin.install",
  "plugin.list",
  "policy.explain",
  "policy.publish",
  "policy.test",
  "provider.describe",
  "provider.disable",
  "provider.enable",
  "provider.list",
  "publish.request",
  "run.cancel",
  "run.get",
  "run.list",
  "run.resume",
  "run.trace",
  "run.undo",
  "runner.drain",
  "runner.enroll",
  "runner.list",
  "runner.revoke",
  "scout.report",
  "scout.run",
  "screen.snapshot",
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
  "target.status",
  "test.run",
  "tool.call",
  "tool.describe",
  "tool.search",
  "toolset.create",
  "toolset.list",
  "toolset.update",
  "trace.export",
  "trace.show",
  "unknown.list",
  "unknown.settle",
  "vault.rotate",
  "version.activate",
  "version.bundle",
  "version.diff",
  "version.import",
  "version.migrate",
  "version.publish",
  "version.rollback",
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
   * Read the audit log, oldest first, and check its chain. The engine action is
   * `audit.query`.
   */
  auditQuery(input = {}, options = {}) {
    return this.call("audit.query", input, options);
  }

  /**
   * The sign-in method of a surface, with evidence, and a recommended identity mode.
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
   * Fetch a pinned browser for the pack that drives one. The engine action is
   * `browser.install`.
   */
  browserInstall(input = {}, options = {}) {
    return this.call("browser.install", input, options);
  }

  /**
   * Begin recording actions and traffic on a surface, redacted as it is recorded. The
   * engine action is `capture.start`.
   */
  captureStart(input, options = {}) {
    return this.call("capture.start", input, options);
  }

  /**
   * End the recording and store it. The engine action is `capture.stop`.
   */
  captureStop(input, options = {}) {
    return this.call("capture.stop", input, options);
  }

  /**
   * Give the configuration an MCP client needs to start the bridge for this project:
   * printed for the `generic` profile, or set in the file a client profile names, shown
   * first and written by a local person. The engine action is `client.connect`.
   */
  clientConnect(input = {}, options = {}) {
    return this.call("client.connect", input, options);
  }

  /**
   * The outside programs providers need: found or not, with path and digest. The engine
   * action is `component.list`.
   */
  componentList(input = {}, options = {}) {
    return this.call("component.list", input, options);
  }

  /**
   * Change one setting in a settings file. Loosening a protected setting needs a person
   * present. The engine action is `config.set`.
   */
  configSet(input, options = {}) {
    return this.call("config.set", input, options);
  }

  /**
   * The effective settings, and for each the layer that set it. With a key, that key's
   * documentation. The engine action is `config.show`.
   */
  configShow(input = {}, options = {}) {
    return this.call("config.show", input, options);
  }

  /**
   * A person lets an agent act for them. The engine action is `delegation.grant`.
   */
  delegationGrant(input, options = {}) {
    return this.call("delegation.grant", input, options);
  }

  /**
   * The delegations a person has given. The engine action is `delegation.list`.
   */
  delegationList(input = {}, options = {}) {
    return this.call("delegation.list", input, options);
  }

  /**
   * A person withdraws a delegation. The engine action is `delegation.revoke`.
   */
  delegationRevoke(input, options = {}) {
    return this.call("delegation.revoke", input, options);
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
   * Endpoint templates, schemas, and dependencies worked out from stored recordings.
   * The engine action is `endpoints.infer`.
   */
  endpointsInfer(input, options = {}) {
    return this.call("endpoints.infer", input, options);
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
   * Grant a principal or a group the use of tools up to an effect class. The engine
   * action is `grant.add`.
   */
  grantAdd(input, options = {}) {
    return this.call("grant.add", input, options);
  }

  /**
   * The grants of the workspace. The engine action is `grant.list`.
   */
  grantList(input = {}, options = {}) {
    return this.call("grant.list", input, options);
  }

  /**
   * Remove a grant. The engine action is `grant.remove`.
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
   * Print a compiled plan as text: its tools, steps, surfaces, and permissions. The
   * engine action is `plan.show`.
   */
  planShow(input, options = {}) {
    return this.call("plan.show", input, options);
  }

  /**
   * Install a plug-in from a file, after checking its manifest and signature. The
   * engine action is `plugin.install`.
   */
  pluginInstall(input, options = {}) {
    return this.call("plugin.install", input, options);
  }

  /**
   * The plug-ins found on this machine. The engine action is `plugin.list`.
   */
  pluginList(input = {}, options = {}) {
    return this.call("plugin.list", input, options);
  }

  /**
   * Why a past call was allowed or denied: the decision, the policy version, and the
   * rules that made it. The engine action is `policy.explain`.
   */
  policyExplain(input, options = {}) {
    return this.call("policy.explain", input, options);
  }

  /**
   * Publish a policy set after its own tests pass. The engine action is
   * `policy.publish`.
   */
  policyPublish(input, options = {}) {
    return this.call("policy.publish", input, options);
  }

  /**
   * Run a policy set's decision table and report each failure. The engine action is
   * `policy.test`.
   */
  policyTest(input, options = {}) {
    return this.call("policy.test", input, options);
  }

  /**
   * One provider's manifest: what it supplies, its effects, its requirements, and its
   * settings. The engine action is `provider.describe`.
   */
  providerDescribe(input, options = {}) {
    return this.call("provider.describe", input, options);
  }

  /**
   * Switch a provider off. The engine action is `provider.disable`.
   */
  providerDisable(input, options = {}) {
    return this.call("provider.disable", input, options);
  }

  /**
   * Switch a provider on. A protected setting on one machine where the provider starts
   * processes or reaches a network. The engine action is `provider.enable`.
   */
  providerEnable(input, options = {}) {
    return this.call("provider.enable", input, options);
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
   * A summary of one run's trace, with references to its kept artifacts. The engine
   * action is `run.trace`.
   */
  runTrace(input, options = {}) {
    return this.call("run.trace", input, options);
  }

  /**
   * Build the reverse diff of a completed write run, for a person's approval. The
   * engine action is `run.undo`.
   */
  runUndo(input, options = {}) {
    return this.call("run.undo", input, options);
  }

  /**
   * Stop giving a runner new jobs. The engine action is `runner.drain`.
   */
  runnerDrain(input, options = {}) {
    return this.call("runner.drain", input, options);
  }

  /**
   * Enroll a runner on another machine. The engine action is `runner.enroll`.
   */
  runnerEnroll(input, options = {}) {
    return this.call("runner.enroll", input, options);
  }

  /**
   * The runners, what each serves, and when each was last heard from. The engine action
   * is `runner.list`.
   */
  runnerList(input = {}, options = {}) {
    return this.call("runner.list", input, options);
  }

  /**
   * Revoke a runner's enrollment. The engine action is `runner.revoke`.
   */
  runnerRevoke(input, options = {}) {
    return this.call("runner.revoke", input, options);
  }

  /**
   * The last scout's report on a target: found, drafted, proven, open, and what to look
   * at first. The engine action is `scout.report`.
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
   * A compact snapshot of what a surface shows now, with stable references to its
   * elements. The engine action is `screen.snapshot`.
   */
  screenSnapshot(input, options = {}) {
    return this.call("screen.snapshot", input, options);
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
   * a file of the project's `sources` folder: exactly `scout.run` with `files` naming
   * it `description`. Drafts by rule; never writes to the system. The engine action is
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
   * name. The engine action is `tool.describe`.
   */
  toolDescribe(input, options = {}) {
    return this.call("tool.describe", input, options);
  }

  /**
   * Find operations and pipelines of the published targets by words or module. Returns
   * names, one-line descriptions, and effects; call tool.describe for a schema. The
   * engine action is `tool.search`.
   */
  toolSearch(input = {}, options = {}) {
    return this.call("tool.search", input, options);
  }

  /**
   * Create a toolset: which tools, which version, which identity mode. The engine
   * action is `toolset.create`.
   */
  toolsetCreate(input, options = {}) {
    return this.call("toolset.create", input, options);
  }

  /**
   * The toolsets of the workspace. The engine action is `toolset.list`.
   */
  toolsetList(input = {}, options = {}) {
    return this.call("toolset.list", input, options);
  }

  /**
   * Change a toolset. The engine action is `toolset.update`.
   */
  toolsetUpdate(input, options = {}) {
    return this.call("toolset.update", input, options);
  }

  /**
   * Export one run's evidence to a file in the project. The engine action is
   * `trace.export`.
   */
  traceExport(input, options = {}) {
    return this.call("trace.export", input, options);
  }

  /**
   * View one run's evidence: its trace lines and artifact references. The engine action
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
   * Settle an open question about a target, with evidence. The engine action is
   * `unknown.settle`.
   */
  unknownSettle(input, options = {}) {
    return this.call("unknown.settle", input, options);
  }

  /**
   * Rotate one layer of the vault's keys. The engine action is `vault.rotate`.
   */
  vaultRotate(input, options = {}) {
    return this.call("vault.rotate", input, options);
  }

  /**
   * Move a toolset to a version, through the candidate stage. The engine action is
   * `version.activate`.
   */
  versionActivate(input, options = {}) {
    return this.call("version.activate", input, options);
  }

  /**
   * Pack a published version into a signed file. The engine action is `version.bundle`.
   */
  versionBundle(input, options = {}) {
    return this.call("version.bundle", input, options);
  }

  /**
   * Compare two versions of a target and classify each change. The engine action is
   * `version.diff`.
   */
  versionDiff(input, options = {}) {
    return this.call("version.diff", input, options);
  }

  /**
   * Import a signed bundle as a published version. The engine action is
   * `version.import`.
   */
  versionImport(input, options = {}) {
    return this.call("version.import", input, options);
  }

  /**
   * Recompile stored sources for a newer engine. The engine action is
   * `version.migrate`.
   */
  versionMigrate(input, options = {}) {
    return this.call("version.migrate", input, options);
  }

  /**
   * Publish a target's drafts as an immutable version, compiled to a plan. A person's
   * act, never an agent's. The engine action is `version.publish`.
   */
  versionPublish(input, options = {}) {
    return this.call("version.publish", input, options);
  }

  /**
   * Make an earlier version of a target current. The engine action is
   * `version.rollback`.
   */
  versionRollback(input, options = {}) {
    return this.call("version.rollback", input, options);
  }
}

module.exports = { ACTIONS, Actions };
