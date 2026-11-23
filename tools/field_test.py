#!/usr/bin/env python3
"""Run one field test of the product against existing software that we run ourselves.

A field test starts one system from a container image pinned by digest, loads
invented data into it, runs the product's commands against it, asks the
system's own store what it holds, and writes a report. The steps and what each
must show come from a recipe, a JSON file kept outside this repository
(decision 0294): a recipe names the software it starts, and this repository
names no real application (rule 9). This file names none.

    python3 tools/field_test.py <recipe folder> [--binary <path>] [--home <path>] [--keep]

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
    "fault":   {"method", "path", "do", "times", "seconds", "bytes"}
                                      (needs the proxy) the next `times` requests
                                      (default 1) of that method whose path matches
                                      the regular expression `path` meet `do`: pass,
                                      delay (`seconds`), refuse, drop_request,
                                      drop_response, or cut_response (`bytes`); see
                                      tools/field_proxy.py. `"fault": "clear"` clears
                                      the rules and the proxy's record
    "proxied": {}                     (needs the proxy) shows the proxy's record, one
                                      line per request: `METHOD path -> rule-or-none ->
                                      status-or-none`, so `contains` and `lacks` can
                                      expect that a request was or was not sent

`expect` may hold `ok` (true or false), `code` (the failure code of a product
command), `status` (of a request), `contains` and `lacks` (text in the output), and
`rows` (of a query). A step with `gap` expects what the product does today
where that is not what it should do; the report lists these apart, and such a
step failing means the gap has closed and the recipe is to be updated.

Text in `{braces}` is filled in: `{system}` the system's address on the run's closed
network (its container's name and port), `{host}` that name, `{base}` the address the
product's own traffic uses (the system's, or the proxy's when there is one), `{port}` the
system's port, `{project}` the project folder, `{data}` the system's data folder,
`{recipe}` the recipe folder, `{direct}` an address of the system that the runner itself
can reach.

The closed network (decision 0310, point 5). A run makes a docker network with
`--internal`, whose name is unique to the run, and starts the system on it with no
published port. Every `a2m` step runs the product's binary inside a second container on
the same network: a small Debian image pinned by digest, the binary mounted read-only, and
the engine's home and the project folder mounted at the same paths as on this machine. The
binary is built for glibc and needs a glibc at least as new as the machine that built it,
which is why the image is a recent one. Before the first `a2m` step the runner tries, from
inside that container, to reach two public addresses, and stops the run as
`closed_network_open` when either answers.

The runner itself, which is the run's control and not the product, reaches the system
through its address on the network (`{direct}`): the steps `http`, `sqlite`, `exec`, and
`python` run on this machine and never carry the product's traffic.

A recipe may say `"proxy": true` under `"system"`. The runner then starts a fault proxy
(tools/field_proxy.py) in a third container on the same network, from a Python image pinned
by digest, and `{base}` is the proxy's address in the steps that carry the product's own
traffic (`a2m`, `replace`), so the product reaches the system through the proxy. Steps
that talk straight to the system (`http`, `sqlite`, `exec`, `python`) have `{base}` filled
with the system's own address, so that seeding and checking never go through the proxy;
`{direct}` is that address in every step. The runner sets the proxy's rules by running its
control client inside the proxy's container. Without `"proxy": true` nothing changes and
`{direct}` equals the system's own address.

Recipes written when the system was published on a port of this machine keep working with
no change, provided they name the system by `{base}`, `{direct}`, `{system}`, or `{host}`.
A recipe that built an address from `127.0.0.1` and `{port}` does not: `{port}` is now the
system's port on the network, and nothing is published on this machine. Change such an
address to `{system}`.

When the run ends, every container and the network it made are removed, unless `--keep`,
which leaves them running and prints their names so they can be removed.
"""

import argparse
import base64
import glob
import json
import os
import re
import secrets
import shutil
import sqlite3
import subprocess
import sys
import time
import urllib.error
import urllib.request

import field_proxy

# The two images a run adds to the system's own, each pinned by digest. The first holds the
# product's binary; the second holds the fault proxy. A glibc at least as new as the
# build machine's is what the binary needs.
BOX_IMAGE = "debian@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2"
PROXY_IMAGE = "python@sha256:e48f59cdba69a3cbd9ed2cf0f348d90d45a8d350d98134cf1070bc106ee9ed90"
# Addresses on the public internet. From inside the closed network neither may answer.
PUBLIC_ADDRESSES = (("1.1.1.1", 443), ("8.8.8.8", 53))
BOX_BINARY = "/usr/local/bin/anything2mcp"
PROXY_PORT = 8080
CONTROL_PORT = 9099
RUN_LABEL = "a2m-field-run"

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
NO_PROXY = urllib.request.build_opener(urllib.request.ProxyHandler({}))


class CannotRun(Exception):
    """The field test could not be run at all."""


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


