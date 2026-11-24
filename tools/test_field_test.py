#!/usr/bin/env python3
"""Prove the parts of tools/field_test.py that need no container engine.

The closed network, the containers, and the check that no public address can be reached are
built as lists of arguments and decided by small functions; these tests hold what those
lists and functions say. Nothing here starts a container. Run from the repository root:

    python3 tools/test_field_test.py

Standard library only; Python 3.11 or newer.
"""
import asyncio
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import field_proxy  # noqa: E402
import field_test  # noqa: E402

SYSTEM = {"image": "example/system@sha256:" + "0" * 64, "port": 3000, "volume": "/data"}


def flag_value(argv, flag):
    return argv[argv.index(flag) + 1]


class Names(unittest.TestCase):
    def test_every_run_has_its_own_network_and_containers(self):
        first, second = field_test.Run("example"), field_test.Run("example")
        self.assertNotEqual(first.network, second.network)
        for name in (first.system, first.box, first.proxy):
            self.assertTrue(name.startswith(first.network))
            self.assertNotIn(name, (second.system, second.box, second.proxy))
        self.assertTrue(re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9_.-]*", first.system))

    def test_a_recipe_name_cannot_break_a_container_name(self):
        run = field_test.Run("a b/c:d")
        self.assertTrue(re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9_.-]*", run.network))


