#!/usr/bin/env python3
"""Compares the engine's decompiled text with the port's, function by function.

usage: compare.py <engine-dump> <port-dump> [--show N]

Both dumps use the format of GhidraDump.java and src/main.rs. Functions are
matched by address once the engine's image base is taken off. Four tiers, each
looser than the one before: exact; equal after runs of blanks are collapsed;
equal after the engine's addresses and `[hash]` marks are put in the port's
form; different. Prints one table row per program and the first differences.
"""
import difflib
import re
import sys


def parse(path):
    info = {"base": 0, "lang": "", "funcs": {}, "texts": {}, "ms": {}}
    cur = None
    for line in open(path, encoding="utf-8", errors="replace").read().split("\n"):
        if cur is not None:
            if line == "E":
                info["texts"][cur[0]] = "\n".join(cur[1]) + "\n"
                cur = None
            else:
                cur[1].append(line)
            continue
        if line.startswith("L\t"):
            parts = line.split("\t")
            info["lang"] = parts[1]
            for p in parts[2:]:
                if p.startswith("imageBase="):
                    info["base"] = int(p[10:], 16)
        elif line.startswith("F\t"):
            p = line.split("\t")
            info["funcs"][int(p[1], 16)] = p[2]
        elif line.startswith("D\t"):
            p = line.split("\t")
            addr = int(p[1], 16)
            info["ms"][addr] = p[4] if len(p) > 4 else ""
            cur = (addr, [])
    return info


def rebase(info):
    b = info["base"]
    info["funcs"] = {a - b: n for a, n in info["funcs"].items()}
    info["texts"] = {a - b: t for a, t in info["texts"].items()}


def squeeze(t):
    return "\n".join(re.sub(r"[ \t]+", " ", l.strip()) for l in t.split("\n") if l.strip())


def unshift(t, base):
    """Puts the engine's addresses in the port's form (image base taken off)."""
    def hexfix(m):
        v = int(m.group(2), 16)
        if base <= v < base + 0x10000000:
            return m.group(1) + format(v - base, "x" if m.group(1) == "0x" else "08x").rjust(len(m.group(2)), "0")
        return m.group(0)

    t = re.sub(r"(0x)([0-9a-fA-F]{5,})", hexfix, t)
    t = re.sub(r"(_)([0-9a-f]{8})\b", hexfix, t)
    t = re.sub(r"\[[0-9a-f]{8,16}\]", "", t)
    return t


TYPES = {"int1": "char", "int2": "short", "int4": "int", "int8": "long", "uint1": "uchar",
         "uint2": "ushort", "uint4": "uint", "uint8": "ulong"}


def canon(t):
    """Spelling only: the port prints the decompiler core's type names and variable
    prefixes (`int4`, `xunknown8`, `iStack_10`); the engine prints its own (`int`,
    `undefined8`, `local_10`). Both are put in one form."""
    t = re.sub(r"\bxunknown(\d+)\b", r"undefined\1", t)
    t = re.sub(r"\b(u?int[1248])\b", lambda m: TYPES[m.group(1)], t)
    t = re.sub(r"\b[a-z]{1,3}Var(\d+)\b", r"Var\1", t)
    t = re.sub(r"\b(?:[a-z]{1,3}Stack|local)_([0-9a-f]+)\b", r"stk_\1", t)
    return t


def ratio(a, b):
    return difflib.SequenceMatcher(None, a, b, autojunk=False).ratio()


def main():
    eng = parse(sys.argv[1])
    port = parse(sys.argv[2])
    show = int(sys.argv[sys.argv.index("--show") + 1]) if "--show" in sys.argv else 3
    base = eng["base"]
    rebase(eng)
    rebase(port)
    both = sorted(set(port["texts"]) & set(eng["texts"]))
    only_port_dec = sorted(set(port["texts"]) - set(eng["texts"]))
    only_eng_dec = sorted(set(eng["texts"]) - set(port["texts"]))
    tiers = {"exact": [], "blanks": [], "addresses": [], "spelling": [], "different": []}
    ratios = {}
    for a in both:
        e, p = eng["texts"][a], port["texts"][a]
        if e == p:
            tiers["exact"].append(a)
        elif squeeze(e) == squeeze(p):
            tiers["blanks"].append(a)
        elif squeeze(unshift(e, base)) == squeeze(p):
            tiers["addresses"].append(a)
        elif canon(squeeze(unshift(e, base))) == canon(squeeze(p)):
            tiers["spelling"].append(a)
        else:
            tiers["different"].append(a)
            ratios[a] = ratio(canon(squeeze(unshift(e, base))).split("\n"), canon(squeeze(p)).split("\n"))
    fe, fp = set(eng["funcs"]), set(port["funcs"])
    print(f"engine {eng['lang']} base {base:#x}; port {port['lang']}")
    print(f"functions listed: engine {len(fe)}, port {len(fp)}, both {len(fe & fp)}, "
          f"only engine {len(fe - fp)}, only port {len(fp - fe)}")
    print(f"decompiled by port: {len(port['texts'])}; by engine: {len(eng['texts'])}; "
          f"compared (both): {len(both)}; port only: {len(only_port_dec)}; engine only: {len(only_eng_dec)}")
    print("| exact | blanks only | addresses/hash marks only | spelling only | different |")
    print("| --- | --- | --- | --- | --- |")
    print("| " + " | ".join(str(len(tiers[k])) for k in tiers) + " |")
    if "--names" in sys.argv:
        for a in sorted(fe ^ fp):
            print(("engine-only " if a in fe else "port-only   ") + f"{a:#x} {(eng['funcs'].get(a) or port['funcs'].get(a))[:100]}")
    if ratios:
        print("line similarity of the different ones after spelling: min %.2f median %.2f max %.2f" % (
            min(ratios.values()), sorted(ratios.values())[len(ratios) // 2], max(ratios.values())))
    for a in tiers["different"][:show]:
        e = canon(squeeze(unshift(eng["texts"][a], base))).split("\n")
        p = canon(squeeze(port["texts"][a])).split("\n")
        print(f"\n--- {a:#x} {port['funcs'].get(a, '')[:80]}")
        for l in list(difflib.unified_diff(e, p, "engine", "port", n=0, lineterm=""))[:16]:
            print(l)
    if "--rows" in sys.argv:
        for k, v in tiers.items():
            for a in v:
                print(f"ROW\t{a:#x}\t{port['funcs'].get(a, '')[:70]}\t{k}\t{len(port['texts'][a].splitlines())}\t{ratios.get(a, 1.0):.2f}")
        for a in only_port_dec:
            print(f"ROW\t{a:#x}\t{port['funcs'].get(a, '')[:70]}\tport-only\t{len(port['texts'][a].splitlines())}")
        for a in only_eng_dec:
            print(f"ROW\t{a:#x}\t{eng['funcs'].get(a + 0, '')[:70]}\tengine-only\t{len(eng['texts'][a].splitlines())}")


main()
