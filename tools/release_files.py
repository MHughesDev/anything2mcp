#!/usr/bin/env python3
"""Make, gather, and check the files of a release, and check that the release workflow builds them all.

Usage, from the repository root:

    python3 tools/release_files.py archive --target T --binary FILE --version V --out DIR [--mtime EPOCH]
    python3 tools/release_files.py assemble --version V --pip DIR --npm DIR --archives DIR --out DIR
    python3 tools/release_files.py check DIR --version V
    python3 tools/release_files.py changelog V          # exit 1 unless CHANGELOG.md is ready to release V
    python3 tools/release_files.py notes V              # print the changelog's section for V
    python3 tools/release_files.py workflow             # exit 1 unless release.yml builds and gates every piece
    python3 tools/release_files.py ci RUNS.json SHA     # exit 1 unless CI passed on SHA, from GitHub's list of runs
    ... --root DIR                                      # work on another copy of the repository

`archive` makes the standalone archive of one target from the binary taken out of that target's wheel,
so the archive, the wheel, and the npm package carry one binary, built once (spec section 605): a
`.tar.gz` for Linux and macOS holding `anything2mcp`, `a2m` as a link to it, `LICENSE`, and
`THIRD-PARTY-NOTICES.txt`, and a `.zip` for Windows holding `anything2mcp.exe`, the wheel's `a2m.cmd`,
and the same two texts. Members are sorted, owned by nobody, and dated `--mtime`, so one binary gives
one archive byte for byte.

`assemble` gathers what the release page carries into one folder: the eight archives, the eight wheels
and the source distribution, the nine npm packages, the bill of materials in CycloneDX that maturin
writes into each wheel (taken out as `anything2mcp-V-<target>.cdx.json`), the notices file, and
`SHA256SUMS` over all of them. `check` holds that folder to the set: each file present and none other;
`SHA256SUMS` naming each file once with its digest; every wheel, platform package, and archive carrying
`LICENSE` and the notices file as they are in this tree; one binary in each, and for each target the
same binary in its wheel, its npm package, and its archive.

`workflow` reads `.github/workflows/release.yml` and fails when a job of the release is missing, when a
matrix leaves out a target or adds one, when the pull-request paths leave out a file the release is
built from, or when any job that may write anywhere (a registry, the release page, an attestation) is
not one of the publish jobs, gated on `decide` saying to publish and run in a GitHub environment, after
every file of the release has been built and checked, or when the `decide` job does not ask GitHub, with
`actions: read` held by that job alone, whether CI passed on the commit, after deciding to publish.

`ci` reads the list of runs GitHub gives for `GET /repos/OWNER/REPO/actions/workflows/ci.yml/runs` with
`head_sha=SHA` and `branch=main`, and passes only when the newest run on `main` of that very commit,
started by a push or by hand (a run of a pull request tests its merge ref, another commit), is completed
with the conclusion `success`. Standard library only; Python 3.11 or newer.
"""
import gzip
import hashlib
import io
import json
import re
import shutil
import sys
import tarfile
import time
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import check_packages  # noqa: E402
import check_workflows  # noqa: E402

PROGRAM = "anything2mcp"
NOTICES = "THIRD-PARTY-NOTICES.txt"
A2M_CMD = "bindings/python/wheel-data/scripts/a2m.cmd"
# The eight targets of the release: Rust target, npm platform key, and the wheel's platform tag.
TARGETS = {
    "x86_64-unknown-linux-gnu": ("linux-x64", r"manylinux\w*_x86_64"),
    "aarch64-unknown-linux-gnu": ("linux-arm64", r"manylinux\w*_aarch64"),
    "x86_64-unknown-linux-musl": ("linux-x64-musl", r"musllinux\w*_x86_64"),
    "aarch64-unknown-linux-musl": ("linux-arm64-musl", r"musllinux\w*_aarch64"),
    "x86_64-apple-darwin": ("darwin-x64", r"macosx\w*_x86_64"),
    "aarch64-apple-darwin": ("darwin-arm64", r"macosx\w*_arm64"),
    "x86_64-pc-windows-msvc": ("win32-x64", r"win_amd64"),
    "aarch64-pc-windows-msvc": ("win32-arm64", r"win_arm64"),
}
WORKFLOW = ".github/workflows/release.yml"
# Every job of the release; a missing one is a fault.
JOBS = ("decide", "wheels", "source", "pip-package", "npm-platform", "npm-launcher", "npm-package",
        "bill-of-materials", "archives", "release-files", "publish-release", "publish-pypi", "publish-npm")
