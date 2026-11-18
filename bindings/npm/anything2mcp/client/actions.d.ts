// GENERATED: do not edit. Written by scripts/generate-actions.js from the OpenAPI
// document the engine serves at /api/v1/openapi.json (engine version 0.0.1).
// A test fails when this file differs from what the generator writes. To change a
// method, change the action in the engine, save the new document, and run the
// generator.

import type { CallOptions, Response } from "./index";

/**
 * The input of `action.describe`.
 */
export interface ActionDescribeInput {
  action: string;
}

/**
 * The input of `action.run`.
 */
export interface ActionRunInput {
  action: string;
  input?: Record<string, unknown>;
}

/**
 * The input of `action.search`.
 */
export interface ActionSearchInput {
  query?: string;
}

/**
 * The input of `audit.query`.
 */
export interface AuditQueryInput {
  limit?: number;
  offset?: number;
}

/**
 * The input of `auth.detect`.
 */
export interface AuthDetectInput {
  target: string;
  surface?: string;
  wait_seconds?: number;
}

/**
 * The input of `binding.promote`.
 */
export interface BindingPromoteInput {
  mode: string;
  target: string;
  tool: string;
  position?: number;
  run_id?: string;
}

/**
 * The input of `browser.install`.
 */
export type BrowserInstallInput = Record<string, never>;

/**
 * The input of `capture.start`.
 */
export interface CaptureStartInput {
  target: string;
  surface?: string;
}

/**
 * The input of `capture.stop`.
 */
export interface CaptureStopInput {
  target: string;
}

/**
 * The input of `client.connect`.
 */
export interface ClientConnectInput {
  client?: string;
  name?: string;
  print?: boolean;
  toolset?: "use" | "build";
  yes?: boolean;
}

/**
 * The input of `component.list`.
 */
export type ComponentListInput = Record<string, never>;

/**
 * The input of `config.set`.
 */
export interface ConfigSetInput {
  key: string;
  value: string;
  layer?: "user" | "project";
}

/**
 * The input of `config.show`.
 */
export interface ConfigShowInput {
  key?: string;
}

/**
 * The input of `delegation.grant`.
 */
export interface DelegationGrantInput {
  agent: string;
}

/**
 * The input of `delegation.list`.
 */
export type DelegationListInput = Record<string, never>;

/**
 * The input of `delegation.revoke`.
 */
export interface DelegationRevokeInput {
  agent: string;
}

/**
 * The input of `discover.run`.
 */
export interface DiscoverRunInput {
  target: string;
  files?: Record<string, unknown>;
  surface?: string;
}

/**
 * The input of `docs.get`.
 */
export interface DocsGetInput {
  topic?: string;
}

/**
 * The input of `doctor.run`.
 */
export type DoctorRunInput = Record<string, never>;

/**
 * The input of `draft.delete`.
 */
export interface DraftDeleteInput {
  target: string;
  file?: string;
  files?: string[];
}

/**
 * The input of `draft.lint`.
 */
export interface DraftLintInput {
  target: string;
}

/**
 * The input of `draft.read`.
 */
export interface DraftReadInput {
  file: string;
  target: string;
}

/**
 * The input of `draft.try`.
 */
export interface DraftTryInput {
  target: string;
  tool: string;
  input?: Record<string, unknown>;
  mode?: string;
  position?: number;
}

/**
 * The input of `draft.write`.
 */
export interface DraftWriteInput {
  content: string;
  file: string;
  target: string;
}

/**
 * The input of `endpoints.infer`.
 */
export interface EndpointsInferInput {
  target: string;
}

/**
 * The input of `engine.logs`.
 */
export interface EngineLogsInput {
  lines?: number;
}

/**
 * The input of `engine.start`.
 */
export type EngineStartInput = Record<string, never>;

/**
 * The input of `engine.status`.
 */
export type EngineStatusInput = Record<string, never>;

/**
 * The input of `engine.stop`.
 */
export type EngineStopInput = Record<string, never>;

/**
 * The input of `finding.describe`.
 */
export interface FindingDescribeInput {
  finding: string;
  target: string;
}

/**
 * The input of `finding.list`.
 */
export interface FindingListInput {
  target: string;
  kind?: string;
  limit?: number;
  offset?: number;
}

/**
 * The input of `fixture.record`.
 */
export interface FixtureRecordInput {
  target: string;
  tool: string;
  input?: Record<string, unknown>;
  mode?: string;
  name?: string;
  position?: number;
}

/**
 * The input of `grant.add`.
 */
export interface GrantAddInput {
  max_effect: "read" | "write" | "irreversible";
  scope: string;
  who: string;
}

/**
 * The input of `grant.list`.
 */
export type GrantListInput = Record<string, never>;

