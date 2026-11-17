"""Tests of how the client finds the binary it runs (PR-2 of the third audit,
decision 0194): only the program the installed distribution of this very
module put in its scheme's scripts folder, never one a distribution found
first on ``sys.path`` names, which a project folder can plant.

A wheel of the client's own files is made here, with a stand-in program in
its scripts, and installed with pip into a fresh virtual environment, with
no network. Skipped where a virtual environment with pip cannot be made.

Run: ``python -m unittest discover -s tests`` from ``bindings/python``.
"""

from __future__ import annotations

import base64
import hashlib
import os
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
PACKAGE = HERE.parent / "src" / "anything2mcp"
WINDOWS = sys.platform.startswith("win")
PROGRAM = "anything2mcp" + (".exe" if WINDOWS else "")
VERSION = "0.0.1"


def _record_line(name: str, data: bytes) -> str:
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"{name},sha256={digest},{len(data)}"


def _wheel(folder: Path) -> Path:
    """A wheel of the client's own modules, with a stand-in program."""
    files = {}
    for module in sorted(PACKAGE.iterdir()):
        if module.is_file() and module.suffix in (".py", ".typed"):
            files[f"anything2mcp/{module.name}"] = module.read_bytes()
    info = f"anything2mcp-{VERSION}.dist-info"
    files[f"anything2mcp-{VERSION}.data/scripts/{PROGRAM}"] = b"#!/bin/sh\necho stand-in\n"
    files[f"{info}/METADATA"] = f"Metadata-Version: 2.1\nName: anything2mcp\nVersion: {VERSION}\n".encode()
    files[f"{info}/WHEEL"] = b"Wheel-Version: 1.0\nGenerator: tests\nRoot-Is-Purelib: true\nTag: py3-none-any\n"
    record = [_record_line(name, data) for name, data in files.items()]
    record.append(f"{info}/RECORD,,")
    files[f"{info}/RECORD"] = ("\n".join(record) + "\n").encode()
    wheel = folder / f"anything2mcp-{VERSION}-py3-none-any.whl"
    with zipfile.ZipFile(wheel, "w") as archive:
        for name, data in files.items():
            archive.writestr(name, data)
    return wheel


class BinaryPathTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.folder = tempfile.TemporaryDirectory()
        root = Path(cls.folder.name)
        venv = root / "venv"
        made = subprocess.run([sys.executable, "-m", "venv", str(venv)], capture_output=True)
        cls.python = venv / ("Scripts" if WINDOWS else "bin") / ("python.exe" if WINDOWS else "python")
        if made.returncode != 0 or not cls.python.is_file():
            raise unittest.SkipTest("a virtual environment cannot be made here")
        installed = subprocess.run(
            [str(cls.python), "-m", "pip", "install", "--quiet", "--no-index", "--no-deps", str(_wheel(root))],
            capture_output=True,
            text=True,
        )
        if installed.returncode != 0:
            raise unittest.SkipTest(f"pip cannot install a local wheel here: {installed.stderr}")
        cls.scripts = (venv / ("Scripts" if WINDOWS else "bin")).resolve()

    @classmethod
    def tearDownClass(cls) -> None:
        cls.folder.cleanup()

    def found_in(self, cwd: Path) -> str:
        env = {key: value for key, value in os.environ.items() if key != "PYTHONPATH"}
        ran = subprocess.run(
            [str(self.python), "-c", "import anything2mcp; print(anything2mcp.binary_path())"],
            cwd=str(cwd),
            capture_output=True,
            text=True,
            env=env,
        )
        self.assertEqual(ran.returncode, 0, ran.stderr)
        return ran.stdout.strip()

    def test_the_installed_program_is_found_in_its_schemes_scripts_folder(self) -> None:
        self.assertEqual(self.found_in(Path(self.folder.name)), str(self.scripts / PROGRAM))

    def test_a_distribution_planted_in_the_working_folder_is_never_trusted(self) -> None:
        # `python -c` puts the working folder first on `sys.path`: a folder
        # that holds a distribution of the same name, whose record names a
        # program of its own, is found before the installed one.
        planted = Path(self.folder.name) / "project"
        info = planted / "anything2mcp-9.dist-info"
        (planted / "bin").mkdir(parents=True)
        info.mkdir()
        program = planted / "bin" / PROGRAM
        program.write_bytes(b"#!/bin/sh\necho planted\n")
        (info / "METADATA").write_text("Metadata-Version: 2.1\nName: anything2mcp\nVersion: 9\n")
        (info / "RECORD").write_text(f"bin/{PROGRAM},,\nanything2mcp-9.dist-info/RECORD,,\n")
        found = self.found_in(planted)
        self.assertNotEqual(found, str(program.resolve()))
        self.assertEqual(found, str(self.scripts / PROGRAM))


if __name__ == "__main__":
    unittest.main()
