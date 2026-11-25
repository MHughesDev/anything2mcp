#!/usr/bin/env python3
"""Say how much of the roadmap is done, and keep every count of it in the plan true.

    python3 tools/progress.py            # print done of total, by section
    python3 tools/progress.py --write    # rewrite each generated count in place
    python3 tools/progress.py --check    # exit 1 if a generated count is stale
    python3 tools/progress.py --root DIR # work on another copy of the repository

A package counts once, whatever its size, and a package that is partly built
counts as not done, so the figure understates work in hand and never overstates
what is proven. Withdrawn packages are left out.

The counts are generated, never typed (decision 0453). Each sits between
`<!-- progress:begin NAME -->` and `<!-- progress:end -->` in docs/plan/roadmap.md
and docs/plan/build-order.md, and is made from the rows alone:

- `build-order`: done of total, and the packages left, sorted by how the line in
  build-order.md classes their rows (Owner, After the release, or an agent's:
  Build-ready, Needs design, In hand);
- `ready`: the roadmap's sentence naming every package marked `ready`;
- `milestone-N`: the count and the packages left of milestone N, in the
  roadmap's table of milestones.

The roadmap decides whether a package is done; the line decides only whose a
package left is. Where the two disagree, `--check` says so, and `--write`
refuses to write. After a merge conflicts in a generated count, take either
side and run `--write`. docs_audit.py runs the check.
Standard library only; Python 3.11 or newer. tools/test_progress.py proves it.
"""
import collections
import os
import re
import sys

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ROADMAP = os.path.join("docs", "plan", "roadmap.md")
BUILD_ORDER = os.path.join("docs", "plan", "build-order.md")
LINE_HEADING = "## The line: every package left, and whether a worker can take it"
LINE_ANCHOR = "the-line-every-package-left-and-whether-a-worker-can-take-it"
REGION = re.compile(r"<!-- progress:begin ([a-z0-9-]+) -->(.*?)<!-- progress:end -->", re.S)
PACKAGE = re.compile(r"^[A-Z0-9][A-Za-z0-9.]*$")
WORDS = ("no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
         "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen",
         "eighteen", "nineteen", "twenty")
# The classes of the line, in the order the progress sentence gives them.
OWNER, AFTER, AGENT, DONE = "owner", "after the release", "agent", "done"
AGENT_ONE, AGENT_MANY = "is an agent's to finish before it", "are an agent's to finish before it"


def read(root, name):
    with open(os.path.join(root, name), encoding="utf-8") as f:
        return f.read()


def packages(text):
    """The roadmap's rows, in order: (package, state, section). Withdrawn rows are kept."""
    section, found = None, []
    for line in text.splitlines():
        if line.startswith("## ") or line.startswith("### "):
            section = line.strip("# ")
        if line.startswith("| ") and not line.startswith("| #") and not line.startswith("| ---"):
            cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
            if len(cells) >= 6 and PACKAGE.match(cells[0]):
                found.append((cells[0], cells[-1].split()[0].strip(".,:;(").lower() if cells[-1] else "", section))
    return found


def count(path=None):
    """Done and total by section, in the roadmap's order, withdrawn rows left out."""
    with open(path or os.path.join(HERE, ROADMAP), encoding="utf-8") as f:
        rows = packages(f.read())
    order, total, done = [], collections.Counter(), collections.Counter()
    for _, state, section in rows:
        if section not in order:
            order.append(section)
        if state == "withdrawn":
            continue
        total[section] += 1
        done[section] += state == "done"
    return order, total, done


def line(text):
    """The line's rows, in order: (packages of the row, class)."""
    if LINE_HEADING not in text:
        return []
    part = text.split(LINE_HEADING, 1)[1].split("\n## ", 1)[0].split("\n### ", 1)[0]
    rows = []
    for row in part.splitlines():
        if not row.startswith("| ") or row.startswith("| ---") or row.startswith("| Row "):
            continue
        cells = [cell.strip() for cell in row.strip().strip("|").split("|")]
        if len(cells) < 2:
            continue
        names = [n.strip() for n in re.sub(r"\(new\)", "", cells[0].replace("~~", "")).split(",")]
        kind = cells[1].replace("~~", "").strip()
        if "~~" in cells[0] or re.match(r"(?i)done\b", kind) or re.search(r"(?i),\s*done\b", kind):
            cls = DONE
        elif kind.startswith("Owner"):
            cls = OWNER
        elif kind.startswith("After the release"):
            cls = AFTER
        else:
            cls = AGENT
        rows.append(([n for n in names if n], cls))
    return rows


def word(n):
    return WORDS[n] if n < len(WORDS) else str(n)


def listed(names):
    if len(names) < 3:
        return " and ".join(names)
    return ", ".join(names[:-1]) + ", and " + names[-1]


