"""Port coverage of 1.16.1 StarCraft.exe, per original source file.

usage: python -I re/coverage.py <decomp.c> [src_dir] [--html out.html] [--missing game_step|command|both]

<decomp.c> is the Ghidra export (`// ==== 00ADDR NAME` headers; not part of the repo).
Functions are tagged with a source file when their body mentions an assert path like
"Starcraft\\SWAR\\lang\\unit.cpp"; untagged functions inherit the file of the tagged
functions around them (MSVC keeps each object file contiguous). A function counts as
ported when src/ documents its address (`/// 0x...`), and as todo when only a
`todo_port!("0x...")` names it.
"""
import html
import os
import re
import sys
from collections import Counter, defaultdict

HEADER = re.compile(r"^// ==== 00([0-9a-f]{6}) (\S+)")
SRC = re.compile(r'"Starcraft\\\\SWAR\\\\lang\\\\([^"]+?\.(?:cpp|c))"', re.I)
DOC = re.compile(r"///\s*(?:[^\n]*?)\b0x(4[0-9a-f]{5})\b")
CALL = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*\(")
LIST = re.compile(r"///\s*(0x4[0-9a-f]{5}(?:\s*(?:/|,|\+|and|or|\(|\))\s*0x4[0-9a-f]{5})+)")
ADDR = re.compile(r"0x(4[0-9a-f]{5})")
HEAD = re.compile(r"///\s*(0x4[0-9a-f]{5}[^:\n]*):")
TODO = re.compile(r'todo_port!\("0x(4[0-9a-f]{5})"')

# Entry points whose call trees make up the simulation.
SIM_ROOTS = [
    ("game_step 0x4d94b0", [0x4d94b0]),
    ("command dispatch 0x4865d0", [0x4865d0]),
    ("both", [0x4d94b0, 0x4865d0]),
]

# Out of scope: networking is rewritten from scratch (and Storm's SNet lives outside the exe).
EXCLUDE = re.compile(r"^(net_|gluBN|gluChat|gluJoin|gluCreate|gluConn|snp|bnet|battle)", re.I)


def functions(path):
    funcs = []  # (addr, name, files)
    cur = None
    with open(path, errors="replace") as f:
        for line in f:
            m = HEADER.match(line)
            if m:
                cur = [int(m.group(1), 16), m.group(2), Counter(), set(), 0]
                funcs.append(cur)
            elif cur is not None:
                cur[4] += 1
                for s in SRC.findall(line):
                    cur[2][s.replace("\\\\", "/").lower()] += 1
                cur[3].update(CALL.findall(line))
    funcs.sort()
    return funcs


def assign(funcs):
    tag = [f[2].most_common(1)[0][0] if f[2] else None for f in funcs]
    out = list(tag)
    prev = None
    for i, t in enumerate(tag):
        if t:
            prev = t
        else:
            nxt = next((u for u in tag[i + 1:] if u), None)
            # Between two functions of one file: that file. Otherwise unknown, reported
            # as the gap between the neighbouring files.
            out[i] = prev if prev == nxt else f"? {prev or 'start'} .. {nxt or 'end'}"
    return out


def scan_src(root):
    ported, todo = set(), set()
    for d, _, files in os.walk(root):
        for n in files:
            if n.endswith(".rs"):
                t = open(os.path.join(d, n), errors="replace").read()
                ported |= {int(a, 16) for a in DOC.findall(t)}
                # A doc line may open with several addresses: "/// 0xA / 0xB + 0xC:".
                for run in LIST.findall(t):
                    ported |= {int(a, 16) for a in ADDR.findall(run)}
                # "/// 0xA (set) / 0xB (clear): ...": every address before the first colon.
                for head in HEAD.findall(t):
                    ported |= {int(a, 16) for a in ADDR.findall(head)}
                todo |= {int(a, 16) for a in TODO.findall(t)}
    return ported, todo - ported


def main():
    args = sys.argv[1:]
    out_html = None
    missing = None
    if "--missing" in args:
        i = args.index("--missing")
        missing = args[i + 1]
        del args[i:i + 2]
    if "--html" in args:
        i = args.index("--html")
        out_html = args[i + 1]
        del args[i:i + 2]
    decomp = args[0]
    src = args[1] if len(args) > 1 else os.path.join(os.path.dirname(__file__), "..", "src")
    funcs = functions(decomp)
    files = assign(funcs)
    ported, todo = scan_src(src)
    rows = defaultdict(lambda: [0, 0, 0, []])
    for (addr, name, *_), fn in zip(funcs, files):
        r = rows[fn]
        r[0] += 1
        state = "ported" if addr in ported else "todo" if addr in todo else ""
        r[1] += state == "ported"
        r[2] += state == "todo"
        r[3].append((addr, name, state))
    excl = lambda fn: not fn.startswith("?") and bool(EXCLUDE.match(fn.split("/")[-1]))
    tot = [0, 0, 0]
    print(f"{'source file':60} {'funcs':>6} {'ported':>7} {'todo':>5}  %")
    for fn in sorted(rows, key=lambda k: (excl(k), k.startswith("?"), -rows[k][0])):
        n, p, t, _ = rows[fn]
        mark = " (excluded)" if excl(fn) else ""
        if not excl(fn):
            tot[0] += n
            tot[1] += p
            tot[2] += t
        print(f"{fn[:60]:60} {n:6} {p:7} {t:5} {100 * p // max(n, 1):3}{mark}")
    known = sum(rows[k][0] for k in rows if not k.startswith("?"))
    print(f"\n{known} of {len(funcs)} functions placed in a source file; the rest are in gaps "
          "between files (likely still the neighbouring file, or library code).")
    print(f"in scope: {tot[0]} functions, {tot[1]} ported ({100 * tot[1] // max(tot[0], 1)}%), "
          f"{tot[2]} more named by todo_port!, all: {len(funcs)}")
    pa = sum(1 for f in funcs if f[0] in ported)
    print(f"all functions: {len(funcs)} functions, {pa} ported ({100 * pa // max(len(funcs), 1)}%), "
          f"{sum(1 for f in funcs if f[0] in todo)} named by todo_port!")
    # Reachability from the simulation entry points (direct calls only; Ghidra misses
    # some register-passed and indirect calls, so this undercounts).
    by_name = {f[1]: f[0] for f in funcs}
    calls = {f[0]: {by_name[n] for n in f[3] if n in by_name} for f in funcs}
    for label, roots in SIM_ROOTS:
        seen, stack = set(), list(roots)
        while stack:
            a = stack.pop()
            if a in seen or a not in calls:
                continue
            seen.add(a)
            stack.extend(calls[a])
        p = len(seen & ported)
        print(f"reachable from {label}: {len(seen)} functions, {p} ported ({100 * p // max(len(seen), 1)}%), "
              f"{len(seen & todo)} named by todo_port!")
        if missing and label.startswith(missing):
            # Unported reachable functions, those called by ported code first (the frontier),
            # then by how many reachable functions call them.
            info = {f[0]: f for f in funcs}
            file_of = {f[0]: fn for f, fn in zip(funcs, files)}
            callers = defaultdict(set)
            for a in seen:
                for c in calls[a]:
                    callers[c].add(a)
            rows_m = []
            for a in seen - ported:
                pc = len(callers[a] & ported)
                rows_m.append((-(pc > 0), -len(callers[a]), a, pc, len(callers[a]), info[a][4]))
            rows_m.sort()
            print(f"\nunported, reachable from {label}: {len(rows_m)}")
            print(f"{'addr':10} {'name':24} {'ported callers':>14} {'callers':>8} {'lines':>6}  file")
            for _, _, a, pc, nc, ln in rows_m:
                st = " todo" if a in todo else ""
                print(f"0x{a:06x}   {info[a][1][:24]:24} {pc:14} {nc:8} {ln:6}  {file_of[a]}{st}")
    if out_html:
        with open(out_html, "w") as o:
            o.write("<!doctype html><meta charset=utf-8><title>scbw coverage</title>"
                    "<style>body{font:13px monospace}.ported{color:#080}.todo{color:#b60}"
                    "summary{cursor:pointer}</style>")
            for fn in sorted(rows, key=lambda k: -rows[k][0]):
                n, p, t, fl = rows[fn]
                o.write(f"<details><summary>{html.escape(fn)} — {p}/{n} ported, {t} todo"
                        f"{' (excluded)' if excl(fn) else ''}</summary><ul>")
                for addr, name, st in fl:
                    o.write(f"<li class={st or 'none'}>0x{addr:06x} {html.escape(name)} {st}</li>")
                o.write("</ul></details>")


if __name__ == "__main__":
    main()
