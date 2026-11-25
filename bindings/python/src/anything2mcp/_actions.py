"""Typed methods, one per action of the engine. GENERATED: do not edit.

Written by ``tools/generate_actions.py`` from ``tools/openapi.json``, the
OpenAPI document the engine serves at ``/api/v1/openapi.json`` (engine
version 0.1.0). A test fails when this file differs from what the
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

    def assertion_confirm(
        self,
        *,
        target: str,
        assertions: Optional[List[str]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Confirm the reads a target's drafts assert, as the person, so that a try
        of the drafts in an environment of kind `test` may let a page send them.
        A person's act, carried out on the engine's page, never an agent's.

        The engine action is ``assertion.confirm``.
        """
        return self._action(
            "assertion.confirm",
            {
                "target": target,
                "assertions": assertions,
            },
            idempotency_key,
        )

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
        identity kind. Read only.

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
        position: Optional[int] = None,
        run_id: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Make the first binding of a drafted operation verified, on the evidence
        of its offline tests and a trial, or a binding in shadow verified, on
        its run of exact matches. For an operation that writes, and for a
        binding in shadow, a person's act.

        The engine action is ``binding.promote``.
        """
        return self._action(
            "binding.promote",
            {
                "mode": mode,
                "target": target,
                "tool": tool,
                "position": position,
                "run_id": run_id,
            },
            idempotency_key,
        )

    def capture_start(
        self,
        *,
        target: str,
        capture: Optional[str] = None,
        surface: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Ask for a capture of your person's own use of a surface. Nothing records
        until the person begins it on the engine's page; then only the structure
        of what they use is kept, never a value.

        The engine action is ``capture.start``.
        """
        return self._action(
            "capture.start",
            {
                "target": target,
                "capture": capture,
                "surface": surface,
            },
            idempotency_key,
        )

    def capture_stop(
        self,
        *,
        target: str,
        capture: Optional[str] = None,
        wait_seconds: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """End a capture you asked for, or read how it ended; store what it found
        as findings, and scout from them as `scout.run` does from a discovery:
        draft by rule, try the drafted reads, and report. Answers with the
        report's reference; never writes to the system.

        The engine action is ``capture.stop``.
        """
        return self._action(
            "capture.stop",
            {
                "target": target,
                "capture": capture,
                "wait_seconds": wait_seconds,
            },
            idempotency_key,
        )

    def client_connect(
        self,
        *,
        client: Optional[str] = None,
        name: Optional[str] = None,
        print: Optional[bool] = None,
        toolset: Optional[Literal["use", "build"]] = None,
        yes: Optional[bool] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Give the configuration an MCP client needs to start the stdio proxy for
        this project: printed for the `generic` client profile, or set in the
        file a client profile names, shown first and written by a local person.

        The engine action is ``client.connect``.
        """
        return self._action(
            "client.connect",
            {
                "client": client,
                "name": name,
                "print": print,
                "toolset": toolset,
                "yes": yes,
            },
            idempotency_key,
        )

    def component_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The outside programs providers need: found or not, with path and digest,
        known or unverified. Starts nothing.

        The engine action is ``component.list``.
        """
        return self._action("component.list", {}, idempotency_key)

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

    def discover_run(
        self,
        *,
        target: str,
        files: Optional[Dict[str, Any]] = None,
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
                "files": files,
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
        target: str,
        file: Optional[str] = None,
        files: Optional[List[str]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Remove draft files of a target from the project, one by `file` or
        several by `files`, all or none; a published version keeps its own copy.

        The engine action is ``draft.delete``.
        """
        return self._action(
            "draft.delete",
            {
                "target": target,
                "file": file,
                "files": files,
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

    def draft_ready(
        self,
        *,
        target: str,
        tools: Optional[List[str]] = None,
        version: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Make every drafted read a trial in this project read a value through
        ready to publish: record its offline test and fixture, promote its first
        binding, run the publish gates, and ask a person to publish, each step
        by its own action. A write, a pipeline, and a read with no trial are
        left alone, each with its reason.

        The engine action is ``draft.ready``.
        """
        return self._action(
            "draft.ready",
            {
                "target": target,
                "tools": tools,
                "version": version,
            },
            idempotency_key,
        )

    def draft_try(
        self,
        *,
        target: str,
        tool: str,
        input: Optional[Dict[str, Any]] = None,
        mode: Optional[str] = None,
        position: Optional[int] = None,
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
                "mode": mode,
                "position": position,
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
        mode: Optional[str] = None,
        name: Optional[str] = None,
        position: Optional[int] = None,
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
                "mode": mode,
                "name": name,
                "position": position,
            },
            idempotency_key,
        )

    def grant_add(
        self,
        *,
        max_effect: Literal["read", "write", "irreversible"],
        scope: str,
        who: str,
        drafted_by: Optional[str] = None,
        identity_mode: Optional[Literal["per_user_token", "per_user_session", "per_user_device", "service_account"]] = None,
        reviewer: Optional[str] = None,
        tests: Optional[List[Dict[str, Any]]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Grant a principal or a group the use of tools up to an effect class. A
        person's act, published as a new version of the policy set.

        The engine action is ``grant.add``.
        """
        return self._action(
            "grant.add",
            {
                "max_effect": max_effect,
                "scope": scope,
                "who": who,
                "drafted_by": drafted_by,
                "identity_mode": identity_mode,
                "reviewer": reviewer,
                "tests": tests,
            },
            idempotency_key,
        )

    def grant_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The grants, the grants of roles, and who holds which role, in the set in
        force.

        The engine action is ``grant.list``.
        """
        return self._action("grant.list", {}, idempotency_key)

    def grant_remove(
        self,
        *,
        scope: str,
        who: str,
        drafted_by: Optional[str] = None,
        max_effect: Optional[Literal["read", "write", "irreversible"]] = None,
        reviewer: Optional[str] = None,
        tests: Optional[List[Dict[str, Any]]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Remove a grant. A person's act, published as a new version of the policy
        set.

        The engine action is ``grant.remove``.
        """
        return self._action(
            "grant.remove",
            {
                "scope": scope,
                "who": who,
                "drafted_by": drafted_by,
                "max_effect": max_effect,
                "reviewer": reviewer,
                "tests": tests,
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

    def net_current(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """Which facts of the network this machine is on were read, to recognize it
        again, and which were left out and why. Shows no digest and no address.
        Sends nothing.

        The engine action is ``net.current``.
        """
        return self._action("net.current", {}, idempotency_key)

    def net_interfaces(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """This machine's network interfaces and their addresses, read from the
        operating system. Hardware addresses are shown as their length only.
        Sends nothing.

        The engine action is ``net.interfaces``.
        """
        return self._action("net.interfaces", {}, idempotency_key)

    def net_listeners(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The sockets listening on this machine, each with the kind of place its
        address is, and the program that holds it where this user may see it.
        Sends nothing.

        The engine action is ``net.listeners``.
        """
        return self._action("net.listeners", {}, idempotency_key)

    def net_neighbours(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The neighbour table this machine already holds, never refreshed by
        asking. Hardware addresses are shown as their length only. Sends
        nothing.

        The engine action is ``net.neighbours``.
        """
        return self._action("net.neighbours", {}, idempotency_key)

    def net_proxy(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The proxy setting this engine reads, with any credentials removed: on
        Linux this engine's own environment, not the caller's; on Windows the
        user's system setting. Read to be shown, never applied. Sends nothing.

        The engine action is ``net.proxy``.
        """
        return self._action("net.proxy", {}, idempotency_key)

    def net_resolvers(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The name servers and search domains this machine uses, as the system
        states them. Sends nothing.

        The engine action is ``net.resolvers``.
        """
        return self._action("net.resolvers", {}, idempotency_key)

    def net_routes(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """This machine's routes, and each family's default routes, lowest metric
        first. Sends nothing.

        The engine action is ``net.routes``.
        """
        return self._action("net.routes", {}, idempotency_key)

    def net_standing(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """Where this machine stands on its networks: for each interface, the kind
        of place each of its addresses is, and which interface carries each
        family's preferred default route. Sends nothing.

        The engine action is ``net.standing``.
        """
        return self._action("net.standing", {}, idempotency_key)

    def policy_explain(
        self,
        *,
        decision: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Why a past call was allowed or denied: the decision, the policy version,
        and the rules that made it. For an auditor or an administrator.

        The engine action is ``policy.explain``.
        """
        return self._action("policy.explain", {"decision": decision}, idempotency_key)

    def policy_publish(
        self,
        *,
        file: str,
        drafted_by: Optional[str] = None,
        reviewer: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Publish a policy file after its own tests pass. A person's act; where
        the set in force asks for a second administrator, the reviewer proves
        presence of their own.

        The engine action is ``policy.publish``.
        """
        return self._action(
            "policy.publish",
            {
                "file": file,
                "drafted_by": drafted_by,
                "reviewer": reviewer,
            },
            idempotency_key,
        )

    def policy_test(
        self,
        *,
        file: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Run a policy file's decision table against its own layers and report
        each failure. Publishes nothing.

        The engine action is ``policy.test``.
        """
        return self._action("policy.test", {"file": file}, idempotency_key)

    def policy_tools_allowed(
        self,
        *,
        principal: str,
        toolset: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Which published tools a principal may use, through a toolset where one
        is named. Records no decision.

        The engine action is ``policy.tools_allowed``.
        """
        return self._action(
            "policy.tools_allowed",
            {
                "principal": principal,
                "toolset": toolset,
            },
            idempotency_key,
        )

    def policy_versions(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The history of published policy versions, oldest first, and which is in
        force.

        The engine action is ``policy.versions``.
        """
        return self._action("policy.versions", {}, idempotency_key)

    def policy_who_can(
        self,
        *,
        candidates: List[str],
        tool: str,
        toolset: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Which of the principals named may use one published tool, through a
        toolset where one is named. Records no decision.

        The engine action is ``policy.who_can``.
        """
        return self._action(
            "policy.who_can",
            {
                "candidates": candidates,
                "tool": tool,
                "toolset": toolset,
            },
            idempotency_key,
        )

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
        tools: Optional[List[str]] = None,
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
                "tools": tools,
                "version": version,
            },
            idempotency_key,
        )

    def reach_list(self, *, idempotency_key: Optional[str] = None) -> "Response":
        """The reaches a person has named: each label, the names of the facts it
        states, whether it is in force on the network this machine is on now,
        and the entries the person wrote. Shows no digest.

        The engine action is ``reach.list``.
        """
        return self._action("reach.list", {}, idempotency_key)

    def reach_name(
        self,
        *,
        label: str,
        look: Optional[List[str]] = None,
        send: Optional[List[str]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """A person names the network this machine is on now, with the destinations
        that may be looked at and sent to, and the gateway names the person
        chooses. The network is read from the machine, never taken from the
        input. A person's act, never an agent's.

        The engine action is ``reach.name``.
        """
        return self._action(
            "reach.name",
            {
                "label": label,
                "look": look,
                "send": send,
            },
            idempotency_key,
        )

    def reach_withdraw(
        self,
        *,
        label: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """A person forgets one reach by its label.

        The engine action is ``reach.withdraw``.
        """
        return self._action("reach.withdraw", {"label": label}, idempotency_key)

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
        """A summary of one of the caller's own runs' trace: each step's kind,
        outcome, and time, with references to its kept captures, and never a
        value.

        The engine action is ``run.trace``.
        """
        return self._action("run.trace", {"run_id": run_id}, idempotency_key)

    def scout_report(
        self,
        *,
        target: str,
        limit: Optional[int] = None,
        offset: Optional[int] = None,
        part: Optional[Literal["surfaces", "found", "compared.differs", "compared.would_draft", "compared.not_covered", "drafted.written", "drafted.held_back", "drafted.not_drafted", "drafted.conflicts", "drafted.refused", "checked.problems", "checked.tools", "tried.tools", "tried.left_out", "open", "not_looked_at"]] = None,
        scout: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The last scout's report on a target: found, drafted, proven, open, and
        what to look at first. Each list is counted and shows its first entries;
        `part` reads one whole, in pages.

        The engine action is ``scout.report``.
        """
        return self._action(
            "scout.report",
            {
                "target": target,
                "limit": limit,
                "offset": offset,
                "part": part,
                "scout": scout,
            },
            idempotency_key,
        )

    def scout_run(
        self,
        *,
        target: str,
        files: Optional[Dict[str, Any]] = None,
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
                "files": files,
                "fresh": fresh,
                "wait_seconds": wait_seconds,
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

    def session_list(
        self,
        *,
        target: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The sessions of a workspace, without their contents.

        The engine action is ``session.list``.
        """
        return self._action("session.list", {"target": target}, idempotency_key)

    def session_login(
        self,
        *,
        target: str,
        environment: Optional[str] = None,
        surface: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Ask the caller's person to sign in to one surface of a target. A
        credential is never an input: where one is not stored, the person types
        it on the engine's own page, and carries the sign-in out there. Returns
        that a session exists, for whom, where, until when, and at which level;
        never its contents.

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
        wait_seconds: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """A new scout of a target, handed a description the system publishes about
        itself as a file of the project's `inputs` folder: exactly `scout.run`
        with `files` naming it `description`. Drafts by rule; never writes to
        the system.

        The engine action is ``spec.import``.
        """
        return self._action(
            "spec.import",
            {
                "file": file,
                "target": target,
                "wait_seconds": wait_seconds,
            },
            idempotency_key,
        )

    def target_add(
        self,
        *,
        name: str,
        address: Optional[str] = None,
        fields: Optional[Dict[str, Any]] = None,
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
                "fields": fields,
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

    def target_show(
        self,
        *,
        target: str,
        version: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Print a target's compiled target as text: its tools, steps, surfaces,
        and permissions.

        The engine action is ``target.show``.
        """
        return self._action(
            "target.show",
            {
                "target": target,
                "version": version,
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
        tools: Optional[List[str]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Run the publish gates on a target's drafts and return the evidence.

        The engine action is ``test.run``.
        """
        return self._action(
            "test.run",
            {
                "target": target,
                "tools": tools,
            },
            idempotency_key,
        )

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
        tool, by its name, and its digest, which changes when and only when its
        definition does.

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
        module. Returns names, one-line descriptions, effects, and whether each
        can be called now; call tool.describe for a schema.

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
        max_effect: Literal["read", "write", "irreversible"],
        name: str,
        tools: List[str],
        who: List[str],
        drafted_by: Optional[str] = None,
        identity_mode: Optional[Literal["per_user_token", "per_user_session", "per_user_device", "service_account"]] = None,
        reviewer: Optional[str] = None,
        tests: Optional[List[Dict[str, Any]]] = None,
        writes_without_person: Optional[bool] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Create a toolset: who may connect, which tools it selects, the highest
        effect class and the identity kind of its runs. A toolset only narrows
        what grants allow. A person's act, published as a new version of the
        policy set.

        The engine action is ``toolset.create``.
        """
        return self._action(
            "toolset.create",
            {
                "max_effect": max_effect,
                "name": name,
                "tools": tools,
                "who": who,
                "drafted_by": drafted_by,
                "identity_mode": identity_mode,
                "reviewer": reviewer,
                "tests": tests,
                "writes_without_person": writes_without_person,
            },
            idempotency_key,
        )

    def toolset_list(
        self,
        *,
        name: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The toolsets in force, each with the published tools it selects.

        The engine action is ``toolset.list``.
        """
        return self._action("toolset.list", {"name": name}, idempotency_key)

    def toolset_remove(
        self,
        *,
        name: str,
        drafted_by: Optional[str] = None,
        reviewer: Optional[str] = None,
        tests: Optional[List[Dict[str, Any]]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Remove a toolset. A person's act, published as a new version of the
        policy set.

        The engine action is ``toolset.remove``.
        """
        return self._action(
            "toolset.remove",
            {
                "name": name,
                "drafted_by": drafted_by,
                "reviewer": reviewer,
                "tests": tests,
            },
            idempotency_key,
        )

    def toolset_update(
        self,
        *,
        name: str,
        drafted_by: Optional[str] = None,
        identity_mode: Optional[Literal["per_user_token", "per_user_session", "per_user_device", "service_account"]] = None,
        max_effect: Optional[Literal["read", "write", "irreversible"]] = None,
        reviewer: Optional[str] = None,
        tests: Optional[List[Dict[str, Any]]] = None,
        tools: Optional[List[str]] = None,
        who: Optional[List[str]] = None,
        writes_without_person: Optional[bool] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Change the fields given of a toolset. A person's act, published as a new
        version of the policy set.

        The engine action is ``toolset.update``.
        """
        return self._action(
            "toolset.update",
            {
                "name": name,
                "drafted_by": drafted_by,
                "identity_mode": identity_mode,
                "max_effect": max_effect,
                "reviewer": reviewer,
                "tests": tests,
                "tools": tools,
                "who": who,
                "writes_without_person": writes_without_person,
            },
            idempotency_key,
        )

    def trace_export(
        self,
        *,
        run_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Export one run's evidence to one file in the project's working folder,
        `.anything2mcp/exports/<run>.json`: its events, its step trace, and its
        text captures, with a manifest that names each piece and what was left
        out and why.

        The engine action is ``trace.export``.
        """
        return self._action("trace.export", {"run_id": run_id}, idempotency_key)

    def trace_show(
        self,
        *,
        run_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """View one run's evidence: its trace lines and capture references.

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

    def version_publish(
        self,
        *,
        target: str,
        compiled_target_hash: Optional[str] = None,
        tools: Optional[List[str]] = None,
        version: Optional[str] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Publish a target's drafts, compiled, as an immutable version. A person's
        act, never an agent's.

        The engine action is ``version.publish``.
        """
        return self._action(
            "version.publish",
            {
                "target": target,
                "compiled_target_hash": compiled_target_hash,
                "tools": tools,
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
        """Make another published version of a target the one that runs now: an
        earlier one, or a later one again after a rollback. A person's act,
        never an agent's.

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

    def watch_changes(
        self,
        *,
        watch_id: str,
        after: Optional[int] = None,
        limit: Optional[int] = None,
        wait_seconds: Optional[int] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The changes a watch recorded after a number: added, changed, unseen,
        gaps, pauses, and its end, each with the runs of its checks. Waits up to
        `wait_seconds` for the first. Holds no value; read a check's run with
        run.get for that.

        The engine action is ``watch.changes``.
        """
        return self._action(
            "watch.changes",
            {
                "watch_id": watch_id,
                "after": after,
                "limit": limit,
                "wait_seconds": wait_seconds,
            },
            idempotency_key,
        )

    def watch_confirm(
        self,
        *,
        watch_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Confirm a watch the caller's person was asked for, so that its checks
        begin. A person's act, carried out on the engine's page, never an
        agent's.

        The engine action is ``watch.confirm``.
        """
        return self._action("watch.confirm", {"watch_id": watch_id}, idempotency_key)

    def watch_create(
        self,
        *,
        ends_at: int,
        interval_seconds: int,
        tool: str,
        condition: Optional[str] = None,
        environment: Optional[str] = None,
        ignore: Optional[List[str]] = None,
        input: Optional[Dict[str, Any]] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Ask for a watch: one published read, called with one fixed input at a
        fixed interval until an end, that records what was added, changed, or
        went out of view. Nothing runs until the caller's person confirms it on
        the engine's page.

        The engine action is ``watch.create``.
        """
        return self._action(
            "watch.create",
            {
                "ends_at": ends_at,
                "interval_seconds": interval_seconds,
                "tool": tool,
                "condition": condition,
                "environment": environment,
                "ignore": ignore,
                "input": input,
            },
            idempotency_key,
        )

    def watch_end(
        self,
        *,
        watch_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """End a watch, by its person or by the caller that asked for it. Its
        checks and changes are kept.

        The engine action is ``watch.end``.
        """
        return self._action("watch.end", {"watch_id": watch_id}, idempotency_key)

    def watch_get(
        self,
        *,
        watch_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """One watch of the caller's person: its read, interval, end, state, and
        how many checks it made. Never a value the read returned.

        The engine action is ``watch.get``.
        """
        return self._action("watch.get", {"watch_id": watch_id}, idempotency_key)

    def watch_list(
        self,
        *,
        open_only: Optional[bool] = None,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """The watches of the caller's person, oldest first.

        The engine action is ``watch.list``.
        """
        return self._action("watch.list", {"open_only": open_only}, idempotency_key)

    def watch_pause(
        self,
        *,
        watch_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Pause a confirmed watch of the caller's person: no check runs until it
        is resumed.

        The engine action is ``watch.pause``.
        """
        return self._action("watch.pause", {"watch_id": watch_id}, idempotency_key)

    def watch_resume(
        self,
        *,
        watch_id: str,
        idempotency_key: Optional[str] = None,
    ) -> "Response":
        """Let a paused watch of the caller's person go on. One paused because its
        read changed or drifted never goes on.

        The engine action is ``watch.resume``.
        """
        return self._action("watch.resume", {"watch_id": watch_id}, idempotency_key)
