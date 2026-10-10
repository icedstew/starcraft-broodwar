"""Map dat.rs fields to 1.16.1 StarCraft.exe in-memory column addresses.

Usage: python -I re/dat116.py StarCraft.exe src/dat.rs > re/data/dat116.txt
The exe's dat descriptor runs are (ptr, size, count) records; they are matched to
dat.rs tables by run length and entry count.
"""
import re, struct, sys

exe = open(sys.argv[1], 'rb').read()
src = open(sys.argv[2]).read()

def ok(k):
    p, sz, cnt = struct.unpack_from('<III', exe, k)
    return 0x500000 <= p < 0x700000 and sz in (1, 2, 4, 8) and 0 < cnt < 2000

runs = []
k = 0x100000
while k < len(exe) - 24:
    if ok(k) and ok(k + 12):
        run = []
        while ok(k):
            run.append(struct.unpack_from('<III', exe, k)); k += 12
        runs.append(run)
    else:
        k += 4

tables = re.findall(r'dat!\((\w+), "[^"]+", (\d+);(.*?)\n\);', src, re.S)
for name, count, body in tables:
    fields = re.findall(r'^\s+(\w+): \w+ = \((\d+), (\d+), (\d+)\)', body, re.M)
    cand = [r for r in runs if len(r) == len(fields)]
    if not cand:
        print(f'# {name}: no run with {len(fields)} columns'); continue
    run = cand[0]
    print(f'# {name}')
    for (f, sz, n, first), (p, rsz, rcnt) in zip(fields, run):
        print(f'0x{p:x} {name}.{f} size={rsz} count={rcnt}')
