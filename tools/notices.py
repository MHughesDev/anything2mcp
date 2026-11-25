#!/usr/bin/env python3
"""Write, or check, the notices file: the licence texts of every third-party crate in the binary.

Usage, from the repository root:

    python3 tools/notices.py write     # write THIRD-PARTY-NOTICES.txt and its copy in bindings/python/
    python3 tools/notices.py check     # exit 1 unless both files are what `write` would write
    ... --root DIR                     # work on another copy of the repository

Spec section 605 asks that a release built for people to install carries the licence texts of what it
contains. The crates are the ones `cargo tree` gives for the product crate, normal dependencies only,
for each of the eight targets the release builds, as `Cargo.lock` fixes them: the same command and the
same set as the release workflow's bill of materials. The crates of this workspace have a path and are
left out; their licence is `LICENSE`. For each crate, `cargo metadata` gives its licence expression, its
repository, and the folder its source was unpacked to, and every file at the top of that folder whose
name begins LICENSE, LICENCE, COPYING, COPYRIGHT, NOTICE, or UNLICENSE is a licence text, with the file
the manifest names in `license-file`. A few crates publish their source with no licence text (on
2026-11-25, seven of the macOS bindings of one project). For such a crate whose expression offers MIT,
alone or as one choice, the file carries the MIT text, taken from this repository's own `LICENSE` with
its copyright line replaced by one naming the crate's authors and repository, and says that the tool
supplied it. A crate with no text and no MIT choice is a failure, never a guess.

The file lists every crate once, then every distinct text once, under the crates that carry it, so that
the many identical copies of one licence are printed once. Line endings are made `\n` and trailing
spaces dropped; nothing else in a text is changed. The output depends on `Cargo.lock` and the crates'
published sources alone, never on where they were unpacked, so CI can hold it to the tree.

`bindings/python/THIRD-PARTY-NOTICES.txt` is an exact copy, because a wheel's licence files must lie
under its own project folder. Standard library only; Python 3.11 or newer.
"""
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

PRODUCT = "anything2mcp"
NOTICES = "THIRD-PARTY-NOTICES.txt"
COPY = "bindings/python/THIRD-PARTY-NOTICES.txt"
# The eight targets the release builds; tools/test_release_files.py holds tools/release_files.py to them.
TARGETS = (
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin", "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc", "aarch64-pc-windows-msvc",
)
TEXT_NAME = re.compile(r"^(licen[cs]e|copying|copyright|notice|unlicense)", re.I)
TREE_LINE = re.compile(r"^(\S+) v(\S+)$")


class NoticeError(Exception):
    pass


def cargo(root, *args):
    done = subprocess.run(["cargo", *args], cwd=root, capture_output=True, text=True)
    if done.returncode != 0:
        raise NoticeError(f"cargo {' '.join(args)} failed:\n{done.stderr.strip()}")
    return done.stdout


def crates_in_binary(root):
    """The (name, version) of every third-party crate compiled into the product, on any target."""
    found = set()
    for target in TARGETS:
        out = cargo(root, "tree", "--locked", "--package", PRODUCT, "--edges", "normal", "--target", target,
                    "--no-dedupe", "--prefix", "none", "--format", "{p}")
        for line in out.splitlines():
            line = line.strip()
            if not line or " (" in line:
                # A crate with a path is one of this workspace's; `(*)` cannot occur with --no-dedupe.
                continue
            m = TREE_LINE.match(line)
            if not m:
                raise NoticeError(f"cargo tree printed a line this does not read: {line!r}")
            found.add((m.group(1), m.group(2)))
    if not found:
        raise NoticeError("cargo tree gave no third-party crate; the product cannot be built from none")
    return found


def packages(root):
    meta = json.loads(cargo(root, "metadata", "--locked", "--format-version", "1"))
    by_key = {}
    for p in meta["packages"]:
        if p.get("source") is None:
            continue
        by_key.setdefault((p["name"], p["version"]), []).append(p)
    return by_key


def normalize(text):
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    while lines and not lines[0]:
        lines.pop(0)
    while lines and not lines[-1]:
        lines.pop()
    return "\n".join(lines) + "\n"


