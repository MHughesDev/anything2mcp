#!/usr/bin/env python3
"""Keep what a release publishes, and check every build against every kept release (decision 0437).

Usage, from the repository root:

    python3 tools/contracts.py check             # compare the tree with every folder in contracts/
    python3 tools/contracts.py show              # print the tree's contract, and write nothing
    python3 tools/contracts.py write VERSION     # keep the tree's contract as contracts/VERSION/
    python3 tools/contracts.py kept VERSION      # exit 1 unless contracts/VERSION/ is whole
    ... --root DIR                               # work on another copy of the repository

A contract is read from the tree as text, without a build, from the same places the other checks hold to
the built program: the API description from `bindings/python/tools/openapi.json` (which CI holds to what
the built engine serves), with an action built where `a2m-actions` gives it a handler or carries it out
in the client; the failure codes, each with its class and whether it passes by itself, as
`tools/failure_codes.py` reads them; the names of the vocabularies of `a2m-core` that a result, a stored
row, or a file carries, from their `vocabulary!` tables; the settings keys with their types, from the
kernel's schema, less each key marked not built (decision 0455), and from each pack's manifest, read
where the pack's source embeds it with `include_str!`. `write` also compiles the `example` target
through the test `kept_compiled_target` of `a2m-sources`, and keeps its bytes and hash; `a2m-core`'s test `kept_contracts`
proves on every build that each kept compiled target still loads with its hash.

`check` names each break, one line each, in a form `contracts/next-breaks` can list: an action, a route,
a client method, an input, an output field, a failure code, a vocabulary name, or a settings key removed
or renamed; a code moved to another class, or its "passes by itself" changed; an optional input made
required; an input's or an output field's type changed; a member added to a closed vocabulary (failure
class, run state, effect class, evidence level). Additions pass. A break passes only when the kept folder
of a later release, or `contracts/next-breaks` for the release not yet made, lists it as
`<break> <record>`, the record being the number of the decision record that made it; `write` moves
`next-breaks` into the new folder as its `breaks`. `check` also holds the description's `x-sdk` to the built actions:
true for an action with a handler, false for one without, so that a client has a method for an action
from the build that builds it (decision 0437, point 5). A listed break that does not happen is a problem, so
the list stays true. Types are compared one level deep: a field's JSON type, an array's item type, a
reference's name; what an object field holds inside is not compared yet.

A kept folder is never changed after its release. Standard library only; Python 3.11 or newer.
"""
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import failure_codes  # noqa: E402

CONTRACT_FORMAT = 1
OPENAPI = "bindings/python/tools/openapi.json"
HANDLERS = "crates/a2m-actions/src/handlers/mod.rs"
CATALOG = "crates/a2m-actions/src/catalog.rs"
SCHEMA = "crates/a2m-actions/src/settings/schema.rs"
CORE = "crates/a2m-core/src"
NEXT_BREAKS = "contracts/next-breaks"
FILES = ("openapi.json", "failure-codes.json", "vocabularies.json", "settings.json", "compiled-targets.json")

# The vocabularies of a2m-core that a result, a stored row, or a file carries, by the enum's name, and
# whether each is closed: a client may branch on every member of a closed one, so a new member is a
# break (decision 0437, point 4).
VOCABULARIES = {
    "FailureClass": ("failure_class", True),
    "RunState": ("run_state", True),
    "RecordState": ("record_state", False),
    "SessionState": ("session_state", False),
    "EffectClass": ("effect_class", True),
    "Hazard": ("hazard", False),
    "AccessKind": ("access_kind", False),
    "EvidenceLevel": ("evidence_level", True),
    "WriteEvidence": ("write_evidence", False),
    "Coverage": ("coverage", False),
}
CLOSED = {key for key, closed in VOCABULARIES.values() if closed}

VERSION = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")
BREAK_LINE = re.compile(r"^(\S+)\s+(\d{4})\s*(?:#.*)?$")


class Fault(Exception):
    """The tree cannot be read as a contract."""


def strip_comments(text):
    """Rust text with line comments taken out, strings kept whole."""
    out, i = [], 0
    while i < len(text):
        c = text[i]
        if c == '"':
            j = i + 1
            while j < len(text) and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            out.append(text[i:j + 1])
            i = j + 1
        elif text.startswith("//", i):
            j = text.find("\n", i)
            i = len(text) if j < 0 else j
        else:
            out.append(c)
            i += 1
    return "".join(out)


# The API description.

