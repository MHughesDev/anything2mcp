#!/usr/bin/env python3
"""Run one field test of the product against existing software that we run ourselves.

A field test starts one system from a container image pinned by digest, loads
invented data into it, runs the product's commands against it, asks the
system's own store what it holds, and writes a report. The steps and what each
must show come from a recipe, a JSON file kept outside this repository
(decision 0294): a recipe names the software it starts, and this repository
names no real application (rule 9). This file names none.

    python3 tools/field_test.py <recipe folder> [--binary <path>] [--keep]

The recipe folder holds `recipe.json` and any files its steps use. Each run
gets a new folder under `<recipe folder>/runs/`, with the project, the
engine's home, the system's data, and `report.json`.

Exit code 0 when every step showed what the recipe expects, 1 when one did
not, 2 when the field test could not be run.

Recipe, in short:

    {"name": "...",
     "system": {"image": "name@sha256:...", "port": 3000, "volume": "/data",
                "env": {"KEY": "value"}, "ready": "/path/that/answers/200"},
     "steps": [ <step>, ... ]}

A step is an object with one of these keys, and optionally `expect`, `gap`,
and `note`:

    "exec":    [argv]                 a program inside the system's container
    "http":    {"method", "path", "json", "basic"}   a request sent straight
                                      to the system, never through the product
    "a2m":     [argv]                 one command of the product
    "copy":    {"from", "to"}         a file of the recipe folder into the project
    "python":  [script, args...]      a script of the recipe folder
    "replace": {"glob", "pattern", "with"}   a hand edit of the project's files
    "sqlite":  {"file", "query"}      a read of a database file under the
                                      system's data folder, opened read-only

`expect` may hold `ok` (true or false), `code` (the failure code of a product
command), `status` (of a request), `contains` (text in the output), and
`rows` (of a query). A step with `gap` expects what the product does today
where that is not what it should do; the report lists these apart, and such a
step failing means the gap has closed and the recipe is to be updated.

Text in `{braces}` is filled in: `{base}` the system's address, `{port}` its
port, `{project}` the project folder, `{data}` the system's data folder,
`{recipe}` the recipe folder.
"""

import argparse
import base64
import glob
import json
import os
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import time
import urllib.error
import urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NO_PROXY = urllib.request.build_opener(urllib.request.ProxyHandler({}))


class CannotRun(Exception):
    """The field test could not be run at all."""


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def fill(value, names):
    """Fill `{name}` in every string of a JSON value."""
    if isinstance(value, str):
        for name, text in names.items():
            value = value.replace("{" + name + "}", text)
        return value
    if isinstance(value, list):
        return [fill(item, names) for item in value]
    if isinstance(value, dict):
        return {key: fill(item, names) for key, item in value.items()}
    return value


def run(argv, env=None, cwd=None, seconds=300):
    done = subprocess.run(
        argv, env=env, cwd=cwd, capture_output=True, text=True, timeout=seconds, check=False
    )
    return done.returncode, done.stdout, done.stderr


def request(base, spec):
    data = None
    headers = {}
    if "json" in spec:
        data = json.dumps(spec["json"]).encode()
        headers["Content-Type"] = "application/json"
    if "basic" in spec:
        headers["Authorization"] = "Basic " + base64.b64encode(spec["basic"].encode()).decode()
    call = urllib.request.Request(
        base.rstrip("/") + spec["path"], data=data, headers=headers, method=spec.get("method", "GET")
    )
    try:
        with NO_PROXY.open(call, timeout=30) as answer:
            return answer.status, answer.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as refused:
        return refused.code, refused.read().decode("utf-8", "replace")


def first_json(text):
    """The first line of a command's output that is a JSON object."""
    for line in text.splitlines():
        if line.startswith("{"):
            try:
                return json.loads(line)
            except ValueError:
                continue
    return None


def do_step(step, names, env, container):
    """Carry one step out. Returns what it showed, as a dict."""
    if "exec" in step:
        code, out, err = run(["docker", "exec", *step.get("as", []), container, *step["exec"]])
        return {"ok": code == 0, "output": (out + err)[-2000:]}
    if "http" in step:
        status, body = request(names["base"], step["http"])
        return {"ok": 200 <= status < 300, "status": status, "output": body[:2000]}
    if "a2m" in step:
        code, out, err = run([env["A2M_BINARY"], *step["a2m"]], env=env, cwd=names["project"])
        answer = first_json(out) or {}
        error = answer.get("error") or {}
        return {
            "ok": bool(answer.get("ok")) and code == 0,
            "code": error.get("code"),
            "class": error.get("class"),
            "output": out + err,
        }
    if "copy" in step:
        target = os.path.join(names["project"], step["copy"]["to"])
        os.makedirs(os.path.dirname(target), exist_ok=True)
        shutil.copyfile(os.path.join(names["recipe"], step["copy"]["from"]), target)
        return {"ok": True, "output": ""}
    if "python" in step:
        script = os.path.join(names["recipe"], step["python"][0])
        code, out, err = run([sys.executable, "-I", script, *step["python"][1:]], cwd=names["recipe"])
        return {"ok": code == 0, "output": (out + err)[-2000:]}
    if "replace" in step:
        spec = step["replace"]
        changed = 0
        for path in glob.glob(os.path.join(names["project"], spec["glob"])):
            with open(path, encoding="utf-8") as handle:
                before = handle.read()
            after = re.sub(spec["pattern"], spec["with"], before, flags=re.MULTILINE)
            if after != before:
                with open(path, "w", encoding="utf-8") as handle:
                    handle.write(after)
                changed += 1
        return {"ok": changed > 0, "output": f"{changed} files changed"}
    if "sqlite" in step:
        path = os.path.join(names["data"], step["sqlite"]["file"])
        with sqlite3.connect(f"file:{path}?mode=ro", uri=True) as store:
            rows = [list(row) for row in store.execute(step["sqlite"]["query"]).fetchall()]
        return {"ok": True, "rows": rows, "output": json.dumps(rows)[:2000]}
    raise CannotRun(f"A step has no kind this runner knows: {sorted(step)}")


