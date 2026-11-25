#!/usr/bin/env python3
"""Prove the parts of tools/copy_test.py that need no container engine.

The closed network, the containers, and the check that no public address can be reached are
built as lists of arguments and decided by small functions; these tests hold what those
lists and functions say. Nothing here starts a container. Run from the repository root:

    python3 tools/test_copy_test.py

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
import copy_test_proxy  # noqa: E402
import copy_test  # noqa: E402

SYSTEM = {"image": "example/system@sha256:" + "0" * 64, "port": 3000, "volume": "/data"}


def flag_value(argv, flag):
    return argv[argv.index(flag) + 1]


class Names(unittest.TestCase):
    def test_every_run_has_its_own_network_and_containers(self):
        first, second = copy_test.Run("example"), copy_test.Run("example")
        self.assertNotEqual(first.network, second.network)
        for name in (first.system, first.box, first.proxy):
            self.assertTrue(name.startswith(first.network))
            self.assertNotIn(name, (second.system, second.box, second.proxy))
        self.assertTrue(re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9_.-]*", first.system))

    def test_a_recipe_name_cannot_break_a_container_name(self):
        run = copy_test.Run("a b/c:d")
        self.assertTrue(re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9_.-]*", run.network))


class Containers(unittest.TestCase):
    def setUp(self):
        self.run = copy_test.Run("example", token="abc")

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
                with self.assertRaises(copy_test.CannotRun, msg=mounts):
                    copy_test.file_mounts(mounts, recipe)

    def test_the_box_is_given_the_recipe_s_box_mounts_read_only(self):
        with tempfile.TemporaryDirectory() as recipe:
            Path(recipe, "tool").write_text("x")
            self.run.box_mounts = copy_test.file_mounts({"/opt/tool/tool": "tool"}, recipe)
            argv = self.run.box_argv("/b/anything2mcp", "/h", "/p", "0:0")
            self.assertIn(f"{Path(recipe, 'tool').resolve()}:/opt/tool/tool:ro", argv)
            self.assertEqual(argv[-3:], [copy_test.BOX_IMAGE, "sleep", "infinity"])

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

        with mock.patch.object(copy_test, "run", fake), mock.patch.object(copy_test.time, "sleep"):
            copy_test.wait_ready("http://x", {"exec": ["ready-check"]}, container="sys")
        self.assertEqual(seen, [["docker", "exec", "sys", "ready-check"]] * 2)

    def test_a_folder_recipe_refuses_steps_that_need_a_system(self):
        names = {"project": "/p", "recipe": "/r", "data": "/d"}
        for step in ({"exec": ["true"]}, {"http": {"path": "/"}}):
            with self.assertRaises(copy_test.CannotRun):
                copy_test.do_step(step, names, {}, self.run)

    def test_the_binary_is_mounted_read_only_and_home_and_project_are_mounted(self):
        argv = self.run.box_argv("/b/anything2mcp", "/h", "/p/project", "1000:1000")
        self.assertIn(f"/b/anything2mcp:{copy_test.BOX_BINARY}:ro", argv)
        self.assertIn("/h:/h", argv)
        self.assertIn("/p/project:/p/project", argv)
        self.assertEqual(flag_value(argv, "--network"), self.run.network)
        self.assertNotIn("-p", argv)

    def test_the_images_the_runner_adds_are_pinned_by_digest(self):
        for image in (copy_test.BOX_IMAGE, copy_test.PROXY_IMAGE):
            self.assertRegex(image, r"@sha256:[0-9a-f]{64}$")
        self.assertIn(copy_test.BOX_IMAGE, self.run.box_argv("/b", "/h", "/p", "0:0"))
        self.assertIn(copy_test.PROXY_IMAGE, self.run.proxy_argv(3000))

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
        self.assertEqual(argv[-2:], [copy_test.BOX_BINARY, "init"][-2:])
        self.assertLess(argv.index(self.run.box), argv.index(copy_test.BOX_BINARY))

    def test_only_the_product_s_own_variables_reach_the_box(self):
        env = {"A2M_HOME": "/h", "A2M_BINARY": "/x", "PATH": "/bin", "HTTPS_PROXY": "p", "A2M_VAULT_PASSPHRASE": "s"}
        self.assertEqual(copy_test.product_env(env), {"A2M_HOME": "/h", "A2M_VAULT_PASSPHRASE": "s"})


CLOSED = (0, "A2M_VERDICT CLOSED\n", "")
OPEN = (0, "A2M_VERDICT OPEN\n", "")


class PublicAddress(unittest.TestCase):
    def test_only_an_explicit_verdict_from_a_probe_that_ran_counts(self):
        self.assertEqual(copy_test.verdict(*CLOSED[:2]), "CLOSED")
        self.assertEqual(copy_test.verdict(*OPEN[:2]), "OPEN")
        self.assertEqual(copy_test.verdict(0, "noise\nA2M_VERDICT CLOSED\n"), "CLOSED")

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
            self.assertEqual(copy_test.verdict(code, out), "UNPROVEN", (code, out))

    def test_the_probes_cover_ipv4_ipv6_and_a_name(self):
        scripts = [script for _, script in copy_test.probes()]
        self.assertTrue(any("/dev/tcp/1.1.1.1/443" in one for one in scripts))
        self.assertTrue(any("/dev/tcp/2606:4700:4700::1111/443" in one for one in scripts))
        self.assertTrue(any("getent hosts " + copy_test.PUBLIC_NAME in one for one in scripts))
        self.assertEqual(len(scripts), len(copy_test.PUBLIC_ADDRESSES) + 1)

    def test_the_run_is_stopped_by_name_when_an_address_answers(self):
        asked = []

        def execute(argv):
            asked.append(argv)
            return OPEN

        with self.assertRaises(copy_test.CannotRun) as stop:
            copy_test.check_closed("box", execute)
        self.assertIn("closed_network_open", str(stop.exception))
        self.assertEqual(len(asked), 1)
        self.assertIn("box", asked[0])

    def test_a_resolved_name_means_open(self):
        def execute(argv):
            return OPEN if "getent" in argv[-1] else CLOSED

        with self.assertRaises(copy_test.CannotRun) as stop:
            copy_test.check_closed("box", execute)
        self.assertIn("closed_network_open", str(stop.exception))
        self.assertIn(copy_test.PUBLIC_NAME, str(stop.exception))

    def test_an_open_ipv6_address_means_open(self):
        def execute(argv):
            return OPEN if "2606:" in argv[-1] else CLOSED

        with self.assertRaises(copy_test.CannotRun) as stop:
            copy_test.check_closed("box", execute)
        self.assertIn("closed_network_open", str(stop.exception))

    def test_a_probe_that_did_not_run_stops_the_run_as_unproven(self):
        for failure in ((127, "", "sh: bash: not found"), (1, "", "container is not running"), (0, "", "")):
            with self.assertRaises(copy_test.CannotRun) as stop:
                copy_test.check_closed("box", lambda argv, failure=failure: failure)
            self.assertIn("closed_network_unproven", str(stop.exception))

    def test_one_unproven_probe_among_closed_ones_stops_the_run(self):
        def execute(argv):
            return (0, "", "") if "2606:" in argv[-1] else CLOSED

        with self.assertRaises(copy_test.CannotRun) as stop:
            copy_test.check_closed("box", execute)
        self.assertIn("closed_network_unproven", str(stop.exception))

    def test_every_probe_is_tried_when_all_show_closed(self):
        asked = []

        def execute(argv):
            asked.append(argv[-1])
            return CLOSED

        copy_test.check_closed("box", execute)
        self.assertEqual(len(asked), len(copy_test.probes()))

    def test_the_check_comes_before_the_first_command_of_the_product(self):
        run = copy_test.Run("example", token="abc")
        calls = []

        def execute(argv):
            calls.append(argv)
            return (0, "", "") if argv[1] == "run" else CLOSED

        run.ensure_box("/b", "/h", "/p", "0:0", execute)
        run.ensure_box("/b", "/h", "/p", "0:0", execute)  # once only
        self.assertEqual(calls[0][1], "run")
        self.assertTrue(all(call[1] == "exec" for call in calls[1:]))
        self.assertEqual(len(calls), 1 + len(copy_test.probes()))

    def test_a_box_on_an_open_network_runs_no_command(self):
        run = copy_test.Run("example", token="abc")
        with self.assertRaises(copy_test.CannotRun):
            run.ensure_box("/b", "/h", "/p", "0:0", lambda argv: (0, "", "") if argv[1] == "run" else OPEN)

    def test_a_box_whose_probe_did_not_run_runs_no_command(self):
        run = copy_test.Run("example", token="abc")
        with self.assertRaises(copy_test.CannotRun) as stop:
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
        return copy_test.verdict(done.returncode, done.stdout)

    def test_a_tcp_probe_reads_each_ending(self):
        script = copy_test.tcp_probe_script("1.1.1.1", 443)
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
        script = copy_test.dns_probe_script("example.invalid")
        for rc, expected in ((0, "OPEN"), (2, "CLOSED"), (127, "UNPROVEN"), (1, "UNPROVEN")):
            with tempfile.TemporaryDirectory() as folder:
                stub = Path(folder) / "getent"
                stub.write_text(f"#!/bin/sh\nexit {rc}\n")
                stub.chmod(0o755)
                done = subprocess.run(
                    ["sh", "-c", script], env={"PATH": f"{folder}:/usr/bin:/bin"},
                    capture_output=True, text=True, check=False,
                )
            self.assertEqual(copy_test.verdict(done.returncode, done.stdout), expected, rc)


class DirectInEnv(unittest.TestCase):
    def test_a_recipe_that_uses_direct_in_the_system_env_is_refused(self):
        system = {**SYSTEM, "env": {"URL": "{direct}/x"}}
        with self.assertRaises(copy_test.CannotRun) as stop:
            copy_test.system_env(system, {"host": "h"})
        self.assertIn("{direct}", str(stop.exception))

    def test_the_names_that_exist_before_start_are_filled(self):
        system = {**SYSTEM, "env": {"URL": "http://{host}:{port}", "N": "x"}}
        self.assertEqual(
            copy_test.system_env(system, {"host": "h", "port": "3000"}),
            {"URL": "http://h:3000", "N": "x"},
        )

    def test_no_env_is_an_empty_env(self):
        self.assertEqual(copy_test.system_env(SYSTEM, {}), {})

    def test_the_docstring_says_where_the_runner_works(self):
        self.assertIn("Docker Desktop", copy_test.__doc__)
        self.assertIn("Linux", copy_test.__doc__)


class Removal(unittest.TestCase):
    def test_every_container_of_the_run_and_the_network_are_removed(self):
        run = copy_test.Run("example", token="abc")
        calls = []

        def fake(argv, **_):
            calls.append(argv)
            if argv[:3] == ["docker", "ps", "-aq"]:
                return 0, "id1\nid2\n", ""
            return 0, "", ""

        with mock.patch.object(copy_test, "run", fake):
            left = run.remove()
        self.assertEqual(left, [])
        self.assertIn(f"label={run.label}", calls[0])
        self.assertEqual(calls[1][:4], ["docker", "rm", "-f", "-v"])
        self.assertEqual(calls[1][4:], ["id1", "id2"])
        self.assertEqual(calls[2], ["docker", "network", "rm", run.network])

    def test_a_network_that_will_not_go_is_reported(self):
        run = copy_test.Run("example", token="abc")

        def fake(argv, **_):
            if argv[:3] == ["docker", "network", "rm"]:
                return 1, "", "in use"
            return 0, "", ""

        with mock.patch.object(copy_test, "run", fake):
            self.assertEqual(run.remove(), [f"network {run.network}"])


class Placeholders(unittest.TestCase):
    def test_every_placeholder_is_filled_everywhere_in_a_value(self):
        names = {"system": "http://s:3000", "base": "http://p:8080"}
        value = {"a": ["{system}/x", {"b": "{base}"}], "c": 1}
        self.assertEqual(
            copy_test.fill(value, names), {"a": ["http://s:3000/x", {"b": "http://p:8080"}], "c": 1}
        )


class ProxyControl(unittest.TestCase):
    """The control messages the runner sends into the proxy's container."""

    def test_a_rule_a_record_and_a_reset(self):
        proxy = copy_test_proxy.FaultProxy("example", 1)
        self.assertEqual(
            copy_test_proxy.apply_control(
                proxy, {"op": "add_rule", "method": "POST", "path": "^/x", "do": "drop_response", "times": 2}
            ),
            {"ok": True},
        )
        self.assertEqual(copy_test_proxy.apply_control(proxy, {"op": "record"}), {"ok": True, "record": []})
        self.assertEqual(copy_test_proxy.apply_control(proxy, {"op": "reset"}), {"ok": True})
        self.assertFalse(copy_test_proxy.apply_control(proxy, {"op": "nope"})["ok"])

    def test_a_listening_address_other_than_this_machine_s_can_be_asked_for(self):
        async def start():
            proxy = copy_test_proxy.FaultProxy("example", 1, listen_host="127.0.0.1")
            port = await proxy.start(0)
            await proxy.close()
            return port

        self.assertGreater(asyncio.run(start()), 0)