class Containers(unittest.TestCase):
    def setUp(self):
        self.run = field_test.Run("example", token="abc")

    def test_the_network_is_internal_and_labelled(self):
        argv = self.run.network_argv()
        self.assertIn("--internal", argv)
        self.assertEqual(argv[-1], self.run.network)
        self.assertEqual(flag_value(argv, "--label"), self.run.label)

    def test_the_system_publishes_no_port_and_sits_on_the_network(self):
        argv = self.run.system_argv(SYSTEM, "/runs/data", {"K": "v"})
        self.assertNotIn("-p", argv)
        self.assertNotIn("--publish", argv)
        self.assertNotIn("host", argv)
        self.assertEqual(flag_value(argv, "--network"), self.run.network)
        self.assertIn("/runs/data:/data", argv)
        self.assertIn("K=v", argv)
        self.assertEqual(argv[-1], SYSTEM["image"])

    def test_a_system_s_mounts_are_files_of_the_recipe_read_only_and_its_command_follows_the_image(self):
        with tempfile.TemporaryDirectory() as recipe:
            Path(recipe, "files").mkdir()
            Path(recipe, "files", "server").write_text("x")
            system = {**SYSTEM, "mounts": {"/opt/server": "files/server"}, "command": ["/opt/server", "--serve"]}
            argv = self.run.system_argv(system, "/runs/data", {}, recipe)
            self.assertIn(f"{Path(recipe, 'files', 'server').resolve()}:/opt/server:ro", argv)
            self.assertEqual(argv[-3:], [SYSTEM["image"], "/opt/server", "--serve"])
            self.assertNotIn("-p", argv)

    def test_a_mount_outside_the_recipe_or_to_a_relative_path_is_refused(self):
        with tempfile.TemporaryDirectory() as recipe:
            Path(recipe, "inside").write_text("x")
            for mounts in ({"/opt/x": "../outside"}, {"/opt/x": "/etc/passwd"}, {"opt/x": "inside"},
                           {"/opt/x": "missing"}, {"/opt/x:rw": "inside"}):
                with self.assertRaises(field_test.CannotRun, msg=mounts):
                    field_test.file_mounts(mounts, recipe)

    def test_the_box_is_given_the_recipe_s_box_mounts_read_only(self):
        with tempfile.TemporaryDirectory() as recipe:
            Path(recipe, "tool").write_text("x")
            self.run.box_mounts = field_test.file_mounts({"/opt/tool/tool": "tool"}, recipe)
            argv = self.run.box_argv("/b/anything2mcp", "/h", "/p", "0:0")
            self.assertIn(f"{Path(recipe, 'tool').resolve()}:/opt/tool/tool:ro", argv)
            self.assertEqual(argv[-3:], [field_test.BOX_IMAGE, "sleep", "infinity"])

    def test_a_system_in_the_box_s_network_joins_the_box_and_publishes_nothing(self):
        argv = self.run.system_argv({**SYSTEM, "network": "box"}, "/runs/data", {})
        self.assertEqual(flag_value(argv, "--network"), f"container:{self.run.box}")
        self.assertNotIn("-p", argv)
        self.assertNotIn("--publish", argv)
        self.assertNotIn("host", argv)

    def test_a_ready_program_is_run_inside_the_system_until_it_exits_0(self):
        seen = []
        answers = iter([(1, "", ""), (0, "", "")])

        def fake(argv, **_):
            seen.append(argv)
            return next(answers)

        with mock.patch.object(field_test, "run", fake), mock.patch.object(field_test.time, "sleep"):
            field_test.wait_ready("http://x", {"exec": ["ready-check"]}, container="sys")
        self.assertEqual(seen, [["docker", "exec", "sys", "ready-check"]] * 2)

    def test_a_folder_recipe_refuses_steps_that_need_a_system(self):
        names = {"project": "/p", "recipe": "/r", "data": "/d"}
        for step in ({"exec": ["true"]}, {"http": {"path": "/"}}):
            with self.assertRaises(field_test.CannotRun):
                field_test.do_step(step, names, {}, self.run)

    def test_the_binary_is_mounted_read_only_and_home_and_project_are_mounted(self):
        argv = self.run.box_argv("/b/anything2mcp", "/h", "/p/project", "1000:1000")
        self.assertIn(f"/b/anything2mcp:{field_test.BOX_BINARY}:ro", argv)
        self.assertIn("/h:/h", argv)
        self.assertIn("/p/project:/p/project", argv)
        self.assertEqual(flag_value(argv, "--network"), self.run.network)
        self.assertNotIn("-p", argv)

    def test_the_images_the_runner_adds_are_pinned_by_digest(self):
        for image in (field_test.BOX_IMAGE, field_test.PROXY_IMAGE):
            self.assertRegex(image, r"@sha256:[0-9a-f]{64}$")
        self.assertIn(field_test.BOX_IMAGE, self.run.box_argv("/b", "/h", "/p", "0:0"))
        self.assertIn(field_test.PROXY_IMAGE, self.run.proxy_argv(3000))

    def test_the_proxy_is_on_the_network_and_forwards_to_the_system_by_name(self):
        argv = self.run.proxy_argv(3000)
        self.assertEqual(flag_value(argv, "--network"), self.run.network)
        self.assertEqual(flag_value(argv, "--upstream"), f"{self.run.system}:3000")
        self.assertNotIn("-p", argv)
        # The control port is inside the container only.
        self.assertTrue(flag_value(argv, "--control").startswith("127.0.0.1:"))

    def test_a_command_of_the_product_runs_in_the_box_with_the_project_as_its_folder(self):
        argv = self.run.exec_argv({"A2M_HOME": "/h"}, "/p/project", ["init"])
        self.assertEqual(argv[:4], ["docker", "exec", "-w", "/p/project"])
        self.assertIn("A2M_HOME=/h", argv)
        self.assertIn(self.run.box, argv)
        self.assertEqual(argv[-2:], [field_test.BOX_BINARY, "init"][-2:])
        self.assertLess(argv.index(self.run.box), argv.index(field_test.BOX_BINARY))

    def test_only_the_product_s_own_variables_reach_the_box(self):
        env = {"A2M_HOME": "/h", "A2M_BINARY": "/x", "PATH": "/bin", "HTTPS_PROXY": "p", "A2M_VAULT_PASSPHRASE": "s"}
        self.assertEqual(field_test.product_env(env), {"A2M_HOME": "/h", "A2M_VAULT_PASSPHRASE": "s"})


CLOSED = (0, "A2M_VERDICT CLOSED\n", "")
OPEN = (0, "A2M_VERDICT OPEN\n", "")


