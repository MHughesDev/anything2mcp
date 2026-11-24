"""Run the checks of CI's Linux job here, before a push (decision 0372).

Usage: python3 tools/gate.py [--fast]

Runs, in order, and goes on after a failure so that one run shows every one:
the documentation audit, the failure codes check, formatting, the lints of the
whole workspace, and the tests of the whole workspace. `--fast` stops after the
lints, for a check while work is in progress; a push waits for the full run.

The build folder is the one CARGO_TARGET_DIR names, as for any cargo command.
Where other copies of the repository build at the same time, give this run a
folder of its own, so that no other copy's build is taken for this one's.

The gate is the same steps CI's Linux job runs, so a change that fails it would
fail CI; it does not replace CI, which still runs them on all three operating
systems with the jobs that need a real server, a display, or a key store.
"""
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
    steps = STEPS[:4] if "--fast" in argv else STEPS
    results = []
    for name, command in steps:
        ok, seconds, output, failed = run(name, command, env)
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
