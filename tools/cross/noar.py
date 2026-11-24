#!/usr/bin/env python3
# Stand-in archiver for lint-only cross builds: writes an empty archive,
# for an ar-style call (cq <archive> ...) or a lib.exe-style one (-out:<archive>).
import sys
args = sys.argv[1:]
out = None
for x in args:
    for p in ("-out:", "/out:", "-OUT:", "/OUT:"):
        if x.startswith(p):
            out = x[len(p):]
plain = [x for x in args if not x.startswith("-") and not x.startswith("/")]
if out is None and len(plain) >= 2 and plain[0] != "s":
    out = plain[1]
if out:
    with open(out, "wb") as f:
        f.write(b"!<arch>\n")
sys.exit(0)