class PublicAddress(unittest.TestCase):
    def test_only_an_explicit_verdict_from_a_probe_that_ran_counts(self):
        self.assertEqual(field_test.verdict(*CLOSED[:2]), "CLOSED")
        self.assertEqual(field_test.verdict(*OPEN[:2]), "OPEN")
        self.assertEqual(field_test.verdict(0, "noise\nA2M_VERDICT CLOSED\n"), "CLOSED")

    def test_no_verdict_is_unproven_never_closed(self):
        for code, out in (
            (0, ""),  # nothing printed
            (127, ""),  # no shell in the image
            (126, "OCI runtime exec failed"),
            (1, "Error response: container is not running"),
            (1, "A2M_VERDICT CLOSED\n"),  # a verdict from a probe that failed
            (0, "A2M_VERDICT UNKNOWN\n"),
            (0, "A2M_VERDICT CLOSED but\n"),
            (0, "CLOSED\n"),
        ):
            self.assertEqual(field_test.verdict(code, out), "UNPROVEN", (code, out))

    def test_the_probes_cover_ipv4_ipv6_and_a_name(self):
        scripts = [script for _, script in field_test.probes()]
        self.assertTrue(any("/dev/tcp/1.1.1.1/443" in one for one in scripts))
        self.assertTrue(any("/dev/tcp/2606:4700:4700::1111/443" in one for one in scripts))
        self.assertTrue(any("getent hosts " + field_test.PUBLIC_NAME in one for one in scripts))
        self.assertEqual(len(scripts), len(field_test.PUBLIC_ADDRESSES) + 1)

    def test_the_run_is_stopped_by_name_when_an_address_answers(self):
        asked = []

        def execute(argv):
            asked.append(argv)
            return OPEN

        with self.assertRaises(field_test.CannotRun) as stop:
            field_test.check_closed("box", execute)
        self.assertIn("closed_network_open", str(stop.exception))
        self.assertEqual(len(asked), 1)
        self.assertIn("box", asked[0])

    def test_a_resolved_name_means_open(self):
        def execute(argv):
            return OPEN if "getent" in argv[-1] else CLOSED

        with self.assertRaises(field_test.CannotRun) as stop:
            field_test.check_closed("box", execute)
        self.assertIn("closed_network_open", str(stop.exception))
        self.assertIn(field_test.PUBLIC_NAME, str(stop.exception))

    def test_an_open_ipv6_address_means_open(self):
        def execute(argv):
            return OPEN if "2606:" in argv[-1] else CLOSED

        with self.assertRaises(field_test.CannotRun) as stop:
            field_test.check_closed("box", execute)
        self.assertIn("closed_network_open", str(stop.exception))

    def test_a_probe_that_did_not_run_stops_the_run_as_unproven(self):
        for failure in ((127, "", "sh: bash: not found"), (1, "", "container is not running"), (0, "", "")):
            with self.assertRaises(field_test.CannotRun) as stop:
                field_test.check_closed("box", lambda argv, failure=failure: failure)
            self.assertIn("closed_network_unproven", str(stop.exception))

    def test_one_unproven_probe_among_closed_ones_stops_the_run(self):
        def execute(argv):
            return (0, "", "") if "2606:" in argv[-1] else CLOSED

        with self.assertRaises(field_test.CannotRun) as stop:
            field_test.check_closed("box", execute)
        self.assertIn("closed_network_unproven", str(stop.exception))

    def test_every_probe_is_tried_when_all_show_closed(self):
        asked = []

        def execute(argv):
            asked.append(argv[-1])
            return CLOSED

        field_test.check_closed("box", execute)
        self.assertEqual(len(asked), len(field_test.probes()))

    def test_the_check_comes_before_the_first_command_of_the_product(self):
        run = field_test.Run("example", token="abc")
        calls = []

        def execute(argv):
            calls.append(argv)
            return (0, "", "") if argv[1] == "run" else CLOSED

        run.ensure_box("/b", "/h", "/p", "0:0", execute)
        run.ensure_box("/b", "/h", "/p", "0:0", execute)  # once only
        self.assertEqual(calls[0][1], "run")
        self.assertTrue(all(call[1] == "exec" for call in calls[1:]))
        self.assertEqual(len(calls), 1 + len(field_test.probes()))

    def test_a_box_on_an_open_network_runs_no_command(self):
        run = field_test.Run("example", token="abc")
        with self.assertRaises(field_test.CannotRun):
            run.ensure_box("/b", "/h", "/p", "0:0", lambda argv: (0, "", "") if argv[1] == "run" else OPEN)

    def test_a_box_whose_probe_did_not_run_runs_no_command(self):
        run = field_test.Run("example", token="abc")
        with self.assertRaises(field_test.CannotRun) as stop:
            run.ensure_box("/b", "/h", "/p", "0:0", lambda argv: (0, "", "") if argv[1] == "run" else (127, "", ""))
        self.assertIn("closed_network_unproven", str(stop.exception))


