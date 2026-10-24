"""Name 1.16.1 functions after their Rust ports, for rename.py.

A doc block whose first line opens with `/// 0x4xxxxx` (or a list: `0xA / 0xB`) gives
those addresses the name of the `fn` it documents. Entries in data/names116.txt win.
usage: python3 -I re/src_names.py [src_dir] > names.txt
       python3 -I re/rename.py --names <names.txt> bw116.c bw116n.c   (path relative to re/)
"""
import os
import re
import sys

ADDR = re.compile(r"0x(4[0-9a-f]{5})\b")
START = re.compile(r"^\s*///\s*0x4[0-9a-f]{5}\b")
HEAD = re.compile(r"^\s*///\s*(0x4[0-9a-f]{5}(?:\s*(?:/|,|\+|and|or)\s*0x4[0-9a-f]{5})*)")
FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|unsafe\s+|extern\s+\"C\"\s+)*fn\s+([A-Za-z_][A-Za-z0-9_]*)")

root = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "src")
names = {}
for d, _, files in sorted(os.walk(root)):
    for n in sorted(files):
        if not n.endswith(".rs"):
            continue
        lines = open(os.path.join(d, n), errors="replace").read().split("\n")
        pending, prev_doc = set(), False
        for line in lines:
            s = line.strip()
            if s.startswith("///"):
                if not prev_doc or START.match(line):
                    m = HEAD.match(line)
                    if m:
                        pending |= set(ADDR.findall(m.group(1)))
                prev_doc = True
                continue
            prev_doc = False
            if s.startswith("#[") or not s:
                continue
            m = FN.match(line)
            if m and pending:
                for a in pending:
                    names.setdefault(int(a, 16), m.group(1))
            pending = set()

fixed = os.path.join(os.path.dirname(__file__), "data", "names116.txt")
for l in open(fixed):
    if l.strip() and not l.startswith("#"):
        a, n = l.split()[:2]
        names[int(a, 16)] = n

# coverage.py resolves calls by name: one Rust fn porting several functions (a dispatcher)
# gives each of them its name plus the address.
count = {}
for n in names.values():
    count[n] = count.get(n, 0) + 1
names = {a: n if count[n] == 1 else f"{n}_{a:x}" for a, n in names.items()}

print("# 1.16.1 address -> name from src/ doc comments (re/src_names.py) and data/names116.txt")
for a in sorted(names):
    print(f"0x{a:x} {names[a]}")