class HarnessCommands(unittest.TestCase):
    """How the runner builds and starts the copy test harness (decision 0310)."""

    def setUp(self):
        self.run = copy_test.Run("example", token="abc")

    def test_the_harness_serves_in_the_box_with_its_input_held_and_no_passphrase_on_it(self):
        argv = self.run.harness_argv({"A2M_HOME": "/h", "A2M_VAULT_PASSPHRASE": "s"}, "/p/project")
        self.assertEqual(argv[:3], ["docker", "exec", "-i"])
        self.assertEqual(argv[-2:], [copy_test.BOX_HARNESS, "serve"])
        self.assertIn(self.run.box, argv)
        self.assertIn("A2M_VAULT_PASSPHRASE=s", argv)
        self.assertNotIn("--passphrase-from-stdin", " ".join(argv))
        self.assertNotIn(copy_test.BOX_BINARY, argv)

    def test_the_harness_is_mounted_read_only_beside_the_product(self):
        argv = self.run.box_argv("/b/anything2mcp", "/h", "/p", "0:0", harness="/build/a2m-copy-test-harness")
        self.assertIn(f"/build/a2m-copy-test-harness:{copy_test.BOX_HARNESS}:ro", argv)
        self.assertIn(f"/b/anything2mcp:{copy_test.BOX_BINARY}:ro", argv)
        self.assertNotIn(copy_test.BOX_HARNESS, " ".join(self.run.box_argv("/b", "/h", "/p", "0:0")))

    def test_the_harness_is_built_in_a_folder_of_its_own(self):
        argv, env, binary = copy_test.harness_build("/scratch/build-1")
        self.assertEqual(argv[:4], ["cargo", "build", "-p", copy_test.HARNESS_PACKAGE])
        self.assertIn("--locked", argv)
        self.assertEqual(env["CARGO_TARGET_DIR"], "/scratch/build-1/target")
        self.assertTrue(binary.startswith("/scratch/build-1/"))
        self.assertNotIn(copy_test.REPO, env["CARGO_TARGET_DIR"])

    def test_a_build_that_fails_leaves_no_folder_and_stops_the_run(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(copy_test.CannotRun):
                copy_test.build_harness(root, lambda argv, env: (101, "", "error"))
            self.assertEqual(list(Path(root).iterdir()), [])

    def test_a_feature_selects_its_package_beside_the_harness(self):
        argv, _, _ = copy_test.harness_build("/scratch/b", ["example-pack/server"])
        self.assertEqual(argv[:4], ["cargo", "build", "-p", copy_test.HARNESS_PACKAGE])
        self.assertIn("example-pack", argv)
        self.assertEqual(flag_value(argv, "--features"), "example-pack/server")
        self.assertIn("--locked", argv)
        self.assertNotIn("--features", copy_test.harness_build("/scratch/b")[0])

    def test_a_feature_that_is_not_package_and_feature_is_refused_and_leaves_no_folder(self):
        for bad in ("server", "a/b; rm", "--all-features", "a/b/c"):
            with tempfile.TemporaryDirectory() as root:
                with self.assertRaises(copy_test.CannotRun):
                    copy_test.build_harness(root, lambda argv, env: (0, "", ""), features=[bad])
                self.assertEqual(list(Path(root).iterdir()), [])

    def test_the_page_is_asked_inside_the_box_on_its_loopback_address(self):
        argv = self.run.page_argv(4321)
        self.assertEqual(argv[:4], ["docker", "exec", "-i", self.run.box])
        self.assertIn("/dev/tcp/127.0.0.1/4321", argv[-1])

    def test_a_press_carries_the_page_s_own_host_and_origin(self):
        raw = copy_test.page_request("POST", "4321", "/approve/ab/approve", "once=cd").decode()
        self.assertIn("Host: 127.0.0.1:4321\r\n", raw)
        self.assertIn("Origin: http://127.0.0.1:4321\r\n", raw)
        self.assertIn("Content-Length: 7\r\n", raw)
        self.assertTrue(raw.endswith("\r\n\r\nonce=cd"))
        self.assertNotIn("Origin", copy_test.page_request("GET", "4321", "/approve/ab").decode())


HASH = "0123456789abcdef" * 4
SHORT = HASH[: copy_test.SHORT_HASH]
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
        shown = copy_test.press(spec, copy_test.Run("example", token="abc"), harness, execute, wait=2)
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
        self.assertEqual(len(harness.sent[0]), copy_test.SHORT_HASH)
        self.assertIn("not passed", shown["output"])

    def test_a_prompt_for_another_change_is_not_a_pass(self):
        other = "f" * copy_test.SHORT_HASH
        shown, harness, _ = self.press({"link": self.LINK, "approve": HASH}, f"Approve | Change {other}", NOT_PASSED)
        self.assertFalse(shown["ok"])
        self.assertEqual(harness.sent, [SHORT])

    def test_a_press_that_raises_no_prompt_is_not_a_pass(self):
        shown, harness, _ = self.press({"link": self.LINK, "approve": HASH}, None, NOT_PASSED)
        self.assertFalse(shown["ok"])
        self.assertEqual(harness.sent, [])

    def test_a_link_that_is_not_the_engine_s_page_is_refused(self):
        for link in ("http://example.invalid/approve/c0de", "http://127.0.0.1:1/other/c0de", ""):
            with self.assertRaises(copy_test.CannotRun):
                copy_test.press({"link": link, "approve": HASH}, copy_test.Run("e"), FakeHarness(None))

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
        with self.assertRaises(copy_test.CannotRun):
            copy_test.press({"link": self.LINK, "approve": "{change}"}, copy_test.Run("e"), FakeHarness(None))


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
            harness = copy_test.Harness([sys.executable, "-c", CHILD], str(Path(folder) / "harness.log"))
            harness.start(wait=10)
            self.assertEqual(harness.prompts.get(timeout=10), "Approve | Change abc")
            harness.answer("abc")
            harness.stop()
            self.assertIn("answered abc", harness.said())
            self.assertFalse(harness.alive())

    def test_a_harness_that_stops_before_it_serves_stops_the_run_by_name(self):
        with tempfile.TemporaryDirectory() as folder:
            harness = copy_test.Harness(
                [sys.executable, "-c", "import sys; sys.stderr.write('this network is not closed'); sys.exit(2)"],
                str(Path(folder) / "harness.log"),
            )
            with self.assertRaises(copy_test.CannotRun) as stop:
                harness.start(wait=10)
            self.assertIn("copy_test_harness_refused", str(stop.exception))
            self.assertIn("not closed", str(stop.exception))
            harness.stop()

    def test_no_command_runs_once_the_harness_is_gone(self):
        with tempfile.TemporaryDirectory() as folder:
            harness = copy_test.Harness([sys.executable, "-c", CHILD], str(Path(folder) / "harness.log"))
            harness.start(wait=10)
            harness.stop()
            with mock.patch.object(copy_test, "run") as ran:
                with self.assertRaises(copy_test.CannotRun) as stop:
                    copy_test.do_step({"a2m": ["init"]}, {"project": "/p"}, {}, copy_test.Run("e"), None, harness)
            ran.assert_not_called()
            self.assertIn("copy_test_harness_gone", str(stop.exception))


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
        with mock.patch.object(copy_test.NO_PROXY, "open", opened):
            self.assertEqual(copy_test.request("http://system:5000", spec), (200, "{}"))
        self.assertEqual(sent[0].full_url, "http://system:5000/mcp")
        self.assertEqual(sent[0].get_header("Mcp-protocol-version"), "2025-11-25")
        self.assertEqual(sent[0].get_header("Content-type"), "application/json")

    def test_a_secret_the_system_issued_is_kept_by_name_and_shown_nowhere(self):
        names, secrets = {"direct": "http://10.0.0.2:3000"}, {}
        answer = json.dumps({"name": "trial", "sha1": self.TOKEN})
        with mock.patch.object(copy_test, "request", return_value=(201, answer)):
            shown = copy_test.do_step(
                {"http": {"method": "POST", "path": "/tokens"}, "secret": {"name": "token", "path": "sha1"}},
                names, {}, copy_test.Run("e"), secrets=secrets,
            )
        self.assertEqual((names["token"], secrets["token"]), (self.TOKEN, self.TOKEN))
        self.assertTrue(shown["ok"])
        self.assertNotIn(self.TOKEN, json.dumps(shown))

    def test_an_answer_with_no_secret_at_the_path_stops_the_run(self):
        with mock.patch.object(copy_test, "request", return_value=(201, "{}")):
            with self.assertRaises(copy_test.CannotRun):
                copy_test.do_step(
                    {"http": {"path": "/tokens"}, "secret": {"name": "token", "path": "sha1"}},
                    {"direct": "http://10.0.0.2:3000"}, {}, copy_test.Run("e"), secrets={},
                )

    def test_what_a_step_showed_is_cleared_of_every_secret(self):
        shown = {"output": f"Bearer {self.TOKEN}", "list": [self.TOKEN], "n": 1}
        cleared = copy_test.scrubbed(shown, {"token": self.TOKEN})
        self.assertEqual(cleared, {"output": "Bearer [secret token]", "list": ["[secret token]"], "n": 1})

    def test_unseen_names_every_file_that_holds_the_secret(self):
        with tempfile.TemporaryDirectory() as home, tempfile.TemporaryDirectory() as project:
            Path(home, "store").write_bytes(b"nothing here")
            names = {"home": home, "project": project, "log": ""}
            step = {"unseen": "token"}
            shown = copy_test.do_step(step, names, {}, copy_test.Run("e"), secrets={"token": self.TOKEN})
            self.assertTrue(shown["ok"], shown)
            Path(project, "trace").write_text(f"sent {self.TOKEN}")
            shown = copy_test.do_step(step, names, {}, copy_test.Run("e"), secrets={"token": self.TOKEN})
            self.assertFalse(shown["ok"])
            self.assertIn("trace", shown["output"])
            with self.assertRaises(copy_test.CannotRun):
                copy_test.do_step(step, names, {}, copy_test.Run("e"), secrets={})


class Expectations(unittest.TestCase):
    def test_a_count_is_exact(self):
        record = "PATCH /x -> cut_response -> 201\nGET /x -> none -> 200"
        self.assertIsNone(copy_test.met({"count": {"text": "PATCH /x ->", "times": 1}}, {"output": record}))
        self.assertIsNotNone(copy_test.met({"count": {"text": "PATCH /x ->", "times": 2}}, {"output": record}))
        self.assertIsNotNone(copy_test.met({"count": {"text": "PUT", "times": 1}}, {"output": record}))

    def test_a_kept_value_is_read_by_its_path(self):
        answer = {"result": {"run_id": "r_1", "diff": {"hash": HASH}, "list": [{"a": 1}]}}
        self.assertEqual(copy_test.kept(answer, "result.run_id"), "r_1")
        self.assertEqual(copy_test.kept(answer, "result.diff.hash"), HASH)
        self.assertEqual(copy_test.kept(answer, "result.list.0.a"), "1")
        self.assertIsNone(copy_test.kept(answer, "result.nothing"))


PINNED = "example/browser-box@sha256:" + "1" * 64


def masked_eq_allows(rule, flags):
    """Whether one argument rule of a filter lets a call with these flags through."""
    arg = rule["args"][0]
    return flags & arg["value"] == arg["valueTwo"]


class BoxOptions(unittest.TestCase):
    def test_without_options_the_box_is_as_it_was(self):
        box = copy_test.box_options(SYSTEM, "0:0")
        self.assertEqual(box, {"image": copy_test.BOX_IMAGE, "user": "0:0", "filter": None})
        argv = copy_test.Run("example", token="abc").box_argv("/b", "/h", "/p", box["user"])
        self.assertNotIn("--security-opt", argv)
        self.assertEqual(argv[-3:], [copy_test.BOX_IMAGE, "sleep", "infinity"])

    def test_a_box_image_is_named_by_its_digest_and_no_other_way(self):
        for image in ("example/browser-box:latest", "example/browser-box", "example/b@sha256:" + "1" * 63,
                      "example/b@sha256:" + "A" * 64, "example/b@sha256:" + "1" * 64 + " --privileged", 7):
            with self.assertRaises(copy_test.CannotRun, msg=image):
                copy_test.box_options({**SYSTEM, "box_image": image}, "0:0")
        self.assertEqual(copy_test.box_options({**SYSTEM, "box_image": PINNED}, "0:0")["image"], PINNED)

    def test_a_box_user_is_numbers_and_never_root(self):
        for user in ("0:0", "0:1201", "seluser", "1200", "1200:", "root:root", "1200:1201 --privileged", 1200):
            with self.assertRaises(copy_test.CannotRun, msg=user):
                copy_test.box_options({**SYSTEM, "box_user": user}, "0:0")
        self.assertEqual(copy_test.box_options({**SYSTEM, "box_user": "1200:1201"}, "0:0")["user"], "1200:1201")

    def test_only_a_runner_that_is_root_hands_the_box_to_another_user(self):
        with self.assertRaises(copy_test.CannotRun):
            copy_test.box_options({**SYSTEM, "box_user": "1200:1201"}, "1000:1000")
        self.assertEqual(copy_test.box_options({**SYSTEM, "box_user": "1000:1000"}, "1000:1000")["user"], "1000:1000")

    def test_a_filter_is_a_name_of_the_runner_s_own_list_and_never_unconfined(self):
        for name in ("unconfined", "/tmp/profile.json", "default", {"syscalls": []}, ""):
            with self.assertRaises(copy_test.CannotRun, msg=name):
                copy_test.box_options({**SYSTEM, "box_user": "1200:1201", "box_seccomp": name}, "0:0")
        self.assertEqual(list(copy_test.BOX_FILTERS), ["browser_sandbox"])

    def test_the_browser_filter_is_refused_to_a_box_whose_user_is_root(self):
        with self.assertRaises(copy_test.CannotRun):
            copy_test.box_options({**SYSTEM, "box_seccomp": "browser_sandbox"}, "0:0")
        box = copy_test.box_options({**SYSTEM, "box_user": "1200:1201", "box_seccomp": "browser_sandbox"}, "0:0")
        self.assertEqual(box["filter"], "browser_sandbox")

    def test_the_default_filter_kept_here_is_the_one_the_runner_was_written_for(self):
        import hashlib
        raw = Path(copy_test.SECCOMP_DEFAULT).read_bytes()
        self.assertEqual(hashlib.sha256(raw).hexdigest(), copy_test.SECCOMP_DEFAULT_SHA256)
        self.assertEqual(json.loads(raw)["defaultAction"], "SCMP_ACT_ERRNO")

    def test_a_changed_default_filter_is_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            changed = Path(folder, "default.json")
            profile = json.loads(Path(copy_test.SECCOMP_DEFAULT).read_text())
            profile["defaultAction"] = "SCMP_ACT_ALLOW"
            changed.write_text(json.dumps(profile))
            with self.assertRaises(copy_test.CannotRun):
                copy_test.box_filter("browser_sandbox", str(changed))

    def test_the_browser_filter_is_the_default_and_two_rules_and_nothing_else(self):
        default = json.loads(Path(copy_test.SECCOMP_DEFAULT).read_text())
        profile = copy_test.box_filter("browser_sandbox")
        self.assertEqual({k: v for k, v in profile.items() if k != "syscalls"},
                         {k: v for k, v in default.items() if k != "syscalls"})
        self.assertEqual(profile["syscalls"][: len(default["syscalls"])], default["syscalls"])
        added = profile["syscalls"][len(default["syscalls"]):]
        self.assertEqual([rule["names"] for rule in added], [["clone"], ["unshare"]])
        for rule in added:
            self.assertEqual(rule["action"], "SCMP_ACT_ALLOW")
            self.assertEqual(rule["args"][0]["op"], "SCMP_CMP_MASKED_EQ")

    def test_clone_may_make_only_the_sandbox_s_three_namespaces_and_unshare_only_a_user_one(self):
        clone, unshare = copy_test.BOX_FILTERS["browser_sandbox"]
        user, pid, net = copy_test.CLONE_NEWUSER, copy_test.CLONE_NEWPID, copy_test.CLONE_NEWNET
        others = {"mount": 0x00020000, "cgroup": 0x02000000, "uts": 0x04000000, "ipc": 0x08000000}
        sigchld = 17
        self.assertTrue(masked_eq_allows(clone, user | pid | net | sigchld))
        self.assertTrue(masked_eq_allows(clone, sigchld))
        for name, flag in others.items():
            self.assertFalse(masked_eq_allows(clone, user | pid | net | flag), name)
        self.assertTrue(masked_eq_allows(unshare, user))
        for flag in (pid, net, *others.values()):
            self.assertFalse(masked_eq_allows(unshare, user | flag), hex(flag))

    def test_the_box_carries_its_image_filter_and_user_and_never_its_entry_point(self):
        run = copy_test.Run("example", token="abc")
        run.box_image = PINNED
        run.box_security = ["--security-opt", "seccomp=/runs/box-seccomp.json"]
        argv = run.box_argv("/b", "/h", "/p", "1200:1201")
        self.assertEqual(argv[-3:], [PINNED, "sleep", "infinity"])
        self.assertEqual(flag_value(argv, "--entrypoint"), "")
        self.assertEqual(flag_value(argv, "--security-opt"), "seccomp=/runs/box-seccomp.json")
        self.assertEqual(flag_value(argv, "--user"), "1200:1201")
        for wider in ("--privileged", "--cap-add", "seccomp=unconfined", "--userns"):
            self.assertNotIn(wider, " ".join(argv))

    def test_a_recipe_that_switches_the_sandbox_off_is_found_in_its_text_and_its_files(self):
        self.assertEqual(copy_test.sandbox_off_in({"steps": [{"a2m": ["init"]}]}), [])
        for flag in copy_test.SANDBOX_OFF:
            self.assertTrue(copy_test.sandbox_off_in({"steps": [{"a2m": ["x", flag]}]}), flag)
        with tempfile.TemporaryDirectory() as recipe:
            Path(recipe, "wrapper").write_text('#!/bin/sh\nexec /opt/google/chrome/chrome --no-sandbox "$@"\n')
            flags = copy_test.file_mounts({"/opt/wrapper": "wrapper"}, recipe)
            found = copy_test.sandbox_off_in({}, copy_test.mounted_files(flags))
            self.assertEqual(len(found), 1)
            self.assertIn("--no-sandbox", found[0])

    def test_the_sandbox_check_passes_only_on_renderers_each_in_their_own_namespace(self):
        verdict = copy_test.sandboxed_verdict
        self.assertTrue(verdict(0, "A2M_RENDERERS 3 SHARING_THE_BOX_NAMESPACE 0")[0])
        self.assertFalse(verdict(0, "A2M_RENDERERS 0 SHARING_THE_BOX_NAMESPACE 0")[0])
        self.assertFalse(verdict(0, "A2M_RENDERERS 3 SHARING_THE_BOX_NAMESPACE 1")[0])
        self.assertFalse(verdict(0, "A2M_SANDBOX_OFF chrome\nA2M_RENDERERS 3 SHARING_THE_BOX_NAMESPACE 0")[0])
        self.assertFalse(verdict(0, "")[0])
        self.assertFalse(verdict(1, "A2M_RENDERERS 3 SHARING_THE_BOX_NAMESPACE 0")[0])

    @unittest.skipUnless(sys.platform.startswith("linux"), "the check reads /proc")
    def test_the_sandbox_check_s_script_gives_a_verdict(self):
        done = subprocess.run(["sh", "-c", copy_test.sandboxed_script()], capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertRegex(done.stdout, r"A2M_RENDERERS \d+ SHARING_THE_BOX_NAMESPACE \d+")
        self.assertNotIn("readlink", done.stdout, "the check found its own text")

    @unittest.skipUnless(sys.platform.startswith("linux"), "the check reads /proc")
    def test_the_sandbox_check_finds_a_flag_as_a_whole_argument_only(self):
        def shown(flag):
            process = subprocess.Popen(["sh", "-c", "sleep 30", "a2m-test-process", flag])
            try:
                done = subprocess.run(["sh", "-c", copy_test.sandboxed_script()], capture_output=True, text=True, check=False)
            finally:
                process.kill()
                process.wait()
            return [line for line in done.stdout.splitlines() if "a2m-test-process" in line]
        self.assertEqual(len(shown("--no-sandbox")), 1)
        self.assertEqual(len(shown("--no-zygote")), 1)
        self.assertEqual(shown("--no-zygote-sandbox"), [])

    def test_a_sandboxed_step_needs_the_box(self):
        with self.assertRaises(copy_test.CannotRun):
            copy_test.do_step({"sandboxed": {}}, {"project": "/p"}, {}, copy_test.Run("example", token="abc"))

    def test_the_engine_s_home_and_the_project_are_given_to_the_box_s_user(self):
        import os
        with tempfile.TemporaryDirectory() as folder:
            Path(folder, "a", "b").mkdir(parents=True)
            Path(folder, "a", "b", "file").write_text("x")
            user = f"{os.getuid()}:{os.getgid()}"
            seen = []
            with mock.patch.object(copy_test.os, "chown", lambda path, uid, gid, **_: seen.append((path, uid, gid))):
                copy_test.give_to([folder], "1200:1201")
            self.assertEqual(len(seen), 4)
            self.assertTrue(all(entry[1:] == (1200, 1201) for entry in seen))
            copy_test.give_to([folder], user)


DESKTOP = {"kind": "desktop", "box_image": "example/desktop-box@sha256:" + "2" * 64, "box_user": "1200:1201",
           "box_desktop": {"application": ["/usr/bin/example-desk", "--orders", "1001"]}}


class DesktopSession(unittest.TestCase):
    """The box's desktop session (decision 0459)."""

    def box(self, system, runner="0:0"):
        return copy_test.desktop_options(system, copy_test.box_options(system, runner))

    def test_without_it_the_box_has_no_session_and_its_commands_no_display(self):
        self.assertIsNone(self.box(SYSTEM))
        run = copy_test.Run("example", token="abc")
        self.assertNotIn(copy_test.BOX_DESKTOP_SESSION, " ".join(run.box_argv("/b", "/h", "/p", "0:0")))
        argv = run.exec_argv({"A2M_HOME": "/h"}, "/p", ["init"])
        self.assertNotIn("DISPLAY", " ".join(argv))
        self.assertNotIn("DBUS_SESSION_BUS_ADDRESS", " ".join(argv))

    def test_an_application_is_an_argument_list_from_an_absolute_program(self):
        self.assertEqual(self.box(DESKTOP), {"application": DESKTOP["box_desktop"]["application"]})
        for spec in ({"application": []}, {"application": "/usr/bin/example-desk"}, {"application": ["example-desk"]},
                     {"application": ["/usr/bin/../bin/sh"]}, {"application": ["/usr/bin/example-desk", 7]},
                     {"application": ["/usr/bin/example-desk", ""]}, {"application": ["/usr/bin/x"], "env": {"A": "b"}},
                     ["/usr/bin/example-desk"], {}):
            with self.assertRaises(copy_test.CannotRun, msg=spec):
                self.box({**DESKTOP, "box_desktop": spec})

    def test_a_session_needs_an_image_of_its_own_and_a_user_that_is_not_root(self):
        for drop in ("box_image", "box_user"):
            with self.assertRaises(copy_test.CannotRun, msg=drop):
                self.box({key: value for key, value in DESKTOP.items() if key != drop})
        with self.assertRaises(copy_test.CannotRun):
            self.box({**DESKTOP, "box_user": "0:0"})
        with self.assertRaises(copy_test.CannotRun):
            self.box({"kind": "desktop", "box_image": DESKTOP["box_image"], "box_user": "1200:1201"})

    def test_the_session_adds_one_read_only_file_to_the_box_and_nothing_wider(self):
        plain, desk = copy_test.Run("example", token="abc"), copy_test.Run("example", token="abc")
        for run in (plain, desk):
            run.box_image = DESKTOP["box_image"]
        desk.desktop = self.box(DESKTOP)
        before = plain.box_argv("/b", "/h", "/p", "1200:1201")
        after = desk.box_argv("/b", "/h", "/p", "1200:1201")
        at = after.index(f"{copy_test.DESKTOP_SESSION}:{copy_test.BOX_DESKTOP_SESSION}:ro")
        self.assertEqual(after[at - 1], "-v")
        self.assertEqual(after[: at - 1] + after[at + 1:], before)
        for wider in ("--privileged", "--cap-add", "--device", "--ipc", "--security-opt", "/tmp/.X11-unix",
                      "/run/user", "--userns", "DISPLAY", "DBUS"):
            self.assertNotIn(wider, " ".join(after), wider)

    def test_the_harness_and_each_command_get_the_session_s_display_and_bus_only(self):
        run = copy_test.Run("example", token="abc")
        run.desktop = self.box(DESKTOP)
        for argv in (run.exec_argv({"A2M_HOME": "/h"}, "/p", ["init"]), run.harness_argv({"A2M_HOME": "/h"}, "/p")):
            given = [argv[i + 1] for i, part in enumerate(argv) if part == "-e"]
            self.assertEqual(sorted(given), sorted(["A2M_HOME=/h", "DISPLAY=:99",
                                                    "DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/a2m-desktop/bus"]))

    def test_the_session_starts_once_detached_and_waits_for_the_application_on_the_bus(self):
        run = copy_test.Run("example", token="abc")
        run.desktop = self.box(DESKTOP)
        calls, answers = [], iter([0, 1, 1, 0])
        run.start_desktop(execute=lambda argv: calls.append(argv) or (next(answers), "", ""), pause=lambda _: None)
        self.assertTrue(run.desktop_up)
        self.assertEqual(calls[0][:3], ["docker", "exec", "-d"])
        self.assertEqual(calls[0][-5:], [copy_test.BOX_DESKTOP_SESSION, "start", *DESKTOP["box_desktop"]["application"]])
        self.assertEqual([argv[-2:] for argv in calls[1:]], [["ready", "/usr/bin/example-desk"]] * 3)
        self.assertNotIn("--user", " ".join(calls[0]), "the session runs as the box's own user")
        run.start_desktop(execute=lambda argv: calls.append(argv) or (0, "", ""))
        self.assertEqual(len(calls), 4, "a session starts once")

    def test_an_application_that_never_reaches_the_bus_stops_the_run_by_name(self):
        run = copy_test.Run("example", token="abc")
        run.desktop = self.box(DESKTOP)
        now = [0.0]
        def clock():
            now[0] += 10
            return now[0]
        def execute(argv):
            if "start" in argv:
                return 0, "", ""
            if "ready" in argv:
                return 1, "", ""
            return 0, "a2m-desktop: the display did not start", ""
        with self.assertRaises(copy_test.CannotRun) as stop:
            run.start_desktop(execute=execute, wait=30, clock=clock, pause=lambda _: None)
        self.assertIn("desktop_not_ready", str(stop.exception))
        self.assertIn("the display did not start", str(stop.exception))
        self.assertFalse(run.desktop_up)

    def test_a_desktop_step_needs_the_session(self):
        with self.assertRaises(copy_test.CannotRun):
            copy_test.do_step({"desktop": {}}, {"project": "/p"}, {}, copy_test.Run("example", token="abc"))

    def test_the_person_s_hands_need_the_session_and_reach_the_box_by_its_display_alone(self):
        run = copy_test.Run("example", token="abc")
        with self.assertRaises(copy_test.CannotRun):
            copy_test.do_step({"hands": ["/person/hands.sh"]}, {"project": "/p"}, {}, run)
        run.desktop = self.box(DESKTOP)
        argv = run.hands_argv(["/person/hands.sh", "sign_in"])
        self.assertEqual(argv[:2], ["docker", "exec"])
        self.assertEqual(argv[-3:], [run.box, "/person/hands.sh", "sign_in"])
        # The display and nothing else of the runner's: no engine's home, no passphrase.
        flags = argv[2:-3]
        self.assertEqual([flags[at + 1] for at, flag in enumerate(flags) if flag == "-e"], ["DISPLAY=:99"])
        self.assertNotIn("-d", flags)
        self.assertIn("-d", run.hands_argv(["/person/hands.sh"], detach=True))
        for wrong in ([], ["person/hands.sh"], "/person/hands.sh", ["/person/hands.sh", ""], ["/bin/sh", "a\0b"]):
            with self.assertRaises(copy_test.CannotRun):
                run.hands_argv(wrong)

    def test_a_desktop_recipe_is_held_to_the_box_s_refusals(self):
        recipe = {"system": {**DESKTOP, "box_desktop": {"application": ["/usr/bin/example-desk", "--no-sandbox"]}}}
        self.assertTrue(copy_test.sandbox_off_in(recipe))
        with self.assertRaises(copy_test.CannotRun):
            self.box({**DESKTOP, "box_seccomp": "unconfined"})

    def test_the_session_script_names_no_application_and_listens_on_no_tcp_port(self):
        text = Path(copy_test.DESKTOP_SESSION).read_text()
        self.assertIn("-nolisten tcp", text)
        self.assertIn('exec "$@"', text)
        self.assertNotIn("--listen", text)
        self.assertNotIn("tcp:", text)
        for start in ("Xvfb", "dbus-daemon", "at-spi-bus-launcher"):
            self.assertIn(start, text)

    @unittest.skipUnless(sys.platform.startswith("linux"), "the script is a POSIX shell's")
    def test_the_session_script_refuses_root_and_a_bad_call(self):
        done = subprocess.run(["sh", copy_test.DESKTOP_SESSION], capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 2)
        self.assertIn("usage", done.stderr)
        done = subprocess.run(["sh", "-n", copy_test.DESKTOP_SESSION], capture_output=True, text=True, check=False)
        self.assertEqual(done.returncode, 0, done.stderr)


if __name__ == "__main__":
    unittest.main()