def built_actions(root):
    """The kernel's actions that are built: those `handler_for` gives a function, and those the
    client end carries out."""
    text = (root / HANDLERS).read_text(encoding="utf-8")
    start = text.find("fn handler_for")
    if start < 0:
        raise Fault(f"{HANDLERS}: `handler_for` is not there")
    end = text.find("\n}\n", start)
    built = set(re.findall(r'"([a-z_]+(?:\.[a-z_]+)+)"\s*=>', text[start:end]))
    catalog = strip_comments((root / CATALOG).read_text(encoding="utf-8"))
    for m in re.finditer(r'\bact\(\s*"([a-z_.]+)"', catalog):
        done = catalog.find(".done()", m.end())
        if ".client()" in catalog[m.end():done if done >= 0 else len(catalog)]:
            built.add(m.group(1))
    if not built:
        raise Fault(f"{HANDLERS}: no built action could be read")
    return built


def sdk_problems(root, document=None, built=None):
    """Every route whose `x-sdk` does not say whether its action is built. An action has a client method
    from the build that gives it a handler, and none before (decision 0437, point 5): the description
    says `x-sdk: true` for a built action and `false` for one with no handler, so this count of the built
    actions and the clients' generators, which read `x-sdk`, are made from the one place."""
    if document is None:
        document = json.loads((root / OPENAPI).read_text(encoding="utf-8"))
    if built is None:
        built = built_actions(root)
    problems = []
    for path, operations in document.get("paths", {}).items():
        for method, op in operations.items():
            name = op.get("x-action")
            if name and op.get("x-sdk") is not (name in built):
                said = "has no handler" if name not in built else "is built"
                problems.append(f"{OPENAPI}: {method.upper()} {path} says `x-sdk: {json.dumps(op.get('x-sdk'))}`, "
                                f"and `{name}` {said}; regenerate the description and the clients")
    return problems


def kept_openapi(root):
    """The API description as served, cut to the routes of built actions."""
    document = json.loads((root / OPENAPI).read_text(encoding="utf-8"))
    built = built_actions(root)
    disagree = sdk_problems(root, document, built)
    if disagree:
        raise Fault("\n".join(disagree))
    paths = {}
    for path, operations in document.get("paths", {}).items():
        kept = {method: op for method, op in operations.items() if op.get("x-action") in built}
        if kept:
            paths[path] = kept
    return {**document, "paths": paths}


def shape(schema):
    """A field's type, one level deep."""
    if not isinstance(schema, dict):
        return "any"
    if "$ref" in schema:
        return "ref:" + schema["$ref"].rsplit("/", 1)[-1]
    for key in ("anyOf", "oneOf"):
        if key in schema:
            return key + "(" + ",".join(sorted(shape(s) for s in schema[key])) + ")"
    kind = schema.get("type")
    if isinstance(kind, list):
        kind = "|".join(sorted(kind))
    if kind == "array":
        return f"array<{shape(schema.get('items'))}>"
    return kind or "any"


def fields(schema):
    """{name: (type, required)} of an object schema."""
    if not isinstance(schema, dict):
        return {}
    required = set(schema.get("required", []))
    return {name: (shape(s), name in required) for name, s in schema.get("properties", {}).items()}


def actions_of(openapi):
    """{action: {routes, sdk, inputs, outputs}} from an API description."""
    found = {}
    for path, operations in openapi.get("paths", {}).items():
        for method, op in operations.items():
            name = op.get("x-action")
            if not name:
                continue
            action = found.setdefault(name, {"routes": set(), "sdk": False, "inputs": {}, "outputs": {}})
            action["routes"].add(f"{method.upper()} {path}")
            action["sdk"] = action["sdk"] or bool(op.get("x-sdk"))
            if path.endswith("/actions/" + name):
                body = op.get("requestBody", {}).get("content", {}).get("application/json", {}).get("schema")
                action["inputs"] = fields(body)
                answer = op.get("responses", {}).get("200", {}).get("content", {}).get("application/json", {})
                result = answer.get("schema", {}).get("properties", {}).get("result")
                action["outputs"] = fields(result)
    return found


# The vocabularies and the settings.

def vocabularies(root):
    names = {}
    for p in sorted((root / CORE).glob("*.rs")):
        text = strip_comments(p.read_text(encoding="utf-8"))
        for m in re.finditer(r'\benum\s+(\w+)\s*:\s*"[^"]*"\s*\{', text):
            if m.group(1) not in VOCABULARIES:
                continue
            end = failure_codes.block_end(text, m.end())
            names[VOCABULARIES[m.group(1)][0]] = re.findall(r'=>\s*"([^"]+)"', text[m.end():end])
    missing = sorted(key for key, _ in VOCABULARIES.values() if not names.get(key))
    if missing:
        raise Fault(f"{CORE}: no `vocabulary!` table could be read for {', '.join(missing)}")
    return names


