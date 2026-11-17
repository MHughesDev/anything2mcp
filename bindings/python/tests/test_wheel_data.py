"""Tests of what the wheel carries beside the binary: the short name ``a2m``.

pip makes no launcher for a script in a wheel's data directory on Windows,
so the wheel carries ``a2m.cmd`` as well. The wheel itself needs the product
built, which these tests do not do; they hold the files and the project's
rules for carrying them. ``maturin sdist`` was run to see that the source
distribution carries them (see decision 0156).

Run: ``python -m unittest discover -s tests`` from ``bindings/python``.
"""

from __future__ import annotations

import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = ROOT / "wheel-data" / "scripts"


class ShortNameTests(unittest.TestCase):
    def test_the_command_file_runs_the_program_beside_it_with_every_argument(self) -> None:
        raw = (SCRIPTS / "a2m.cmd").read_bytes()
        text = raw.decode("ascii")
        self.assertNotIn(b"\n", raw.replace(b"\r\n", b""), "every line ends with a carriage return and a line feed")
        lines = [line for line in text.split("\r\n") if line]
        self.assertEqual(lines[0], "@echo off")
        # The program beside the file, found from the file's own folder and not the search path.
        self.assertIn('"%~dp0anything2mcp.exe" %*', lines)
        # Its exit code is the command's.
        self.assertEqual(lines[-1], "exit /b %ERRORLEVEL%")
        # A missing program is a named stop with the same code as the Unix script.
        self.assertIn("exit /b 4", text)
        self.assertIn("if not exist \"%~dp0anything2mcp.exe\"", text)
        # The folder holds this file before its program is run: never a
        # program in the working folder, which cmd can name as `%~dp0`.
        self.assertLess(text.index('if not exist "%~dp0a2m.cmd"'), text.index('"%~dp0anything2mcp.exe" %*'))
        self.assertNotIn("%1", text)

    def test_the_unix_script_is_still_there_and_the_two_say_the_same_stop(self) -> None:
        script = (SCRIPTS / "a2m").read_text(encoding="utf-8")
        command = (SCRIPTS / "a2m.cmd").read_text(encoding="ascii")
        self.assertTrue(script.startswith("#!python"))
        for phrase in (
            "the `anything2mcp` program was not found beside `a2m`.",
            "reinstall it with `pip install --force-reinstall anything2mcp`.",
        ):
            self.assertIn(phrase, script)
            self.assertIn(phrase, command)

    def test_only_the_two_commands_are_installed_beside_the_binary(self) -> None:
        # Anything else in this folder would be installed into every user's scripts folder.
        names = sorted(p.name for p in SCRIPTS.iterdir())
        self.assertEqual(names, ["a2m", "a2m.cmd"])

    def test_the_project_file_ships_the_folder_in_the_wheel_and_the_source_distribution(self) -> None:
        project = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
        self.assertIn('data = "wheel-data"', project)
        self.assertIn('include = [{ path = "wheel-data/**/*", format = "sdist" }]', project)


if __name__ == "__main__":
    unittest.main()
