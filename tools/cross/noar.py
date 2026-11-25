#!/usr/bin/env python3
"""Stand-in archiver for lint-only cross builds (decision 0999): writes an
empty archive, for an ar-style call (cq <archive> ...) or a lib.exe-style one
(-out:<archive>). It refuses to run outside a `cargo check`; see lintonly.py."""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lintonly import refuse_unless_lint_only  # noqa: E402

refuse_unless_lint_only()
args = sys.argv[1:]
out = None
for x in args:
    for p in ("-out:", "/out:", "-OUT:", "/OUT:"):
        if x.startswith(p):
            out = x[len(p):]
# A lib.exe-style option is a slash and a word (`/nologo`, `/out:x`); a path
# that begins with a slash, as every path on Linux does, is not one.
plain = [x for x in args if not x.startswith("-") and not re.match(r"^/[A-Za-z]+(?::|$)", x)]
if out is None and len(plain) >= 2 and plain[0] != "s":
    out = plain[1]
if out:
    with open(out, "wb") as f:
        f.write(b"!<arch>\n")
sys.exit(0)
