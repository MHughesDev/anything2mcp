#!/usr/bin/env python3
"""Say how much of the roadmap is done: packages marked done over all packages.

    python3 tools/progress.py

A package counts once, whatever its size, and a package that is partly built
counts as not done, so the figure understates work in hand and never overstates
what is proven. Withdrawn packages are left out.
"""
import collections
import os
import re

ROADMAP = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "docs", "plan", "roadmap.md")


def count():
    section, total, done, order = None, collections.Counter(), collections.Counter(), []
    for line in open(ROADMAP, encoding="utf-8"):
        if line.startswith("## ") or line.startswith("### "):
            section = line.strip("# \n")
            if section not in order:
                order.append(section)
        if line.startswith("| ") and not line.startswith("| #") and not line.startswith("| ---"):
            cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
            if len(cells) >= 6 and re.match(r"^[A-Z0-9][A-Za-z0-9.]*$", cells[0]):
                state = cells[-1].lower()
                if state.startswith("withdrawn"):
                    continue
                total[section] += 1
                if state.startswith("done"):
                    done[section] += 1
    return order, total, done


if __name__ == "__main__":
    order, total, done = count()
    all_total, all_done = sum(total.values()), sum(done.values())
    print(f"{all_done} of {all_total} packages done: {100 * all_done // all_total}%")
    for section in order:
        if total[section]:
            print(f"  {done[section]:>2} of {total[section]:<2} {section}")