def texts_of(package):
    folder = Path(package["manifest_path"]).parent
    names = {f.name for f in folder.iterdir() if f.is_file() and TEXT_NAME.match(f.name)}
    if package.get("license_file"):
        named = (folder / package["license_file"]).resolve()
        if named.is_file():
            try:
                names.add(named.relative_to(folder.resolve()).as_posix())
            except ValueError:
                raise NoticeError(f"{package['name']} {package['version']} names a licence file outside its source")
    texts = []
    for name in sorted(names):
        raw = (folder / name).read_bytes()
        try:
            text = raw.decode("utf-8")
        except UnicodeDecodeError:
            text = raw.decode("latin-1")
        text = normalize(text)
        if text.strip():
            texts.append((name, text))
    return texts


def supplied_mit(root, package):
    """The MIT text for a crate that carries none, or None when its expression offers no MIT."""
    if "MIT" not in re.split(r"[\s()]+", package.get("license") or ""):
        return None
    body = (root / "LICENSE").read_text(encoding="utf-8").split("\n")
    lines = [line for line in body if not line.startswith("Copyright")]
    if len(lines) != len(body) - 1:
        raise NoticeError("LICENSE does not have exactly one copyright line to replace")
    at = next(i for i, line in enumerate(body) if line.startswith("Copyright"))
    where = package.get("repository") or "its published source"
    lines.insert(at, f"Copyright (c) the authors of {package['name']} ({where})")
    note = ("[The source of this crate as published carries no licence text. tools/notices.py supplied the "
            "MIT text, which its licence expression offers.]")
    return "supplied MIT text", note + "\n\n" + normalize("\n".join(lines))


def render(root):
    crates = crates_in_binary(root)
    by_key = packages(root)
    rows, carried, missing = [], {}, []
    for key in sorted(crates):
        found = by_key.get(key)
        if not found or len(found) != 1:
            raise NoticeError(f"cargo metadata has {len(found or [])} packages for {key[0]} {key[1]}; expected one")
        p = found[0]
        licence = p.get("license") or "(none stated)"
        rows.append(f"{p['name']} {p['version']}  |  {licence}  |  {p.get('repository') or '(no repository stated)'}")
        texts = texts_of(p)
        if not texts:
            supplied = supplied_mit(root, p)
            texts = [supplied] if supplied else []
        if not texts:
            missing.append(f"{p['name']} {p['version']} ({licence})")
        for name, text in texts:
            carried.setdefault(text, []).append(f"{p['name']} {p['version']} ({name})")
    if missing:
        raise NoticeError("these crates carry no licence text in their source, and the notices file needs one "
                          "for each: " + ", ".join(missing))
    out = [
        "THIRD-PARTY NOTICES",
        "",
        "Anything2MCP is under the MIT licence; see LICENSE. The anything2mcp program is built with the",
        "third-party Rust crates below, as Cargo.lock fixes them, on one or more of the eight targets the",
        "release builds. Each crate's licence text follows the list, once for every distinct text, under",
        "the crates that carry it. Written by tools/notices.py from Cargo.lock; do not edit by hand.",
        "",
        f"Crates: {len(rows)}. Distinct licence texts: {len(carried)}.",
        "",
        "=" * 78,
        "CRATES  (name version  |  licence  |  repository)",
        "=" * 78,
        "",
        *rows,
    ]
    for text, who in sorted(carried.items(), key=lambda item: (item[1][0], hashlib.sha256(item[0].encode()).hexdigest())):
        out += ["", "", "=" * 78, "Carried by:", *(f"  {w}" for w in who), "-" * 78, "", text.rstrip("\n")]
    return "\n".join(out) + "\n"


def main(argv):
    root = Path(argv[argv.index("--root") + 1]).resolve() if "--root" in argv else Path(__file__).resolve().parent.parent
    command = next((a for a in argv[1:] if a in ("write", "check")), None)
    if command is None:
        print(__doc__.strip().split("\n\n")[1])
        return 2
    try:
        wanted = render(root)
    except NoticeError as error:
        print(f"error: {error}")
        return 1
    if command == "write":
        for rel in (NOTICES, COPY):
            (root / rel).write_text(wanted, encoding="utf-8", newline="\n")
        print(f"wrote {NOTICES} and {COPY}")
        return 0
    stale = [rel for rel in (NOTICES, COPY)
             if not (root / rel).is_file() or (root / rel).read_text(encoding="utf-8") != wanted]
    for rel in stale:
        print(f"- {rel} is not what tools/notices.py writes from Cargo.lock; run python3 tools/notices.py write "
              "and commit it")
    print(f"notices: {len(stale)} problem(s)")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
