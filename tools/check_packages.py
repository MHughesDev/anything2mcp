#!/usr/bin/env python3
"""Check that a pip wheel or an npm package holds the expected binary and no other.

    python3 tools/check_packages.py wheel <file.whl>...
    python3 tools/check_packages.py npm   <file.tgz>...
    python3 tools/check_packages.py npm-launcher <file.tgz>...

A package holds a binary when any file in it begins with the signature of a
program of the operating systems we ship for (ELF, Mach-O, or a Windows
executable), or has the name of one. A wheel holds exactly one, the product's
own program in the scripts folder of the wheel's data; a platform package of
npm holds exactly one, in `bin/`; the launcher of npm holds none. The copy test
harness (decision 0310) and every other program of the workspace must never
reach a package, so any other binary, and any file named for the harness, is a
fault. Exit code 0 when every package is as expected, 1 when one is not.
"""
import io
import re
import sys
import tarfile
import zipfile

PROGRAM = "anything2mcp"
SIGNATURES = (
    b"\x7fELF",  # Linux
    b"MZ",  # Windows
    b"\xfe\xed\xfa\xce", b"\xfe\xed\xfa\xcf", b"\xce\xfa\xed\xfe", b"\xcf\xfa\xed\xfe",  # Mach-O
    b"\xca\xfe\xba\xbe",  # a macOS universal binary
)
BINARY_NAMES = re.compile(r"\.(exe|dll|so|dylib|node|a|lib)$", re.I)
HARNESS = re.compile(r"(copy[-_]?test|field)[-_]?harness", re.I)  # its name, and the name it had (decision 0330)


def is_binary(name, head):
    """Whether a file with this name and first bytes is a binary."""
    return head.startswith(SIGNATURES) or bool(BINARY_NAMES.search(name))


def wheel_files(path):
    with zipfile.ZipFile(path) as wheel:
        for info in wheel.infolist():
            if not info.is_dir():
                with wheel.open(info) as handle:
                    yield info.filename, handle.read(8)


def tarball_files(path):
    with tarfile.open(path, "r:*") as archive:
        for member in archive.getmembers():
            if member.isfile():
                yield member.name, archive.extractfile(member).read(8)
            elif member.issym() or member.islnk():
                yield member.name, b""


def check(kind, files):
    """The faults of one package, given its (name, first bytes) pairs."""
    found, faults = [], []
    for name, head in files:
        if HARNESS.search(name):
            faults.append(f"{name} is named for the copy test harness, which is never packaged")
        if is_binary(name, head):
            found.append(name)
    if kind == "wheel":
        expected = re.compile(rf"\.data/scripts/{PROGRAM}(\.exe)?$")
    elif kind == "npm":
        expected = re.compile(rf"^package/bin/{PROGRAM}(\.exe)?$")
    else:
        expected = None
    for name in found:
        if expected is None or not expected.search(name):
            faults.append(f"{name} is a binary the package is not to hold")
    held = [name for name in found if expected is not None and expected.search(name)]
    if expected is not None and len(held) != 1:
        faults.append(f"expected exactly one {PROGRAM} binary, found {len(held)}: {held}")
    return faults


def main(argv):
    if len(argv) < 3 or argv[1] not in ("wheel", "npm", "npm-launcher"):
        print(__doc__)
        return 2
    kind, failed = argv[1], False
    for path in argv[2:]:
        files = wheel_files(path) if kind == "wheel" else tarball_files(path)
        faults = check("wheel" if kind == "wheel" else "npm" if kind == "npm" else "launcher", files)
        for fault in faults:
            print(f"{path}: {fault}")
        failed = failed or bool(faults)
        if not faults:
            print(f"{path}: holds the expected binaries only")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
