#!/usr/bin/env python3
"""Prove the parts of tools/field_test.py that need no container engine.

The closed network, the containers, and the check that no public address can be reached are
built as lists of arguments and decided by small functions; these tests hold what those
lists and functions say. Nothing here starts a container. Run from the repository root:

    python3 tools/test_field_test.py

Standard library only; Python 3.11 or newer.
"""
import asyncio
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


if __name__ == "__main__":
    unittest.main()