# The only jobs that may write outside the run, and what each must wait for besides `decide`.
PUBLISH = {
    "publish-release": {"release-files"},
    "publish-pypi": {"pip-package", "publish-release"},
    "publish-npm": {"npm-package", "publish-release"},
}
GATE = "needs.decide.outputs.publish == 'true'"
# What the release is built from; a pull request that changes one runs the workflow.
PATHS = ("bindings/python/**", "bindings/npm/**", WORKFLOW, NOTICES, "tools/release_files.py",
         "tools/check_packages.py")
# What `decide` holds, and what its step that asks for CI's result must carry.
DECIDE_PERMISSIONS = {"contents": "read", "actions": "read"}
CI_STEP = ("actions/workflows/ci.yml/runs", 'head_sha="$GITHUB_SHA"', "-f branch=main",
           "tools/release_files.py ci", "exit 1")
CI_STEP_ENV = {"GH_TOKEN": "${{ github.token }}", "PUBLISH": "${{ steps.decide.outputs.publish }}"}
# The runs of CI that test the commit itself: on main, started by a push or by hand (decision 0372).
CI_EVENTS = ("push", "workflow_dispatch")
DEFAULT_MTIME = 315532800  # 1980-01-01, the earliest a zip can date a member


def is_windows(target):
    return "windows" in target


def archive_name(version, target):
    return f"{PROGRAM}-{version}-{target}" + (".zip" if is_windows(target) else ".tar.gz")


def sha256(data):
    return hashlib.sha256(data).hexdigest()


# ---- archive ------------------------------------------------------------------------------------


def make_archive(root, target, binary, version, out, mtime=DEFAULT_MTIME):
    if target not in TARGETS:
        raise SystemExit(f"error: no target {target}; known: {', '.join(TARGETS)}")
    top = f"{PROGRAM}-{version}-{target}"
    licence = (root / "LICENSE").read_bytes()
    notices = (root / NOTICES).read_bytes()
    program = Path(binary).read_bytes()
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    path = out / archive_name(version, target)
    if is_windows(target):
        members = [(f"{PROGRAM}.exe", program, 0o755), ("a2m.cmd", (root / A2M_CMD).read_bytes(), 0o644),
                   ("LICENSE", licence, 0o644), (NOTICES, notices, 0o644)]
        stamp = tuple(time.gmtime(max(mtime, DEFAULT_MTIME))[:6])
        with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
            for name, data, mode in sorted(members):
                info = zipfile.ZipInfo(f"{top}/{name}", stamp)
                info.external_attr = (0o100000 | mode) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                z.writestr(info, data)
    else:
        members = [(PROGRAM, program, 0o755), ("LICENSE", licence, 0o644), (NOTICES, notices, 0o644)]
        raw = io.BytesIO()
        with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as tar:
            entries = [(name, data, mode, None) for name, data, mode in members] + [("a2m", b"", 0o777, PROGRAM)]
            for name, data, mode, link in sorted(entries):
                info = tarfile.TarInfo(f"{top}/{name}")
                info.mtime, info.mode, info.uid, info.gid, info.uname, info.gname = mtime, mode, 0, 0, "", ""
                if link:
                    info.type, info.linkname = tarfile.SYMTYPE, link
                    tar.addfile(info)
                else:
                    info.size = len(data)
                    tar.addfile(info, io.BytesIO(data))
        with open(path, "wb") as handle, gzip.GzipFile(filename="", mode="wb", fileobj=handle, mtime=0) as gz:
            gz.write(raw.getvalue())
    return path


def archive_members(path):
    """{name: (kind, bytes or link target, mode)} of an archive's members."""
    found = {}
    if path.name.endswith(".zip"):
        with zipfile.ZipFile(path) as z:
            for info in z.infolist():
                if not info.is_dir():
                    found[info.filename] = ("file", z.read(info), (info.external_attr >> 16) & 0o777)
    else:
        with tarfile.open(path, "r:gz") as tar:
            for m in tar.getmembers():
                if m.isfile():
                    found[m.name] = ("file", tar.extractfile(m).read(), m.mode & 0o777)
                elif m.issym():
                    found[m.name] = ("link", m.linkname, m.mode & 0o777)
                elif not m.isdir():
                    found[m.name] = ("other", b"", 0)
    return found


