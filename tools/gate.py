"""Run the checks of CI's Linux job here, before a push (decision 0372).

Usage: python3 tools/gate.py [--fast] [--shard K/N [--tests-only] [--record FILE]] [--cross]
       python3 tools/gate.py --list-tests FILE
       python3 tools/gate.py --compare-tests FULL SHARD...

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

CI's Rust jobs run the tests the same way (decision 0999): each system's
tests are shared over three jobs, each running `--shard K/3 --tests-only
--record FILE`, which builds and runs its share alone and writes, for every test
program it ran, the tests that program holds. `--list-tests FILE` writes the
same for the whole workspace, as `cargo test --workspace -- --list` names every
program and test, and `--compare-tests FULL SHARD...` fails on a test the shards
left out, ran twice, or ran though the full list has no such test, so that the
shards together are proven to be the one run they stand in for.

The gate is the same steps CI's Linux job runs, so a change that fails it would
fail CI; it does not replace CI, which still runs them on all three operating
systems with the jobs that need a real server, a display, or a key store.
"""
import json
import os
import re
import shutil
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


def shard_tests(k, n, env, verbose=False):
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
        return False, round(time.monotonic() - started), built.stderr, [], []
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
    output, failed, ran = [], [], []
    # Cargo gives the tests it runs the program that runs them; a test that
    # starts cargo again finds it there.
    cargo = env.get("CARGO") or shutil.which("cargo") or "cargo"
    for _, label, executable, folder, package in mine:
        run_env = {**env, "CARGO": cargo, "CARGO_MANIFEST_DIR": folder, "CARGO_PKG_NAME": package}
        listed = subprocess.run([executable, "--list", "--format", "terse"], cwd=folder, env=run_env, capture_output=True, text=True)
        if listed.returncode != 0:
            failed.append(f"{label} (its tests could not be listed)")
            output.append(listed.stdout[-4000:] + listed.stderr[-2000:])
        ran += [(program_key(executable), name) for name in listed_names(listed.stdout)]
        program_started = time.monotonic()
        done = subprocess.run([executable], cwd=folder, env=run_env, capture_output=True, text=True)
        if verbose:
            print(f"  {round(time.monotonic() - program_started):>4}s {'ok  ' if done.returncode == 0 else 'FAIL'} {label}", flush=True)
        if done.returncode != 0:
            failed.append(label)
            failed += [f"  {t}" for t in re.findall(r"^    (\S+)$", done.stdout, re.M)][:20]
            if verbose:
                print(done.stdout + done.stderr, flush=True)
            output.append(done.stdout[-4000:] + done.stderr[-2000:])
    if k == 1:
        quiet = {**env, "CARGO_TERM_COLOR": "never"}
        listed = subprocess.run(["cargo", "test", "--workspace", "--locked", "--doc", "--", "--list", "--format", "terse"], env=quiet, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        if listed.returncode != 0:
            failed.append("doctests (they could not be listed)")
            output.append(listed.stdout[-4000:])
        ran += parse_listing(listed.stdout)
        doc = subprocess.run(["cargo", "test", "--workspace", "--locked", "--doc", "--no-fail-fast"], env=env, capture_output=True, text=True)
        if doc.returncode != 0:
            failed.append("doctests")
            if verbose:
                print(doc.stdout + doc.stderr, flush=True)
            output.append(doc.stdout[-4000:] + doc.stderr[-2000:])
    return not failed, round(time.monotonic() - started), "\n".join(output), failed, ran


ANSI = re.compile(r"\x1b\[[0-9;]*m")


def program_key(executable):
    """The name a test program is known by in the lists the shards and the
    full run write: its file name as cargo built it, hash included and
    `.exe` left off, so that two programs of one name in two packages differ."""
    name = re.split(r"[\\/]", executable)[-1]
    return name[:-4] if name.endswith(".exe") else name


def listed_names(text):
    """The tests one program's `--list --format terse` names."""
    return [m.group(1) for m in re.finditer(r"^(.+): (?:test|bench)$", ANSI.sub("", text), re.M)]


def parse_listing(text):
    """Each (program, test) a `cargo test ... -- --list --format terse` run
    names, with cargo's own lines and the programs' lists in one stream: a
    `Running` line names the program the lines after it list, and a
    `Doc-tests` line a package's doctests."""
    entries, program = [], None
    for line in ANSI.sub("", text).splitlines():
        running = re.match(r"^\s*Running (?:unittests )?.*\((.+)\)\s*$", line)
        doc = re.match(r"^\s*Doc-tests (\S+)\s*$", line)
        test = re.match(r"^(.+): (?:test|bench)$", line)
        if running:
            program = program_key(running.group(1))
        elif doc:
            program = f"doctests {doc.group(1)}"
        elif test:
            if program is None:
                raise ValueError(f"a test is listed before any program: {line}")
            entries.append((program, test.group(1)))
    return entries


def write_record(path, entries):
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        for program, name in sorted(entries):
            f.write(f"{program}\t{name}\n")


def read_record(path):
    with open(path, encoding="utf-8") as f:
        return [tuple(line.rstrip("\n").split("\t", 1)) for line in f if line.strip()]


def list_tests(path, env):
    """Write every test of the workspace, as one `cargo test --workspace`
    names them (decision 0999), and say how many."""
    done = subprocess.run(
        ["cargo", "test", "--workspace", "--locked", "--no-fail-fast", "--", "--list", "--format", "terse"],
        env={**env, "CARGO_TERM_COLOR": "never"}, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    if done.returncode != 0:
        print(done.stdout[-6000:])
        print("the workspace's tests could not be listed")
        return 1
    entries = parse_listing(done.stdout)
    write_record(path, entries)
    print(f"{len(entries)} tests in {len({p for p, _ in entries})} test programs listed in {path}")
    return 0 if entries else 1


def compare_tests(full, shards):
    """What is wrong with the shards' records against the full list: each
    test the shards left out, each they ran more than once, and each they
    ran that the full list has not got. Empty when they match."""
    from collections import Counter
    want = Counter(full)
    got = Counter()
    for record in shards:
        got.update(record)
    problems = []
    if not want:
        problems.append("the full list holds no test")
    for entry, count in sorted(want.items()):
        if got[entry] < count:
            problems.append(f"missing: {entry[0]} {entry[1]}")
        elif got[entry] > count:
            problems.append(f"run {got[entry]} times: {entry[0]} {entry[1]}")
    for entry in sorted(set(got) - set(want)):
        problems.append(f"not in the full list: {entry[0]} {entry[1]}")
    return problems


def compare_main(paths):
    if len(paths) < 2:
        print("--compare-tests takes the full list and at least one shard's record")
        return 2
    missing = [p for p in paths if not os.path.isfile(p)]
    if missing:
        for p in missing:
            print(f"no record at {p}: the job that writes it did not pass")
        return 1
    full = read_record(paths[0])
    shards = [read_record(p) for p in paths[1:]]
    problems = compare_tests(full, shards)
    for line in problems[:200]:
        print(line)
    if problems:
        print(f"{len(problems)} problem(s): the shards are not the one run of the workspace's tests")
        return 1
    print(f"the {len(shards)} shards ran each of the {len(full)} tests of the workspace exactly once")
    return 0


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
    if "--list-tests" in argv:
        return list_tests(argv[argv.index("--list-tests") + 1], env)
    if "--compare-tests" in argv:
        return compare_main(argv[argv.index("--compare-tests") + 1:])
    before_tests = [step for step in STEPS if step[0] != "test"]
    steps = [(name, command, {}) for name, command in (before_tests if "--fast" in argv else STEPS)]
    if "--shard" in argv:
        k, n = (int(x) for x in argv[argv.index("--shard") + 1].split("/"))
        if not 1 <= k <= n:
            print("--shard takes K/N with 1 <= K <= N")
            return 2
        # CI runs the steps before the tests once per system in a job of their
        # own, and its shards run the tests alone (decision 0999).
        first = k == 1 and "--tests-only" not in argv
        steps = [step for step in steps if step[0] != "test" and first] + ([("test", ("shard", k, n), {})] if "--fast" not in argv else [])
    if "--cross" in argv:
        steps += cross_steps()
    steps = [(TOOLCHAIN[0], TOOLCHAIN[1], {})] + steps
    results = []
    for name, command, extra in steps:
        if command[0] == "shard":
            ok, seconds, output, failed, ran = shard_tests(command[1], command[2], {**env, **extra}, verbose="--tests-only" in argv)
            if "--record" in argv:
                write_record(argv[argv.index("--record") + 1], ran)
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