def facts(root):
    """What the rows say, and every way the roadmap and the line disagree."""
    rows = packages(read(root, ROADMAP))
    state = {name: s for name, s, _ in rows}
    left = [name for name, s, _ in rows if s not in ("done", "withdrawn")]
    problems, cls_of = [], {}
    for names, cls in line(read(root, BUILD_ORDER)):
        for name in names:
            if name not in state:
                problems.append(f"build-order.md: the line names {name}, which is no row of the roadmap")
            elif cls == DONE and state[name] != "done":
                problems.append(f"build-order.md: the line marks {name} done; the roadmap marks it {state[name]}")
            elif cls != DONE and state[name] == "done":
                problems.append(f"build-order.md: the line still gives {name} as {cls}; the roadmap marks it done")
            elif cls != DONE and name in cls_of:
                problems.append(f"build-order.md: the line gives {name} in two rows")
            elif cls != DONE:
                cls_of[name] = cls
    for name in left:
        if name not in cls_of:
            problems.append(f"build-order.md: {name} is left in the roadmap and has no row in the line")
    return rows, left, cls_of, problems


def render(root):
    """The text of every generated count, by its region's name."""
    rows, left, cls_of, problems = facts(root)
    counted = [r for r in rows if r[1] != "withdrawn"]
    done = sum(1 for r in counted if r[1] == "done")
    total = len(counted)
    out = {}
    by = {c: [n for names, k in line(read(root, BUILD_ORDER)) for n in names
              if k == c and cls_of.get(n) == c] for c in (OWNER, AFTER, AGENT)}

    def some(names, one, many):
        return f"{len(names)} {one if len(names) == 1 else many}" + (f" ({', '.join(names)})" if names else "")
    out["build-order"] = (
        f"{done} of {total} packages done, {100 * done // total if total else 0}%, counted from the "
        f"roadmap's rows. Of the {len(left)} left, {some(by[OWNER], 'waits on the owner', 'wait on the owner')}, "
        f"{some(by[AFTER], 'on the first release', 'on the first release')}, and "
        f"{some(by[AGENT], AGENT_ONE, AGENT_MANY)}; "
        f"which of them a worker can take is in [the line](#{LINE_ANCHOR}) below.")
    ready = [name for name, s, _ in rows if s == "ready"]
    if not ready:
        out["ready"] = "Right now, no package is marked `ready`."
    elif len(ready) == 1:
        out["ready"] = f"Right now, one package is marked `ready`: {ready[0]}."
    else:
        out["ready"] = f"Right now, the packages marked `ready` are {listed(ready)}: {word(len(ready))}."
    for section in dict.fromkeys(r[2] for r in rows):
        m = re.match(r"Milestone (\d+):", section or "")
        if not m:
            continue
        mine = [r for r in rows if r[2] == section]
        n_total = sum(1 for r in mine if r[1] != "withdrawn")
        n_done = sum(1 for r in mine if r[1] == "done")
        text = f"{n_done} of {n_total} done"
        groups = collections.OrderedDict()
        for name, s, _ in mine:
            if s != "done":
                groups.setdefault(s, []).append(name)
        if groups:
            text += "; " + "; ".join(f"{s}: {', '.join(names)}" for s, names in groups.items())
        out[f"milestone-{m.group(1)}"] = text
    return out, problems


def regions(text):
    return [(m.group(1), m.group(2), m.start(2), m.end(2)) for m in REGION.finditer(text)]


def check(root=HERE):
    """Every way a generated count is stale or missing, as sentences."""
    want, problems = render(root)
    seen = set()
    for name in (ROADMAP, BUILD_ORDER):
        for region, body, _, _ in regions(read(root, name)):
            where = os.path.basename(name)
            if region in seen:
                problems.append(f"{where}: the count {region} is given twice")
            seen.add(region)
            if region not in want:
                problems.append(f"{where}: no rows make the count {region}")
            elif body != want[region]:
                problems.append(f"{where}: the count {region} is stale; run python3 tools/progress.py --write")
    for region in want:
        if region not in seen:
            problems.append(f"the count {region} has no place between progress markers in the plan")
    return problems


def write(root=HERE):
    """Rewrite every generated count in place. Refuses while the rows disagree."""
    want, problems = render(root)
    if problems:
        return problems
    for name in (ROADMAP, BUILD_ORDER):
        text = read(root, name)
        new = REGION.sub(lambda m: f"<!-- progress:begin {m.group(1)} -->{want.get(m.group(1), m.group(2))}"
                                   f"<!-- progress:end -->", text)
        if new != text:
            with open(os.path.join(root, name), "w", encoding="utf-8", newline="\n") as f:
                f.write(new)
    return check(root)


def main(argv):
    root = os.path.abspath(argv[argv.index("--root") + 1]) if "--root" in argv else HERE
    if "--write" in argv or "--check" in argv:
        problems = write(root) if "--write" in argv else check(root)
        for problem in problems:
            print(problem)
        print(f"{len(problems)} stale or disagreeing count(s)")
        return 1 if problems else 0
    order, total, done = count(os.path.join(root, ROADMAP))
    all_total, all_done = sum(total.values()), sum(done.values())
    print(f"{all_done} of {all_total} packages done: {100 * all_done // all_total}%")
    for section in order:
        if total[section]:
            print(f"  {done[section]:>2} of {total[section]:<2} {section}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
