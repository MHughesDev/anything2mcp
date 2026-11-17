"""Typed methods, one per action of the engine. GENERATED: do not edit.

Written by ``tools/generate_actions.py`` from ``tools/openapi.json``, the
OpenAPI document the engine serves at ``/api/v1/openapi.json`` (engine
version 0.0.1). A test fails when this file differs from what the
generator writes from that document. To change a method, change the action
in the engine, save the new document, and run the generator.

Each method names its action and passes its input. It returns the engine's
:class:`Response` as :meth:`Client.call` does, and branches on nothing.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any, Dict, List, Literal, Mapping, Optional, Union

if TYPE_CHECKING:
    from ._client import Response

__all__ = ["Actions", "ACTIONS"]

#: Every action this module has a method for, in the order of the document.
ACTIONS = (
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
)


class Actions:
    """The typed methods. :class:`Client` inherits them."""

    def call(
        self,
        action: str,
        input: Optional[Mapping[str, Any]] = None,
        *,
        idempotency_key: Optional[str] = None,
    ) -> "Response":  # pragma: no cover - the client supplies it
        raise NotImplementedError

    def _action(
        self, action: str, fields: Dict[str, Any], idempotency_key: Optional[str]
    ) -> "Response":
        # An input left out is left out of the request; nothing else is decided here.
        given = {key: value for key, value in fields.items() if value is not None}
        return self.call(action, given, idempotency_key=idempotency_key)

    def action_describe(
        self,
        *,
        action: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """One action in full: its summary, input schema, output schema, examples,
        effect, and role.

        The engine action is ``action.describe``.
        """
        return self._action("action.describe", {"action": action}, idempotency_key)

    def action_run(
        self,
        *,
        action: str,
        input: Optional[Dict[str, Any]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Run any action by name with its input. The action's own role, schema,
        and policy decision apply.

        The engine action is ``action.run``.
        """
        return self._action(
            "action.run",
            {
                "action": action,
                "input": input,
            },
            idempotency_key,
        )

    def action_search(
        self,
        *,
        query: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Find actions by words. Returns names and one-line summaries only.

        The engine action is ``action.search``.
        """
        return self._action("action.search", {"query": query}, idempotency_key)

    def audit_query(
        self,
        *,
        limit: Optional[int] = None,
        offset: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Read the audit log, oldest first, and check its chain.

        The engine action is ``audit.query``.
        """
        return self._action(
            "audit.query",
            {
                "limit": limit,
                "offset": offset,
            },
            idempotency_key,
        )

    def auth_detect(
        self,
        *,
        target: str,
        surface: Optional[str] = None,
        wait_seconds: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The sign-in method of a surface, with evidence, and a recommended
        identity mode. Read only.

        The engine action is ``auth.detect``.
        """
        return self._action(
            "auth.detect",
            {
                "target": target,
                "surface": surface,
                "wait_seconds": wait_seconds,
            },
            idempotency_key,
        )

    def binding_promote(
        self,
        *,
        mode: str,
        target: str,
        tool: str,
        run_id: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Make the first binding of a drafted operation verified, on the evidence
        of its offline tests and a trial. For an operation that writes, a
        person's act.

        The engine action is ``binding.promote``.
        """
        return self._action(
            "binding.promote",
            {
                "mode": mode,
                "target": target,
                "tool": tool,
                "run_id": run_id,
            },
            idempotency_key,
        )

    def browser_install(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """Fetch a pinned browser for the pack that drives one.

        The engine action is ``browser.install``.
        """
        return self._action("browser.install", {}, idempotency_key)

    def capture_start(
        self,
        *,
        target: str,
        surface: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Begin recording actions and traffic on a surface, redacted as it is
        recorded.

        The engine action is ``capture.start``.
        """
        return self._action(
            "capture.start",
            {
                "target": target,
                "surface": surface,
            },
            idempotency_key,
        )

    def capture_stop(
        self,
        *,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """End the recording and store it.

        The engine action is ``capture.stop``.
        """
        return self._action("capture.stop", {"target": target}, idempotency_key)

    def client_connect(
        self,
        *,
        client: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Write the configuration an MCP client needs to reach this engine.

        The engine action is ``client.connect``.
        """
        return self._action("client.connect", {"client": client}, idempotency_key)

    def component_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The outside programs providers need: found or not, with path and digest.

        The engine action is ``component.list``.
        """
        return self._action("component.list", {}, idempotency_key)

    def config_set(
        self,
        *,
        key: str,
        value: str,
        layer: Optional[Literal["user", "project"]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Change one setting in a settings file. Loosening a protected setting
        needs a person present.

        The engine action is ``config.set``.
        """
        return self._action(
            "config.set",
            {
                "key": key,
                "value": value,
                "layer": layer,
            },
            idempotency_key,
        )

    def config_show(
        self,
        *,
        key: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The effective settings, and for each the layer that set it. With a key,
        that key's documentation.

        The engine action is ``config.show``.
        """
        return self._action("config.show", {"key": key}, idempotency_key)

    def delegation_grant(
        self,
        *,
        agent: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """A person lets an agent act for them.

        The engine action is ``delegation.grant``.
        """
        return self._action("delegation.grant", {"agent": agent}, idempotency_key)

    def delegation_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The delegations a person has given.

        The engine action is ``delegation.list``.
        """
        return self._action("delegation.list", {}, idempotency_key)

    def delegation_revoke(
        self,
        *,
        agent: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """A person withdraws a delegation.

        The engine action is ``delegation.revoke``.
        """
        return self._action("delegation.revoke", {"agent": agent}, idempotency_key)

    def discover_run(
        self,
        *,
        target: str,
        surface: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Run the discoverers that suit a target's surfaces, in the sandbox.
        Returns findings; never writes.

        The engine action is ``discover.run``.
        """
        return self._action(
            "discover.run",
            {
                "target": target,
                "surface": surface,
            },
            idempotency_key,
        )

    def docs_get(
        self,
        *,
        topic: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """A built-in reference page, by topic or action name. With no topic, the
        list of topics.

        The engine action is ``docs.get``.
        """
        return self._action("docs.get", {"topic": topic}, idempotency_key)

    def doctor_run(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """Check the settings, the store, the key store, the providers, and the
        clock, and give a fix for each problem. Makes no outside connection.

        The engine action is ``doctor.run``.
        """
        return self._action("doctor.run", {}, idempotency_key)

    def draft_delete(
        self,
        *,
        file: str,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Remove one draft file of a target from the project; a published version
        keeps its own copy.

        The engine action is ``draft.delete``.
        """
        return self._action(
            "draft.delete",
            {
                "file": file,
                "target": target,
            },
            idempotency_key,
        )

    def draft_lint(
        self,
        *,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Lint and compile a target's draft files and report every problem with
        its file and line. Changes nothing.

        The engine action is ``draft.lint``.
        """
        return self._action("draft.lint", {"target": target}, idempotency_key)

    def draft_read(
        self,
        *,
        file: str,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Read one draft file of a target; stands in for a file system.

        The engine action is ``draft.read``.
        """
        return self._action(
            "draft.read",
            {
                "file": file,
                "target": target,
            },
            idempotency_key,
        )

    def draft_try(
        self,
        *,
        target: str,
        tool: str,
        input: Optional[Dict[str, Any]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Run one draft binding in the sandbox, where writes go only to declared
        test records.

        The engine action is ``draft.try``.
        """
        return self._action(
            "draft.try",
            {
                "target": target,
                "tool": tool,
                "input": input,
            },
            idempotency_key,
        )

    def draft_write(
        self,
        *,
        content: str,
        file: str,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Write one draft file of a target after validating and linting it.

        The engine action is ``draft.write``.
        """
        return self._action(
            "draft.write",
            {
                "content": content,
                "file": file,
                "target": target,
            },
            idempotency_key,
        )

    def endpoints_infer(
        self,
        *,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Endpoint templates, schemas, and dependencies worked out from stored
        recordings.

        The engine action is ``endpoints.infer``.
        """
        return self._action("endpoints.infer", {"target": target}, idempotency_key)

    def engine_logs(
        self,
        *,
        lines: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The local engine's recent log lines.

        The engine action is ``engine.logs``.
        """
        return self._action("engine.logs", {"lines": lines}, idempotency_key)

    def engine_start(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """Start the local engine.

        The engine action is ``engine.start``.
        """
        return self._action("engine.start", {}, idempotency_key)

    def engine_status(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """Whether the local engine runs, its health state, and the cause of
        anything but `ok`.

        The engine action is ``engine.status``.
        """
        return self._action("engine.status", {}, idempotency_key)

    def engine_stop(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """Stop the local engine after saving its sessions.

        The engine action is ``engine.stop``.
        """
        return self._action("engine.stop", {}, idempotency_key)

    def finding_describe(
        self,
        *,
        finding: str,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """One finding in full, with its evidence references.

        The engine action is ``finding.describe``.
        """
        return self._action(
            "finding.describe",
            {
                "finding": finding,
                "target": target,
            },
            idempotency_key,
        )

    def finding_list(
        self,
        *,
        target: str,
        kind: Optional[str] = None,
        limit: Optional[int] = None,
        offset: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Findings for a target, by level, kind, or source.

        The engine action is ``finding.list``.
        """
        return self._action(
            "finding.list",
            {
                "target": target,
                "kind": kind,
                "limit": limit,
                "offset": offset,
            },
            idempotency_key,
        )

    def fixture_record(
        self,
        *,
        target: str,
        tool: str,
        input: Optional[Dict[str, Any]] = None,
        name: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Record an offline test's fixture from one real read, every value
        scrubbed by rule, and the test that replays it.

        The engine action is ``fixture.record``.
        """
        return self._action(
            "fixture.record",
            {
                "target": target,
                "tool": tool,
                "input": input,
                "name": name,
            },
            idempotency_key,
        )

    def grant_add(
        self,
        *,
        max_effect: Literal["read", "write", "irreversible"],
        scope: str,
        who: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Grant a principal or a group the use of tools up to an effect class.

        The engine action is ``grant.add``.
        """
        return self._action(
            "grant.add",
            {
                "max_effect": max_effect,
                "scope": scope,
                "who": who,
            },
            idempotency_key,
        )

    def grant_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The grants of the workspace.

        The engine action is ``grant.list``.
        """
        return self._action("grant.list", {}, idempotency_key)

    def grant_remove(
        self,
        *,
        scope: str,
        who: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Remove a grant.

        The engine action is ``grant.remove``.
        """
        return self._action(
            "grant.remove",
            {
                "scope": scope,
                "who": who,
            },
            idempotency_key,
        )

    def killswitch_set(
        self,
        *,
        on: bool,
        scope: Literal["everything", "workspace", "target"],
        target: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Stop, or release, a target, a workspace, or the whole system. Releasing
        needs a person present.

        The engine action is ``killswitch.set``.
        """
        return self._action(
            "killswitch.set",
            {
                "on": on,
                "scope": scope,
                "target": target,
            },
            idempotency_key,
        )

    def plan_show(
        self,
        *,
        target: str,
        version: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Print a compiled plan as text: its tools, steps, surfaces, and
        permissions.

        The engine action is ``plan.show``.
        """
        return self._action(
            "plan.show",
            {
                "target": target,
                "version": version,
            },
            idempotency_key,
        )

    def plugin_install(
        self,
        *,
        file: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Install a plug-in from a file, after checking its manifest and
        signature.

        The engine action is ``plugin.install``.
        """
        return self._action("plugin.install", {"file": file}, idempotency_key)

    def plugin_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The plug-ins found on this machine.

        The engine action is ``plugin.list``.
        """
        return self._action("plugin.list", {}, idempotency_key)

    def policy_explain(
        self,
        *,
        decision: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Why a past call was allowed or denied: the decision, the policy version,
        and the rules that made it.

        The engine action is ``policy.explain``.
        """
        return self._action("policy.explain", {"decision": decision}, idempotency_key)

    def policy_publish(
        self,
        *,
        file: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Publish a policy set after its own tests pass.

        The engine action is ``policy.publish``.
        """
        return self._action("policy.publish", {"file": file}, idempotency_key)

    def policy_test(
        self,
        *,
        file: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Run a policy set's decision table and report each failure.

        The engine action is ``policy.test``.
        """
        return self._action("policy.test", {"file": file}, idempotency_key)

    def provider_describe(
        self,
        *,
        provider: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """One provider's manifest: what it supplies, its effects, its
        requirements, and its settings.

        The engine action is ``provider.describe``.
        """
        return self._action(
            "provider.describe",
            {
                "provider": provider,
            },
            idempotency_key,
        )

    def provider_disable(
        self,
        *,
        provider: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Switch a provider off.

        The engine action is ``provider.disable``.
        """
        return self._action("provider.disable", {"provider": provider}, idempotency_key)

    def provider_enable(
        self,
        *,
        provider: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Switch a provider on. A protected setting on one machine where the
        provider starts processes or reaches a network.

        The engine action is ``provider.enable``.
        """
        return self._action("provider.enable", {"provider": provider}, idempotency_key)

    def provider_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """Every pack and provider: installed, enabled, ready, or what it lacks and
        how to get it.

        The engine action is ``provider.list``.
        """
        return self._action("provider.list", {}, idempotency_key)

    def publish_request(
        self,
        *,
        target: str,
        version: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Submit a target's drafts for a person's review. Publishing itself is a
        person's act.

        The engine action is ``publish.request``.
        """
        return self._action(
            "publish.request",
            {
                "target": target,
                "version": version,
            },
            idempotency_key,
        )

    def run_cancel(
        self,
        *,
        run_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Stop a running or waiting run. Says what was completed before it
        stopped.

        The engine action is ``run.cancel``.
        """
        return self._action("run.cancel", {"run_id": run_id}, idempotency_key)

    def run_get(
        self,
        *,
        run_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The status, progress, result, and trace reference of one run.

        The engine action is ``run.get``.
        """
        return self._action("run.get", {"run_id": run_id}, idempotency_key)

    def run_list(
        self,
        *,
        after: Optional[str] = None,
        limit: Optional[int] = None,
        since: Optional[int] = None,
        state: Optional[List[str]] = None,
        target: Optional[str] = None,
        tool: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The caller's recent runs, newest first: each run's state, whether a
        record of it is in doubt, whether it waits for a person, and the next
        step. After a lost answer, list the runs since the call was sent to find
        its run before calling again.

        The engine action is ``run.list``.
        """
        return self._action(
            "run.list",
            {
                "after": after,
                "limit": limit,
                "since": since,
                "state": state,
                "target": target,
                "tool": tool,
            },
            idempotency_key,
        )

    def run_resume(
        self,
        *,
        run_id: str,
        wait_seconds: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Continue a run that rests or stopped. The engine reconciles every
        doubtful record first; an approved write goes on from its checkpoint.

        The engine action is ``run.resume``.
        """
        return self._action(
            "run.resume",
            {
                "run_id": run_id,
                "wait_seconds": wait_seconds,
            },
            idempotency_key,
        )

    def run_trace(
        self,
        *,
        run_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """A summary of one run's trace, with references to its kept artifacts.

        The engine action is ``run.trace``.
        """
        return self._action("run.trace", {"run_id": run_id}, idempotency_key)

    def run_undo(
        self,
        *,
        run_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Build the reverse diff of a completed write run, for a person's
        approval.

        The engine action is ``run.undo``.
        """
        return self._action("run.undo", {"run_id": run_id}, idempotency_key)

    def runner_drain(
        self,
        *,
        runner: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Stop giving a runner new jobs.

        The engine action is ``runner.drain``.
        """
        return self._action("runner.drain", {"runner": runner}, idempotency_key)

    def runner_enroll(
        self,
        *,
        name: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Enroll a runner on another machine.

        The engine action is ``runner.enroll``.
        """
        return self._action("runner.enroll", {"name": name}, idempotency_key)

    def runner_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The runners, what each serves, and when each was last heard from.

        The engine action is ``runner.list``.
        """
        return self._action("runner.list", {}, idempotency_key)

    def runner_revoke(
        self,
        *,
        runner: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Revoke a runner's enrollment.

        The engine action is ``runner.revoke``.
        """
        return self._action("runner.revoke", {"runner": runner}, idempotency_key)

    def scout_report(
        self,
        *,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The last scout's report on a target: found, drafted, proven, open, and
        what to look at first.

        The engine action is ``scout.report``.
        """
        return self._action("scout.report", {"target": target}, idempotency_key)

    def scout_run(
        self,
        *,
        target: str,
        fresh: Optional[bool] = None,
        wait_seconds: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Identify a target, run the discoverers that suit it, draft what follows
        by rule, and try the drafted reads. Resumable; never writes to the
        system.

        The engine action is ``scout.run``.
        """
        return self._action(
            "scout.run",
            {
                "target": target,
                "fresh": fresh,
                "wait_seconds": wait_seconds,
            },
            idempotency_key,
        )

    def screen_snapshot(
        self,
        *,
        target: str,
        limit: Optional[int] = None,
        offset: Optional[int] = None,
        surface: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """A compact snapshot of what a surface shows now, with stable references
        to its elements.

        The engine action is ``screen.snapshot``.
        """
        return self._action(
            "screen.snapshot",
            {
                "target": target,
                "limit": limit,
                "offset": offset,
                "surface": surface,
            },
            idempotency_key,
        )

    def secret_set(
        self,
        *,
        name: str,
        stdin: Optional[bool] = None,
        value: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Store a secret under a name, taken from a prompt or standard input.
        Nothing reads it back.

        The engine action is ``secret.set``.
        """
        return self._action(
            "secret.set",
            {
                "name": name,
                "stdin": stdin,
                "value": value,
            },
            idempotency_key,
        )

    def self_update(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """Update a standalone install to the newest release of its channel.

        The engine action is ``self.update``.
        """
        return self._action("self.update", {}, idempotency_key)

    def session_end(
        self,
        *,
        session_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """End someone's session. It cannot be used to enter one.

        The engine action is ``session.end``.
        """
        return self._action("session.end", {"session_id": session_id}, idempotency_key)

    def session_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The sessions of a workspace, without their contents.

        The engine action is ``session.list``.
        """
        return self._action("session.list", {}, idempotency_key)

    def session_login(
        self,
        *,
        target: str,
        environment: Optional[str] = None,
        surface: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Ask the caller's person to sign in to one surface of a target. Returns a
        link for the person; the agent never sees a credential.

        The engine action is ``session.login``.
        """
        return self._action(
            "session.login",
            {
                "target": target,
                "environment": environment,
                "surface": surface,
            },
            idempotency_key,
        )

    def session_revoke(
        self,
        *,
        session_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """End one of the caller's own sessions.

        The engine action is ``session.revoke``.
        """
        return self._action(
            "session.revoke",
            {
                "session_id": session_id,
            },
            idempotency_key,
        )

    def session_status(
        self,
        *,
        target: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Whether each target is signed in for the caller. Shows states and times,
        never a credential.

        The engine action is ``session.status``.
        """
        return self._action("session.status", {"target": target}, idempotency_key)

    def shadow_report(
        self,
        *,
        target: str,
        tool: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """How often a shadow binding matched the verified one, and where it
        differed.

        The engine action is ``shadow.report``.
        """
        return self._action(
            "shadow.report",
            {
                "target": target,
                "tool": tool,
            },
            idempotency_key,
        )

    def spec_import(
        self,
        *,
        file: str,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Operations drafted from a description the system publishes about itself,
        given as a file in the project.

        The engine action is ``spec.import``.
        """
        return self._action(
            "spec.import",
            {
                "file": file,
                "target": target,
            },
            idempotency_key,
        )

    def target_add(
        self,
        *,
        name: str,
        address: Optional[str] = None,
        kind: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Register a target in the project: makes its folder and a first target
        file, with one surface where an address and its kind are given.

        The engine action is ``target.add``.
        """
        return self._action(
            "target.add",
            {
                "name": name,
                "address": address,
                "kind": kind,
            },
            idempotency_key,
        )

    def target_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The targets in the project and the versions published from them.

        The engine action is ``target.list``.
        """
        return self._action("target.list", {}, idempotency_key)

    def target_probe(
        self,
        *,
        target: str,
        surface: Optional[str] = None,
        wait_seconds: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The platform, framework, support tier, and surfaces at an address or in
        a running application. Read only.

        The engine action is ``target.probe``.
        """
        return self._action(
            "target.probe",
            {
                "target": target,
                "surface": surface,
                "wait_seconds": wait_seconds,
            },
            idempotency_key,
        )

    def target_status(
        self,
        *,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The target's build state and the next action to take.

        The engine action is ``target.status``.
        """
        return self._action("target.status", {"target": target}, idempotency_key)

    def test_run(
        self,
        *,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Run the publish gates on a target's drafts and return the evidence.

        The engine action is ``test.run``.
        """
        return self._action("test.run", {"target": target}, idempotency_key)

    def tool_call(
        self,
        *,
        tool: str,
        environment: Optional[str] = None,
        input: Optional[Dict[str, Any]] = None,
        source_file: Optional[str] = None,
        source_rows: Optional[List[Dict[str, Any]]] = None,
        wait_seconds: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Call one published operation or pipeline by name with its input. Returns
        the result of a read, or the run's resting state (such as
        needs_approval) with the next step. A write never proceeds without a
        person's approval.

        The engine action is ``tool.call``.
        """
        return self._action(
            "tool.call",
            {
                "tool": tool,
                "environment": environment,
                "input": input,
                "source_file": source_file,
                "source_rows": source_rows,
                "wait_seconds": wait_seconds,
            },
            idempotency_key,
        )

    def tool_describe(
        self,
        *,
        tool: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The full description, input schema, and output schema of one published
        tool, by its name.

        The engine action is ``tool.describe``.
        """
        return self._action("tool.describe", {"tool": tool}, idempotency_key)

    def tool_search(
        self,
        *,
        full: Optional[bool] = None,
        kind: Optional[Literal["operation", "pipeline"]] = None,
        limit: Optional[int] = None,
        module: Optional[str] = None,
        offset: Optional[int] = None,
        query: Optional[str] = None,
        target: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Find operations and pipelines of the published targets by words or
        module. Returns names, one-line descriptions, and effects; call
        tool.describe for a schema.

        The engine action is ``tool.search``.
        """
        return self._action(
            "tool.search",
            {
                "full": full,
                "kind": kind,
                "limit": limit,
                "module": module,
                "offset": offset,
                "query": query,
                "target": target,
            },
            idempotency_key,
        )

    def toolset_create(
        self,
        *,
        name: str,
        tools: List[str],
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Create a toolset: which tools, which version, which identity mode.

        The engine action is ``toolset.create``.
        """
        return self._action(
            "toolset.create",
            {
                "name": name,
                "tools": tools,
            },
            idempotency_key,
        )

    def toolset_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The toolsets of the workspace.

        The engine action is ``toolset.list``.
        """
        return self._action("toolset.list", {}, idempotency_key)

    def toolset_update(
        self,
        *,
        name: str,
        tools: Optional[List[str]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Change a toolset.

        The engine action is ``toolset.update``.
        """
        return self._action(
            "toolset.update",
            {
                "name": name,
                "tools": tools,
            },
            idempotency_key,
        )

    def trace_export(
        self,
        *,
        run_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Export one run's evidence to a file in the project.

        The engine action is ``trace.export``.
        """
        return self._action("trace.export", {"run_id": run_id}, idempotency_key)

    def trace_show(
        self,
        *,
        run_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """View one run's evidence: its trace lines and artifact references.

        The engine action is ``trace.show``.
        """
        return self._action("trace.show", {"run_id": run_id}, idempotency_key)

    def unknown_list(
        self,
        *,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The open questions about a target.

        The engine action is ``unknown.list``.
        """
        return self._action("unknown.list", {"target": target}, idempotency_key)

    def unknown_settle(
        self,
        *,
        evidence: str,
        unknown: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Settle an open question about a target, with evidence.

        The engine action is ``unknown.settle``.
        """
        return self._action(
            "unknown.settle",
            {
                "evidence": evidence,
                "unknown": unknown,
            },
            idempotency_key,
        )

    def vault_rotate(
        self,
        *,
        layer: Literal["root", "workspace", "owner"],
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Rotate one layer of the vault's keys.

        The engine action is ``vault.rotate``.
        """
        return self._action("vault.rotate", {"layer": layer}, idempotency_key)

    def version_activate(
        self,
        *,
        target: str,
        version: str,
        toolset: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Move a toolset to a version, through the candidate stage.

        The engine action is ``version.activate``.
        """
        return self._action(
            "version.activate",
            {
                "target": target,
                "version": version,
                "toolset": toolset,
            },
            idempotency_key,
        )

    def version_bundle(
        self,
        *,
        target: str,
        version: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Pack a published version into a signed file.

        The engine action is ``version.bundle``.
        """
        return self._action(
            "version.bundle",
            {
                "target": target,
                "version": version,
            },
            idempotency_key,
        )

    def version_diff(
        self,
        *,
        from_: str,
        target: str,
        to: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Compare two versions of a target and classify each change.

        The engine action is ``version.diff``.
        """
        return self._action(
            "version.diff",
            {
                "from": from_,
                "target": target,
                "to": to,
            },
            idempotency_key,
        )

    def version_import(
        self,
        *,
        file: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Import a signed bundle as a published version.

        The engine action is ``version.import``.
        """
        return self._action("version.import", {"file": file}, idempotency_key)

    def version_migrate(
        self,
        *,
        target: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Recompile stored sources for a newer engine.

        The engine action is ``version.migrate``.
        """
        return self._action("version.migrate", {"target": target}, idempotency_key)

    def version_publish(
        self,
        *,
        target: str,
        plan_hash: Optional[str] = None,
        version: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Publish a target's drafts as an immutable version, compiled to a plan. A
        person's act, never an agent's.

        The engine action is ``version.publish``.
        """
        return self._action(
            "version.publish",
            {
                "target": target,
                "plan_hash": plan_hash,
                "version": version,
            },
            idempotency_key,
        )

    def version_rollback(
        self,
        *,
        target: str,
        version: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Make an earlier version of a target current.

        The engine action is ``version.rollback``.
        """
        return self._action(
            "version.rollback",
            {
                "target": target,
                "version": version,
            },
            idempotency_key,
        )