/**
 * The input of `grant.remove`.
 */
export interface GrantRemoveInput {
  scope: string;
  who: string;
}

/**
 * The input of `killswitch.set`.
 */
export interface KillswitchSetInput {
  on: boolean;
  scope: "everything" | "workspace" | "target";
  target?: string;
}

/**
 * The input of `net.interfaces`.
 */
export type NetInterfacesInput = Record<string, never>;

/**
 * The input of `net.listeners`.
 */
export type NetListenersInput = Record<string, never>;

/**
 * The input of `net.neighbours`.
 */
export type NetNeighboursInput = Record<string, never>;

/**
 * The input of `net.proxy`.
 */
export type NetProxyInput = Record<string, never>;

/**
 * The input of `net.resolvers`.
 */
export type NetResolversInput = Record<string, never>;

/**
 * The input of `net.routes`.
 */
export type NetRoutesInput = Record<string, never>;

/**
 * The input of `net.standing`.
 */
export type NetStandingInput = Record<string, never>;

/**
 * The input of `plan.show`.
 */
export interface PlanShowInput {
  target: string;
  version?: string;
}

/**
 * The input of `plugin.install`.
 */
export interface PluginInstallInput {
  file: string;
}

/**
 * The input of `plugin.list`.
 */
export type PluginListInput = Record<string, never>;

/**
 * The input of `policy.explain`.
 */
export interface PolicyExplainInput {
  decision: string;
}

/**
 * The input of `policy.publish`.
 */
export interface PolicyPublishInput {
  file: string;
}

/**
 * The input of `policy.test`.
 */
export interface PolicyTestInput {
  file: string;
}

/**
 * The input of `provider.describe`.
 */
export interface ProviderDescribeInput {
  provider: string;
}

/**
 * The input of `provider.disable`.
 */
export interface ProviderDisableInput {
  provider: string;
}

/**
 * The input of `provider.enable`.
 */
export interface ProviderEnableInput {
  provider: string;
}

/**
 * The input of `provider.list`.
 */
export type ProviderListInput = Record<string, never>;

/**
 * The input of `publish.request`.
 */
export interface PublishRequestInput {
  target: string;
  version?: string;
}

/**
 * The input of `run.cancel`.
 */
export interface RunCancelInput {
  run_id: string;
}

/**
 * The input of `run.get`.
 */
export interface RunGetInput {
  run_id: string;
}

/**
 * The input of `run.list`.
 */
export interface RunListInput {
  after?: string;
  limit?: number;
  since?: number;
  state?: string[];
  target?: string;
  tool?: string;
}

/**
 * The input of `run.resume`.
 */
export interface RunResumeInput {
  run_id: string;
  wait_seconds?: number;
}

/**
 * The input of `run.trace`.
 */
export interface RunTraceInput {
  run_id: string;
}

/**
 * The input of `run.undo`.
 */
export interface RunUndoInput {
  run_id: string;
}

/**
 * The input of `runner.drain`.
 */
export interface RunnerDrainInput {
  runner: string;
}

/**
 * The input of `runner.enroll`.
 */
export interface RunnerEnrollInput {
  name: string;
}

/**
 * The input of `runner.list`.
 */
export type RunnerListInput = Record<string, never>;

/**
 * The input of `runner.revoke`.
 */
export interface RunnerRevokeInput {
  runner: string;
}

/**
 * The input of `scout.report`.
 */
export interface ScoutReportInput {
  target: string;
}

/**
 * The input of `scout.run`.
 */
export interface ScoutRunInput {
  target: string;
  files?: Record<string, unknown>;
  fresh?: boolean;
  wait_seconds?: number;
}

/**
 * The input of `screen.snapshot`.
 */
export interface ScreenSnapshotInput {
  target: string;
  limit?: number;
  offset?: number;
  surface?: string;
}

/**
 * The input of `secret.set`.
 */
export interface SecretSetInput {
  name: string;
  stdin?: boolean;
  value?: string;
}

/**
 * The input of `self.update`.
 */
export type SelfUpdateInput = Record<string, never>;

/**
 * The input of `session.end`.
 */
export interface SessionEndInput {
  session_id: string;
}

/**
 * The input of `session.list`.
 */
export interface SessionListInput {
  target?: string;
}

/**
 * The input of `session.login`.
 */
export interface SessionLoginInput {
  target: string;
  environment?: string;
  surface?: string;
}

/**
 * The input of `session.revoke`.
 */
export interface SessionRevokeInput {
  session_id: string;
}

/**
 * The input of `session.status`.
 */
export interface SessionStatusInput {
  target?: string;
}

/**
 * The input of `shadow.report`.
 */
export interface ShadowReportInput {
  target: string;
  tool?: string;
}

