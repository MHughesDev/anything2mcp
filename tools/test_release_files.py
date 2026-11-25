#!/usr/bin/env python3
"""Proves tools/release_files.py makes a whole release set, and fails on each kind of fault in one.

The set is built from small stand-in wheels and npm packages, one per target, each holding a stand-in
binary, so that no build is needed.
"""
import io
import json
import shutil
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import release_files as rf  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
VERSION = "0.0.1"
TAGS = {
    "x86_64-unknown-linux-gnu": "manylinux_2_17_x86_64.manylinux2014_x86_64",
    "aarch64-unknown-linux-gnu": "manylinux_2_17_aarch64.manylinux2014_aarch64",
    "x86_64-unknown-linux-musl": "musllinux_1_2_x86_64",
    "aarch64-unknown-linux-musl": "musllinux_1_2_aarch64",
    "x86_64-apple-darwin": "macosx_10_12_x86_64",
    "aarch64-apple-darwin": "macosx_11_0_arm64",
    "x86_64-pc-windows-msvc": "win_amd64",
    "aarch64-pc-windows-msvc": "win_arm64",
}


def program(target):
    head = b"MZ" if rf.is_windows(target) else b"\xcf\xfa\xed\xfe" if "apple" in target else b"\x7fELF"
    return head + target.encode() + b"\0" * 32


def write_wheel(path, target, binary=None, notices=None):
    exe = ".exe" if rf.is_windows(target) else ""
    info = f"anything2mcp-{VERSION}.dist-info"
    with zipfile.ZipFile(path, "w") as z:
        z.writestr(f"anything2mcp-{VERSION}.data/scripts/anything2mcp{exe}", binary or program(target))
        z.writestr("anything2mcp/__init__.py", "")
        z.writestr(f"{info}/licenses/LICENSE", (REPO / "LICENSE").read_bytes())
        z.writestr(f"{info}/licenses/THIRD-PARTY-NOTICES.txt", notices or (REPO / rf.NOTICES).read_bytes())
        z.writestr(f"{info}/sboms/anything2mcp.cyclonedx.json",
                   json.dumps({"bomFormat": "CycloneDX", "components": [{"name": "x"}]}))


def write_tgz(path, members):
    with tarfile.open(path, "w:gz") as tar:
        for name, data in members.items():
            info = tarfile.TarInfo(name)
            info.size, info.mode = len(data), 0o755
            tar.addfile(info, io.BytesIO(data))