KINDS = {"Kind::Bool": "bool", "Kind::Int": "int", "Kind::Text": "text", "Kind::Word": "word",
         "Kind::List": "list", "Kind::Words": "words"}


def call_end(text, start):
    """The index of the parenthesis that closes the call opened just before `start`, skipping
    strings; -1 when it is not closed."""
    depth, i = 1, start
    while i < len(text):
        c = text[i]
        if c == '"':
            i += 1
            while i < len(text) and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return -1


INCLUDE = re.compile(r'\binclude_str!\(\s*"([^"]+\.toml)"\s*\)')


def pack_manifests(root):
    """Every pack manifest the registry reads: each TOML file with a `[provider]` table that a crate's
    source, outside its tests, embeds with `include_str!`, wherever in the crate the file is."""
    found = set()
    crates = root / "crates"
    for crate_dir in sorted(d for d in crates.iterdir() if d.is_dir()) if crates.is_dir() else []:
        for p in failure_codes.source_files(crate_dir):
            for m in INCLUDE.finditer(strip_comments(failure_codes.without_tests(p.read_text(encoding="utf-8")))):
                path = (p.parent / m.group(1)).resolve()
                if path.is_file() and crate_dir.resolve() in path.parents:
                    found.add(path)
    return sorted(found)


def settings(root):
    """{key: type} of every setting that is built: the kernel's schema, less each key marked not built
    (decision 0455), and each pack's manifest."""
    text = strip_comments((root / SCHEMA).read_text(encoding="utf-8"))
    consts = {m.group(1): KINDS.get(m.group(2)) for m in re.finditer(r"\bconst\s+(\w+)\s*:\s*Kind\s*=\s*(Kind::\w+)", text)}
    start = text.find("pub const SCHEMA")
    if start < 0:
        raise Fault(f"{SCHEMA}: `SCHEMA` is not there")
    later = []
    for m in re.finditer(r"\bnot_built\(", text[start:]):
        end = call_end(text, start + m.end())
        if end < 0:
            raise Fault(f"{SCHEMA}: a `not_built(` call is not closed")
        later.append((start + m.end(), end))
    found = {}
    for m in re.finditer(r'\bdef\(\s*"([^"]+)"\s*,\s*(Kind::\w+|\w+)', text[start:]):
        kind = KINDS.get(m.group(2)) or consts.get(m.group(2))
        if not kind:
            raise Fault(f"{SCHEMA}: the type `{m.group(2)}` of `{m.group(1)}` cannot be read")
        if any(a <= start + m.start() < b for a, b in later):
            continue
        found[m.group(1)] = kind
    if not found:
        raise Fault(f"{SCHEMA}: no setting could be read")
    for manifest in pack_manifests(root):
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        pack = data.get("provider", {}).get("name")
        if not pack:
            continue
        for setting in data.get("settings", []):
            found[f"packs.{pack}.{setting['key']}"] = setting["type"]
    return dict(sorted(found.items()))


def codes(root):
    rows, faults = failure_codes.resolve(failure_codes.scan(root), root)
    if faults:
        raise Fault("the failure codes have faults; run `python3 tools/failure_codes.py --check`")
    return {code: {"class": cls, "passes_by_itself": passes == "yes"} for cls, code, _, _, _, passes in rows}


def tree(root):
    """The contract of the tree, as the files of a kept folder hold it."""
    return {
        "openapi.json": kept_openapi(root),
        "failure-codes.json": codes(root),
        "vocabularies.json": vocabularies(root),
        "settings.json": settings(root),
    }


# The comparison.

