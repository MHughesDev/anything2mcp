"""Take the workspace's own crates out of a build folder before CI stores it.

Usage: python3 tools/ci_cache.py prune <profile folder>   (e.g. target/debug)
       python3 tools/ci_cache.py prune <profile folder> --dry-run

CI keeps the compiled dependencies between runs, keyed on the lock file and
the toolchain (decision 0372). The workspace's own crates change with every
commit, so storing them would only make the stored folder larger: this takes
out every file and folder of a workspace member from `deps/`, `build/`,
`.fingerprint/` and `examples/`, the incremental folder, and the programs and
libraries at the top of the folder. What is left is what the dependencies
compiled to, which the next build reuses and checks as cargo always does.
"""
import json
import os
import shutil
import subprocess
import sys


def members():
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        check=True, capture_output=True, text=True,
    ).stdout
    packages, libraries, programs = set(), set(), set()
    for package in json.loads(out)["packages"]:
        packages.add(package["name"].replace("-", "_"))
        for target in package["targets"]:
            name = target["name"].replace("-", "_")
            if any(kind in ("lib", "rlib", "proc-macro", "cdylib", "staticlib", "dylib") for kind in target["kind"]):
                libraries.add(name)
            else:
                programs.add(name)
    return {"packages": packages, "libraries": libraries, "programs": programs}


def is_member(entry, names, sub="deps"):
    """Whether a file or folder of a build folder is a workspace member's.

    `build/` and `.fingerprint/` hold `<package>-<hash>`; `deps/` holds
    `lib<library>-<hash>.<ext>` for a library and `<program>-<hash>[.<ext>]`
    for a test, a program, or an example. A dependency that shares a name with
    a test target of the workspace (a `log` test and the `log` crate) is told
    apart by the `lib` prefix.
    """
    stem = entry.split(".", 1)[0]
    base = stem.rsplit("-", 1)[0] if "-" in stem else stem
    base = base.replace("-", "_")
    if sub in ("build", ".fingerprint"):
        return base in names["packages"]
    if base.startswith("lib") and base[3:] in names["libraries"]:
        return True
    return base in names["programs"] or base in names["libraries"] and not entry.startswith("lib")


def prune(folder, dry_run):
    names = members()
    taken = 0
    def remove(path):
        nonlocal taken
        taken += 1
        if dry_run:
            return
        if os.path.isdir(path) and not os.path.islink(path):
            shutil.rmtree(path, ignore_errors=True)
        else:
            try:
                os.remove(path)
            except FileNotFoundError:
                pass
    if not os.path.isdir(folder):
        print(f"{folder} does not exist; nothing to take out.")
        return
    incremental = os.path.join(folder, "incremental")
    if os.path.isdir(incremental):
        remove(incremental)
    for sub in ("deps", "build", ".fingerprint", "examples"):
        path = os.path.join(folder, sub)
        if not os.path.isdir(path):
            continue
        for entry in os.listdir(path):
            if is_member(entry, names, sub):
                remove(os.path.join(path, entry))
    for entry in os.listdir(folder):
        path = os.path.join(folder, entry)
        if os.path.isfile(path) and is_member(entry, names):
            remove(path)
    print(f"{'would take' if dry_run else 'took'} out {taken} entries of the workspace's {len(names['packages'])} packages from {folder}")


def main(argv):
    if len(argv) < 3 or argv[1] != "prune":
        print(__doc__)
        return 2
    prune(argv[2], "--dry-run" in argv)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