# ---- assemble and check ---------------------------------------------------------------------------


def expected_names(version):
    names = {archive_name(version, t) for t in TARGETS}
    names |= {f"{PROGRAM}-{version}-{t}.cdx.json" for t in TARGETS}
    names |= {f"{PROGRAM}-{p}-{version}.tgz" for p, _ in TARGETS.values()}
    names |= {f"{PROGRAM}-{version}.tgz", f"{PROGRAM}-{version}.tar.gz", NOTICES}
    return names


def wheel_target(name, version):
    m = re.fullmatch(rf"{PROGRAM}-{re.escape(version)}-py3-none-(\S+)\.whl", name)
    if not m:
        return None
    hits = [t for t, (_, tag) in TARGETS.items() if re.fullmatch(rf"(\w+\.)*{tag}(\.\w+)*", m.group(1))]
    return hits[0] if len(hits) == 1 else None


def write_sums(folder):
    lines = [f"{sha256((folder / n).read_bytes())}  {n}" for n in sorted(p.name for p in folder.iterdir())
             if n != "SHA256SUMS"]
    (folder / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def assemble(root, version, pip, npm, archives, out):
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    for folder in (pip, npm, archives):
        for f in sorted(Path(folder).iterdir()):
            if f.is_file():
                shutil.copy2(f, out / f.name)
    for wheel in sorted(out.glob("*.whl")):
        target = wheel_target(wheel.name, version)
        if target is None:
            raise SystemExit(f"error: {wheel.name} is no wheel of the eight targets at version {version}")
        with zipfile.ZipFile(wheel) as z:
            boms = [n for n in z.namelist() if re.search(r"\.dist-info/sboms/[^/]+\.cyclonedx\.json$", n)]
            if len(boms) != 1:
                raise SystemExit(f"error: {wheel.name} holds {len(boms)} CycloneDX bills of materials; expected one")
            (out / f"{PROGRAM}-{version}-{target}.cdx.json").write_bytes(z.read(boms[0]))
    shutil.copy2(root / NOTICES, out / NOTICES)
    write_sums(out)


def check(root, folder, version):
    folder = Path(folder)
    faults = []
    licence, notices = (root / "LICENSE").read_bytes(), (root / NOTICES).read_bytes()
    present = {p.name for p in folder.iterdir() if p.is_file()}
    wheels = {n for n in present if n.endswith(".whl")}
    by_target = {}
    for n in sorted(wheels):
        t = wheel_target(n, version)
        if t is None:
            faults.append(f"{n} is no wheel of the eight targets at version {version}")
        elif t in by_target:
            faults.append(f"two wheels for {t}: {by_target[t]} and {n}")
        else:
            by_target[t] = n
    for t in TARGETS:
        if t not in by_target:
            faults.append(f"no wheel for {t}")
    wanted = expected_names(version) | wheels | {"SHA256SUMS"}
    for n in sorted(wanted - present - wheels):
        faults.append(f"{n} is missing")
    for n in sorted(present - wanted):
        faults.append(f"{n} is not a file of the release")
    if (folder / NOTICES).is_file() and (folder / NOTICES).read_bytes() != notices:
        faults.append(f"{NOTICES} is not the tree's")
    # The checksums.
    sums = folder / "SHA256SUMS"
    if sums.is_file():
        listed = {}
        for line in sums.read_text(encoding="utf-8").splitlines():
            m = re.fullmatch(r"([0-9a-f]{64})  (\S+)", line)
            if not m:
                faults.append(f"SHA256SUMS has a line this does not read: {line!r}")
                continue
            if m.group(2) in listed:
                faults.append(f"SHA256SUMS names {m.group(2)} twice")
            listed[m.group(2)] = m.group(1)
        for n in sorted(present - {"SHA256SUMS"}):
            if n not in listed:
                faults.append(f"SHA256SUMS does not name {n}")
            elif listed[n] != sha256((folder / n).read_bytes()):
                faults.append(f"the digest of {n} in SHA256SUMS is not its own")
        for n in sorted(set(listed) - present):
            faults.append(f"SHA256SUMS names {n}, which is not in the release")
    # One binary per target, the same in its three packages, each beside the licence texts.
    for target, (platform, _) in TARGETS.items():
        exe = ".exe" if is_windows(target) else ""
        binaries = {}
        wheel = by_target.get(target)
        if wheel:
            with zipfile.ZipFile(folder / wheel) as z:
                names = z.namelist()
                faults += [f"{wheel}: {f}" for f in check_packages.check("wheel", check_packages.wheel_files(folder / wheel))]
                for want, data in (("LICENSE", licence), (NOTICES, notices)):
                    hit = [n for n in names if n.endswith(f".dist-info/licenses/{want}")]
                    if len(hit) != 1 or z.read(hit[0]) != data:
                        faults.append(f"{wheel} does not carry {want} as it is in the tree")
                program = [n for n in names if n.endswith(f".data/scripts/{PROGRAM}{exe}")]
                if len(program) == 1:
                    binaries["wheel"] = sha256(z.read(program[0]))
        package = folder / f"{PROGRAM}-{platform}-{version}.tgz"
        if package.is_file():
            faults += [f"{package.name}: {f}" for f in check_packages.check("npm", check_packages.tarball_files(package))]
            with tarfile.open(package, "r:gz") as tar:
                members = {m.name: m for m in tar.getmembers() if m.isfile()}
                for want, data in (("LICENSE", licence), (NOTICES, notices)):
                    m = members.get(f"package/{want}")
                    if m is None or tar.extractfile(m).read() != data:
                        faults.append(f"{package.name} does not carry {want} as it is in the tree")
                m = members.get(f"package/bin/{PROGRAM}{exe}")
                if m is not None:
                    binaries["npm package"] = sha256(tar.extractfile(m).read())
        archive = folder / archive_name(version, target)
        if archive.is_file():
            faults += [f"{archive.name}: {f}" for f in check_archive(root, archive, version, target, licence, notices)]
            member = archive_members(archive).get(f"{PROGRAM}-{version}-{target}/{PROGRAM}{exe}")
            if member and member[0] == "file":
                binaries["archive"] = sha256(member[1])
        bom = folder / f"{PROGRAM}-{version}-{target}.cdx.json"
        if bom.is_file():
            try:
                data = json.loads(bom.read_bytes())
                if data.get("bomFormat") != "CycloneDX" or not data.get("components"):
                    faults.append(f"{bom.name} is not a CycloneDX bill of materials with components")
            except json.JSONDecodeError:
                faults.append(f"{bom.name} is not JSON")
        if len(binaries) == 3 and len(set(binaries.values())) != 1:
            faults.append(f"{target}: the wheel, the npm package, and the archive carry different binaries: {binaries}")
        elif wheel and package.is_file() and archive.is_file() and len(binaries) != 3:
            faults.append(f"{target}: the binary was not found in each of its three packages ({sorted(binaries)})")
    return faults


def check_archive(root, path, version, target, licence, notices):
    top = f"{PROGRAM}-{version}-{target}"
    members = archive_members(path)
    faults = []
    if is_windows(target):
        wanted = {f"{PROGRAM}.exe": None, "a2m.cmd": (root / A2M_CMD).read_bytes()}
    else:
        wanted = {PROGRAM: None, "a2m": PROGRAM}
    wanted.update({"LICENSE": licence, NOTICES: notices})
    names = {f"{top}/{n}" for n in wanted}
    for n in sorted(set(members) - names):
        faults.append(f"holds {n}, which an archive does not carry")
    for n, data in wanted.items():
        m = members.get(f"{top}/{n}")
        if m is None:
            faults.append(f"does not hold {top}/{n}")
        elif n == "a2m":
            if m[0] != "link" or m[1] != PROGRAM:
                faults.append(f"{top}/a2m is not a link to {PROGRAM}")
        elif m[0] != "file":
            faults.append(f"{top}/{n} is not a file")
        elif data is not None and m[1] != data:
            faults.append(f"{top}/{n} is not as it is in the tree")
    program = members.get(f"{top}/{PROGRAM}" + (".exe" if is_windows(target) else ""))
    if program and program[0] == "file":
        if not check_packages.is_binary(PROGRAM, program[1][:8]) or len(program[1]) == 0:
            faults.append("the program is not a binary")
        if not is_windows(target) and program[2] & 0o111 != 0o111:
            faults.append("the program is not executable")
    for n, m in members.items():
        if not n.endswith(("/" + PROGRAM, "/" + PROGRAM + ".exe")) and m[0] == "file" \
                and check_packages.is_binary(n, m[1][:8]):
            faults.append(f"{n} is a binary the archive is not to hold")
    return faults


# ---- the changelog --------------------------------------------------------------------------------


def changelog_sections(root):
    text = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    parts = re.split(r"(?m)^## ", text)
    return {p.split("\n", 1)[0].strip(): p.split("\n", 1)[1] if "\n" in p else "" for p in parts[1:]}


def changelog_problems(root, version):
    sections = changelog_sections(root)
    problems = []
    heads = [h for h in sections if re.fullmatch(rf"\[{re.escape(version)}\] - \d{{4}}-\d{{2}}-\d{{2}}", h)]
    if len(heads) != 1:
        problems.append(f"CHANGELOG.md has no section headed `## [{version}] - YYYY-MM-DD`")
    elif not re.search(r"(?m)^- ", sections[heads[0]]):
        problems.append(f"the section for {version} in CHANGELOG.md lists nothing")
    unreleased = sections.get("[Unreleased]")
    if unreleased is None:
        problems.append("CHANGELOG.md has no `## [Unreleased]` section")
    elif re.search(r"(?m)^(### |- )", unreleased):
        problems.append("the Unreleased section of CHANGELOG.md is not empty")
    link = re.search(rf"(?m)^\[{re.escape(version)}\]: \S+", (root / "CHANGELOG.md").read_text(encoding="utf-8"))
    if not link:
        problems.append(f"CHANGELOG.md has no link line `[{version}]: ...` at its foot")
    return problems


def notes(root, version):
    for head, body in changelog_sections(root).items():
        if head.startswith(f"[{version}]"):
            return re.split(r"(?m)^\[[^\]]+\]: ", body)[0].strip() + "\n"
    return None


# ---- the workflow ---------------------------------------------------------------------------------


def matrix_targets(job):
    include = (job.get("strategy") or {}).get("matrix", {}).get("include", [])
    return [entry.get("target") for entry in include if isinstance(entry, dict)]


def workflow_problems(root):
    try:
        flow = check_workflows.read((root / WORKFLOW).read_text(encoding="utf-8"))
    except (OSError, check_workflows.YamlError) as error:
        return [f"{WORKFLOW} does not read: {error}"]
    problems = []
    jobs = flow.get("jobs", {})
    for name in JOBS:
        if name not in jobs:
            problems.append(f"the job `{name}` is missing")
    if flow.get("permissions") != {"contents": "read"}:
        problems.append("the workflow's own permissions are not `contents: read` alone")
    paths = ((flow.get("on") or {}).get("pull_request") or {}).get("paths", [])
    for p in PATHS:
        if p not in paths:
            problems.append(f"a pull request that changes `{p}` does not run the release workflow")
    for name in ("wheels", "npm-platform", "archives"):
        if name in jobs:
            got = matrix_targets(jobs[name])
            if sorted(got) != sorted(TARGETS) or len(got) != len(set(got)):
                problems.append(f"the matrix of `{name}` is not the eight targets once each: {got}")
    if "npm-platform" in jobs:
        for entry in jobs["npm-platform"].get("strategy", {}).get("matrix", {}).get("include", []):
            t = entry.get("target")
            if t in TARGETS and entry.get("platform") != TARGETS[t][0]:
                problems.append(f"`npm-platform` packs {t} as {entry.get('platform')}, not {TARGETS[t][0]}")
    for name, job in jobs.items():
        perms = job.get("permissions") or {}
        writes = sorted(k for k, v in perms.items() if v == "write") if isinstance(perms, dict) else ["(all)"]
        if name not in PUBLISH:
            if writes or (isinstance(perms, str) and perms != "read-all"):
                problems.append(f"`{name}` may write ({', '.join(writes) or perms}) and is not a publish job")
            continue
        if job.get("if") != GATE:
            problems.append(f"`{name}` does not run only when `decide` says to publish (`if: {GATE}`)")
        if not job.get("environment"):
            problems.append(f"`{name}` runs in no GitHub environment, so no person can be asked first")
        needs = job.get("needs") or []
        needs = {needs} if isinstance(needs, str) else set(needs)
        for need in {"decide"} | PUBLISH[name]:
            if need not in needs:
                problems.append(f"`{name}` does not wait for `{need}`")
    for name, job in jobs.items():
        perms = job.get("permissions")
        if name != "decide" and isinstance(perms, dict) and "actions" in perms:
            problems.append(f"`{name}` holds `actions`, which only `decide` may hold")
    if "decide" in jobs:
        problems.extend(decide_problems(jobs["decide"]))
    return problems


def decide_problems(job):
    problems = []
    if job.get("permissions") != DECIDE_PERMISSIONS:
        problems.append("`decide` does not hold `contents: read` and `actions: read` alone, to ask how CI ended")
    steps = [s for s in job.get("steps") or [] if isinstance(s, dict)]
    ids = [s.get("id") for s in steps]
    at = ids.index("decide") if "decide" in ids else len(steps)
    asks = [i for i, s in enumerate(steps)
            if all(part in (s.get("run") or "") for part in CI_STEP) and s.get("env") == CI_STEP_ENV]
    if not asks:
        problems.append("`decide` has no step that asks GitHub whether CI passed on the commit and refuses it if not")
    elif asks[0] < at:
        problems.append("`decide` asks how CI ended before it has decided whether to publish")
    return problems


def ci_verdict(runs, sha):
    """Whether CI passed on `sha`, from GitHub's list of the CI workflow's runs, and why."""
    mine = [r for r in (runs or {}).get("workflow_runs") or []
            if isinstance(r, dict) and r.get("head_sha") == sha and r.get("head_branch") == "main"
            and r.get("event") in CI_EVENTS]
    if not mine:
        return False, f"CI has no run on main for {sha}, by a push or by hand"
    run = max(mine, key=lambda r: (r.get("created_at") or "", r.get("id") or 0))
    where = f"CI's run {run.get('id')} ({run.get('event')}, attempt {run.get('run_attempt', 1)}) on {sha}"
    if run.get("status") != "completed":
        return False, f"{where} is {run.get('status')}, not completed"
    if run.get("conclusion") != "success":
        return False, f"{where} ended {run.get('conclusion')}, not success"
    return True, f"{where} ended success"


# ---- main -----------------------------------------------------------------------------------------


def option(argv, flag, default=None):
    return argv[argv.index(flag) + 1] if flag in argv else default


def main(argv):
    root = Path(option(argv, "--root", HERE.parent)).resolve()
    command = argv[1] if len(argv) > 1 else ""
    if command == "archive":
        path = make_archive(root, option(argv, "--target"), option(argv, "--binary"), option(argv, "--version"),
                            option(argv, "--out"), int(option(argv, "--mtime", DEFAULT_MTIME)))
        print(f"wrote {path} ({sha256(path.read_bytes())})")
        return 0
    if command == "assemble":
        assemble(root, option(argv, "--version"), option(argv, "--pip"), option(argv, "--npm"),
                 option(argv, "--archives"), option(argv, "--out"))
        print(f"gathered the release in {option(argv, '--out')}")
        return 0
    if command == "check":
        faults = check(root, argv[2], option(argv, "--version"))
    elif command == "changelog":
        faults = changelog_problems(root, argv[2])
    elif command == "notes":
        text = notes(root, argv[2])
        if text is None:
            print(f"error: CHANGELOG.md has no section for {argv[2]}")
            return 1
        sys.stdout.write(text)
        return 0
    elif command == "ci":
        try:
            runs = json.loads(Path(argv[2]).read_text(encoding="utf-8"))
        except (OSError, ValueError) as error:
            print(f"error: the list of CI's runs does not read: {error}")
            return 1
        passed, why = ci_verdict(runs, argv[3])
        print(why)
        return 0 if passed else 1
    elif command == "workflow":
        faults = workflow_problems(root)
    else:
        print(__doc__.strip().split("\n\n")[1])
        return 2
    for fault in faults:
        print(f"- {fault}")
    print(f"{command}: {len(faults)} problem(s)")
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