/**
 * The input of `spec.import`.
 */
export interface SpecImportInput {
  file: string;
  target: string;
  wait_seconds?: number;
}

/**
 * The input of `target.add`.
 */
export interface TargetAddInput {
  name: string;
  address?: string;
  kind?: string;
}

/**
 * The input of `target.list`.
 */
export type TargetListInput = Record<string, never>;

/**
 * The input of `target.probe`.
 */
export interface TargetProbeInput {
  target: string;
  surface?: string;
  wait_seconds?: number;
}

/**
 * The input of `target.status`.
 */
export interface TargetStatusInput {
  target: string;
}

/**
 * The input of `test.run`.
 */
export interface TestRunInput {
  target: string;
}

/**
 * The input of `tool.call`.
 */
export interface ToolCallInput {
  tool: string;
  environment?: string;
  input?: Record<string, unknown>;
  source_file?: string;
  source_rows?: Record<string, unknown>[];
  wait_seconds?: number;
}

/**
 * The input of `tool.describe`.
 */
export interface ToolDescribeInput {
  tool: string;
}

/**
 * The input of `tool.search`.
 */
export interface ToolSearchInput {
  full?: boolean;
  kind?: "operation" | "pipeline";
  limit?: number;
  module?: string;
  offset?: number;
  query?: string;
  target?: string;
}

/**
 * The input of `toolset.create`.
 */
export interface ToolsetCreateInput {
  name: string;
  tools: string[];
}

/**
 * The input of `toolset.list`.
 */
export type ToolsetListInput = Record<string, never>;

/**
 * The input of `toolset.update`.
 */
export interface ToolsetUpdateInput {
  name: string;
  tools?: string[];
}

/**
 * The input of `trace.export`.
 */
export interface TraceExportInput {
  run_id: string;
}

/**
 * The input of `trace.show`.
 */
export interface TraceShowInput {
  run_id: string;
}

/**
 * The input of `unknown.list`.
 */
export interface UnknownListInput {
  target: string;
}

/**
 * The input of `unknown.settle`.
 */
export interface UnknownSettleInput {
  evidence: string;
  unknown: string;
}

/**
 * The input of `vault.rotate`.
 */
export interface VaultRotateInput {
  layer: "root" | "workspace" | "owner";
}

/**
 * The input of `version.activate`.
 */
export interface VersionActivateInput {
  target: string;
  version: string;
  toolset?: string;
}

/**
 * The input of `version.bundle`.
 */
export interface VersionBundleInput {
  target: string;
  version?: string;
}

/**
 * The input of `version.diff`.
 */
export interface VersionDiffInput {
  from: string;
  target: string;
  to: string;
}

/**
 * The input of `version.import`.
 */
export interface VersionImportInput {
  file: string;
}

/**
 * The input of `version.migrate`.
 */
export interface VersionMigrateInput {
  target: string;
}

/**
 * The input of `version.publish`.
 */
export interface VersionPublishInput {
  target: string;
  plan_hash?: string;
  version?: string;
}

/**
 * The input of `version.rollback`.
 */
export interface VersionRollbackInput {
  target: string;
  version: string;
}

/**
 * The input of `watch.changes`.
 */
export interface WatchChangesInput {
  watch_id: string;
  after?: number;
  limit?: number;
  wait_seconds?: number;
}

/**
 * The input of `watch.confirm`.
 */
export interface WatchConfirmInput {
  watch_id: string;
}

/**
 * The input of `watch.create`.
 */
export interface WatchCreateInput {
  ends_at: number;
  interval_seconds: number;
  tool: string;
  environment?: string;
  ignore?: string[];
  input?: Record<string, unknown>;
}

/**
 * The input of `watch.end`.
 */
export interface WatchEndInput {
  watch_id: string;
}

/**
 * The input of `watch.get`.
 */
export interface WatchGetInput {
  watch_id: string;
}

/**
 * The input of `watch.list`.
 */
export interface WatchListInput {
  open_only?: boolean;
}

/**
 * The input of `watch.pause`.
 */
export interface WatchPauseInput {
  watch_id: string;
}

/**
 * The input of `watch.resume`.
 */
export interface WatchResumeInput {
  watch_id: string;
}

/** Every action this module has a method for, in the order of the document. */
export declare const ACTIONS: readonly string[];

/** The typed methods. `Client` extends this class. */
export declare abstract class Actions {
  /** Sends one action by its name, such as `engine.status`, with its input. */
  abstract call(action: string, input?: Record<string, unknown>, options?: CallOptions): Promise<Response>;

  /**
   * One action in full: its summary, input schema, output schema, examples, effect, and
   * role. The engine action is `action.describe`.
   */
  actionDescribe(input: ActionDescribeInput, options?: CallOptions): Promise<Response>;