class ProbeScripts(unittest.TestCase):
    """The probe scripts as a real shell reads them, with `timeout` replaced by a stub that
    ends the way a try at an address ends. No container engine, no network."""

    def verdict_of(self, script, rc, message):
        with tempfile.TemporaryDirectory() as folder:
            stub = Path(folder) / "timeout"
            stub.write_text(f"#!/bin/sh\necho '{message}' >&2\nexit {rc}\n")
            stub.chmod(0o755)
            done = subprocess.run(
                ["sh", "-c", script], env={"PATH": f"{folder}:/usr/bin:/bin"},
                capture_output=True, text=True, check=False,
            )
        return field_test.verdict(done.returncode, done.stdout)

    def test_a_tcp_probe_reads_each_ending(self):
        script = field_test.tcp_probe_script("1.1.1.1", 443)
        for rc, message, expected in (
            (0, "", "OPEN"),
            (1, "connect: Connection refused", "OPEN"),
            (1, "Connection reset by peer", "OPEN"),
            (124, "", "CLOSED"),
            (1, "connect: Network is unreachable", "CLOSED"),
            (1, "connect: No route to host", "CLOSED"),
            (1, "socket: Address family not supported by protocol", "CLOSED"),
            (127, "bash: not found", "UNPROVEN"),
            (1, "something nobody expected", "UNPROVEN"),
            (1, "", "UNPROVEN"),
        ):
            self.assertEqual(self.verdict_of(script, rc, message), expected, (rc, message))

    def test_a_name_probe_reads_each_ending(self):
        # `getent` is the program the probe runs; a stub named `getent` stands in for it.
        script = field_test.dns_probe_script("example.invalid")
        for rc, expected in ((0, "OPEN"), (2, "CLOSED"), (127, "UNPROVEN"), (1, "UNPROVEN")):
            with tempfile.TemporaryDirectory() as folder:
                stub = Path(folder) / "getent"
                stub.write_text(f"#!/bin/sh\nexit {rc}\n")
                stub.chmod(0o755)
                done = subprocess.run(
                    ["sh", "-c", script], env={"PATH": f"{folder}:/usr/bin:/bin"},
                    capture_output=True, text=True, check=False,
                )
            self.assertEqual(field_test.verdict(done.returncode, done.stdout), expected, rc)


class DirectInEnv(unittest.TestCase):
    def test_a_recipe_that_uses_direct_in_the_system_env_is_refused(self):
        system = {**SYSTEM, "env": {"URL": "{direct}/x"}}
        with self.assertRaises(field_test.CannotRun) as stop:
            field_test.system_env(system, {"host": "h"})
        self.assertIn("{direct}", str(stop.exception))

    def test_the_names_that_exist_before_start_are_filled(self):
        system = {**SYSTEM, "env": {"URL": "http://{host}:{port}", "N": "x"}}
        self.assertEqual(
            field_test.system_env(system, {"host": "h", "port": "3000"}),
            {"URL": "http://h:3000", "N": "x"},
        )

    def test_no_env_is_an_empty_env(self):
        self.assertEqual(field_test.system_env(SYSTEM, {}), {})

    def test_the_docstring_says_where_the_runner_works(self):
        self.assertIn("Docker Desktop", field_test.__doc__)
        self.assertIn("Linux", field_test.__doc__)


class Removal(unittest.TestCase):
    def test_every_container_of_the_run_and_the_network_are_removed(self):
        run = field_test.Run("example", token="abc")
        calls = []

        def fake(argv, **_):
            calls.append(argv)
            if argv[:3] == ["docker", "ps", "-aq"]:
                return 0, "id1\nid2\n", ""
            return 0, "", ""

        with mock.patch.object(field_test, "run", fake):
            left = run.remove()
        self.assertEqual(left, [])
        self.assertIn(f"label={run.label}", calls[0])
        self.assertEqual(calls[1][:4], ["docker", "rm", "-f", "-v"])
        self.assertEqual(calls[1][4:], ["id1", "id2"])
        self.assertEqual(calls[2], ["docker", "network", "rm", run.network])

    def test_a_network_that_will_not_go_is_reported(self):
        run = field_test.Run("example", token="abc")

        def fake(argv, **_):
            if argv[:3] == ["docker", "network", "rm"]:
                return 1, "", "in use"
            return 0, "", ""

        with mock.patch.object(field_test, "run", fake):
            self.assertEqual(run.remove(), [f"network {run.network}"])


