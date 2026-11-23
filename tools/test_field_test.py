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
import sys
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


class PublicAddress(unittest.TestCase):
    def test_an_answer_or_a_refusal_means_reached(self):
        self.assertTrue(field_test.reached(0, ""))
        self.assertTrue(field_test.reached(1, "bash: connect: Connection refused"))
        self.assertTrue(field_test.reached(1, "Connection reset by peer"))

    def test_no_route_and_a_timeout_mean_closed(self):
        self.assertFalse(field_test.reached(1, "bash: connect: Network is unreachable"))
        self.assertFalse(field_test.reached(124, ""))

    def test_the_run_is_stopped_by_name_when_an_address_answers(self):
        asked = []

        def execute(argv):
            asked.append(argv)
            return 0, "", ""

        with self.assertRaises(field_test.CannotRun) as stop:
            field_test.check_closed("box", execute)
        self.assertIn("closed_network_open", str(stop.exception))
        self.assertEqual(len(asked), 1)
        self.assertIn("box", asked[0])

    def test_every_public_address_is_tried_when_none_answers(self):
        asked = []

        def execute(argv):
            asked.append(argv[-1])
            return 1, "", "Network is unreachable"

        field_test.check_closed("box", execute)
        self.assertEqual(len(asked), len(field_test.PUBLIC_ADDRESSES))
        for host, port in field_test.PUBLIC_ADDRESSES:
            self.assertTrue(any(f"{host}/{port}" in one for one in asked))

    def test_the_check_comes_before_the_first_command_of_the_product(self):
        run = field_test.Run("example", token="abc")
        calls = []

        def execute(argv):
            calls.append(argv)
            return (0, "", "") if argv[1] == "run" else (1, "", "Network is unreachable")

        run.ensure_box("/b", "/h", "/p", "0:0", execute)
        run.ensure_box("/b", "/h", "/p", "0:0", execute)  # once only
        self.assertEqual(calls[0][1], "run")
        self.assertTrue(all(call[1] == "exec" for call in calls[1:]))
        self.assertEqual(len(calls), 1 + len(field_test.PUBLIC_ADDRESSES))

    def test_a_box_on_an_open_network_runs_no_command(self):
        run = field_test.Run("example", token="abc")
        with self.assertRaises(field_test.CannotRun):
            run.ensure_box("/b", "/h", "/p", "0:0", lambda argv: (0, "", ""))


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