  /**
   * Run any action by name with its input. The action's own role, schema, and policy
   * decision apply. The engine action is `action.run`.
   */
  actionRun(input: ActionRunInput, options?: CallOptions): Promise<Response>;

  /**
   * Find actions by words. Returns names and one-line summaries only. The engine action
   * is `action.search`.
   */
  actionSearch(input?: ActionSearchInput, options?: CallOptions): Promise<Response>;

  /**
   * Read the audit log, oldest first, and check its chain. The engine action is
   * `audit.query`.
   */
  auditQuery(input?: AuditQueryInput, options?: CallOptions): Promise<Response>;

  /**
   * The sign-in method of a surface, with evidence, and a recommended identity mode.
   * Read only. The engine action is `auth.detect`.
   */
  authDetect(input: AuthDetectInput, options?: CallOptions): Promise<Response>;

  /**
   * Make the first binding of a drafted operation verified, on the evidence of its
   * offline tests and a trial, or a binding in shadow verified, on its run of exact
   * matches. For an operation that writes, and for a binding in shadow, a person's act.
   * The engine action is `binding.promote`.
   */
  bindingPromote(input: BindingPromoteInput, options?: CallOptions): Promise<Response>;

  /**
   * Fetch a pinned browser for the pack that drives one. The engine action is
   * `browser.install`.
   */
  browserInstall(input?: BrowserInstallInput, options?: CallOptions): Promise<Response>;

  /**
   * Begin recording actions and traffic on a surface, redacted as it is recorded. The
   * engine action is `capture.start`.
   */
  captureStart(input: CaptureStartInput, options?: CallOptions): Promise<Response>;

  /**
   * End the recording and store it. The engine action is `capture.stop`.
   */
  captureStop(input: CaptureStopInput, options?: CallOptions): Promise<Response>;

  /**
   * Give the configuration an MCP client needs to start the bridge for this project:
   * printed for the `generic` profile, or set in the file a client profile names, shown
   * first and written by a local person. The engine action is `client.connect`.
   */
  clientConnect(input?: ClientConnectInput, options?: CallOptions): Promise<Response>;

  /**
   * The outside programs providers need: found or not, with path and digest. The engine
   * action is `component.list`.
   */
  componentList(input?: ComponentListInput, options?: CallOptions): Promise<Response>;

  /**
   * Change one setting in a settings file. Loosening a protected setting needs a person
   * present. The engine action is `config.set`.
   */
  configSet(input: ConfigSetInput, options?: CallOptions): Promise<Response>;

  /**
   * The effective settings, and for each the layer that set it. With a key, that key's
   * documentation. The engine action is `config.show`.
   */
  configShow(input?: ConfigShowInput, options?: CallOptions): Promise<Response>;

  /**
   * A person lets an agent act for them. The engine action is `delegation.grant`.
   */
  delegationGrant(input: DelegationGrantInput, options?: CallOptions): Promise<Response>;

  /**
   * The delegations a person has given. The engine action is `delegation.list`.
   */
  delegationList(input?: DelegationListInput, options?: CallOptions): Promise<Response>;

  /**
   * A person withdraws a delegation. The engine action is `delegation.revoke`.
   */
  delegationRevoke(input: DelegationRevokeInput, options?: CallOptions): Promise<Response>;

  /**
   * Run the discoverers that suit a target's surfaces, in the sandbox. Returns
   * findings; never writes. The engine action is `discover.run`.
   */
  discoverRun(input: DiscoverRunInput, options?: CallOptions): Promise<Response>;

  /**
   * A built-in reference page, by topic or action name. With no topic, the list of
   * topics. The engine action is `docs.get`.
   */
  docsGet(input?: DocsGetInput, options?: CallOptions): Promise<Response>;

  /**
   * Check the settings, the store, the key store, the providers, and the clock, and
   * give a fix for each problem. Makes no outside connection. The engine action is
   * `doctor.run`.
   */
  doctorRun(input?: DoctorRunInput, options?: CallOptions): Promise<Response>;

  /**
   * Remove draft files of a target from the project, one by `file` or several by
   * `files`, all or none; a published version keeps its own copy. The engine action is
   * `draft.delete`.
   */
  draftDelete(input: DraftDeleteInput, options?: CallOptions): Promise<Response>;

  /**
   * Lint and compile a target's draft files and report every problem with its file and
   * line. Changes nothing. The engine action is `draft.lint`.
   */
  draftLint(input: DraftLintInput, options?: CallOptions): Promise<Response>;

  /**
   * Read one draft file of a target; stands in for a file system. The engine action is
   * `draft.read`.
   */
  draftRead(input: DraftReadInput, options?: CallOptions): Promise<Response>;

