#!/usr/bin/env python3
"""Install a locally built wheel into a clean environment and say what it measured.

    python3 bindings/python/tools/try_wheel.py <file.whl> [--out <result.json>]

Build the wheel first, the way CI does and without publishing anything:

    python3 -m venv /tmp/tools && /tmp/tools/bin/pip install maturin==1.16.0
    cd bindings/python && /tmp/tools/bin/maturin build --release --out dist

The wheel is installed into a new virtual environment with nothing else in
it. The script then runs both names of the program, imports the package and
asks it for the program it found, and prints (and, with --out, stores) the
wheel's size, the size of the program inside it, how long the install took,
and how long each name takes to print its version. Exit code 0 when the
install and every check held, 1 when one did not. Nothing leaves the machine.
"""
import json
import os
import platform
import statistics
import subprocess
import sys
import tempfile
import time
import venv
import zipfile

RUNS = 15


def bin_dir(env):
    return os.path.join(env, "Scripts" if os.name == "nt" else "bin")


def exe(name):
    return name + (".exe" if os.name == "nt" else "")


def milliseconds(command):
    """The wall time in milliseconds of each of RUNS runs of a command."""
    times = []
    for _ in range(RUNS):
        started = time.perf_counter()
        done = subprocess.run(command, capture_output=True, check=True)
        times.append((time.perf_counter() - started) * 1000)
        assert done.stdout.startswith(b"anything2mcp "), done.stdout
    return times


def try_wheel(wheel):
    """Installs the wheel into a clean environment; returns what was found."""
    found = {
        "wheel": os.path.basename(wheel),
        "system": f"{platform.system()} {platform.machine()}",
        "wheel_bytes": os.path.getsize(wheel),
    }
    with zipfile.ZipFile(wheel) as archive:
        for info in archive.infolist():
            if info.filename.endswith(".data/scripts/" + exe("anything2mcp")):
                found["program_bytes"] = info.file_size
    with tempfile.TemporaryDirectory() as folder:
        env = os.path.join(folder, "env")
        venv.create(env, with_pip=True)
        python = os.path.join(bin_dir(env), exe("python"))
        started = time.perf_counter()
        subprocess.run(
            [python, "-m", "pip", "install", "--quiet", "--no-input", wheel],
            check=True,
        )
        found["install_seconds"] = round(time.perf_counter() - started, 2)
        program = os.path.join(bin_dir(env), exe("anything2mcp"))
        found["program_median_ms"] = round(statistics.median(milliseconds([program, "--version"])), 1)
        # The short name is a script on macOS and Linux; Windows has a .cmd
        # that only a shell runs, and pip makes no launcher for it.
        short = os.path.join(bin_dir(env), "a2m")
        if os.name != "nt":
            found["short_name_median_ms"] = round(statistics.median(milliseconds([short, "--version"])), 1)
        # The client finds the program its own distribution installed.
        said = subprocess.run(
            [python, "-c", "import anything2mcp; print(anything2mcp.binary_path())"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        found["client_finds_program"] = os.path.samefile(said, program)
    return found


def main(arguments):
    out = None
    if "--out" in arguments:
        at = arguments.index("--out")
        out = arguments[at + 1]
        del arguments[at : at + 2]
    if len(arguments) != 1:
        print(__doc__)
        return 1
    found = try_wheel(arguments[0])
    text = json.dumps(found, indent=2)
    print(text)
    if out:
        with open(out, "w", encoding="utf-8") as handle:
            handle.write(text + "\n")
    return 0 if found["client_finds_program"] else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