class ReleaseSet(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        pip, npm, archives = (self.tmp / n for n in ("pip", "npm", "archives"))
        for folder in (pip, npm, archives):
            folder.mkdir()
        (pip / f"anything2mcp-{VERSION}.tar.gz").write_bytes(b"source")
        write_tgz(npm / f"anything2mcp-{VERSION}.tgz", {"package/package.json": b"{}"})
        for target, (platform, _) in rf.TARGETS.items():
            exe = ".exe" if rf.is_windows(target) else ""
            write_wheel(pip / f"anything2mcp-{VERSION}-py3-none-{TAGS[target]}.whl", target)
            write_tgz(npm / f"anything2mcp-{platform}-{VERSION}.tgz", {
                "package/package.json": b"{}", f"package/bin/anything2mcp{exe}": program(target),
                "package/LICENSE": (REPO / "LICENSE").read_bytes(),
                "package/THIRD-PARTY-NOTICES.txt": (REPO / rf.NOTICES).read_bytes()})
            binary = self.tmp / f"bin-{target}"
            binary.write_bytes(program(target))
            rf.make_archive(REPO, target, binary, VERSION, archives)
        self.out = self.tmp / "release"
        rf.assemble(REPO, VERSION, pip, npm, archives, self.out)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def faults(self):
        return rf.check(REPO, self.out, VERSION)

    def resum(self):
        rf.write_sums(self.out)

    def assert_fault(self, *words):
        faults = self.faults()
        self.assertTrue(any(all(w in f for w in words) for f in faults), faults)

    def test_a_whole_set_passes(self):
        self.assertEqual(self.faults(), [])
        self.assertEqual(len(list(self.out.iterdir())), 8 + 8 + 1 + 9 + 8 + 1 + 1)

    def test_an_archive_is_the_same_byte_for_byte_when_made_twice(self):
        target = "x86_64-unknown-linux-gnu"
        binary = self.tmp / f"bin-{target}"
        one = rf.make_archive(REPO, target, binary, VERSION, self.tmp / "one").read_bytes()
        two = rf.make_archive(REPO, target, binary, VERSION, self.tmp / "two").read_bytes()
        self.assertEqual(one, two)

    def test_an_archive_holds_the_program_its_link_and_the_licence_texts(self):
        members = rf.archive_members(self.out / rf.archive_name(VERSION, "aarch64-apple-darwin"))
        top = f"anything2mcp-{VERSION}-aarch64-apple-darwin"
        self.assertEqual(sorted(members), sorted(f"{top}/{n}" for n in ("anything2mcp", "a2m", "LICENSE", rf.NOTICES)))
        self.assertEqual(members[f"{top}/a2m"][:2], ("link", "anything2mcp"))
        self.assertEqual(members[f"{top}/anything2mcp"][2], 0o755)
        members = rf.archive_members(self.out / rf.archive_name(VERSION, "x86_64-pc-windows-msvc"))
        self.assertIn(f"anything2mcp-{VERSION}-x86_64-pc-windows-msvc/a2m.cmd", members)

    def test_a_missing_archive_fails(self):
        (self.out / rf.archive_name(VERSION, "aarch64-pc-windows-msvc")).unlink()
        self.resum()
        self.assert_fault(rf.archive_name(VERSION, "aarch64-pc-windows-msvc"), "missing")

    def test_a_missing_wheel_fails(self):
        (self.out / f"anything2mcp-{VERSION}-py3-none-{TAGS['x86_64-apple-darwin']}.whl").unlink()
        self.resum()
        self.assert_fault("no wheel for x86_64-apple-darwin")

    def test_a_file_outside_the_set_fails(self):
        (self.out / "extra.bin").write_bytes(b"x")
        self.resum()
        self.assert_fault("extra.bin is not a file of the release")

    def test_a_checksum_that_is_wrong_or_left_out_fails(self):
        sums = self.out / "SHA256SUMS"
        lines = sums.read_text().splitlines()
        sums.write_text("\n".join(lines[1:]) + "\n")
        self.assert_fault("SHA256SUMS does not name")
        self.resum()
        name = rf.archive_name(VERSION, "x86_64-unknown-linux-gnu")
        (self.out / name).write_bytes((self.out / name).read_bytes() + b"\0")
        self.assert_fault(f"the digest of {name}")

    def test_a_wheel_with_another_binary_than_its_archive_fails(self):
        target = "x86_64-unknown-linux-musl"
        write_wheel(self.out / f"anything2mcp-{VERSION}-py3-none-{TAGS[target]}.whl", target,
                    binary=b"\x7fELF" + b"another build" + b"\0" * 20)
        self.resum()
        self.assert_fault(target, "carry different binaries")

    def test_a_wheel_without_the_notices_file_as_in_the_tree_fails(self):
        target = "aarch64-unknown-linux-gnu"
        write_wheel(self.out / f"anything2mcp-{VERSION}-py3-none-{TAGS[target]}.whl", target, notices=b"old\n")
        self.resum()
        self.assert_fault("does not carry THIRD-PARTY-NOTICES.txt")

    def test_an_npm_package_without_the_notices_file_fails(self):
        target = "x86_64-pc-windows-msvc"
        write_tgz(self.out / f"anything2mcp-win32-x64-{VERSION}.tgz", {
            "package/package.json": b"{}", "package/bin/anything2mcp.exe": program(target),
            "package/LICENSE": (REPO / "LICENSE").read_bytes()})
        self.resum()
        self.assert_fault("anything2mcp-win32-x64", "does not carry THIRD-PARTY-NOTICES.txt")

    def test_an_archive_with_a_second_binary_fails(self):
        target = "x86_64-unknown-linux-gnu"
        name = rf.archive_name(VERSION, target)
        top = f"anything2mcp-{VERSION}-{target}"
        with tarfile.open(self.out / name, "r:gz") as tar:
            kept = [(m, tar.extractfile(m).read() if m.isfile() else None) for m in tar.getmembers()]
        with tarfile.open(self.out / name, "w:gz") as tar:
            for m, data in kept:
                tar.addfile(m, io.BytesIO(data) if data is not None else None)
            info = tarfile.TarInfo(f"{top}/helper")
            info.size = 8
            tar.addfile(info, io.BytesIO(b"\x7fELF\0\0\0\0"))
        self.resum()
        self.assert_fault(f"{top}/helper")


class Targets(unittest.TestCase):
    def test_the_notices_file_and_the_release_name_the_same_eight_targets(self):
        import notices
        self.assertEqual(tuple(rf.TARGETS), notices.TARGETS)
        self.assertEqual(sorted(TAGS), sorted(rf.TARGETS))


class Changelog(unittest.TestCase):
    def changelog(self, text):
        root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        (root / "CHANGELOG.md").write_text(text)
        return root

    def test_a_released_section_passes_and_gives_its_notes(self):
        root = self.changelog("# Changelog\n\n## [Unreleased]\n\n## [0.1.0] - 2026-12-01\n\n### Added\n\n- A thing.\n\n"
                              "[Unreleased]: https://example.com/a\n[0.1.0]: https://example.com/b\n")
        self.assertEqual(rf.changelog_problems(root, "0.1.0"), [])
        self.assertEqual(rf.notes(root, "0.1.0"), "### Added\n\n- A thing.\n")

    def test_an_unreleased_section_that_is_not_empty_fails(self):
        root = self.changelog("## [Unreleased]\n\n### Added\n\n- Left behind.\n\n## [0.1.0] - 2026-12-01\n\n- A thing.\n\n"
                              "[0.1.0]: https://example.com/b\n")
        self.assertTrue(any("Unreleased" in p for p in rf.changelog_problems(root, "0.1.0")))

    def test_a_version_with_no_dated_section_or_no_link_fails(self):
        root = self.changelog("## [Unreleased]\n\n## [0.1.0]\n\n- A thing.\n")
        problems = rf.changelog_problems(root, "0.1.0")
        self.assertTrue(any("no section headed" in p for p in problems), problems)
        self.assertTrue(any("no link line" in p for p in problems), problems)

    def test_the_tree_is_not_ready_to_release_a_version_it_has_no_section_for(self):
        self.assertTrue(rf.changelog_problems(REPO, "0.1.0"))


class Workflow(unittest.TestCase):
    def copy(self, change):
        root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        (root / ".github" / "workflows").mkdir(parents=True)
        text = (REPO / rf.WORKFLOW).read_text()
        changed = change(text)
        self.assertNotEqual(text, changed)
        (root / rf.WORKFLOW).write_text(changed)
        return rf.workflow_problems(root)

    def test_the_tree_passes(self):
        self.assertEqual(rf.workflow_problems(REPO), [])

    def test_a_publish_job_that_is_not_gated_fails(self):
        problems = self.copy(lambda t: t.replace(
            "    needs: [decide, npm-package, publish-release]\n    if: needs.decide.outputs.publish == 'true'\n",
            "    needs: [decide, npm-package, publish-release]\n"))
        self.assertTrue(any("publish-npm" in p and "decide" in p for p in problems), problems)

    def test_a_publish_job_that_does_not_wait_for_the_release_page_fails(self):
        problems = self.copy(lambda t: t.replace("needs: [decide, pip-package, publish-release]",
                                                 "needs: [decide, pip-package]"))
        self.assertIn("`publish-pypi` does not wait for `publish-release`", problems)

    def test_a_job_that_may_write_and_is_not_a_publish_job_fails(self):
        problems = self.copy(lambda t: t.replace(
            "    name: Gather and check every file of the release\n",
            "    name: Gather and check every file of the release\n    permissions:\n      contents: write\n"))
        self.assertTrue(any("`release-files` may write" in p for p in problems), problems)

    def test_a_missing_job_or_target_fails(self):
        problems = self.copy(lambda t: t.replace("          - { target: aarch64-pc-windows-msvc, exe: \".exe\" }\n", ""))
        self.assertTrue(any("matrix of `archives`" in p for p in problems), problems)
        problems = self.copy(lambda t: t.replace("  publish-release:\n", "  publish-page:\n"))
        self.assertIn("the job `publish-release` is missing", problems)

    def test_a_publish_job_without_an_environment_fails(self):
        problems = self.copy(lambda t: t.replace("    environment: release\n", ""))
        self.assertTrue(any("`publish-release` runs in no GitHub environment" in p for p in problems), problems)


if __name__ == "__main__":
    unittest.main()
