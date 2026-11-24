#!/usr/bin/env python3
# Stand-in C compiler for lint-only cross builds: nothing is linked, so each
# object asked for is written empty by the host compiler.
import subprocess, sys
a = sys.argv[1:]
out = None
for i, x in enumerate(a):
    if x == "-o" and i + 1 < len(a):
        out = a[i + 1]
    for p in ("-Fo", "/Fo"):
        if x.startswith(p) and len(x) > len(p):
            out = x[len(p):]
if "-E" in a or "/E" in a or "-?" in a or "--version" in a:
    sys.exit(0)
if out:
    sys.exit(subprocess.run(["cc", "-c", "-x", "c", "/dev/null", "-o", out]).returncode)
sys.exit(0)
