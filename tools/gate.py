"""Run the checks of CI's Linux job here, before a push (decision 0372).

Usage: python3 tools/gate.py [--fast] [--shard K/N] [--cross]

Runs, in order, and goes on after a failure so that one run shows every one:
the documentation audit, the failure codes check, the reading of every workflow
file as YAML, the check against every kept release's contract (decision 0437),
formatting, the lints of the whole workspace, and the tests of the whole
workspace. `--fast` stops after the lints, for a check while work is in
progress; a push waits for the full run.

`--shard K/N` splits the run over N machines: shard 1 runs the audits,
formatting, the lints, and the doctests; every shard builds the tests of the
whole workspace, with the features the whole workspace unifies as CI's run
does, and runs its share of the test programs, shared out by how many tests
each holds, so that N machines together run every test program once. `--cross` adds the lints of the whole
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

# The toolchain rust-toolchain.toml pins, with its components, as CI's first
# step installs it: a machine that lacks rustfmt or clippy gets them here.
TOOLCHAIN = ("toolchain", ["rustup", "toolchain", "install"])

STEPS = [
    ("documents", ["python3", "tools/docs_audit.py"]),
    ("failure codes", ["python3", "tools/failure_codes.py", "--check"]),
    ("workflows", ["python3", "tools/check_workflows.py"]),
    ("contracts", ["python3", "tools/contracts.py", "check"]),
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


def shard_tests(k, n, env):
    """Run shard K of N of the workspace's tests, as `cargo test --workspace`
    builds them: every shard builds all of them with the features the whole
    workspace unifies, as CI does, so that no shard tests a build CI never
    makes; then it runs its share of the test programs, each from its
    package's folder with the variables cargo gives a test, and shard 1 also
    runs the doctests."""
    started = time.monotonic()
    built = subprocess.run(
        ["cargo", "test", "--workspace", "--locked", "--no-run", "--message-format=json-render-diagnostics"],
        env=env, capture_output=True, text=True,
    )
    if built.returncode != 0:
        return False, round(time.monotonic() - started), built.stderr, []
    programs = []
    for line in built.stdout.splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("reason") != "compiler-artifact" or not message.get("executable") or not message.get("profile", {}).get("test"):
            continue
        target = message["target"]
        manifest = message["manifest_path"]
        source = target["src_path"]
        count = program_weight(target["kind"], source)
        name = message["package_id"].split("#")[-1].split("@")[0].rsplit("/", 1)[-1]
        programs.append((count, f"{name} {target['kind'][0]} {target['name']}", message["executable"], os.path.dirname(manifest), name))
    bins = [[0, []] for _ in range(n)]
    for program in sorted(programs, key=lambda p: (-p[0], p[1])):
        lightest = min(range(n), key=lambda i: (bins[i][0], i))
        bins[lightest][0] += max(program[0], 1)
        bins[lightest][1].append(program)
    mine = sorted(bins[k - 1][1], key=lambda p: p[1])
    print(f"shard {k} of {n} runs {len(mine)} of {len(programs)} test programs", flush=True)
    output, failed = [], []
    for _, label, executable, folder, package in mine:
        run_env = {**env, "CARGO_MANIFEST_DIR": folder, "CARGO_PKG_NAME": package}
        done = subprocess.run([executable], cwd=folder, env=run_env, capture_output=True, text=True)
        if done.returncode != 0:
            failed.append(label)
            failed += [f"  {t}" for t in re.findall(r"^    (\S+)$", done.stdout, re.M)][:20]
            output.append(done.stdout[-4000:] + done.stderr[-2000:])
    if k == 1:
        doc = subprocess.run(["cargo", "test", "--workspace", "--locked", "--doc", "--no-fail-fast"], env=env, capture_output=True, text=True)
        if doc.returncode != 0:
            failed.append("doctests")
            output.append(doc.stdout[-4000:] + doc.stderr[-2000:])
    return not failed, round(time.monotonic() - started), "\n".join(output), failed


def program_weight(kinds, source):
    """How many tests a test program holds, for sharing programs out: the
    tests of its source file, or, for a library, a program, or a test target
    whose source is a `main.rs` and whose other files are its modules
    (decision 0446), the tests of the whole folder that file is in."""
    try:
        with open(source, encoding="utf-8", errors="replace") as f:
            count = len(re.findall(r"#\[(?:tokio::)?test\b", f.read()))
    except OSError:
        count = 0
    if "lib" in kinds or "bin" in kinds or os.path.basename(source) == "main.rs":
        count = max(count, weights_of_folder(os.path.dirname(source)))
    return count


def weights_of_folder(folder):
    count = 0
    for root, dirs, files in os.walk(folder):
        dirs[:] = [d for d in dirs if d not in ("target", "node_modules", ".git")]
        for name in files:
            if name.endswith(".rs"):
                with open(os.path.join(root, name), encoding="utf-8", errors="replace") as f:
                    count += len(re.findall(r"#\[(?:tokio::)?test\b", f.read()))
    return count


def cross_steps():
    # The two targets' standard libraries, which a machine set up for Linux
    # alone has not got.
    steps = [("cross targets", ["rustup", "target", "add"] + CROSS_TARGETS, {})]
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
    env.setdefault("CARGO_TERM_COLOR", "never")
    before_tests = [step for step in STEPS if step[0] != "test"]
    steps = [(name, command, {}) for name, command in (before_tests if "--fast" in argv else STEPS)]
    if "--shard" in argv:
        k, n = (int(x) for x in argv[argv.index("--shard") + 1].split("/"))
        if not 1 <= k <= n:
            print("--shard takes K/N with 1 <= K <= N")
            return 2
        steps = [step for step in steps if step[0] != "test" and k == 1] + ([("test", ("shard", k, n), {})] if "--fast" not in argv else [])
    if "--cross" in argv:
        steps += cross_steps()
    steps = [(TOOLCHAIN[0], TOOLCHAIN[1], {})] + steps
    results = []
    for name, command, extra in steps:
        if command[0] == "shard":
            ok, seconds, output, failed = shard_tests(command[1], command[2], {**env, **extra})
        else:
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
