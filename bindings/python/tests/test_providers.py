"""Tests of the folders a provider search looks in (package F3).

A wheel of a provider package is made by hand and installed with pip into a
fresh virtual environment, with no network. The package's ``__init__`` writes
a marker file when it is imported. The tests show that the folders reported
by the interpreter of that environment are the ones that hold the package,
that its entry is visible in the group, and that nothing here imported it.
Skipped where a virtual environment with pip cannot be made.

Run: ``python -m unittest discover -s tests`` from ``bindings/python``.
"""

from __future__ import annotations

import base64
import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

WINDOWS = sys.platform.startswith("win")
PACKAGE = "example_sheet_provider"
DIST = "example_sheet_provider-0.1.0.dist-info"
MANIFEST = b"manifest_format = 1\ncontract = 1\n"
SOURCE = Path(__file__).resolve().parent.parent / "src" / "anything2mcp" / "_providers.py"


def _record_line(name: str, data: bytes) -> str:
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"{name},sha256={digest},{len(data)}"


def _wheel(folder: Path, marker: Path) -> Path:
    files = {
        f"{PACKAGE}/__init__.py": f"open({str(marker)!r}, 'w').write('imported')\n".encode(),
        f"{PACKAGE}/anything2mcp-provider.toml": MANIFEST,
        f"{DIST}/METADATA": b"Metadata-Version: 2.1\nName: example_sheet_provider\nVersion: 0.1.0\n",
        f"{DIST}/WHEEL": b"Wheel-Version: 1.0\nGenerator: tests\nRoot-Is-Purelib: true\nTag: py3-none-any\n",
        f"{DIST}/entry_points.txt": b"[anything2mcp.providers]\nexample_sheet = example_sheet_provider\n",
    }
    record = [_record_line(name, data) for name, data in files.items()]
    record.append(f"{DIST}/RECORD,,")
    files[f"{DIST}/RECORD"] = ("\n".join(record) + "\n").encode()
    wheel = folder / "example_sheet_provider-0.1.0-py3-none-any.whl"
    with zipfile.ZipFile(wheel, "w") as archive:
        for name, data in files.items():
            archive.writestr(name, data)
    return wheel


class ProviderSiteFolderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.folder = tempfile.TemporaryDirectory()
        root = Path(cls.folder.name)
        cls.marker = root / "imported-marker"
        venv = root / "venv"
        made = subprocess.run([sys.executable, "-m", "venv", str(venv)], capture_output=True)
        cls.python = venv / ("Scripts" if WINDOWS else "bin") / ("python.exe" if WINDOWS else "python")
        if made.returncode != 0 or not cls.python.is_file():
            raise unittest.SkipTest("a virtual environment cannot be made here")
        installed = subprocess.run(
            [
                str(cls.python), "-m", "pip", "install", "--quiet", "--no-index", "--no-deps",
                str(_wheel(root, cls.marker)),
            ],
            capture_output=True,
            text=True,
        )
        if installed.returncode != 0:
            raise unittest.SkipTest(f"pip cannot install a local wheel here: {installed.stderr}")

    @classmethod
    def tearDownClass(cls) -> None:
        cls.folder.cleanup()

    def ask(self, code: str) -> str:
        ran = subprocess.run([str(self.python), "-I", "-c", code], capture_output=True, text=True)
        self.assertEqual(ran.returncode, 0, ran.stderr)
        return ran.stdout.strip()

    def test_the_folders_are_this_interpreters_own_and_hold_the_installed_package(self) -> None:
        # The module is loaded from its file, so that the test needs no
        # installed client.
        listed = json.loads(
            self.ask(
                "import importlib.util, json;"
                f"spec = importlib.util.spec_from_file_location('_providers', {str(SOURCE)!r});"
                "module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module);"
                "print(json.dumps(module.provider_site_folders()))"
            )
        )
        self.assertTrue(listed)
        self.assertEqual(len(listed), len(set(listed)))
        self.assertTrue(
            any((Path(folder) / DIST / "entry_points.txt").is_file() for folder in listed), listed
        )
        self.assertFalse(self.marker.exists(), "asking for the folders imported the package")

    def test_the_entry_is_in_the_group_and_is_not_loaded_by_listing_it(self) -> None:
        listed = self.ask(
            "from importlib.metadata import entry_points;"
            "print([(e.name, e.value) for e in entry_points(group='anything2mcp.providers')])"
        )
        self.assertIn("('example_sheet', 'example_sheet_provider')", listed)
        self.assertFalse(self.marker.exists(), "listing the entry imported the package")


if __name__ == "__main__":
    unittest.main()
