"""Port coverage of 1.16.1 StarCraft.exe, per original source file.

usage: python -I re/coverage.py <decomp.c> [src_dir] [--html out.html] [--missing game_step|command|both|front|ai|ai-front|rest] [--all]

Unported functions are split by re/buckets.txt into "replaced" (the Rust port has its own
equivalent: CRT, Storm / Win32, UI redraw plumbing), "deferred" (save / load, networking)
and "left" (still to port). `--missing` lists only the left ones unless `--all` is given.

<decomp.c> is the Ghidra export (`// ==== 00ADDR NAME` headers; not part of the repo).
Functions are tagged with a source file when their body mentions an assert path like
"Starcraft\\SWAR\\lang\\unit.cpp"; untagged functions inherit the file of the tagged
functions around them (MSVC keeps each object file contiguous). A function counts as
ported when src/ documents its address (`/// 0x...` on the first line of a doc
comment, or at the start of a later doc line), and as todo when only a
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
START = re.compile(r"///\s*0x4[0-9a-f]{5}\b")
LIST = re.compile(r"///\s*(0x4[0-9a-f]{5}(?:\s*(?:/|,|\+|and|or|\(|\))\s*0x4[0-9a-f]{5})+)")
ADDR = re.compile(r"0x(4[0-9a-f]{5})")
HEAD = re.compile(r"///\s*(0x4[0-9a-f]{5}[^:\n]*):")
TODO = re.compile(r'todo_port!\("0x(4[0-9a-f]{5})"')

# Entry points whose call trees make up the simulation.
SIM_ROOTS = [
    ("game_step 0x4d94b0", [0x4d94b0]),
    ("command dispatch 0x4865d0", [0x4865d0]),
    ("both", [0x4d94b0, 0x4865d0]),
    ("front end 0x4de200", [0x4de200]),
]

# Entry points into the computer players' AI. Functions only reachable through these
# are counted in a separate "AI" bucket (traversal from the simulation roots stops at
# them).
AI_ROOTS = {
    0x4a2b00,  # ai_update (scripts, regions, towns)
    0x4a28b0,  # ComputerAI order: worker / building / military AI
    0x4a1e50, 0x439d60, 0x434c90, 0x4477c0,  # a unit leaves the AI (death, transfers)
    0x45aef0, 0x45b170, 0x45aea0,  # AI scripts started by triggers
    0x477160,  # AI target choice in attack orders
    0x462930,  # AI flee destination
    0x447a70, 0x4f39a0, 0x444430,  # AI resource bookkeeping from harvesting
    # AI hooks in unit code: towns gaining units, region reachability, spellcasting,
    # stim, cloak, medics, guards, building AI train slots.
    0x433dd0, 0x433fe0, 0x435770, 0x437e70, 0x438e70, 0x43f320, 0x43db50, 0x43c580,
    0x438410, 0x439550, 0x43aa10, 0x43c170, 0x43b970, 0x43df30, 0x43ffd0, 0x442460,
    0x462830, 0x462ea0, 0x463040, 0x4a13c0, 0x432430,
}

def load_buckets(path=os.path.join(os.path.dirname(__file__), "buckets.txt")):
    """re/buckets.txt: [(lo, hi, bucket)]."""
    out = []
    for line in open(path):
        line = line.split("#")[0].split()
        if len(line) != 2:
            continue
        lo, _, hi = line[1].partition("-")
        out.append((int(lo, 16), int(hi or lo, 16), line[0]))
    return out


def bucket_of(addr, name, buckets):
    """Why an unported function is not to be ported: replaced / deferred, else left."""
    if not name.startswith("FUN_"):
        return "replaced"  # Storm ordinals, Win32 / CRT imports
    for lo, hi, b in buckets:
        if lo <= addr <= hi:
            return b
    return "left"


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
                # The first address of a doc block's first line ("/// 0xA: ..."), and of
                # any doc line that starts with one; later lines only mention callees.
                prev = False
                for line in t.split("\n"):
                    s = line.strip()
                    doc = s.startswith("///")
                    if doc and (not prev or START.match(s)):
                        ported |= {int(a, 16) for a in DOC.findall(s)}
                    prev = doc
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
    show_all = "--all" in args
    if show_all:
        args.remove("--all")
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
    def reach(roots, cut=frozenset()):
        seen, stack = set(), list(roots)
        while stack:
            a = stack.pop()
            if a in seen or a not in calls:
                continue
            seen.add(a)
            if a not in cut:
                stack.extend(calls[a])
        return seen

    buckets = load_buckets()
    name_of = {f[0]: f[1] for f in funcs}

    def line(label, seen):
        p = len(seen & ported)
        why = Counter(bucket_of(a, name_of[a], buckets) for a in seen - ported)
        print(f"reachable from {label}: {len(seen)} functions, {p} ported ({100 * p // max(len(seen), 1)}%), "
              f"{len(seen & todo)} named by todo_port!; unported: {why['replaced']} replaced by Rust, "
              f"{why['deferred']} deferred, {why['left']} left")

    for label, roots in SIM_ROOTS:
        full = reach(roots)
        # Without the AI: stop at the AI entry points (they themselves are AI too).
        sim = reach(roots, AI_ROOTS) - AI_ROOTS
        ai = full - sim
        line(label + " (no AI)", sim)
        line(label + " (AI only)", ai)
        line(label, full)
        ai_mode = missing and missing.startswith("ai")
        if missing and (label.startswith(missing) or ai_mode and label.startswith(missing[3:] or "both")):
            seen = ai if ai_mode else sim
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
                if not show_all and bucket_of(a, info[a][1], buckets) != "left":
                    continue
                pc = len(callers[a] & ported)
                rows_m.append((-(pc > 0), -len(callers[a]), a, pc, len(callers[a]), info[a][4]))
            rows_m.sort()
            print(f"\nunported, reachable from {label}: {len(rows_m)}")
            print(f"{'addr':10} {'name':24} {'ported callers':>14} {'callers':>8} {'lines':>6}  file")
            for _, _, a, pc, nc, ln in rows_m:
                st = (" todo" if a in todo else "") + ("" if not show_all else " " + bucket_of(a, info[a][1], buckets))
                print(f"0x{a:06x}   {info[a][1][:24]:24} {pc:14} {nc:8} {ln:6}  {file_of[a]}{st}")
    if missing == "rest":
        # Every unported "left" function in scope, reachable or not (dialog procs, tables
        # and other indirect calls); those with ported callers first.
        callers = defaultdict(set)
        for a, cs in calls.items():
            for c in cs:
                callers[c].add(a)
        rows_m = sorted((-len(callers[f[0]] & ported), f[0], f[1], len(callers[f[0]]), f[4], fn)
                        for f, fn in zip(funcs, files)
                        if f[0] not in ported and not excl(fn)
                        and bucket_of(f[0], f[1], buckets) == "left")
        print(f"\nunported, in scope: {len(rows_m)}")
        print(f"{'addr':10} {'name':24} {'ported callers':>14} {'callers':>8} {'lines':>6}  file")
        for npc, a, name, nc, ln, fn in rows_m:
            print(f"0x{a:06x}   {name[:24]:24} {-npc:14} {nc:8} {ln:6}  {fn}{' todo' if a in todo else ''}")
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