def breaks(kept, now):
    """Every break from the contract `kept` to the contract `now`, as names `next-breaks` can list."""
    out = []
    old, new = actions_of(kept["openapi.json"]), actions_of(now["openapi.json"])
    for name in sorted(old):
        if name not in new:
            out.append(f"action.removed:{name}")
            continue
        a, b = old[name], new[name]
        out += [f"route.removed:{name}:{route.replace(' ', ':')}" for route in sorted(a["routes"] - b["routes"])]
        if a["sdk"] and not b["sdk"]:
            out.append(f"sdk.removed:{name}")
        for field, (kind, required) in sorted(a["inputs"].items()):
            if field not in b["inputs"]:
                out.append(f"input.removed:{name}.{field}")
                continue
            if b["inputs"][field][0] != kind:
                out.append(f"input.type:{name}.{field}")
            if b["inputs"][field][1] and not required:
                out.append(f"input.required:{name}.{field}")
        for field, (kind, required) in sorted(b["inputs"].items()):
            if required and field not in a["inputs"]:
                out.append(f"input.required:{name}.{field}")
        for field, (kind, _) in sorted(a["outputs"].items()):
            if field not in b["outputs"]:
                out.append(f"output.removed:{name}.{field}")
            elif b["outputs"][field][0] != kind:
                out.append(f"output.type:{name}.{field}")
    old_codes, new_codes = kept["failure-codes.json"], now["failure-codes.json"]
    for code, row in sorted(old_codes.items()):
        if code not in new_codes:
            out.append(f"code.removed:{code}")
            continue
        if new_codes[code]["class"] != row["class"]:
            out.append(f"code.class:{code}")
        if new_codes[code]["passes_by_itself"] != row["passes_by_itself"]:
            out.append(f"code.passes:{code}")
    for vocabulary, names in sorted(kept["vocabularies.json"].items()):
        have = now["vocabularies.json"].get(vocabulary, [])
        out += [f"vocabulary.removed:{vocabulary}.{n}" for n in names if n not in have]
        if vocabulary in CLOSED:
            out += [f"vocabulary.added:{vocabulary}.{n}" for n in have if n not in names]
    for key, kind in sorted(kept["settings.json"].items()):
        if key not in now["settings.json"]:
            out.append(f"setting.removed:{key}")
        elif now["settings.json"][key] != kind:
            out.append(f"setting.type:{key}")
    return out


def version_key(text):
    m = VERSION.match(text)
    return tuple(int(x) for x in m.groups()) if m else None


def kept_versions(root):
    folder = root / "contracts"
    if not folder.is_dir():
        return []
    return sorted((p.name for p in folder.iterdir() if p.is_dir() and version_key(p.name)), key=version_key)


def read_breaks(path, root, problems):
    """{break: record} listed in `path`; a line that cannot be read, or names no record, is a problem."""
    listed = {}
    if not path.is_file():
        return listed
    records = {p.name[:4] for p in (root / "docs" / "decisions").glob("[0-9][0-9][0-9][0-9]-*.md")}
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        m = BREAK_LINE.match(line)
        if not m:
            problems.append(f"{path.relative_to(root).as_posix()}:{number}: write each break as `<break> <record>`")
            continue
        if m.group(2) not in records:
            problems.append(f"{path.relative_to(root).as_posix()}:{number}: no decision record {m.group(2)} exists")
        listed[m.group(1)] = m.group(2)
    return listed


def read_kept(folder):
    return {name: json.loads((folder / name).read_text(encoding="utf-8")) for name in FILES}


def kept_problems(root, version):
    """Every way contracts/VERSION/ is not whole: a file missing, or a compiled target whose bytes are
    not the hash kept for them."""
    folder = root / "contracts" / version
    if not folder.is_dir():
        return [f"contracts/{version}/ is not there: a release keeps its contract before it is tagged; "
                f"run `python3 tools/contracts.py write {version}`"]
    problems = [f"contracts/{version}/{name} is not there" for name in FILES if not (folder / name).is_file()]
    if problems:
        return problems
    compiled = json.loads((folder / "compiled-targets.json").read_text(encoding="utf-8"))
    if not compiled:
        problems.append(f"contracts/{version}/compiled-targets.json lists no compiled target")
    for entry in compiled:
        path = folder / entry["file"]
        if not path.is_file():
            problems.append(f"contracts/{version}/{entry['file']} is not there")
        elif hashlib.sha256(path.read_bytes()).hexdigest() != entry["hash"]:
            problems.append(f"contracts/{version}/{entry['file']} is not the bytes its hash was kept for")
    return problems