  /**
   * Run one draft binding in the sandbox, where writes go only to declared test
   * records. The engine action is `draft.try`.
   */
  draftTry(input: DraftTryInput, options?: CallOptions): Promise<Response>;

  /**
   * Write one draft file of a target after validating and linting it. The engine action
   * is `draft.write`.
   */
  draftWrite(input: DraftWriteInput, options?: CallOptions): Promise<Response>;

  /**
   * Endpoint templates, schemas, and dependencies worked out from stored recordings.
   * The engine action is `endpoints.infer`.
   */
  endpointsInfer(input: EndpointsInferInput, options?: CallOptions): Promise<Response>;

  /**
   * The local engine's recent log lines. The engine action is `engine.logs`.
   */
  engineLogs(input?: EngineLogsInput, options?: CallOptions): Promise<Response>;

  /**
   * Start the local engine. The engine action is `engine.start`.
   */
  engineStart(input?: EngineStartInput, options?: CallOptions): Promise<Response>;

  /**
   * Whether the local engine runs, its health state, and the cause of anything but
   * `ok`. The engine action is `engine.status`.
   */
  engineStatus(input?: EngineStatusInput, options?: CallOptions): Promise<Response>;

  /**
   * Stop the local engine after saving its sessions. The engine action is
   * `engine.stop`.
   */
  engineStop(input?: EngineStopInput, options?: CallOptions): Promise<Response>;

  /**
   * One finding in full, with its evidence references. The engine action is
   * `finding.describe`.
   */
  findingDescribe(input: FindingDescribeInput, options?: CallOptions): Promise<Response>;

  /**
   * Findings for a target, by level, kind, or source. The engine action is
   * `finding.list`.
   */
  findingList(input: FindingListInput, options?: CallOptions): Promise<Response>;

  /**
   * Record an offline test's fixture from one real read, every value scrubbed by rule,
   * and the test that replays it. The engine action is `fixture.record`.
   */
  fixtureRecord(input: FixtureRecordInput, options?: CallOptions): Promise<Response>;

  /**
   * Grant a principal or a group the use of tools up to an effect class. The engine
   * action is `grant.add`.
   */
  grantAdd(input: GrantAddInput, options?: CallOptions): Promise<Response>;

  /**
   * The grants of the workspace. The engine action is `grant.list`.
   */
  grantList(input?: GrantListInput, options?: CallOptions): Promise<Response>;

  /**
   * Remove a grant. The engine action is `grant.remove`.
   */
  grantRemove(input: GrantRemoveInput, options?: CallOptions): Promise<Response>;

  /**
   * Stop, or release, a target, a workspace, or the whole system. Releasing needs a
   * person present. The engine action is `killswitch.set`.
   */
  killswitchSet(input: KillswitchSetInput, options?: CallOptions): Promise<Response>;

  /**
   * This machine's network interfaces and their addresses, read from the operating
   * system. Hardware addresses are shown as their length only. Sends nothing. The
   * engine action is `net.interfaces`.
   */
  netInterfaces(input?: NetInterfacesInput, options?: CallOptions): Promise<Response>;

  /**
   * The sockets listening on this machine, each with the kind of place its address is,
   * and the program that holds it where this user may see it. Sends nothing. The engine
   * action is `net.listeners`.
   */
  netListeners(input?: NetListenersInput, options?: CallOptions): Promise<Response>;

  /**
   * The neighbour table this machine already holds, never refreshed by asking. Hardware
   * addresses are shown as their length only. Sends nothing. The engine action is
   * `net.neighbours`.
   */
  netNeighbours(input?: NetNeighboursInput, options?: CallOptions): Promise<Response>;

  /**
   * The proxy setting this engine reads, with any credentials removed: on Linux this
   * engine's own environment, not the caller's; on Windows the user's system setting.
   * Read to be shown, never applied. Sends nothing. The engine action is `net.proxy`.
   */
  netProxy(input?: NetProxyInput, options?: CallOptions): Promise<Response>;

  /**
   * The name servers and search domains this machine uses, as the system states them.
   * Sends nothing. The engine action is `net.resolvers`.
   */
  netResolvers(input?: NetResolversInput, options?: CallOptions): Promise<Response>;

  /**
   * This machine's routes, and each family's default routes, lowest metric first. Sends
   * nothing. The engine action is `net.routes`.
   */
  netRoutes(input?: NetRoutesInput, options?: CallOptions): Promise<Response>;

  /**
   * Where this machine stands on its networks: for each interface, the kind of place
   * each of its addresses is, and which interface carries each family's preferred
   * default route. Sends nothing. The engine action is `net.standing`.
   */
  netStanding(input?: NetStandingInput, options?: CallOptions): Promise<Response>;