class Placeholders(unittest.TestCase):
    def test_every_placeholder_is_filled_everywhere_in_a_value(self):
        names = {"system": "http://s:3000", "base": "http://p:8080"}
        value = {"a": ["{system}/x", {"b": "{base}"}], "c": 1}
        self.assertEqual(
            field_test.fill(value, names), {"a": ["http://s:3000/x", {"b": "http://p:8080"}], "c": 1}
        )


class ProxyControl(unittest.TestCase):
    """The control messages the runner sends into the proxy's container."""

    def test_a_rule_a_record_and_a_reset(self):
        proxy = field_proxy.FaultProxy("example", 1)
        self.assertEqual(
            field_proxy.apply_control(
                proxy, {"op": "add_rule", "method": "POST", "path": "^/x", "do": "drop_response", "times": 2}
            ),
            {"ok": True},
        )
        self.assertEqual(field_proxy.apply_control(proxy, {"op": "record"}), {"ok": True, "record": []})
        self.assertEqual(field_proxy.apply_control(proxy, {"op": "reset"}), {"ok": True})
        self.assertFalse(field_proxy.apply_control(proxy, {"op": "nope"})["ok"])

    def test_a_listening_address_other_than_this_machine_s_can_be_asked_for(self):
        async def start():
            proxy = field_proxy.FaultProxy("example", 1, listen_host="127.0.0.1")
            port = await proxy.start(0)
            await proxy.close()
            return port

        self.assertGreater(asyncio.run(start()), 0)