def check(root, now=None):
    """(problems, compared): every break not listed, every listed break that does not happen, and every
    kept folder that is not whole."""
    problems = []
    versions = kept_versions(root)
    pending = read_breaks(root / NEXT_BREAKS, root, problems)
    if versions and now is None:
        now = tree(root)
    listed_by = {v: read_breaks(root / "contracts" / v / "breaks", root, problems) for v in versions}
    used = set()
    for i, version in enumerate(versions):
        whole = kept_problems(root, version)
        if whole:
            problems += whole
            continue
        allowed = dict(pending)
        for later in versions[i + 1:]:
            allowed.update(listed_by[later])
        for found in breaks(read_kept(root / "contracts" / version), now):
            if found in allowed:
                used.add(found)
            else:
                problems.append(f"against {version}: {found}")
    for found in sorted(set(pending) - used):
        problems.append(f"{NEXT_BREAKS}: `{found}` is listed and is not a break")
    return problems, len(versions)


def write(root, version):
    """Keep the tree's contract as contracts/VERSION/."""
    if not version_key(version):
        raise Fault(f"`{version}` is not a version of the form MAJOR.MINOR.PATCH")
    folder = root / "contracts" / version
    if folder.exists():
        raise Fault(f"contracts/{version}/ is kept already, and a kept folder is never changed")
    problems, _ = check(root)
    if problems:
        raise Fault("the tree breaks a kept release:\n" + "\n".join(f"- {p}" for p in problems))
    versions = kept_versions(root)
    listed = (root / NEXT_BREAKS).read_text(encoding="utf-8") if (root / NEXT_BREAKS).is_file() else ""
    if read_breaks(root / NEXT_BREAKS, root, []) and versions:
        last, this = version_key(versions[-1]), version_key(version)
        may_break = this[0] > last[0] if last[0] >= 1 else (this[0], this[1]) > (last[0], last[1])
        if not may_break:
            raise Fault(f"{NEXT_BREAKS} lists breaks, and {version} after {versions[-1]} may not break: "
                        "before 1.0 a minor version may, and from 1.0 only a major one")
    now = tree(root)
    staging = folder.with_name(version + ".partial")
    shutil.rmtree(staging, ignore_errors=True)
    (staging / "compiled").mkdir(parents=True)
    for name, data in now.items():
        (staging / name).write_text(json.dumps(data, indent=1, sort_keys=True, ensure_ascii=False) + "\n",
                                    encoding="utf-8")
    run = subprocess.run(
        ["cargo", "test", "-p", "a2m-sources", "--test", "kept_compiled_target", "--locked"],
        cwd=root, env={**os.environ, "A2M_CONTRACT_COMPILED_OUT": str(staging / "compiled")},
        capture_output=True, text=True,
    )
    targets = sorted((staging / "compiled").glob("example.format-*.target"))
    if run.returncode != 0 or not targets:
        shutil.rmtree(staging, ignore_errors=True)
        raise Fault("the example target's compiled target could not be written:\n" + (run.stdout + run.stderr)[-3000:])
    entries = [{"file": f"compiled/{p.name}", "format": int(p.name.split("format-")[1].split(".")[0]),
                "hash": hashlib.sha256(p.read_bytes()).hexdigest()} for p in targets]
    (staging / "compiled-targets.json").write_text(json.dumps(entries, indent=1) + "\n", encoding="utf-8")
    header = f"# The breaks {version} makes on purpose, each with its decision record.\n"
    (staging / "breaks").write_text(header + "".join(l + "\n" for l in listed.splitlines() if l.strip() and not l.startswith("#")),
                                    encoding="utf-8")
    staging.rename(folder)
    if (root / NEXT_BREAKS).is_file():
        (root / NEXT_BREAKS).unlink()
    return folder


def main(argv):
    root = Path(argv[argv.index("--root") + 1]).resolve() if "--root" in argv else HERE.parent
    args = [a for i, a in enumerate(argv[1:], 1) if a != "--root" and argv[i - 1] != "--root"]
    command = args[0] if args else "check"
    try:
        if command == "show":
            print(json.dumps(tree(root), indent=1, sort_keys=True, ensure_ascii=False))
            return 0
        if command == "write" and len(args) == 2:
            print(f"kept {write(root, args[1]).relative_to(root).as_posix()}/")
            return 0
        if command == "kept" and len(args) == 2:
            problems = kept_problems(root, args[1])
            for p in problems:
                print("-", p)
            print(f"contracts/{args[1]}/: {'whole' if not problems else f'{len(problems)} problem(s)'}")
            return 1 if problems else 0
        if command == "check":
            problems, compared = check(root)
            problems = sdk_problems(root) + problems
            for p in problems:
                print("-", p)
            print(f"{compared} kept release(s) compared; {len(problems)} problem(s)")
            return 1 if problems else 0
    except Fault as fault:
        print(f"fault: {fault}")
        return 1
    print(__doc__.split("\n\n")[1])
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