  /**
   * Print a compiled plan as text: its tools, steps, surfaces, and permissions. The
   * engine action is `plan.show`.
   */
  planShow(input: PlanShowInput, options?: CallOptions): Promise<Response>;

  /**
   * Install a plug-in from a file, after checking its manifest and signature. The
   * engine action is `plugin.install`.
   */
  pluginInstall(input: PluginInstallInput, options?: CallOptions): Promise<Response>;

  /**
   * The plug-ins found on this machine. The engine action is `plugin.list`.
   */
  pluginList(input?: PluginListInput, options?: CallOptions): Promise<Response>;

  /**
   * Why a past call was allowed or denied: the decision, the policy version, and the
   * rules that made it. The engine action is `policy.explain`.
   */
  policyExplain(input: PolicyExplainInput, options?: CallOptions): Promise<Response>;

  /**
   * Publish a policy set after its own tests pass. The engine action is
   * `policy.publish`.
   */
  policyPublish(input: PolicyPublishInput, options?: CallOptions): Promise<Response>;

  /**
   * Run a policy set's decision table and report each failure. The engine action is
   * `policy.test`.
   */
  policyTest(input: PolicyTestInput, options?: CallOptions): Promise<Response>;

  /**
   * One provider's manifest: what it supplies, its effects, its requirements, and its
   * settings. The engine action is `provider.describe`.
   */
  providerDescribe(input: ProviderDescribeInput, options?: CallOptions): Promise<Response>;

  /**
   * Switch a provider off. The engine action is `provider.disable`.
   */
  providerDisable(input: ProviderDisableInput, options?: CallOptions): Promise<Response>;

  /**
   * Switch a provider on. A protected setting on one machine where the provider starts
   * processes or reaches a network. The engine action is `provider.enable`.
   */
  providerEnable(input: ProviderEnableInput, options?: CallOptions): Promise<Response>;

  /**
   * Every pack and provider: installed, enabled, ready, or what it lacks and how to get
   * it. The engine action is `provider.list`.
   */
  providerList(input?: ProviderListInput, options?: CallOptions): Promise<Response>;

  /**
   * Submit a target's drafts for a person's review. Publishing itself is a person's
   * act. The engine action is `publish.request`.
   */
  publishRequest(input: PublishRequestInput, options?: CallOptions): Promise<Response>;

  /**
   * Stop a running or waiting run. Says what was completed before it stopped. The
   * engine action is `run.cancel`.
   */
  runCancel(input: RunCancelInput, options?: CallOptions): Promise<Response>;

  /**
   * The status, progress, result, and trace reference of one run. The engine action is
   * `run.get`.
   */
  runGet(input: RunGetInput, options?: CallOptions): Promise<Response>;

  /**
   * The caller's recent runs, newest first: each run's state, whether a record of it is
   * in doubt, whether it waits for a person, and the next step. After a lost answer,
   * list the runs since the call was sent to find its run before calling again. The
   * engine action is `run.list`.
   */
  runList(input?: RunListInput, options?: CallOptions): Promise<Response>;

  /**
   * Continue a run that rests or stopped. The engine reconciles every doubtful record
   * first; an approved write goes on from its checkpoint. The engine action is
   * `run.resume`.
   */
  runResume(input: RunResumeInput, options?: CallOptions): Promise<Response>;

  /**
   * A summary of one run's trace, with references to its kept artifacts. The engine
   * action is `run.trace`.
   */
  runTrace(input: RunTraceInput, options?: CallOptions): Promise<Response>;

  /**
   * Build the reverse diff of a completed write run, for a person's approval. The
   * engine action is `run.undo`.
   */
  runUndo(input: RunUndoInput, options?: CallOptions): Promise<Response>;

  /**
   * Stop giving a runner new jobs. The engine action is `runner.drain`.
   */
  runnerDrain(input: RunnerDrainInput, options?: CallOptions): Promise<Response>;

  /**
   * Enroll a runner on another machine. The engine action is `runner.enroll`.
   */
  runnerEnroll(input: RunnerEnrollInput, options?: CallOptions): Promise<Response>;

  /**
   * The runners, what each serves, and when each was last heard from. The engine action
   * is `runner.list`.
   */
  runnerList(input?: RunnerListInput, options?: CallOptions): Promise<Response>;

  /**
   * Revoke a runner's enrollment. The engine action is `runner.revoke`.
   */
  runnerRevoke(input: RunnerRevokeInput, options?: CallOptions): Promise<Response>;

  /**
   * The last scout's report on a target: found, drafted, proven, open, and what to look
   * at first. The engine action is `scout.report`.
   */
  scoutReport(input: ScoutReportInput, options?: CallOptions): Promise<Response>;

