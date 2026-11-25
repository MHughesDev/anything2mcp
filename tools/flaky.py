#!/usr/bin/env python3
"""Look for flaky tests: run the workspace's tests under CPU load, and the
programs most prone to races several times (decision 0452).

Usage, from the repository root:

    python3 tools/flaky.py [--load N] [--repeat R] [--minutes M] [--focus PKG,...] [-p PKG ...]

It builds the tests as `cargo test --workspace` builds them (or of the
packages `-p` names), starts N processes that each keep one processor busy
(by default one per processor, as the builders' `yes > /dev/null` did by hand),
and then, with the load running:

1. runs the programs of the focus packages (by default those whose tests start
   engines, browsers, child processes, or take locks) once, whatever the time;
2. runs every other test program once, starting from a different program each
   day, while the time M minutes from the start allows;
3. runs the focus programs R more times each, in passes, each repeat started
   only when the time that program took before still fits.

So a run ends near M minutes. On CI's Linux runner the workspace's tests took
about 36 minutes unloaded on 2026-11-25, so a run under load may not reach
every program; those it does not reach are named, and are not failures, and
the daily rotation reaches them on later nights.

Every program is run from its package's folder with the variables cargo gives a
test, as `tools/gate.py` runs them. A failing run is never run again to make it
pass: each failure is reported with its pass, its program, and every test it
named, with the run's output, and the run then fails. A program that does not
end within `PROGRAM_MINUTES` is stopped and reported as hung. The doctests are
not run: they start nothing and hold no lock.

It is the nightly workflow `.github/workflows/flaky.yml`, which no pull request
waits for; it runs anywhere, and is slow.
"""
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gate  # noqa: E402

# The packages whose tests start an engine, a browser, a stand's server or
# another child process, or contend for a lock or a port: where a race under
# load has been found, and where one is most likely.
FOCUS = [
    "a2m-engine",
    "a2m-ledger",
    "a2m-local",
    "a2m-mcp",
    "a2m-pack-web-browser",
    "a2m-pack-web-ui",
    "a2m-provider",
    "a2m-runner",
    "a2m-sched",
    "anything2mcp",
]
PROGRAM_MINUTES = 20


def failed_tests(output):
    """The tests a libtest run names as failed."""
    named = re.findall(r"^test (\S+) \.\.\. FAILED$", output, re.M)
    listed = re.findall(r"^    (\S+)$", output.split("\nfailures:\n")[-1], re.M) if "\nfailures:\n" in output else []
    return sorted(set(named) | set(listed))


def schedule(programs, focus, repeat, day):
    """The runs to make, in order: the focus programs once, every other
    program once from the one `day` picks, then the focus programs again in
    `repeat` passes. Each run is (pass, whether it is a focus program, program)."""
    again = [p for p in programs if p[4] in focus]
    others = [p for p in programs if p[4] not in focus]
    if others:
        start = day % len(others)
        others = others[start:] + others[:start]
    runs = [(1, True, p) for p in again] + [(1, False, p) for p in others]
    for n in range(2, repeat + 2):
        runs += [(n, True, p) for p in again]
    return runs


def fits(pass_number, focused, last_seconds, elapsed, budget):
    """Whether a run may start: a focus program's first run always does;
    another program's first run while there is time left; a repeat only when
    the time it took before still fits."""
    if pass_number == 1:
        return focused or elapsed < budget
    return elapsed + last_seconds <= budget


def start_load(n):
    busy = "while True:\n    pass\n"
    return [subprocess.Popen([sys.executable, "-c", busy], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) for _ in range(n)]


def stop_load(load):
    for p in load:
        p.kill()
    for p in load:
        p.wait()


def option(argv, name, default):
    return argv[argv.index(name) + 1] if name in argv else default


def main(argv):
    started = time.monotonic()
    load_n = int(option(argv, "--load", str(os.cpu_count() or 2)))
    repeat = int(option(argv, "--repeat", "4"))
    budget = float(option(argv, "--minutes", "40")) * 60
    focus = set(option(argv, "--focus", ",".join(FOCUS)).split(","))
    packages = [argv[i + 1] for i, a in enumerate(argv) if a == "-p"]
    env = dict(os.environ)
    env.setdefault("CARGO_TERM_COLOR", "never")
    which = ["-p" + p for p in packages] if packages else ["--workspace"]
    built = subprocess.run(["cargo", "test", *which, "--locked", "--no-run", "--message-format=json-render-diagnostics"], env=env, capture_output=True, text=True)
    if built.returncode != 0:
        print(built.stderr[-6000:])
        print("flaky: the tests did not build")
        return 1
    programs = sorted(gate.built_programs(built.stdout), key=lambda p: p[1])
    unknown = sorted(focus - {p[4] for p in programs}) if not packages else []
    if unknown:
        print(f"flaky: no test program in the focus package(s) {', '.join(unknown)}")
        return 2
    runs = schedule(programs, focus, repeat, int(time.time() // 86400))
    print(f"flaky: {len(programs)} test programs, {len(runs) - len(programs)} repeats of {len({p[1] for n, _, p in runs if n > 1})}, under {load_n} busy processes, within {budget / 60:.0f} minutes", flush=True)
    cargo = env.get("CARGO") or "cargo"
    failures, skipped, last = [], [], {}
    load = start_load(load_n)
    try:
        for n, focused, (_, label, executable, folder, package) in runs:
            if not fits(n, focused, last.get(label, 0), time.monotonic() - started, budget):
                skipped.append((n, label))
                continue
            run_env = {**env, "CARGO": cargo, "CARGO_MANIFEST_DIR": folder, "CARGO_PKG_NAME": package}
            t = time.monotonic()
            try:
                done = subprocess.run([executable], cwd=folder, env=run_env, capture_output=True, text=True, timeout=PROGRAM_MINUTES * 60)
                code, output = done.returncode, done.stdout + done.stderr
            except subprocess.TimeoutExpired as e:
                code = None
                output = (e.stdout or b"").decode("utf-8", "replace") if isinstance(e.stdout, bytes) else (e.stdout or "")
            last[label] = time.monotonic() - t
            verdict = "ok  " if code == 0 else ("HUNG" if code is None else "FAIL")
            print(f"  pass {n} {round(last[label]):>4}s {verdict} {label}", flush=True)
            if code != 0:
                tests = failed_tests(output)
                failures.append((n, label, tests, code))
                print(output[-8000:], flush=True)
    finally:
        stop_load(load)
    unreached = [label for n, label in skipped if n == 1]
    if unreached:
        print(f"flaky: {len(unreached)} program(s) not reached in the time, to be reached on a later day: {', '.join(unreached)}")
    if len(skipped) > len(unreached):
        print(f"flaky: {len(skipped) - len(unreached)} repeat(s) left out for time")
    lines = []
    for n, label, tests, code in failures:
        what = "hung, stopped after %d minutes" % PROGRAM_MINUTES if code is None else f"exit {code}"
        lines.append(f"pass {n}: {label} ({what}): {', '.join(tests) if tests else 'no test named'}")
    for line in lines:
        print(f"FAILED {line}")
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write("## Tests under load\n\n" + ("\n".join(f"- {line}" for line in lines) if lines else "No test failed.") + "\n")
    ran = len(runs) - len(skipped)
    print(f"flaky: {ran} runs in {round((time.monotonic() - started) / 60)} minutes; {len(failures)} failed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