def met(expect, shown):
    """Why a step did not show what was expected, or None when it did."""
    for key in ("ok", "code", "status", "rows"):
        if key in expect and shown.get(key) != expect[key]:
            return f"`{key}` was {shown.get(key)!r}, expected {expect[key]!r}"
    if "contains" in expect and expect["contains"] not in shown.get("output", ""):
        return f"the output does not contain {expect['contains']!r}"
    return None


def wait_ready(base, path, seconds=90):
    until = time.time() + seconds
    while time.time() < until:
        try:
            status, _ = request(base, {"path": path})
            if status == 200:
                return
        except OSError:
            pass
        time.sleep(1)
    raise CannotRun(f"The system did not answer 200 at {path} within {seconds} seconds.")


def main():
    parser = argparse.ArgumentParser(description="Run one field test against software we run ourselves.")
    parser.add_argument("recipe", help="the recipe folder, outside this repository")
    parser.add_argument("--binary", default=os.path.join(REPO, "target", "debug", "anything2mcp"))
    parser.add_argument("--keep", action="store_true", help="leave the system running afterwards")
    parser.add_argument(
        "--home",
        help="the engine's home for this run; give a short path where the run folder is deep, "
        "because the engine's socket lives under it and a socket's path is short",
    )
    args = parser.parse_args()

    recipe_dir = os.path.abspath(args.recipe)
    if os.path.commonpath([recipe_dir, REPO]) == REPO:
        print("A recipe names real software and is kept outside this repository (rule 9).")
        return 2
    with open(os.path.join(recipe_dir, "recipe.json"), encoding="utf-8") as handle:
        recipe = json.load(handle)
    if not os.path.isfile(args.binary):
        print(f"No product binary at {args.binary}; build it first.")
        return 2
    if run(["docker", "info"])[0] != 0:
        print("The container engine is not running; start it, then run the field test again.")
        return 2
    system = recipe["system"]
    if "@sha256:" not in system["image"]:
        print("The recipe's image is not pinned by digest.")
        return 2

    run_dir = os.path.join(recipe_dir, "runs", time.strftime("%Y%m%d-%H%M%S"))
    names = {
        "recipe": recipe_dir,
        "project": os.path.join(run_dir, "project"),
        "data": os.path.join(run_dir, "data"),
        "port": str(free_port()),
    }
    names["base"] = f"http://127.0.0.1:{names['port']}"
    for folder in (names["project"], names["data"]):
        os.makedirs(folder)
    env = dict(os.environ)
    env.update(
        A2M_HOME=os.path.abspath(args.home) if args.home else os.path.join(run_dir, "home"),
        A2M_VAULT_PASSPHRASE="an invented passphrase for field tests",
        A2M_BINARY=args.binary,
        NO_PROXY="127.0.0.1,localhost",
        no_proxy="127.0.0.1,localhost",
    )
    container = f"field-{recipe['name']}-{names['port']}"
    start = ["docker", "run", "-d", "--name", container, "-p", f"127.0.0.1:{names['port']}:{system['port']}"]
    if "volume" in system:
        start += ["-v", f"{names['data']}:{system['volume']}"]
    for key, value in fill(system.get("env", {}), names).items():
        start += ["-e", f"{key}={value}"]
    start.append(system["image"])

    report = {"field_test": recipe["name"], "image": system["image"], "steps": []}
    failed = 0
    try:
        code, _, err = run(start)
        if code != 0:
            raise CannotRun(f"The system did not start: {err.strip()[-400:]}")
        wait_ready(names["base"], system["ready"])
        for number, raw in enumerate(recipe["steps"], 1):
            step = fill(raw, names)
            shown = do_step(step, names, env, container)
            why = met(step.get("expect", {"ok": True}), shown)
            entry = {
                "step": number,
                "what": next(key for key in step if key not in ("expect", "gap", "note", "as")),
                "note": step.get("note"),
                "gap": step.get("gap"),
                "passed": why is None,
                "why": why,
                "shown": {**shown, "output": shown.get("output", "")[-4000:]},
            }
            report["steps"].append(entry)
            mark = "ok  " if why is None else "FAIL"
            print(f"{mark} {number:>2}. {step.get('note') or entry['what']}" + (f": {why}" if why else ""))
            if why is not None:
                failed += 1
    except CannotRun as stop:
        print(f"The field test could not be run: {stop}")
        report["could_not_run"] = str(stop)
        failed = -1
    finally:
        if os.path.isdir(env["A2M_HOME"]):
            run([args.binary, "engine", "stop"], env=env, cwd=names["project"], seconds=60)
        if not args.keep:
            run(["docker", "rm", "-f", container])
        gaps = [entry for entry in report["steps"] if entry["gap"]]
        report["gaps"] = [{"step": entry["step"], "gap": entry["gap"], "still_open": entry["passed"]} for entry in gaps]
        with open(os.path.join(run_dir, "report.json"), "w", encoding="utf-8") as handle:
            json.dump(report, handle, indent=1)
    for gap in report["gaps"]:
        state = "still open" if gap["still_open"] else "no longer shows as recorded: closed, or the step broke earlier"
        print(f"gap, step {gap['step']} ({state}): {gap['gap']}")
    print(f"report: {os.path.join(run_dir, 'report.json')}")
    if failed < 0:
        return 2
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