  /**
   * Identify a target, run the discoverers that suit it, draft what follows by rule,
   * and try the drafted reads. Resumable; never writes to the system. The engine action
   * is `scout.run`.
   */
  scoutRun(input: ScoutRunInput, options?: CallOptions): Promise<Response>;

  /**
   * A compact snapshot of what a surface shows now, with stable references to its
   * elements. The engine action is `screen.snapshot`.
   */
  screenSnapshot(input: ScreenSnapshotInput, options?: CallOptions): Promise<Response>;

  /**
   * Store a secret under a name, taken from a prompt or standard input. Nothing reads
   * it back. The engine action is `secret.set`.
   */
  secretSet(input: SecretSetInput, options?: CallOptions): Promise<Response>;

  /**
   * Update a standalone install to the newest release of its channel. The engine action
   * is `self.update`.
   */
  selfUpdate(input?: SelfUpdateInput, options?: CallOptions): Promise<Response>;

  /**
   * End someone's session. It cannot be used to enter one. The engine action is
   * `session.end`.
   */
  sessionEnd(input: SessionEndInput, options?: CallOptions): Promise<Response>;

  /**
   * The sessions of a workspace, without their contents. The engine action is
   * `session.list`.
   */
  sessionList(input?: SessionListInput, options?: CallOptions): Promise<Response>;

  /**
   * Ask the caller's person to sign in to one surface of a target. A credential is
   * never an input: where one is not stored, the person types it on the engine's own
   * page, and carries the sign-in out there. Returns that a session exists, for whom,
   * where, until when, and at which level; never its contents. The engine action is
   * `session.login`.
   */
  sessionLogin(input: SessionLoginInput, options?: CallOptions): Promise<Response>;

  /**
   * End one of the caller's own sessions. The engine action is `session.revoke`.
   */
  sessionRevoke(input: SessionRevokeInput, options?: CallOptions): Promise<Response>;

  /**
   * Whether each target is signed in for the caller. Shows states and times, never a
   * credential. The engine action is `session.status`.
   */
  sessionStatus(input?: SessionStatusInput, options?: CallOptions): Promise<Response>;

  /**
   * How often a shadow binding matched the verified one, and where it differed. The
   * engine action is `shadow.report`.
   */
  shadowReport(input: ShadowReportInput, options?: CallOptions): Promise<Response>;

  /**
   * A new scout of a target, handed a description the system publishes about itself as
   * a file of the project's `sources` folder: exactly `scout.run` with `files` naming
   * it `description`. Drafts by rule; never writes to the system. The engine action is
   * `spec.import`.
   */
  specImport(input: SpecImportInput, options?: CallOptions): Promise<Response>;

  /**
   * Register a target in the project: makes its folder and a first target file, with
   * one surface where an address and its kind are given. The engine action is
   * `target.add`.
   */
  targetAdd(input: TargetAddInput, options?: CallOptions): Promise<Response>;

  /**
   * The targets in the project and the versions published from them. The engine action
   * is `target.list`.
   */
  targetList(input?: TargetListInput, options?: CallOptions): Promise<Response>;

  /**
   * The platform, framework, support tier, and surfaces at an address or in a running
   * application. Read only. The engine action is `target.probe`.
   */
  targetProbe(input: TargetProbeInput, options?: CallOptions): Promise<Response>;

  /**
   * The target's build state and the next action to take. The engine action is
   * `target.status`.
   */
  targetStatus(input: TargetStatusInput, options?: CallOptions): Promise<Response>;

  /**
   * Run the publish gates on a target's drafts and return the evidence. The engine
   * action is `test.run`.
   */
  testRun(input: TestRunInput, options?: CallOptions): Promise<Response>;

  /**
   * Call one published operation or pipeline by name with its input. Returns the result
   * of a read, or the run's resting state (such as needs_approval) with the next step.
   * A write never proceeds without a person's approval. The engine action is
   * `tool.call`.
   */
  toolCall(input: ToolCallInput, options?: CallOptions): Promise<Response>;

  /**
   * The full description, input schema, and output schema of one published tool, by its
   * name. The engine action is `tool.describe`.
   */
  toolDescribe(input: ToolDescribeInput, options?: CallOptions): Promise<Response>;

  /**
   * Find operations and pipelines of the published targets by words or module. Returns
   * names, one-line descriptions, and effects; call tool.describe for a schema. The
   * engine action is `tool.search`.
   */
  toolSearch(input?: ToolSearchInput, options?: CallOptions): Promise<Response>;

  /**
   * Create a toolset: which tools, which version, which identity mode. The engine
   * action is `toolset.create`.
   */
  toolsetCreate(input: ToolsetCreateInput, options?: CallOptions): Promise<Response>;

