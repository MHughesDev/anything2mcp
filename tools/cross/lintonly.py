"""The guard of the lint-only stand-ins in this folder (decision 0999).

`nocc.py` and `noar.py` write empty objects and archives, so that a C
dependency's build script passes in a lint of another system's code from Linux,
where no C compiler for that system is installed. Code built with them is not
the code it claims to be, so they must never take part in a build that links or
ships. Each stand-in calls `refuse_unless_lint_only()` first, and refuses,
with a message and exit code 3, unless both of these hold:

- the variable `A2M_LINT_ONLY_STUBS` is `1`, which `tools/gate.py` sets for its
  cross lints alone, so that the stand-ins are never picked up by chance;
- the nearest cargo among the processes that started this one is running
  `cargo check`, which `cargo clippy` runs and which writes metadata only and
  never links. A `cargo build`, `cargo test`, `cargo run`, `cargo install`, or
  any other command is refused, and so is a run where the processes cannot be
  read: the guard reads `/proc`, so it holds on Linux, where the stand-ins are
  used, and fails closed anywhere else.
"""
import os
import sys

MARKER = "A2M_LINT_ONLY_STUBS"


def ancestors(pid=None, proc="/proc"):
    """The command lines of the processes above `pid`, nearest first."""
    pid = os.getpid() if pid is None else pid
    found = []
    for _ in range(64):
        with open(os.path.join(proc, str(pid), "stat"), encoding="utf-8", errors="replace") as f:
            stat = f.read()
        parent = int(stat[stat.rindex(")") + 2:].split(" ")[1])
        if parent <= 0:
            break
        with open(os.path.join(proc, str(parent), "cmdline"), "rb") as f:
            argv = [a.decode("utf-8", "replace") for a in f.read().split(b"\0") if a]
        found.append(argv)
        pid = parent
        if parent == 1:
            break
    return found


def cargo_subcommand(argv):
    """The subcommand of a cargo command line, or None if it is not cargo's."""
    if not argv or os.path.basename(argv[0]) not in ("cargo", "cargo.exe"):
        return None
    rest = [a for a in argv[1:] if not a.startswith("+")]
    for arg in rest:
        if not arg.startswith("-"):
            return arg
    return ""


def why_refused(env, chain):
    """Why a stand-in may not run here, or None when it may."""
    if env.get(MARKER) != "1":
        return f"{MARKER} is not 1: these stand-ins are for tools/gate.py's cross lints alone"
    for argv in chain:
        sub = cargo_subcommand(argv)
        if sub is None:
            continue
        if sub == "check":
            return None
        return f"the cargo that runs this is `cargo {sub}`, not `cargo check`: a stand-in object must never be linked or shipped"
    return "no cargo was found among the processes that started this one"


def refuse_unless_lint_only():
    try:
        chain = ancestors()
    except (OSError, ValueError, IndexError) as e:
        chain, reason = None, f"the processes that started this one could not be read ({e})"
    if chain is not None:
        reason = why_refused(os.environ, chain)
    if reason:
        sys.stderr.write(f"lint-only stand-in refused: {reason}\n")
        sys.exit(3)