class Run:
    """What one field test made on the container engine: a closed network and the
    containers on it. Everything carries the run's label, so that the end of the run can
    remove all of it and nothing else."""

    def __init__(self, recipe_name, token=None):
        self.token = token or secrets.token_hex(4)
        stem = re.sub(r"[^a-zA-Z0-9_.-]", "-", recipe_name)
        self.network = f"field-{stem}-{self.token}"
        self.system = f"{self.network}-system"
        self.box = f"{self.network}-box"
        self.proxy = f"{self.network}-proxy"
        self.label = f"{RUN_LABEL}={self.token}"
        self.box_up = False

    def network_argv(self):
        return ["docker", "network", "create", "--internal", "--label", self.label, self.network]

    def system_argv(self, system, data, env):
        """The system's container: on the closed network, with no published port."""
        argv = ["docker", "run", "-d", "--name", self.system, "--network", self.network, "--label", self.label]
        if "volume" in system:
            argv += ["-v", f"{data}:{system['volume']}"]
        for key, value in env.items():
            argv += ["-e", f"{key}={value}"]
        return argv + [system["image"]]

    def box_argv(self, binary, home, project, user):
        """The container the product's binary runs in: the binary read-only, and the
        engine's home and the project folder at the paths they have on this machine."""
        return [
            "docker", "run", "-d", "--init", "--name", self.box, "--network", self.network,
            "--label", self.label, "--user", user, "-e", f"HOME={home}",
            "-v", f"{binary}:{BOX_BINARY}:ro", "-v", f"{home}:{home}", "-v", f"{project}:{project}",
            BOX_IMAGE, "sleep", "infinity",
        ]

    def proxy_argv(self, system_port):
        script = os.path.join(os.path.dirname(os.path.abspath(__file__)), "field_proxy.py")
        return [
            "docker", "run", "-d", "--name", self.proxy, "--network", self.network,
            "--label", self.label, "-v", f"{script}:/field_proxy.py:ro", PROXY_IMAGE,
            "python", "/field_proxy.py", "--upstream", f"{self.system}:{system_port}",
            "--listen", f"0.0.0.0:{PROXY_PORT}", "--control", f"127.0.0.1:{CONTROL_PORT}",
        ]

    def ensure_box(self, binary, home, project, user, execute=None):
        """Start the binary's container and check that the network is closed, once, before
        the first command of the product."""
        execute = execute or run
        if self.box_up:
            return
        code, _, err = execute(self.box_argv(binary, home, project, user))
        if code != 0:
            raise CannotRun(f"The container for the product's binary did not start: {err.strip()[-400:]}")
        self.box_up = True
        check_closed(self.box, execute)

    def exec_argv(self, env, cwd, argv):
        """One command of the product, inside the binary's container."""
        flags = []
        for key, value in env.items():
            flags += ["-e", f"{key}={value}"]
        # A command starts with the umask a person's shell has. The engine's container may
        # start commands with none, and the engine refuses a project file others may change.
        return ["docker", "exec", "-w", cwd, *flags, self.box, "sh", "-c", 'umask 022; exec "$0" "$@"', BOX_BINARY, *argv]

    def remove(self):
        """Remove every container of the run, then its network. Returns what was left."""
        left = []
        _, out, _ = run(["docker", "ps", "-aq", "--filter", f"label={self.label}"])
        ids = out.split()
        if ids and run(["docker", "rm", "-f", "-v", *ids])[0] != 0:
            left.append("containers")
        if run(["docker", "network", "rm", self.network])[0] != 0:
            if run(["docker", "network", "inspect", self.network])[0] == 0:
                left.append(f"network {self.network}")
        return left


def product_env(env):
    """The part of the environment a command of the product gets inside its container."""
    return {k: v for k, v in env.items() if k.startswith("A2M_") and k != "A2M_BINARY"}


def reached(code, err):
    """Whether a try at a public address got an answer. A refusal is an answer: something
    on the way sent it. Unreachable, no route, and a timeout are not."""
    return code == 0 or "refused" in err.lower() or "reset" in err.lower()


def check_closed(box, execute=None):
    """Raise CannotRun, by name, when a public address can be reached from the box."""
    execute = execute or run
    for host, port in PUBLIC_ADDRESSES:
        code, _, err = execute(
            ["docker", "exec", box, "timeout", "4", "bash", "-c", f"exec 3<>/dev/tcp/{host}/{port}"]
        )
        if reached(code, err):
            raise CannotRun(
                f"closed_network_open: {host}:{port} can be reached from inside the run's network; "
                "no a2m step was run. The network is not closed."
            )


