#!/usr/bin/env python3
"""Proves tools/check_packages.py fails when a package holds a binary it should not."""
import io
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_packages as cp  # noqa: E402

ELF = b"\x7fELF" + b"\0" * 60


def wheel(members):
    with tempfile.NamedTemporaryFile(suffix=".whl", delete=False) as handle:
        with zipfile.ZipFile(handle, "w") as archive:
            for name, data in members.items():
                archive.writestr(name, data)
    return handle.name


def tarball(members):
    with tempfile.NamedTemporaryFile(suffix=".tgz", delete=False) as handle:
        with tarfile.open(fileobj=handle, mode="w:gz") as archive:
            for name, data in members.items():
                info = tarfile.TarInfo(name)
                info.size = len(data)
                archive.addfile(info, io.BytesIO(data))
    return handle.name


GOOD_WHEEL = {
    "anything2mcp-0.0.1.data/scripts/anything2mcp": ELF,
    "anything2mcp-0.0.1.data/scripts/a2m": b"#!/bin/sh\nexec anything2mcp \"$@\"\n",
    "anything2mcp/__init__.py": b"",
}


class Packages(unittest.TestCase):
    def faults(self, kind, path, reader):
        return cp.check(kind, reader(path))

    def test_a_wheel_with_only_the_program_passes(self):
        self.assertEqual(self.faults("wheel", wheel(GOOD_WHEEL), cp.wheel_files), [])

    def test_a_wheel_that_also_holds_the_copy_test_harness_fails(self):
        members = dict(GOOD_WHEEL, **{"anything2mcp-0.0.1.data/scripts/a2m-copy-test-harness": ELF})
        faults = self.faults("wheel", wheel(members), cp.wheel_files)
        self.assertTrue(any("copy test harness" in f for f in faults), faults)
        self.assertTrue(any("binary the package is not to hold" in f for f in faults), faults)

    def test_a_wheel_that_holds_another_binary_under_any_name_fails(self):
        members = dict(GOOD_WHEEL, **{"anything2mcp/helper": ELF})
        self.assertTrue(self.faults("wheel", wheel(members), cp.wheel_files))
        members = dict(GOOD_WHEEL, **{"anything2mcp/lib.dll": b"text"})
        self.assertTrue(self.faults("wheel", wheel(members), cp.wheel_files))

    def test_a_wheel_without_the_program_fails(self):
        members = {"anything2mcp/__init__.py": b""}
        self.assertTrue(self.faults("wheel", wheel(members), cp.wheel_files))

    def test_a_platform_package_holds_the_one_program_in_bin(self):
        good = {"package/package.json": b"{}", "package/bin/anything2mcp": ELF}
        self.assertEqual(self.faults("npm", tarball(good), cp.tarball_files), [])
        bad = dict(good, **{"package/bin/a2m-copy-test-harness": ELF})
        self.assertTrue(self.faults("npm", tarball(bad), cp.tarball_files))

    def test_the_launcher_holds_no_binary(self):
        good = {"package/package.json": b"{}", "package/bin/anything2mcp.js": b"#!/usr/bin/env node\n"}
        self.assertEqual(self.faults("launcher", tarball(good), cp.tarball_files), [])
        bad = dict(good, **{"package/bin/anything2mcp": ELF})
        self.assertTrue(self.faults("launcher", tarball(bad), cp.tarball_files))


if __name__ == "__main__":
    unittest.main()
