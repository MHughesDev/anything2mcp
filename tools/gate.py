"""Run the checks of CI's Linux job here, before a push (decision 0372).

Usage: python3 tools/gate.py [--fast] [--shard K/N] [--cross]

Runs, in order, and goes on after a failure so that one run shows every one:
the documentation audit, the failure codes check, formatting, the lints of the
whole workspace, and the tests of the whole workspace. `--fast` stops after the
lints, for a check while work is in progress; a push waits for the full run.

`--shard K/N` splits the run over N machines: shard 1 runs the audits,
formatting, and the lints, and every shard runs the tests of its share of the
workspace's packages, shared out by how many tests each package holds, so that
N machines together run every test once. `--cross` adds the lints of the whole
workspace built for Windows and for macOS from this machine: nothing is
linked, so the two C dependencies are given stand-ins (`tools/cross/`) that
write empty objects, and the lints see the code each of those systems
compiles and Linux never does. It runs no test of those systems.

The build folder is the one CARGO_TARGET_DIR names, as for any cargo command.
Where other copies of the repository build at the same time, give this run a
folder of its own, so that no other copy's build is taken for this one's.

The gate is the same steps CI's Linux job runs, so a change that fails it would
fail CI; it does not replace CI, which still runs them on all three operating
systems with the jobs that need a real server, a display, or a key store.
"""
import json
import os
import re
import subprocess
import sys
import time

STEPS = [
    ("documents", ["python3", "tools/docs_audit.py"]),
    ("failure codes", ["python3", "tools/failure_codes.py", "--check"]),
    ("format", ["cargo", "fmt", "--all", "--check"]),
    ("lint", ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--keep-going", "--", "-D", "warnings"]),
    ("test", ["cargo", "test", "--workspace", "--locked", "--no-fail-fast"]),
]


CROSS_TARGETS = ["x86_64-pc-windows-msvc", "aarch64-apple-darwin"]
HERE = os.path.dirname(os.path.abspath(__file__))


def weights():
    """Each workspace package and how many tests its files hold."""
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        check=True, capture_output=True, text=True,
    ).stdout
    found = {}
    for package in json.loads(out)["packages"]:
        folder = os.path.dirname(package["manifest_path"])
        count = 0
        for root, dirs, files in os.walk(folder):
            dirs[:] = [d for d in dirs if d not in ("target", "node_modules", ".git")]
            for name in files:
                if name.endswith(".rs"):
                    with open(os.path.join(root, name), encoding="utf-8", errors="replace") as f:
                        count += len(re.findall(r"#\[(?:tokio::)?test\b", f.read()))
        found[package["name"]] = count
    return found


def share(k, n):
    """The packages shard K of N tests: the heaviest first, each to the
    lightest shard so far, so that every package is in exactly one shard."""
    bins = [[0, []] for _ in range(n)]
    for name, count in sorted(weights().items(), key=lambda item: (-item[1], item[0])):
        lightest = min(range(n), key=lambda i: (bins[i][0], i))
        bins[lightest][0] += max(count, 1)
        bins[lightest][1].append(name)
    return sorted(bins[k - 1][1])


def cross_steps():
    steps = []
    for target in CROSS_TARGETS:
        under = target.replace("-", "_")
        steps.append((
            f"lint for {target}",
            ["cargo", "clippy", "--workspace", "--all-targets", "--locked", "--keep-going", "--target", target, "--", "-D", "warnings"],
            {f"CC_{under}": os.path.join(HERE, "cross", "nocc.py"), f"AR_{under}": os.path.join(HERE, "cross", "noar.py")},
        ))
    return steps


def run(name, command, env):
    started = time.monotonic()
    done = subprocess.run(command, env=env, capture_output=True, text=True)
    output = done.stdout + done.stderr
    seconds = round(time.monotonic() - started)
    ok = done.returncode == 0
    if name == "documents" and not re.search(r"^0 finding\(s\)", output, re.M):
        ok = False
    failed = []
    if name == "test":
        failed = sorted(set(re.findall(r"^    (\S+)$", output, re.M)))
        failed += re.findall(r"^\s+`(-p \S+ --\S+(?: \S+)?)`$", output, re.M)
    return ok, seconds, output, failed


def main(argv):
    env = dict(os.environ)
    env.setdefault("CARGO_PROFILE_DEV_DEBUG", "line-tables-only")
    env.setdefault("CARGO_TERM_COLOR", "never")
    steps = [(name, command, {}) for name, command in (STEPS[:4] if "--fast" in argv else STEPS)]
    if "--shard" in argv:
        k, n = (int(x) for x in argv[argv.index("--shard") + 1].split("/"))
        if not 1 <= k <= n:
            print("--shard takes K/N with 1 <= K <= N")
            return 2
        packages = share(k, n)
        print(f"shard {k} of {n} tests {len(packages)} packages: {' '.join(packages)}", flush=True)
        test = ["cargo", "test", "--locked", "--no-fail-fast"] + [a for p in packages for a in ("-p", p)]
        steps = [step for step in steps[:4] if k == 1] + ([("test", test, {})] if "--fast" not in argv else [])
    if "--cross" in argv:
        steps += cross_steps()
    results = []
    for name, command, extra in steps:
        ok, seconds, output, failed = run(name, command, {**env, **extra})
        results.append((name, ok, seconds))
        print(f"{'ok  ' if ok else 'FAIL'} {name} ({seconds}s)", flush=True)
        if not ok:
            lines = [l for l in output.splitlines() if l.strip()]
            for line in (failed or lines[-30:]):
                print(f"     {line}")
    red = [name for name, ok, _ in results if not ok]
    print("gate: green" if not red else f"gate: red ({', '.join(red)})")
    return 1 if red else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