class ProxyBox:
    """The fault proxy, in a container on the run's network. Its rules are set by running
    its control client inside the container."""

    def __init__(self, name):
        self.name = name

    def _argv(self, message):
        return ["docker", "exec", self.name, "python", "/field_proxy.py",
                "--control", f"127.0.0.1:{CONTROL_PORT}", "--send", json.dumps(message)]

    def send(self, message):
        code, out, err = run(self._argv(message))
        if code != 0:
            raise CannotRun(f"The fault proxy did not answer: {(out + err).strip()[-300:]}")
        return json.loads(out)

    def wait_ready(self, seconds=30):
        until = time.time() + seconds
        while time.time() < until:
            if run(self._argv({"op": "record"}))[0] == 0:
                return
            time.sleep(0.5)
        raise CannotRun("The fault proxy did not start.")


def do_step(step, names, env, active, proxy=None):
    """Carry one step out. Returns what it showed, as a dict."""
    if "fault" in step or "proxied" in step:
        if proxy is None:
            raise CannotRun('A fault or proxied step needs `"proxy": true` under `"system"`.')
        if "proxied" in step:
            return {"ok": True, "output": field_proxy.render_record(proxy.send({"op": "record"})["record"])}
        spec = step["fault"]
        if spec == "clear":
            proxy.send({"op": "reset"})
            return {"ok": True, "output": "cleared"}
        proxy.send(
            {
                "op": "add_rule",
                "method": spec.get("method", "GET"),
                "path": spec["path"],
                "do": spec["do"],
                "times": spec.get("times", 1),
                "seconds": spec.get("seconds", 1),
                "bytes": spec.get("bytes", 0),
            }
        )
        return {"ok": True, "output": f"{spec['do']}: the next {spec.get('times', 1)} {spec.get('method', 'GET')} {spec['path']}"}
    if "exec" in step:
        code, out, err = run(["docker", "exec", *step.get("as", []), active.system, *step["exec"]])
        return {"ok": code == 0, "output": (out + err)[-2000:]}
    if "http" in step:
        status, body = request(names["direct"], step["http"])
        return {"ok": 200 <= status < 300, "status": status, "output": body[:2000]}
    if "a2m" in step:
        active.ensure_box(env["A2M_BINARY"], env["A2M_HOME"], names["project"], f"{os.getuid()}:{os.getgid()}")
        code, out, err = run(active.exec_argv(product_env(env), names["project"], step["a2m"]))
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
    if "lacks" in expect and expect["lacks"] in shown.get("output", ""):
        return f"the output contains {expect['lacks']!r}"
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
    active = Run(recipe["name"])
    names = {
        "recipe": recipe_dir,
        "project": os.path.join(run_dir, "project"),
        "data": os.path.join(run_dir, "data"),
        "port": str(system["port"]),
        "host": active.system,
    }
    names["system"] = f"http://{active.system}:{system['port']}"
    names["base"] = names["system"]
    for folder in (names["project"], names["data"]):
        os.makedirs(folder)
    home = os.path.abspath(args.home) if args.home else os.path.join(run_dir, "home")
    os.makedirs(home, exist_ok=True)
    env = dict(os.environ)
    env.update(
        A2M_HOME=home,
        A2M_VAULT_PASSPHRASE="an invented passphrase for field tests",
        A2M_BINARY=os.path.abspath(args.binary),
    )

    proxy = None
    report = {"field_test": recipe["name"], "image": system["image"], "steps": []}
    failed = 0
    try:
        code, _, err = run(active.network_argv())
        if code != 0:
            raise CannotRun(f"The closed network was not made: {err.strip()[-400:]}")
        code, _, err = run(active.system_argv(system, names["data"], fill(system.get("env", {}), names)))
        if code != 0:
            raise CannotRun(f"The system did not start: {err.strip()[-400:]}")
        _, address, _ = run(
            ["docker", "inspect", active.system, "--format",
             "{{(index .NetworkSettings.Networks \"" + active.network + "\").IPAddress}}"]
        )
        names["direct"] = f"http://{address.strip()}:{system['port']}"
        wait_ready(names["direct"], system["ready"])
        if system.get("proxy"):
            code, _, err = run(active.proxy_argv(system["port"]))
            if code != 0:
                raise CannotRun(f"The fault proxy did not start: {err.strip()[-400:]}")
            proxy = ProxyBox(active.proxy)
            proxy.wait_ready()
            names["base"] = f"http://{active.proxy}:{PROXY_PORT}"
        for number, raw in enumerate(recipe["steps"], 1):
            straight = any(kind in raw for kind in ("http", "sqlite", "exec", "python"))
            step = fill(raw, {**names, "base": names["direct"]} if straight else names)
            shown = do_step(step, names, env, active, proxy)
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
        if active.box_up and os.path.isdir(home):
            run(active.exec_argv(product_env(env), names["project"], ["engine", "stop"]), seconds=60)
        if args.keep:
            print(f"left running: network {active.network}; containers {active.system}, {active.box}, {active.proxy}")
        else:
            left = active.remove()
            if left:
                print(f"NOT REMOVED, remove by hand: {', '.join(left)} (label {active.label})")
                report["not_removed"] = left
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