  /**
   * The toolsets of the workspace. The engine action is `toolset.list`.
   */
  toolsetList(input?: ToolsetListInput, options?: CallOptions): Promise<Response>;

  /**
   * Change a toolset. The engine action is `toolset.update`.
   */
  toolsetUpdate(input: ToolsetUpdateInput, options?: CallOptions): Promise<Response>;

  /**
   * Export one run's evidence to a file in the project. The engine action is
   * `trace.export`.
   */
  traceExport(input: TraceExportInput, options?: CallOptions): Promise<Response>;

  /**
   * View one run's evidence: its trace lines and artifact references. The engine action
   * is `trace.show`.
   */
  traceShow(input: TraceShowInput, options?: CallOptions): Promise<Response>;

  /**
   * The open questions about a target. The engine action is `unknown.list`.
   */
  unknownList(input: UnknownListInput, options?: CallOptions): Promise<Response>;

  /**
   * Settle an open question about a target, with evidence. The engine action is
   * `unknown.settle`.
   */
  unknownSettle(input: UnknownSettleInput, options?: CallOptions): Promise<Response>;

  /**
   * Rotate one layer of the vault's keys. The engine action is `vault.rotate`.
   */
  vaultRotate(input: VaultRotateInput, options?: CallOptions): Promise<Response>;

  /**
   * Move a toolset to a version, through the candidate stage. The engine action is
   * `version.activate`.
   */
  versionActivate(input: VersionActivateInput, options?: CallOptions): Promise<Response>;

  /**
   * Pack a published version into a signed file. The engine action is `version.bundle`.
   */
  versionBundle(input: VersionBundleInput, options?: CallOptions): Promise<Response>;

  /**
   * Compare two versions of a target and classify each change. The engine action is
   * `version.diff`.
   */
  versionDiff(input: VersionDiffInput, options?: CallOptions): Promise<Response>;

  /**
   * Import a signed bundle as a published version. The engine action is
   * `version.import`.
   */
  versionImport(input: VersionImportInput, options?: CallOptions): Promise<Response>;

  /**
   * Recompile stored sources for a newer engine. The engine action is
   * `version.migrate`.
   */
  versionMigrate(input: VersionMigrateInput, options?: CallOptions): Promise<Response>;

  /**
   * Publish a target's drafts as an immutable version, compiled to a plan. A person's
   * act, never an agent's. The engine action is `version.publish`.
   */
  versionPublish(input: VersionPublishInput, options?: CallOptions): Promise<Response>;

  /**
   * Make an earlier version of a target current. The engine action is
   * `version.rollback`.
   */
  versionRollback(input: VersionRollbackInput, options?: CallOptions): Promise<Response>;

  /**
   * The changes a watch recorded after a number: added, changed, unseen, gaps, pauses,
   * and its end, each with the runs of its checks. Waits up to `wait_seconds` for the
   * first. Holds no value; read a check's run with run.get for that. The engine action
   * is `watch.changes`.
   */
  watchChanges(input: WatchChangesInput, options?: CallOptions): Promise<Response>;

  /**
   * Confirm a watch the caller's person was asked for, so that its checks begin. A
   * person's act, carried out on the engine's page, never an agent's. The engine action
   * is `watch.confirm`.
   */
  watchConfirm(input: WatchConfirmInput, options?: CallOptions): Promise<Response>;

  /**
   * Ask for a watch: one published read, called with one fixed input at a fixed
   * interval until an end, that records what was added, changed, or went out of view.
   * Nothing runs until the caller's person confirms it on the engine's page. The engine
   * action is `watch.create`.
   */
  watchCreate(input: WatchCreateInput, options?: CallOptions): Promise<Response>;

  /**
   * End a watch, by its person or by the caller that asked for it. Its checks and
   * changes are kept. The engine action is `watch.end`.
   */
  watchEnd(input: WatchEndInput, options?: CallOptions): Promise<Response>;

  /**
   * One watch of the caller's person: its read, interval, end, state, and how many
   * checks it made. Never a value the read returned. The engine action is `watch.get`.
   */
  watchGet(input: WatchGetInput, options?: CallOptions): Promise<Response>;

  /**
   * The watches of the caller's person, oldest first. The engine action is
   * `watch.list`.
   */
  watchList(input?: WatchListInput, options?: CallOptions): Promise<Response>;

  /**
   * Pause a confirmed watch of the caller's person: no check runs until it is resumed.
   * The engine action is `watch.pause`.
   */
  watchPause(input: WatchPauseInput, options?: CallOptions): Promise<Response>;

  /**
   * Let a paused watch of the caller's person go on. One paused because its read
   * changed or drifted never goes on. The engine action is `watch.resume`.
   */
  watchResume(input: WatchResumeInput, options?: CallOptions): Promise<Response>;
}