class HarnessCommands(unittest.TestCase):
    """How the runner builds and starts the field harness (decision 0310)."""

    def setUp(self):
        self.run = field_test.Run("example", token="abc")

    def test_the_harness_serves_in_the_box_with_its_input_held_and_no_passphrase_on_it(self):
        argv = self.run.harness_argv({"A2M_HOME": "/h", "A2M_VAULT_PASSPHRASE": "s"}, "/p/project")
        self.assertEqual(argv[:3], ["docker", "exec", "-i"])
        self.assertEqual(argv[-2:], [field_test.BOX_HARNESS, "serve"])
        self.assertIn(self.run.box, argv)
        self.assertIn("A2M_VAULT_PASSPHRASE=s", argv)
        self.assertNotIn("--passphrase-from-stdin", " ".join(argv))
        self.assertNotIn(field_test.BOX_BINARY, argv)

    def test_the_harness_is_mounted_read_only_beside_the_product(self):
        argv = self.run.box_argv("/b/anything2mcp", "/h", "/p", "0:0", harness="/build/a2m-field-harness")
        self.assertIn(f"/build/a2m-field-harness:{field_test.BOX_HARNESS}:ro", argv)
        self.assertIn(f"/b/anything2mcp:{field_test.BOX_BINARY}:ro", argv)
        self.assertNotIn(field_test.BOX_HARNESS, " ".join(self.run.box_argv("/b", "/h", "/p", "0:0")))

    def test_the_harness_is_built_in_a_folder_of_its_own(self):
        argv, env, binary = field_test.harness_build("/scratch/build-1")
        self.assertEqual(argv[:4], ["cargo", "build", "-p", field_test.HARNESS_PACKAGE])
        self.assertIn("--locked", argv)
        self.assertEqual(env["CARGO_TARGET_DIR"], "/scratch/build-1/target")
        self.assertTrue(binary.startswith("/scratch/build-1/"))
        self.assertNotIn(field_test.REPO, env["CARGO_TARGET_DIR"])

    def test_a_build_that_fails_leaves_no_folder_and_stops_the_run(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(field_test.CannotRun):
                field_test.build_harness(root, lambda argv, env: (101, "", "error"))
            self.assertEqual(list(Path(root).iterdir()), [])

    def test_a_feature_selects_its_package_beside_the_harness(self):
        argv, _, _ = field_test.harness_build("/scratch/b", ["example-pack/server"])
        self.assertEqual(argv[:4], ["cargo", "build", "-p", field_test.HARNESS_PACKAGE])
        self.assertIn("example-pack", argv)
        self.assertEqual(flag_value(argv, "--features"), "example-pack/server")
        self.assertIn("--locked", argv)
        self.assertNotIn("--features", field_test.harness_build("/scratch/b")[0])

    def test_a_feature_that_is_not_package_and_feature_is_refused_and_leaves_no_folder(self):
        for bad in ("server", "a/b; rm", "--all-features", "a/b/c"):
            with tempfile.TemporaryDirectory() as root:
                with self.assertRaises(field_test.CannotRun):
                    field_test.build_harness(root, lambda argv, env: (0, "", ""), features=[bad])
                self.assertEqual(list(Path(root).iterdir()), [])

    def test_the_page_is_asked_inside_the_box_on_its_loopback_address(self):
        argv = self.run.page_argv(4321)
        self.assertEqual(argv[:4], ["docker", "exec", "-i", self.run.box])
        self.assertIn("/dev/tcp/127.0.0.1/4321", argv[-1])

    def test_a_press_carries_the_page_s_own_host_and_origin(self):
        raw = field_test.page_request("POST", "4321", "/approve/ab/approve", "once=cd").decode()
        self.assertIn("Host: 127.0.0.1:4321\r\n", raw)
        self.assertIn("Origin: http://127.0.0.1:4321\r\n", raw)
        self.assertIn("Content-Length: 7\r\n", raw)
        self.assertTrue(raw.endswith("\r\n\r\nonce=cd"))
        self.assertNotIn("Origin", field_test.page_request("GET", "4321", "/approve/ab").decode())


HASH = "0123456789abcdef" * 4
SHORT = HASH[: field_test.SHORT_HASH]
PAGE = (
    "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\n\r\n<h1>A write waits for your approval</h1>"
    f"<dd><code>{HASH}</code></dd>"
    '<form method="post" action="/approve/c0de/approve">\n<input type="hidden" name="once" value="0e0e">\n</form>'
)


class FakeHarness:
    def __init__(self, prompt):
        self.prompts = __import__("queue").Queue()
        self.prompt = prompt
        self.sent = []
        self.process = object()

    def alive(self):
        return True

    def stray(self):
        return []

    def answer(self, line):
        self.sent.append(line)


def page(prompt, answer_page, harness):
    """An `execute` that serves the page, and on the press raises the prompt and answers
    with `answer_page` once the harness was answered."""
    asked = []

    def execute(argv, data):
        asked.append(data.decode())
        if data.startswith(b"GET"):
            return subprocess.CompletedProcess(argv, 0, PAGE.encode(), b"")
        if prompt is not None:
            harness.prompts.put(prompt)
            for _ in range(200):
                if harness.sent:
                    break
                __import__("time").sleep(0.01)
        return subprocess.CompletedProcess(argv, 0, answer_page.encode(), b"")

    return execute, asked


DONE = "HTTP/1.1 200 OK\r\n\r\n<h1>Approved</h1><p>Recorded.</p>"
NOT_PASSED = "HTTP/1.1 403 Forbidden\r\n\r\n<h1>Nothing was approved</h1><p>The prompt on this machine was answered and not passed.</p>"


class PersonStep(unittest.TestCase):
    LINK = "http://127.0.0.1:4321/approve/c0de"

    def press(self, spec, prompt, answer_page):
        harness = FakeHarness(prompt)
        execute, asked = page(prompt, answer_page, harness)
        shown = field_test.press(spec, field_test.Run("example", token="abc"), harness, execute, wait=2)
        return shown, harness, asked

    def test_only_the_short_hash_the_recipe_means_is_sent(self):
        shown, harness, asked = self.press(
            {"link": self.LINK, "approve": HASH}, f"Approve a write by `t` | Change {SHORT}", DONE
        )
        self.assertTrue(shown["ok"], shown)
        self.assertEqual(harness.sent, [SHORT])
        self.assertIn("POST /approve/c0de/approve HTTP/1.1", asked[1])
        self.assertTrue(asked[1].endswith("once=0e0e"))

    def test_another_hash_is_sent_when_the_recipe_asks_and_nothing_passes(self):
        shown, harness, _ = self.press(
            {"link": self.LINK, "approve": HASH, "send": "other"}, f"Approve | Change {SHORT}", NOT_PASSED
        )
        self.assertFalse(shown["ok"])
        self.assertNotEqual(harness.sent, [SHORT])
        self.assertEqual(len(harness.sent[0]), field_test.SHORT_HASH)
        self.assertIn("not passed", shown["output"])

    def test_a_prompt_for_another_change_is_not_a_pass(self):
        other = "f" * field_test.SHORT_HASH
        shown, harness, _ = self.press({"link": self.LINK, "approve": HASH}, f"Approve | Change {other}", NOT_PASSED)
        self.assertFalse(shown["ok"])
        self.assertEqual(harness.sent, [SHORT])

    def test_a_press_that_raises_no_prompt_is_not_a_pass(self):
        shown, harness, _ = self.press({"link": self.LINK, "approve": HASH}, None, NOT_PASSED)
        self.assertFalse(shown["ok"])
        self.assertEqual(harness.sent, [])

    def test_a_link_that_is_not_the_engine_s_page_is_refused(self):
        for link in ("http://example.invalid/approve/c0de", "http://127.0.0.1:1/other/c0de", ""):
            with self.assertRaises(field_test.CannotRun):
                field_test.press({"link": link, "approve": HASH}, field_test.Run("e"), FakeHarness(None))

    def test_a_sign_in_types_each_credential_into_its_field_of_the_page(self):
        shown, harness, asked = self.press(
            {"link": self.LINK, "approve": HASH, "signs_in": {"token": "an invented/token&1"}},
            f"Carry out | Act {SHORT}",
            DONE,
        )
        self.assertTrue(shown["ok"], shown)
        self.assertTrue(asked[1].endswith("once=0e0e&secret.token=an%20invented%2Ftoken%261"), asked[1])
        self.assertNotIn("invented", shown["output"])

    def test_a_press_needs_a_hash_to_send(self):
        with self.assertRaises(field_test.CannotRun):
            field_test.press({"link": self.LINK, "approve": "{change}"}, field_test.Run("e"), FakeHarness(None))


# A stand-in for the harness, run as a real child process: it says it serves on standard
# error, prints one prompt on standard output, and reports the line it read.
CHILD = (
    "import sys; sys.stderr.write('anything2mcp: the local engine is ready.\\n'); sys.stderr.flush();"
    "print('Approve | Change abc', flush=True); line = sys.stdin.readline();"
    "sys.stderr.write('answered ' + line); sys.stderr.flush()"
)


class HarnessProcess(unittest.TestCase):
    def test_a_prompt_is_read_from_its_output_and_answered_on_its_input(self):
        with tempfile.TemporaryDirectory() as folder:
            harness = field_test.Harness([sys.executable, "-c", CHILD], str(Path(folder) / "harness.log"))
            harness.start(wait=10)
            self.assertEqual(harness.prompts.get(timeout=10), "Approve | Change abc")
            harness.answer("abc")
            harness.stop()
            self.assertIn("answered abc", harness.said())
            self.assertFalse(harness.alive())

    def test_a_harness_that_stops_before_it_serves_stops_the_run_by_name(self):
        with tempfile.TemporaryDirectory() as folder:
            harness = field_test.Harness(
                [sys.executable, "-c", "import sys; sys.stderr.write('this network is not closed'); sys.exit(2)"],
                str(Path(folder) / "harness.log"),
            )
            with self.assertRaises(field_test.CannotRun) as stop:
                harness.start(wait=10)
            self.assertIn("field_harness_refused", str(stop.exception))
            self.assertIn("not closed", str(stop.exception))
            harness.stop()

    def test_no_command_runs_once_the_harness_is_gone(self):
        with tempfile.TemporaryDirectory() as folder:
            harness = field_test.Harness([sys.executable, "-c", CHILD], str(Path(folder) / "harness.log"))
            harness.start(wait=10)
            harness.stop()
            with mock.patch.object(field_test, "run") as ran:
                with self.assertRaises(field_test.CannotRun) as stop:
                    field_test.do_step({"a2m": ["init"]}, {"project": "/p"}, {}, field_test.Run("e"), None, harness)
            ran.assert_not_called()
            self.assertIn("field_harness_gone", str(stop.exception))


class Secrets(unittest.TestCase):
    TOKEN = "an-invented-token-0042"

    def test_a_request_to_the_system_carries_the_headers_its_step_names(self):
        sent = []

        class Answer:
            status = 200

            def __enter__(self):
                return self

            def __exit__(self, *_):
                return False

            def read(self):
                return b"{}"

        def opened(call, timeout):
            sent.append(call)
            return Answer()

        spec = {"method": "POST", "path": "/mcp", "json": {"id": 1}, "headers": {"MCP-Protocol-Version": "2025-11-25"}}
        with mock.patch.object(field_test.NO_PROXY, "open", opened):
            self.assertEqual(field_test.request("http://system:5000", spec), (200, "{}"))
        self.assertEqual(sent[0].full_url, "http://system:5000/mcp")
        self.assertEqual(sent[0].get_header("Mcp-protocol-version"), "2025-11-25")
        self.assertEqual(sent[0].get_header("Content-type"), "application/json")

    def test_a_secret_the_system_issued_is_kept_by_name_and_shown_nowhere(self):
        names, secrets = {"direct": "http://10.0.0.2:3000"}, {}
        answer = json.dumps({"name": "trial", "sha1": self.TOKEN})
        with mock.patch.object(field_test, "request", return_value=(201, answer)):
            shown = field_test.do_step(
                {"http": {"method": "POST", "path": "/tokens"}, "secret": {"name": "token", "path": "sha1"}},
                names, {}, field_test.Run("e"), secrets=secrets,
            )
        self.assertEqual((names["token"], secrets["token"]), (self.TOKEN, self.TOKEN))
        self.assertTrue(shown["ok"])
        self.assertNotIn(self.TOKEN, json.dumps(shown))

    def test_an_answer_with_no_secret_at_the_path_stops_the_run(self):
        with mock.patch.object(field_test, "request", return_value=(201, "{}")):
            with self.assertRaises(field_test.CannotRun):
                field_test.do_step(
                    {"http": {"path": "/tokens"}, "secret": {"name": "token", "path": "sha1"}},
                    {"direct": "http://10.0.0.2:3000"}, {}, field_test.Run("e"), secrets={},
                )

    def test_what_a_step_showed_is_cleared_of_every_secret(self):
        shown = {"output": f"Bearer {self.TOKEN}", "list": [self.TOKEN], "n": 1}
        cleared = field_test.scrubbed(shown, {"token": self.TOKEN})
        self.assertEqual(cleared, {"output": "Bearer [secret token]", "list": ["[secret token]"], "n": 1})

    def test_unseen_names_every_file_that_holds_the_secret(self):
        with tempfile.TemporaryDirectory() as home, tempfile.TemporaryDirectory() as project:
            Path(home, "store").write_bytes(b"nothing here")
            names = {"home": home, "project": project, "log": ""}
            step = {"unseen": "token"}
            shown = field_test.do_step(step, names, {}, field_test.Run("e"), secrets={"token": self.TOKEN})
            self.assertTrue(shown["ok"], shown)
            Path(project, "trace").write_text(f"sent {self.TOKEN}")
            shown = field_test.do_step(step, names, {}, field_test.Run("e"), secrets={"token": self.TOKEN})
            self.assertFalse(shown["ok"])
            self.assertIn("trace", shown["output"])
            with self.assertRaises(field_test.CannotRun):
                field_test.do_step(step, names, {}, field_test.Run("e"), secrets={})


class Expectations(unittest.TestCase):
    def test_a_count_is_exact(self):
        record = "PATCH /x -> cut_response -> 201\nGET /x -> none -> 200"
        self.assertIsNone(field_test.met({"count": {"text": "PATCH /x ->", "times": 1}}, {"output": record}))
        self.assertIsNotNone(field_test.met({"count": {"text": "PATCH /x ->", "times": 2}}, {"output": record}))
        self.assertIsNotNone(field_test.met({"count": {"text": "PUT", "times": 1}}, {"output": record}))

    def test_a_kept_value_is_read_by_its_path(self):
        answer = {"result": {"run_id": "r_1", "diff": {"hash": HASH}, "list": [{"a": 1}]}}
        self.assertEqual(field_test.kept(answer, "result.run_id"), "r_1")
        self.assertEqual(field_test.kept(answer, "result.diff.hash"), HASH)
        self.assertEqual(field_test.kept(answer, "result.list.0.a"), "1")
        self.assertIsNone(field_test.kept(answer, "result.nothing"))


if __name__ == "__main__":
    unittest.main()
