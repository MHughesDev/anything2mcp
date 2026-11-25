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
    "http":    {"method", "path", "json", "basic", "headers"}   a request sent
                                      straight to the system, never through the product
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
    "person":  {"link", "approve", "send"}   the person's press on the engine's own page,
                                      with the field harness's prompt answered: the
                                      runner opens `link` (an approval's or an act's
                                      page, from an earlier step's answer), presses
                                      Approve or Carry out, reads the prompt the harness
                                      prints, and writes on its standard input the short
                                      hash of `approve`, the change's hash or the act's
                                      identifier the recipe means to pass. `"send":
                                      "other"` writes another hash instead, to show that
                                      a wrong answer passes nothing. `"signs_in": {role:
                                      value}` types each value into the page's field for
                                      that credential's role, as a person signs in on
                                      the page of `session.login`
    "unseen":  "name"                 the secret kept as `{name}` is in no file under the
                                      engine's home, the project, or the harness's log
    "sandboxed": {}                   a browser runs in the box with its sandbox: at least
                                      one renderer, each in a user namespace that is not the
                                      box's own, and no process of the box carrying a flag
                                      that switches the sandbox off
    "desktop": {}                     (needs `box_desktop`) the applications the box's
                                      accessibility registry lists, one line each:
                                      `A2M_APPLICATION <bus name> <program> <name>`, asked of
                                      the bus by the runner and never through the product

`expect` may hold `ok` (true or false), `code` (the failure code of a product
command), `status` (of a request), `contains` and `lacks` (text in the output),
`count` (`{"text", "times"}`: the text is in the output exactly that many times), and
`rows` (of a query). A step of the product may say `keep`, a map from a name to a
dotted path into its answer, such as `{"run": "result.run_id"}`; later steps use the
value as `{run}`. An `http` step may say `secret`, `{"name", "path"}`: the value at that
dotted path of the system's answer, such as a token the system issued, is kept as
`{name}` and is a secret of the run. What every step showed is cleared of each secret
before it is printed or written to the report, and the `http` step that made it shows
nothing of its answer. A step with `gap` expects what the product does today
where that is not what it should do; the report lists these apart, and such a
step failing means the gap has closed and the recipe is to be updated.

Text in `{braces}` is filled in: `{system}` the system's address on the run's closed
network (its container's name and port), `{host}` that name, `{base}` the address the
product's own traffic uses (the system's, or the proxy's when there is one), `{port}` the
system's port, `{project}` the project folder, `{data}` the system's data folder,
`{recipe}` the recipe folder, `{home}` the engine's home, `{direct}` an address of the system that the runner itself
can reach. `{direct}` exists only after the system has started, so it cannot be used in
`system.env`: a recipe that does so is refused before anything is started.

The closed network (decision 0310, point 5). A run makes a docker network with
`--internal`, whose name is unique to the run, and starts the system on it with no
published port. Every `a2m` step runs the product's binary inside a second container on
the same network: a small Debian image pinned by digest, the binary mounted read-only, and
the engine's home and the project folder mounted at the same paths as on this machine. The
binary is built for glibc and needs a glibc at least as new as the machine that built it,
which is why the image is a recent one. Before the first `a2m` step the runner probes,
from inside that container, three public addresses (two IPv4 and one IPv6) and resolves one
public name. Each probe must print an explicit verdict: CLOSED only on a connect timeout,
"network unreachable", "no route", or a name the resolver does not find; OPEN when an address
answers, refuses, or resets, or the name resolves. The runner stops the run as
`closed_network_open` when any probe says OPEN, and as `closed_network_unproven` when any
probe gives no verdict or an unrecognised one (no shell in the image, the container not
running, `docker exec` failing). Only a CLOSED from every probe lets the run go on.

Where it runs. The runner itself reaches the system by the container's address on the
`--internal` network, so it works only where this machine can reach a container's address
on such a network: Linux with docker. It does not work on Docker Desktop, where containers
live in a virtual machine and their addresses are not reachable from the host.

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

The field harness (decision 0310). The engine of every `a2m` step is the field harness,
`a2m-field-harness serve`, not the product's own `serve`: the product's binary runs each
command as a thin client of it. Before anything is started the runner builds the harness
from this repository into a build folder of its own, under `--harness-build` (the system's
temporary folder unless given), with a cargo target folder of its own, and deletes the
folder when the run ends, so that the harness is never left behind or packaged. The
harness's binary is mounted read-only in the box beside the product's. After the
closed-network check the runner starts it in the box with `docker exec -i`, holds its
standard input, reads its standard output, and waits for it to say that it serves. Before
every later `a2m` step it checks that the harness still runs, because a command that finds
no engine starts the product's own. Only a `person` step writes on the harness's standard
input, and it writes only the short hash the recipe means to pass. The vault's passphrase
reaches the harness in its environment; the harness is never given
`--passphrase-from-stdin`, because its standard input carries the prompt's answers. What the
harness writes on its standard error is kept in the run folder as `harness.log`.

Recipes written when the system was published on a port of this machine keep working with
no change, provided they name the system by `{base}`, `{direct}`, `{system}`, or `{host}`.
A recipe that built an address from `127.0.0.1` and `{port}` does not: `{port}` is now the
system's port on the network, and nothing is published on this machine. Change such an
address to `{system}`.

More things a recipe may say under `"system"`, for a system that is not a request
service:

- `"ready": {"exec": [argv]}` in place of a path: the system is ready when that program,
  run inside its container, exits 0. For a system that answers no HTTP request, such as a
  database server.
- `"network": "box"`: the system's container shares the network of the product's box
  (`--network container:<box>`), so that the product reaches it on the box's loopback address,
  127.0.0.1, as the product sees a system on the person's own machine. The box is then started,
  and the closed-network check made, before the system. `{host}` is `127.0.0.1`, `{system}` is
  `http://127.0.0.1:<port>`, and `{direct}` is the box's address on the closed network. It
  cannot be combined with `"proxy": true`, whose upstream is the system's container by name.
- `"mounts": {"/path/inside": "file of the recipe folder"}` and `"command": [argv]`: files the
  system's container is given read-only, and the argument list after its image. With them a
  release binary pinned by digest, which the recipe holds, runs in the box's own image.
- `"box_mounts": {"/path/inside": "file of the recipe folder"}`: files the product's box is given
  read-only, such as a command-line program or a local MCP server's program that the product
  starts on its own machine. A path inside a container is absolute, and a file stays inside the
  recipe folder; a recipe that breaks either is refused before anything is started.
- `"harness_features": ["<package>/<feature>", ...]`: features of workspace packages that the
  field harness is built with, such as a store a pack speaks to only behind a feature. The
  report states them, because a run with them is not a run of the product as it is shipped.

The box's three options (decision 0447), for a browser in the box:

- `"box_image": "name@sha256:<64 hex>"`: the image the box runs in place of the runner's own,
  named by its digest and no other way, such as one that holds a browser at one of the web
  pack's known places. Its entry point is never run. Its glibc must be at least as new as
  the one the product's binary was built against, and it must hold `sh`, `bash`, `timeout`,
  and `getent`, or the closed-network check cannot prove the network closed and stops.
- `"box_user": "uid:gid"`: the user the box runs as, in numbers, never root (`0`). The
  runner gives it the engine's home and the project, before the first step and after every
  step that writes in them from this machine, which only a runner that is root can do; a
  runner that is not root may name only its own user. Without it the box runs as the
  runner's user, as before, which is root on a machine where the runner is.
- `"box_seccomp": "browser_sandbox"`: the box's system-call filter, by a name from the
  runner's own list, `BOX_FILTERS`. `browser_sandbox` is the container engine's default
  filter, kept in `tools/field-box/seccomp-default.json` and checked against its hash, plus
  two rules: `clone` may make a user, a process, and a network namespace, and `unshare` a
  user namespace, and neither any other. It is refused for a box whose user is root. No name
  means unconfined, and a recipe cannot give a filter of its own.

Whatever the options, a recipe that holds a flag that switches a browser's sandbox off
(`SANDBOX_OFF`), in its own text or in a file it gives the box, is refused before anything
is started. The report states the box's image, user, and filter.

The box's desktop session (decision 0459), for a desktop application in the box:

- `"box_desktop": {"application": ["/absolute/program", "argument", ...]}`: with the box, at
  the first `a2m` or `desktop` step and before the field harness starts, the runner starts in
  the box, as the box's user, the session script kept in `tools/field-box/desktop-session.sh`
  (mounted read-only): a virtual display that listens on no TCP port, a session bus, and the
  accessibility bus, each a socket in a folder of the box's own, and then the application,
  which the recipe names and the script does not. The runner waits, at most `DESKTOP_WAIT`
  seconds, until the accessibility registry lists an application whose process runs that
  program, and stops the run as `desktop_not_ready` otherwise. The harness and every command
  of the product get the session's `DISPLAY` and `DBUS_SESSION_BUS_ADDRESS`, as a person's
  shell has them, and nothing else of it. It needs a `box_image`, which must hold `Xvfb`,
  `dbus-daemon`, `dbus-send`, `/usr/libexec/at-spi-bus-launcher`, and the application, and a
  passwd entry for the box's user, without which the session bus does not start; and a
  `box_user` that is not root, since a desktop session is a person's. Nothing of this
  machine's own display or buses reaches the box, and the session widens the box by nothing:
  no device, socket, or option is added but the script's one read-only file. The report states the application and the script's hash.

A recipe whose system is the box's own desktop application says `"system": {"kind": "desktop"}`
with `box_desktop`, and names no image: no system's container is started. The steps `exec`
and `http`, and a proxy, are refused in such a recipe, as for a folder.

A recipe whose system is a folder of files, not a program, says `"system": {"kind": "folder"}`
and names no image: no system's container is started, and the folder is made by the recipe's
own steps (`python`, `copy`) under `{project}` or `{data}`. The steps `exec` and `http`, and a
proxy, need a system's container and are refused in such a recipe.

When the run ends, every container and the network it made are removed, unless `--keep`,
which leaves them running and prints their names so they can be removed.
"""

import argparse
import base64
import glob
import hashlib
import html
import json
import os
import queue
import re
import secrets
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

import field_proxy

# The two images a run adds to the system's own, each pinned by digest. The first holds the
# product's binary; the second holds the fault proxy. A glibc at least as new as the
# build machine's is what the binary needs.
BOX_IMAGE = "debian@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2"
PROXY_IMAGE = "python@sha256:e48f59cdba69a3cbd9ed2cf0f348d90d45a8d350d98134cf1070bc106ee9ed90"
# Addresses on the public internet, IPv4 and IPv6. From inside the closed network none may answer.
PUBLIC_ADDRESSES = (("1.1.1.1", 443), ("8.8.8.8", 53), ("2606:4700:4700::1111", 443))
# A public name. From inside the closed network it must not resolve.
PUBLIC_NAME = "one.one.one.one"
BOX_BINARY = "/usr/local/bin/anything2mcp"
BOX_HARNESS = "/usr/local/bin/a2m-field-harness"
HARNESS_PACKAGE = "a2m-field-harness"
# What the harness writes on its standard error once its engine serves.
HARNESS_READY = "the local engine is ready"
# How long the runner waits for the harness to serve, and for a prompt after a press.
HARNESS_WAIT = 120
PROMPT_WAIT = 60
# The characters of a hash or an act's identifier that a prompt shows.
SHORT_HASH = 12
PROXY_PORT = 8080
CONTROL_PORT = 9099
RUN_LABEL = "a2m-field-run"

# A feature a recipe may build the field harness with: a workspace package and its feature.
FEATURE = re.compile(r"^[a-z0-9][a-z0-9_-]*/[a-z0-9][a-z0-9_-]*$")

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# The box's three options (decision 0447). An image in place of BOX_IMAGE is named by its
# digest, in this form and no other.
PINNED_IMAGE = re.compile(r"^[a-z0-9][a-z0-9._/:-]*@sha256:[0-9a-f]{64}$")
# A box user is numbers, `uid:gid`, so that the runner can give it the engine's home and the
# project without asking the image who a name is.
BOX_USER = re.compile(r"^([0-9]{1,9}):([0-9]{1,9})$")
# The container engine's own default system-call filter, kept unchanged in the repository,
# and its hash: a file that no longer has it is refused, so that the base cannot be widened
# by an edit nobody sees. tools/field-box/NOTICE says where it came from.
SECCOMP_DEFAULT = os.path.join(REPO, "tools", "field-box", "seccomp-default.json")
SECCOMP_DEFAULT_SHA256 = "3320527d4568265ac97a077609b1bfd4cfd0d789eeb9239b7e094b1526675406"
# The flags of clone and unshare that make a namespace, and the three the browser's sandbox
# makes: a user namespace, and in it a process and a network namespace.
CLONE_NAMESPACES = 0x7E020000
CLONE_NEWUSER = 0x10000000
CLONE_NEWPID = 0x20000000
CLONE_NEWNET = 0x40000000


def namespace_rule(name, allowed):
    """A rule that lets `name` make the namespaces in `allowed` and no other: the call is let
    through when its flags, masked by every other namespace flag, are zero. Not on s390,
    whose clone takes its flags in another place."""
    return {
        "names": [name],
        "action": "SCMP_ACT_ALLOW",
        "args": [{"index": 0, "value": CLONE_NAMESPACES & ~allowed, "valueTwo": 0, "op": "SCMP_CMP_MASKED_EQ"}],
        "excludes": {"caps": ["CAP_SYS_ADMIN"], "arches": ["s390", "s390x"]},
        "comment": f"decision 0447: {name} may make only the namespaces the browser's sandbox makes",
    }


# The named filters a recipe may give the box, each the default above plus these rules and
# nothing else. Measured on 2026-11-25 (decision 0447): the browser's sandbox starts under the
# default plus these two, and under the default plus any one of them alone, or with clone
# kept from the process or the network namespace, it does not.
BOX_FILTERS = {
    "browser_sandbox": [
        namespace_rule("clone", CLONE_NEWUSER | CLONE_NEWPID | CLONE_NEWNET),
        namespace_rule("unshare", CLONE_NEWUSER),
    ],
}
# Flags that switch a browser's sandbox off, in whole or in part. A recipe that holds one, in
# its own text or in a file it gives the box, is refused: the product never passes one
# (rule 5), and a recipe cannot pass one for it, through a wrapper program for one.
SANDBOX_OFF = (
    "--no-sandbox",
    "--disable-setuid-sandbox",
    "--disable-namespace-sandbox",
    "--disable-seccomp-filter-sandbox",
    "--disable-gpu-sandbox",
    "--single-process",
    "--no-zygote",
)
# The box's desktop session (decision 0459): the script kept here, the place it is mounted in
# the box, what the harness and the product's commands are given of it, and how long the
# runner waits for the application to be on the accessibility bus.
DESKTOP_SESSION = os.path.join(REPO, "tools", "field-box", "desktop-session.sh")
BOX_DESKTOP_SESSION = "/usr/local/lib/a2m-field/desktop-session.sh"
DESKTOP_ENV = {"DISPLAY": ":99", "DBUS_SESSION_BUS_ADDRESS": "unix:path=/tmp/a2m-desktop/bus"}
DESKTOP_WAIT = 60
NO_PROXY = urllib.request.build_opener(urllib.request.ProxyHandler({}))


class CannotRun(Exception):
    """The field test could not be run at all."""


def system_env(system, names):
    """The environment of the system's container, filled with the names that exist before it
    starts. `{direct}` is made from the started container's address, so it cannot be here."""
    env = system.get("env", {})
    if "{direct}" in json.dumps(env):
        raise CannotRun(
            "A recipe's `system.env` uses `{direct}`, which exists only after the system has "
            "started. Use `{host}`, `{system}`, or `{port}`, or set the value in a step."
        )
    return fill(env, names)


def file_mounts(mounts, recipe):
    """The `-v` flags that mount files of the recipe folder read-only into a container:
    `mounts` maps a path inside the container to a file of the recipe folder. A path inside
    must be absolute, and a file must be named relative to the recipe folder and stay inside
    it, so that a recipe mounts nothing of this machine but its own files."""
    argv = []
    home = os.path.realpath(recipe)
    for inside, source in sorted(mounts.items()):
        if not inside.startswith("/") or ":" in inside:
            raise CannotRun(f"A mount's path inside the container, `{inside}`, is not an absolute path.")
        resolved = os.path.realpath(os.path.join(home, source))
        if os.path.isabs(source) or os.path.commonpath([resolved, home]) != home:
            raise CannotRun(f"A mount's file, `{source}`, is not a file of the recipe folder.")
        if not os.path.isfile(resolved):
            raise CannotRun(f"A mount's file, `{source}`, is not in the recipe folder.")
        argv += ["-v", f"{resolved}:{inside}:ro"]
    return argv


def box_options(system, runner_user):
    """The box's image, user, and system-call filter, from a recipe's `system` (decision 0447),
    as `{"image", "user", "filter"}`. `runner_user` is `uid:gid` of this runner. Each is
    refused, before anything is started, when it is not one the runner allows."""
    image = system.get("box_image", BOX_IMAGE)
    if not isinstance(image, str) or not PINNED_IMAGE.match(image):
        raise CannotRun(f"The recipe's `box_image` is not an image named by its digest, `name@sha256:<64 hex>`: {image!r}")
    user = system.get("box_user", runner_user)
    match = BOX_USER.match(user) if isinstance(user, str) else None
    if not match:
        raise CannotRun(f"The recipe's `box_user` is not `uid:gid` in numbers: {user!r}")
    if "box_user" in system and match.group(1) == "0":
        raise CannotRun("The recipe's `box_user` is root. Name a user that is not, or leave it out.")
    if user != runner_user and not runner_user.startswith("0:"):
        raise CannotRun(
            f"The recipe's `box_user` {user} is not this runner's user {runner_user}, and only a "
            "runner that is root can give the engine's home and the project to another user."
        )
    name = system.get("box_seccomp")
    if name is not None:
        if not isinstance(name, str) or name not in BOX_FILTERS:
            raise CannotRun(
                f"The recipe's `box_seccomp` is {name!r}. A box's filter is named from the "
                f"runner's own list, {sorted(BOX_FILTERS)}; none is unconfined, and a recipe "
                "cannot give a filter of its own."
            )
        if match.group(1) == "0":
            raise CannotRun(
                f"The filter `{name}` is given only to a box whose user is not root; name a "
                "`box_user`."
            )
    return {"image": image, "user": user, "filter": name}


def box_filter(name, default_path=None):
    """The system-call filter of that name: the container engine's default, checked against
    its hash, plus the filter's own rules. Returns the profile, a JSON value."""
    default_path = default_path or SECCOMP_DEFAULT
    with open(default_path, "rb") as handle:
        raw = handle.read()
    if hashlib.sha256(raw).hexdigest() != SECCOMP_DEFAULT_SHA256:
        raise CannotRun(
            f"The default system-call filter {default_path} is not the file the runner was "
            "written for: its hash differs. Restore it, or record its replacement in a decision."
        )
    profile = json.loads(raw)
    profile["syscalls"] = profile["syscalls"] + BOX_FILTERS[name]
    return profile


def desktop_options(system, box):
    """The box's desktop session from a recipe's `system` (decision 0459), as
    `{"application": [argv]}`, or None when the recipe asks for none. `box` is what
    `box_options` gave. Refused, before anything is started, when it is not one the runner
    allows."""
    if system.get("kind") == "desktop" and "box_desktop" not in system:
        raise CannotRun("A recipe whose system is `desktop` names its application under `box_desktop`.")
    if "box_desktop" not in system:
        return None
    spec = system["box_desktop"]
    if not isinstance(spec, dict) or set(spec) != {"application"}:
        raise CannotRun(f"The recipe's `box_desktop` holds `application` and nothing else: {spec!r}")
    argv = spec["application"]
    if (
        not isinstance(argv, list)
        or not argv
        or not all(isinstance(part, str) and part and "\0" not in part for part in argv)
        or not argv[0].startswith("/")
        or "/../" in argv[0] + "/"
    ):
        raise CannotRun(
            "The recipe's `box_desktop.application` is not an argument list whose first part is "
            f"the program's absolute path: {argv!r}"
        )
    if "box_image" not in system:
        raise CannotRun(
            "A desktop session needs a `box_image` that holds a virtual display, the buses, and "
            "the application; the runner's own image holds none of them."
        )
    if "box_user" not in system or box["user"].startswith("0:"):
        raise CannotRun("A desktop session is a person's and never root's; name a `box_user` that is not root.")
    return {"application": argv}


def desktop_hash(path=None):
    """The SHA-256 of the desktop session's script, which the report states."""
    with open(path or DESKTOP_SESSION, "rb") as handle:
        return hashlib.sha256(handle.read()).hexdigest()


def sandbox_off_in(recipe, files=()):
    """Where a flag that switches a browser's sandbox off is found: in the recipe's own text,
    or in a file it gives the box. Returns the places, empty when there is none."""
    found = []
    text = json.dumps(recipe)
    found += [f"the recipe: {flag}" for flag in SANDBOX_OFF if flag in text]
    for path in files:
        with open(path, "rb") as handle:
            content = handle.read()
        found += [f"{path}: {flag}" for flag in SANDBOX_OFF if flag.encode() in content]
    return found


def give_to(paths, user):
    """Give every file and folder under `paths` to the box's user, `uid:gid`, so that the
    engine in the box owns its home and the project as a person owns theirs."""
    uid, gid = (int(part) for part in user.split(":"))
    for top in paths:
        os.chown(top, uid, gid)
        for folder, dirs, files in os.walk(top):
            for name in dirs + files:
                os.chown(os.path.join(folder, name), uid, gid, follow_symlinks=False)


def mounted_files(flags):
    """The files of the recipe folder that `-v` flags of `file_mounts` mount."""
    return [flags[i + 1].split(":")[0] for i in range(0, len(flags), 2) if flags[i] == "-v"]


def sandboxed_script():
    """A shell script, run in the box, that says whether the browser's renderers run in its
    sandbox: each renderer's user namespace is not the box's own, and no process of the box
    carries a flag that switches the sandbox off."""
    # A flag is matched as a whole argument: the browser starts its own unsandboxed zygote,
    # for the processes its design leaves outside the sandbox, with `--no-zygote-sandbox`,
    # which is not `--no-zygote`.
    off = "".join(
        f'case "$c" in *" {flag} "*) echo "A2M_SANDBOX_OFF {flag} in ${{c# }}" | cut -c1-160;; esac; '
        for flag in SANDBOX_OFF
    )
    return (
        'box=$(readlink /proc/1/ns/user); n=0; shared=0; '
        "for p in /proc/[0-9]*; do "
        'c=" $(tr "\\0" " " < "$p/cmdline" 2>/dev/null)" || continue; '
        # The check itself, whose own text holds every flag it looks for.
        'case "$c" in *A2M_RENDERERS*) continue;; esac; '
        'case "$c" in *" --type=renderer "*) n=$((n+1)); '
        '[ "$(readlink "$p/ns/user")" = "$box" ] && shared=$((shared+1));; esac; '
        f"{off}"
        "done; "
        'echo "A2M_RENDERERS $n SHARING_THE_BOX_NAMESPACE $shared"'
    )


def sandboxed_verdict(code, out):
    """What the sandbox check showed: ok, and a line saying why."""
    if code != 0:
        return False, f"the check did not run (exit {code})"
    off = [line for line in out.splitlines() if line.startswith("A2M_SANDBOX_OFF")]
    if off:
        return False, "a process of the box carries a flag that switches the sandbox off: " + "; ".join(off)
    found = re.search(r"A2M_RENDERERS (\d+) SHARING_THE_BOX_NAMESPACE (\d+)", out)
    if not found:
        return False, "the check gave no verdict"
    renderers, shared = int(found.group(1)), int(found.group(2))
    if renderers == 0:
        return False, "no renderer of a browser runs in the box"
    if shared:
        return False, f"{shared} of {renderers} renderers run in the box's own user namespace: unsandboxed"
    return True, f"{renderers} renderers, each in a user namespace of its own"


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
    headers.update(spec.get("headers", {}))
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
        # Files of the recipe folder mounted read-only into the box, as `-v` flags: a program
        # the product starts on this machine, such as a command-line program or a local MCP
        # server's program (`system.box_mounts`).
        self.box_mounts = []
        # The box's image, and the flags of its system-call filter (decision 0447).
        self.box_image = BOX_IMAGE
        self.box_security = []
        # The box's desktop session, `{"application": [argv]}`, or None (decision 0459).
        self.desktop = None
        self.desktop_up = False

    def network_argv(self):
        return ["docker", "network", "create", "--internal", "--label", self.label, self.network]

    def system_argv(self, system, data, env, recipe=""):
        """The system's container: on the closed network, with no published port. With
        `"network": "box"`, in the box's own network, so that it is on the box's loopback
        address and on the closed network through the box, and nowhere else. `mounts` are
        files of the recipe folder, read-only; `command` is the argument list after the
        image, for an image that is not the system's own, such as the box's image running a
        release binary the recipe holds."""
        network = f"container:{self.box}" if system.get("network") == "box" else self.network
        argv = ["docker", "run", "-d", "--name", self.system, "--network", network, "--label", self.label]
        if "volume" in system:
            argv += ["-v", f"{data}:{system['volume']}"]
        argv += file_mounts(system.get("mounts", {}), recipe)
        for key, value in env.items():
            argv += ["-e", f"{key}={value}"]
        return argv + [system["image"], *system.get("command", [])]

    def box_argv(self, binary, home, project, user, harness=None):
        """The container the product's binary runs in: the binary read-only, the field
        harness read-only beside it when there is one, and the engine's home and the
        project folder at the paths they have on this machine."""
        mounts = ["-v", f"{binary}:{BOX_BINARY}:ro"]
        if harness:
            mounts += ["-v", f"{harness}:{BOX_HARNESS}:ro"]
        mounts += self.box_mounts
        if self.desktop:
            mounts += ["-v", f"{DESKTOP_SESSION}:{BOX_DESKTOP_SESSION}:ro"]
        # The image's own entry point is never run: a box image with a browser may start
        # programs of its own from one, and the box runs only what the runner starts in it.
        return [
            "docker", "run", "-d", "--init", "--name", self.box, "--network", self.network,
            "--label", self.label, "--user", user, "-e", f"HOME={home}", *self.box_security,
            *mounts, "-v", f"{home}:{home}", "-v", f"{project}:{project}",
            "--entrypoint", "", self.box_image, "sleep", "infinity",
        ]

    def proxy_argv(self, system_port):
        script = os.path.join(os.path.dirname(os.path.abspath(__file__)), "field_proxy.py")
        return [
            "docker", "run", "-d", "--name", self.proxy, "--network", self.network,
            "--label", self.label, "-v", f"{script}:/field_proxy.py:ro", PROXY_IMAGE,
            "python", "/field_proxy.py", "--upstream", f"{self.system}:{system_port}",
            "--listen", f"0.0.0.0:{PROXY_PORT}", "--control", f"127.0.0.1:{CONTROL_PORT}",
        ]

    def ensure_box(self, binary, home, project, user, execute=None, harness=None):
        """Start the binary's container and check that the network is closed, once, before
        the first command of the product."""
        execute = execute or run
        if self.box_up:
            return
        code, _, err = execute(self.box_argv(binary, home, project, user, harness))
        if code != 0:
            raise CannotRun(f"The container for the product's binary did not start: {err.strip()[-400:]}")
        self.box_up = True
        check_closed(self.box, execute)

    def session_env(self, env):
        """`env` with what a person's shell has of the box's desktop session, when there is one."""
        return {**env, **DESKTOP_ENV} if self.desktop else env

    def desktop_argv(self, *args, detach=False):
        """The desktop session's script in the box, as the box's user: `start` (detached),
        `ready`, or `list`."""
        flags = ["-d"] if detach else []
        return ["docker", "exec", *flags, "-w", "/", self.box, "sh", BOX_DESKTOP_SESSION, *args]

    def start_desktop(self, execute=None, wait=DESKTOP_WAIT, clock=time.monotonic, pause=time.sleep):
        """Start the box's desktop session and its application, once, and wait until the
        accessibility registry lists an application whose process runs the program."""
        execute = execute or run
        if not self.desktop or self.desktop_up:
            return
        program = self.desktop["application"][0]
        code, _, err = execute(self.desktop_argv("start", *self.desktop["application"], detach=True))
        if code != 0:
            raise CannotRun(f"desktop_not_ready: the desktop session did not start: {err.strip()[-400:]}")
        until = clock() + wait
        while True:
            if execute(self.desktop_argv("ready", program))[0] == 0:
                self.desktop_up = True
                return
            if clock() >= until:
                _, said, _ = execute(
                    ["docker", "exec", self.box, "sh", "-c", "tail -n 5 /tmp/a2m-desktop/*.log"]
                )
                raise CannotRun(
                    f"desktop_not_ready: no application of the accessibility registry runs {program} "
                    f"after {wait} seconds: {said.strip()[-800:]}"
                )
            pause(1)

    def exec_argv(self, env, cwd, argv):
        """One command of the product, inside the binary's container."""
        flags = []
        for key, value in self.session_env(env).items():
            flags += ["-e", f"{key}={value}"]
        # A command starts with the umask a person's shell has. The engine's container may
        # start commands with none, and the engine refuses a project file others may change.
        return ["docker", "exec", "-w", cwd, *flags, self.box, "sh", "-c", 'umask 022; exec "$0" "$@"', BOX_BINARY, *argv]

    def harness_argv(self, env, cwd):
        """The field harness's `serve`, inside the binary's container, with its standard input
        kept open (`-i`) for the runner alone. It is given no other argument: in particular
        not `--passphrase-from-stdin`, because its standard input carries prompt answers."""
        flags = []
        for key, value in self.session_env(env).items():
            flags += ["-e", f"{key}={value}"]
        return ["docker", "exec", "-i", "-w", cwd, *flags, self.box, "sh", "-c", 'umask 022; exec "$0" "$@"', BOX_HARNESS, "serve"]

    def page_argv(self, port):
        """A request to the engine's own page, which listens on the box's loopback address:
        the request on standard input, the answer on standard output, through the box's own
        shell, so that nothing outside the box reaches the page."""
        script = f"exec 3<>/dev/tcp/127.0.0.1/{int(port)}; cat >&3; cat <&3"
        return ["docker", "exec", "-i", self.box, "bash", "-c", script]

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


def tcp_probe_script(host, port):
    """A shell script that prints one verdict line for a try at an address: CLOSED on a
    timeout, "unreachable", "no route", or an address family the box lacks; OPEN on an answer, a refusal, or a reset; UNKNOWN
    for anything else."""
    return (
        f"out=$(timeout 4 bash -c 'exec 3<>/dev/tcp/{host}/{port}' 2>&1); rc=$?; "
        'if [ "$rc" -eq 0 ]; then echo A2M_VERDICT OPEN; '
        'elif [ "$rc" -eq 124 ]; then echo A2M_VERDICT CLOSED; '
        'else case "$out" in '
        '*nreachable*|*"No route"*|*"Address family not supported"*) echo A2M_VERDICT CLOSED;; '
        "*refused*|*reset*) echo A2M_VERDICT OPEN;; "
        "*) echo A2M_VERDICT UNKNOWN;; esac; fi"
    )


def dns_probe_script(name):
    """A shell script that prints one verdict line for resolving a name: OPEN when it
    resolves, CLOSED when the resolver says the name is not found, UNKNOWN otherwise (the
    tool missing, for one)."""
    return (
        f"getent hosts {name} >/dev/null 2>&1; rc=$?; "
        'if [ "$rc" -eq 0 ]; then echo A2M_VERDICT OPEN; '
        'elif [ "$rc" -eq 2 ]; then echo A2M_VERDICT CLOSED; '
        "else echo A2M_VERDICT UNKNOWN; fi"
    )


def probes():
    """Every probe of the check, as (what it tries, the script that tries it)."""
    found = [(f"{host}:{port}", tcp_probe_script(host, port)) for host, port in PUBLIC_ADDRESSES]
    found.append((f"the name {PUBLIC_NAME}", dns_probe_script(PUBLIC_NAME)))
    return found


def verdict(code, out):
    """What one probe showed: "OPEN", "CLOSED", or "UNPROVEN". Only an explicit verdict line
    from a probe that ran and exited 0 counts; anything else, including no output at all,
    is unproven, never closed."""
    if code != 0:
        return "UNPROVEN"
    lines = [line.strip() for line in out.splitlines() if line.strip()]
    if not lines or lines[-1] not in ("A2M_VERDICT CLOSED", "A2M_VERDICT OPEN"):
        return "UNPROVEN"
    return lines[-1].split()[1]


def check_closed(box, execute=None):
    """Raise CannotRun, by name, unless every probe from the box shows the network closed."""
    execute = execute or run
    for what, script in probes():
        code, out, err = execute(["docker", "exec", box, "sh", "-c", script])
        shown = verdict(code, out)
        if shown == "OPEN":
            raise CannotRun(
                f"closed_network_open: {what} can be reached from inside the run's network; "
                "no a2m step was run. The network is not closed."
            )
        if shown != "CLOSED":
            raise CannotRun(
                f"closed_network_unproven: the probe of {what} gave no verdict "
                f"(exit {code}: {(out + err).strip()[-200:]!r}); no a2m step was run. "
                "The network is not proven closed."
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


def harness_build(build_dir, features=()):
    """The command that builds the field harness into `build_dir`, and its environment: a
    cargo target folder of the run's own, so that the harness is never in the shared one,
    and no debug information or incremental state, which only cost disk. Each feature is
    `<package>/<feature>`; its package is selected beside the harness, so that cargo's one
    resolution of features gives the harness the package with that feature on."""
    env = dict(os.environ)
    env.update(
        CARGO_TARGET_DIR=os.path.join(build_dir, "target"),
        CARGO_INCREMENTAL="0",
        CARGO_PROFILE_DEV_DEBUG="0",
    )
    argv = ["cargo", "build", "-p", HARNESS_PACKAGE]
    for feature in features:
        if not FEATURE.match(feature):
            raise CannotRun(f"A harness feature is not `<package>/<feature>`: {feature!r}")
        argv += ["-p", feature.split("/")[0]]
    if features:
        argv += ["--features", ",".join(features)]
    argv += ["--locked", "--quiet"]
    return argv, env, os.path.join(build_dir, "target", "debug", HARNESS_PACKAGE)


def build_harness(build_root, execute=None, features=()):
    """Build the field harness into a new folder under `build_root`. Returns the folder,
    which the caller deletes, and the binary."""
    execute = execute or (lambda argv, env: run(argv, env=env, cwd=REPO, seconds=3600))
    build_dir = tempfile.mkdtemp(prefix="a2m-field-harness-", dir=build_root)
    try:
        argv, env, binary = harness_build(build_dir, features)
    except CannotRun:
        shutil.rmtree(build_dir, ignore_errors=True)
        raise
    code, out, err = execute(argv, env)
    if code != 0 or not os.path.isfile(binary):
        shutil.rmtree(build_dir, ignore_errors=True)
        raise CannotRun(f"The field harness did not build: {(out + err).strip()[-600:]}")
    return build_dir, binary


class Harness:
    """The field harness serving in the box: its standard input held by the runner, its
    standard output read line by line, where it writes each prompt, and its standard error
    kept in a file."""

    def __init__(self, argv, log_path, popen=None):
        self.argv = argv
        self.log_path = log_path
        self.popen = popen or subprocess.Popen
        self.process = None
        self.prompts = queue.Queue()
        self.log = None

    def start(self, wait=HARNESS_WAIT):
        self.log = open(self.log_path, "w", encoding="utf-8")
        self.process = self.popen(
            self.argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log,
            text=True, bufsize=1,
        )
        threading.Thread(target=self._read, daemon=True).start()
        until = time.time() + wait
        while time.time() < until:
            if HARNESS_READY in self.said():
                return
            if self.process.poll() is not None:
                raise CannotRun(f"field_harness_refused: the field harness stopped before it served: {self.said().strip()[-600:]}")
            time.sleep(0.2)
        raise CannotRun(f"The field harness did not serve within {wait} seconds: {self.said().strip()[-400:]}")

    def _read(self):
        with self.process.stdout:
            for line in self.process.stdout:
                self.prompts.put(line.rstrip("\n"))

    def said(self):
        try:
            with open(self.log_path, encoding="utf-8") as handle:
                return handle.read()
        except OSError:
            return ""

    def alive(self):
        return self.process is not None and self.process.poll() is None

    def stray(self):
        """Every prompt line that waits unanswered, taken off the queue."""
        found = []
        while True:
            try:
                found.append(self.prompts.get_nowait())
            except queue.Empty:
                return found

    def answer(self, line):
        self.process.stdin.write(line + "\n")
        self.process.stdin.flush()

    def stop(self, seconds=30):
        if self.process is None:
            return
        try:
            self.process.stdin.close()
        except OSError:
            pass
        try:
            self.process.wait(timeout=seconds)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        if self.log:
            self.log.close()


PAGE_LINK = re.compile(r"^http://127\.0\.0\.1:(\d+)(/(?:approve|act)/[0-9a-f]+)$")
PRESS = {"approve": "approve", "act": "confirm"}


def page_request(method, port, path, body=""):
    """One HTTP request to the engine's own page, as the person's browser on this machine
    sends it: the page's own host, and on a press its own origin."""
    head = [f"{method} {path} HTTP/1.1", f"Host: 127.0.0.1:{port}", "Connection: close"]
    if method == "POST":
        data = body.encode()
        head += [
            f"Origin: http://127.0.0.1:{port}",
            "Content-Type: application/x-www-form-urlencoded",
            f"Content-Length: {len(data)}",
        ]
    return ("\r\n".join(head) + "\r\n\r\n" + body).encode()


def page_answer(raw):
    """The status, the title, and the plain text of a page's answer."""
    text = raw.decode("utf-8", "replace")
    head, _, body = text.partition("\r\n\r\n")
    match = re.match(r"HTTP/1\.[01] (\d{3})", head)
    status = int(match.group(1)) if match else None
    title = re.search(r"<h1>(.*?)</h1>", body, re.S)
    words = html.unescape(re.sub(r"<[^>]+>", " ", body))
    return status, html.unescape(title.group(1)) if title else "", " ".join(words.split()), body


def once_of(body, press_path):
    """The one-time value of the form that presses `press_path` on the page."""
    form = re.search(
        r'<form method="post" action="' + re.escape(press_path) + r'">\s*<input type="hidden" name="once" value="([0-9a-f]+)">',
        body,
    )
    return form.group(1) if form else None


def prompt_short(line):
    """The short hash a prompt line of the harness shows: its last word."""
    words = line.split()
    return words[-1] if words else ""


def other_short(short):
    """A short hash of the same form that is not `short`."""
    return "".join("0" if c != "0" else "1" for c in short) or "0" * SHORT_HASH


def press(spec, active, harness, execute=None, wait=PROMPT_WAIT):
    """A `person` step: open the page, press, answer the prompt with the short hash the
    recipe means to pass (or, with `"send": "other"`, another), and show what the page
    answered."""
    execute = execute or (lambda argv, data: subprocess.run(argv, input=data, capture_output=True, timeout=wait + 30, check=False))
    if harness is None or not harness.alive():
        raise CannotRun("A person step needs the field harness serving: run an `a2m` step first.")
    link = PAGE_LINK.match(spec.get("link", ""))
    if not link:
        raise CannotRun(f"A person step's `link` is not a page of the engine on the box: {spec.get('link')!r}")
    meant = str(spec.get("approve", ""))
    if not re.fullmatch(r"[0-9a-f]{%d,}" % SHORT_HASH, meant):
        raise CannotRun("A person step's `approve` is not a change's hash or an act's identifier.")
    port, path = link.group(1), link.group(2)
    short = meant[:SHORT_HASH]
    sent = other_short(short) if spec.get("send") == "other" else short
    shown = execute(active.page_argv(port), page_request("GET", port, path))
    status, title, words, body = page_answer(shown.stdout)
    if status != 200:
        return {"ok": False, "status": status, "output": f"the page answered {status}: {title} {words}"[:2000]}
    if meant not in words and short not in words:
        return {"ok": False, "status": status, "output": f"the page does not show {short}: {words}"[:2000]}
    press_path = f"{path}/{PRESS[path.split('/')[1]]}"
    once = once_of(body, press_path)
    if once is None:
        return {"ok": False, "status": status, "output": f"the page has no press for {press_path}"}
    stray = harness.stray()
    result = {}
    # What the person types into the page's fields, one for each credential's role.
    typed = "".join(
        f"&{urllib.parse.quote('secret.' + str(role))}={urllib.parse.quote(str(value), safe='')}"
        for role, value in sorted(spec.get("signs_in", {}).items())
    )
    worker = threading.Thread(
        target=lambda: result.update(
            done=execute(active.page_argv(port), page_request("POST", port, press_path, f"once={once}{typed}"))
        )
    )
    worker.start()
    asked = None
    until = time.time() + wait
    while time.time() < until and asked is None:
        try:
            asked = harness.prompts.get(timeout=0.2)
        except queue.Empty:
            if not worker.is_alive():
                break
    if asked is not None:
        harness.answer(sent)
    worker.join(wait + 30)
    if "done" not in result:
        raise CannotRun("The press on the engine's page did not end.")
    status, title, words, _ = page_answer(result["done"].stdout)
    lines = [
        f"prompt: {asked if asked is not None else 'none was asked'}",
        f"shown: {prompt_short(asked) if asked else 'none'}; sent: {sent if asked else 'nothing'}",
        f"page: {status} {title}: {words}",
    ]
    if stray:
        lines.insert(0, f"prompts that waited before the press: {stray}")
    passed = status == 200 and asked is not None and prompt_short(asked) == sent and not stray
    return {"ok": passed, "status": status, "output": "\n".join(lines)[:4000]}


def kept(answer, path):
    """The value at a dotted path of a JSON answer, as text, or None."""
    value = answer
    for part in path.split("."):
        if isinstance(value, dict) and part in value:
            value = value[part]
        elif isinstance(value, list) and part.isdigit() and int(part) < len(value):
            value = value[int(part)]
        else:
            return None
    return value if isinstance(value, str) else json.dumps(value)


def holders(secret, places):
    """Every file under `places` (folders, or one file each) whose bytes hold `secret`."""
    needle = secret.encode()
    found = []
    for place in places:
        if not place:
            continue
        files = [place] if os.path.isfile(place) else [
            os.path.join(folder, name) for folder, _, names in os.walk(place) for name in names
        ]
        for path in files:
            try:
                with open(path, "rb") as handle:
                    if needle in handle.read():
                        found.append(path)
            except OSError:
                continue
    return found


def scrubbed(value, secrets):
    """`value` with every secret of the run in its strings replaced by its name."""
    if isinstance(value, str):
        for name, secret in (secrets or {}).items():
            value = value.replace(secret, f"[secret {name}]")
        return value
    if isinstance(value, list):
        return [scrubbed(item, secrets) for item in value]
    if isinstance(value, dict):
        return {key: scrubbed(item, secrets) for key, item in value.items()}
    return value


def do_step(step, names, env, active, proxy=None, harness=None, secrets=None):
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
    if ("exec" in step or "http" in step) and "direct" not in names:
        raise CannotRun("An `exec` or `http` step needs a system's container; this recipe's system is a folder.")
    if "exec" in step:
        code, out, err = run(["docker", "exec", *step.get("as", []), active.system, *step["exec"]])
        return {"ok": code == 0, "output": (out + err)[-2000:]}
    if "http" in step:
        status, body = request(names["direct"], step["http"])
        if "secret" in step:
            name = step["secret"]["name"]
            try:
                value = kept(json.loads(body), step["secret"]["path"])
            except ValueError:
                value = None
            if not value or not 200 <= status < 300:
                raise CannotRun(f"The system's answer holds no `{step['secret']['path']}` to keep as the secret `{{{name}}}`.")
            names[name] = value
            if secrets is not None:
                secrets[name] = value
            return {"ok": True, "status": status, "output": f"the answer held a secret, kept as `{{{name}}}`; nothing of it is shown"}
        return {"ok": 200 <= status < 300, "status": status, "output": body[:2000]}
    if "unseen" in step:
        name = step["unseen"]
        value = (secrets or {}).get(name)
        if not value:
            raise CannotRun(f"No secret was kept as `{{{name}}}` before this step.")
        holding = holders(value, [names["home"], names["project"], names.get("log", "")])
        return {"ok": not holding, "output": "in none of the files" if not holding else "held by: " + ", ".join(holding)}
    if "person" in step:
        return press(step["person"], active, harness)
    if "sandboxed" in step:
        if not active.box_up:
            raise CannotRun("A `sandboxed` step needs the box, which starts with the first `a2m` step.")
        code, out, err = run(["docker", "exec", active.box, "sh", "-c", sandboxed_script()])
        ok, why = sandboxed_verdict(code, out + err)
        return {"ok": ok, "output": why}
    if "desktop" in step:
        if not active.desktop_up:
            raise CannotRun("A `desktop` step needs the box's desktop session, which a recipe asks for with `box_desktop`.")
        code, out, err = run(active.desktop_argv("list"))
        listed = [line for line in out.splitlines() if line.startswith("A2M_APPLICATION ")]
        return {"ok": code == 0 and bool(listed), "output": "\n".join(listed) or (out + err)[-2000:]}
    if "a2m" in step:
        if harness is not None and harness.alive() is False and harness.process is not None:
            raise CannotRun(
                "field_harness_gone: the field harness no longer runs, and a command would start "
                f"the product's own engine in its place: {harness.said().strip()[-400:]}"
            )
        code, out, err = run(active.exec_argv(product_env(env), names["project"], step["a2m"]))
        answer = first_json(out) or {}
        error = answer.get("error") or {}
        return {
            "ok": bool(answer.get("ok")) and code == 0,
            "code": error.get("code"),
            "class": error.get("class"),
            "answer": answer,
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
    if "count" in expect:
        found = shown.get("output", "").count(expect["count"]["text"])
        if found != expect["count"]["times"]:
            return f"the output holds {expect['count']['text']!r} {found} times, expected {expect['count']['times']}"
    return None


def wait_ready(base, path, seconds=90, container=None):
    """Wait until the system answers 200 at `path`, or, when `path` is `{"exec": [argv]}`,
    until that program exits 0 inside `container`."""
    until = time.time() + seconds
    if isinstance(path, dict):
        while time.time() < until:
            if run(["docker", "exec", container, *path["exec"]], seconds=30)[0] == 0:
                return
            time.sleep(1)
        raise CannotRun(f"The system's readiness check {path['exec']} did not exit 0 within {seconds} seconds.")
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
    parser.add_argument(
        "--harness-build",
        default=tempfile.gettempdir(),
        help="the folder under which the field harness is built for this run, in a folder of "
        "its own that is deleted when the run ends",
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
    a_folder = system.get("kind") in ("folder", "desktop")
    if a_folder:
        if "image" in system or system.get("proxy") or system.get("network"):
            print(f"A recipe whose system is a {system['kind']} names no image, no proxy, and no network.")
            return 2
    elif "@sha256:" not in system.get("image", ""):
        print("The recipe's image is not pinned by digest.")
        return 2
    if system.get("network") == "box" and system.get("proxy"):
        print("A system in the box's network cannot have the proxy, whose upstream is the system's container.")
        return 2
    try:
        system_env(system, {})
        file_mounts(system.get("mounts", {}), recipe_dir)
        box_mounts = file_mounts(system.get("box_mounts", {}), recipe_dir)
        box = box_options(system, f"{os.getuid()}:{os.getgid()}")
        desktop = desktop_options(system, box)
        off = sandbox_off_in(recipe, mounted_files(box_mounts))
        if off:
            raise CannotRun(
                "The recipe holds a flag that switches a browser's sandbox off, which no field "
                "test may pass (rule 5): " + "; ".join(off)
            )
        profile = box_filter(box["filter"]) if box["filter"] else None
    except CannotRun as stop:
        print(f"The field test could not be run: {stop}")
        return 2

    run_dir = os.path.join(recipe_dir, "runs", time.strftime("%Y%m%d-%H%M%S"))
    active = Run(recipe["name"])
    active.box_mounts = box_mounts
    active.box_image = box["image"]
    active.desktop = desktop
    names = {
        "recipe": recipe_dir,
        "project": os.path.join(run_dir, "project"),
        "data": os.path.join(run_dir, "data"),
        "log": os.path.join(run_dir, "harness.log"),
    }
    # The secrets the run made, by name: cleared from everything a step showed.
    secrets = {}
    if not a_folder:
        names["port"] = str(system["port"])
        names["host"] = "127.0.0.1" if system.get("network") == "box" else active.system
        names["system"] = f"http://{names['host']}:{system['port']}"
        names["base"] = names["system"]
    for folder in (names["project"], names["data"]):
        os.makedirs(folder)
    home = os.path.abspath(args.home) if args.home else os.path.join(run_dir, "home")
    os.makedirs(home, exist_ok=True)
    names["home"] = home
    if profile is not None:
        filter_path = os.path.join(run_dir, "box-seccomp.json")
        with open(filter_path, "w", encoding="utf-8") as handle:
            json.dump(profile, handle, indent=1)
        active.box_security = ["--security-opt", f"seccomp={filter_path}"]
    # The engine's home and the project belong to the box's user, as a person's own do.
    handed = box["user"] != f"{os.getuid()}:{os.getgid()}"
    if handed:
        give_to([home, names["project"]], box["user"])
    env = dict(os.environ)
    env.update(
        A2M_HOME=home,
        A2M_VAULT_PASSPHRASE="an invented passphrase for field tests",
        A2M_BINARY=os.path.abspath(args.binary),
    )

    proxy = None
    harness = None
    build_dir = None
    report = {
        "field_test": recipe["name"],
        "image": system.get("image", f"none: the system is a {system.get('kind')}"),
        "harness_features": system.get("harness_features", []),
        "box": {
            "image": box["image"],
            "user": box["user"],
            "filter": box["filter"] or "the container engine's own default",
            "filter_sha256": (
                hashlib.sha256(json.dumps(profile, indent=1).encode()).hexdigest() if profile else None
            ),
            "desktop": (
                {"application": desktop["application"], "session_sha256": desktop_hash()} if desktop else None
            ),
        },
        "steps": [],
    }
    failed = 0

    def start_engine(harness_binary):
        """The box, the closed-network check, and the field harness serving in the box."""
        active.ensure_box(
            env["A2M_BINARY"], home, names["project"], box["user"], harness=harness_binary
        )
        active.start_desktop()
        started = Harness(active.harness_argv(product_env(env), names["project"]), os.path.join(run_dir, "harness.log"))
        started.start()
        return started

    try:
        print("building the field harness")
        build_dir, harness_binary = build_harness(args.harness_build, features=system.get("harness_features", ()))
        code, _, err = run(active.network_argv())
        if code != 0:
            raise CannotRun(f"The closed network was not made: {err.strip()[-400:]}")
        if not a_folder and system.get("network") == "box":
            # The box first, and the closed-network check with it, so that the system's
            # container can join the box's network.
            active.ensure_box(
                env["A2M_BINARY"], home, names["project"], box["user"], harness=harness_binary
            )
        if not a_folder:
            code, _, err = run(active.system_argv(system, names["data"], system_env(system, names), recipe_dir))
            if code != 0:
                raise CannotRun(f"The system did not start: {err.strip()[-400:]}")
            holder = active.box if system.get("network") == "box" else active.system
            _, address, _ = run(
                ["docker", "inspect", holder, "--format",
                 "{{(index .NetworkSettings.Networks \"" + active.network + "\").IPAddress}}"]
            )
            names["direct"] = f"http://{address.strip()}:{system['port']}"
            wait_ready(names["direct"], system["ready"], container=active.system)
        if system.get("proxy"):
            code, _, err = run(active.proxy_argv(system["port"]))
            if code != 0:
                raise CannotRun(f"The fault proxy did not start: {err.strip()[-400:]}")
            proxy = ProxyBox(active.proxy)
            proxy.wait_ready()
            names["base"] = f"http://{active.proxy}:{PROXY_PORT}"
        for number, raw in enumerate(recipe["steps"], 1):
            straight = any(kind in raw for kind in ("http", "sqlite", "exec", "python"))
            step = fill(raw, {**names, "base": names["direct"]} if straight and "direct" in names else names)
            if ("a2m" in step or "desktop" in step) and harness is None:
                harness = start_engine(harness_binary)
            shown = do_step(step, names, env, active, proxy, harness, secrets)
            if handed and any(kind in step for kind in ("copy", "replace", "python")):
                give_to([home, names["project"]], box["user"])
            answer = shown.pop("answer", None)
            for name, path in step.get("keep", {}).items():
                value = kept(answer or {}, path)
                if value is None:
                    raise CannotRun(
                        f"Step {number} has no `{path}` in its answer to keep as `{{{name}}}`; "
                        f"it showed: {shown.get('output', '')[-1500:]}"
                    )
                names[name] = value
            why = met(step.get("expect", {"ok": True}), shown)
            shown = scrubbed(shown, secrets)
            entry = {
                "step": number,
                "what": next(key for key in step if key not in ("expect", "gap", "note", "as", "keep", "secret")),
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
        print(f"The field test could not be run: {scrubbed(str(stop), secrets)}")
        report["could_not_run"] = str(stop)
        failed = -1
    finally:
        if harness is not None and harness.alive():
            run(active.exec_argv(product_env(env), names["project"], ["engine", "stop"]), seconds=60)
        if harness is not None:
            harness.stop()
        if build_dir is not None:
            shutil.rmtree(build_dir, ignore_errors=True)
            if os.path.exists(build_dir):
                print(f"NOT REMOVED, remove by hand: the field harness's build folder {build_dir}")
                report.setdefault("not_removed", []).append(build_dir)
        if args.keep:
            print(f"left running: network {active.network}; containers {active.system}, {active.box}, {active.proxy}")
        else:
            left = active.remove()
            if left:
                print(f"NOT REMOVED, remove by hand: {', '.join(left)} (label {active.label})")
                report.setdefault("not_removed", []).extend(left)
        gaps = [entry for entry in report["steps"] if entry["gap"]]
        report["gaps"] = [{"step": entry["step"], "gap": entry["gap"], "still_open": entry["passed"]} for entry in gaps]
        report = scrubbed(report, secrets)
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
