"""The script that installs a locally built wheel and measures it.

The install itself needs a built wheel, which only a build with the Rust
toolchain makes; point `A2M_TEST_WHEEL` at one to run it. The rest is checked
without a wheel.
"""
import importlib.util
import os
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, "..", "tools", "try_wheel.py")


def load():
    spec = importlib.util.spec_from_file_location("try_wheel", TOOL)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class TryWheel(unittest.TestCase):
    def test_a_command_line_without_one_wheel_is_refused(self):
        tool = load()
        self.assertEqual(tool.main([]), 1)
        self.assertEqual(tool.main(["a.whl", "b.whl"]), 1)

    def test_the_names_of_the_program_follow_the_system(self):
        tool = load()
        self.assertEqual(tool.exe("anything2mcp").endswith(".exe"), os.name == "nt")
        self.assertTrue(tool.bin_dir("env").endswith("Scripts" if os.name == "nt" else "bin"))

    @unittest.skipUnless(os.environ.get("A2M_TEST_WHEEL"), "no built wheel was given")
    def test_a_built_wheel_installs_into_a_clean_environment_and_runs(self):
        found = load().try_wheel(os.environ["A2M_TEST_WHEEL"])
        self.assertTrue(found["client_finds_program"])
        self.assertGreater(found["program_bytes"], 1_000_000)
        self.assertGreater(found["program_median_ms"], 0)


if __name__ == "__main__":
    unittest.main()
