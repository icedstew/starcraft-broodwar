"""Fast RIP-relative xref finder for the dumped x64 image (raw == virtual layout).

usage: python3 -I xref.py <dump.exe> <hex target or "string">...
Prints code addresses whose disp32 resolves to the target, plus the containing
function (from .pdata).
"""
import struct, sys
import numpy as np

path = sys.argv[1]
d = open(path, 'rb').read()
pe = struct.unpack_from('<I', d, 0x3c)[0]
base = struct.unpack_from('<Q', d, pe + 24 + 24)[0]
nsec = struct.unpack_from('<H', d, pe + 6)[0]
opt = struct.unpack_from('<H', d, pe + 20)[0]
secs = {}
for i in range(nsec):
    o = pe + 24 + opt + i * 40
    vs, va = struct.unpack_from('<II', d, o + 8)
    secs[d[o:o + 8].rstrip(b'\0').decode()] = (va, vs)
tva, tvs = secs['.text']
pva, pvs = secs['.pdata']
funcs = np.frombuffer(d[pva:pva + pvs // 12 * 12], dtype='<u4').reshape(-1, 3)
funcs = funcs[funcs[:, 0] != 0]
funcs = funcs[np.argsort(funcs[:, 0])]

text = np.frombuffer(d[tva:tva + tvs], dtype=np.uint8)
n = len(text) - 4
disp = (text[:n].astype(np.int64) | text[1:n + 1].astype(np.int64) << 8 |
        text[2:n + 2].astype(np.int64) << 16 | text[3:n + 3].astype(np.int64) << 24)
disp = np.where(disp >= 1 << 31, disp - (1 << 32), disp)
pos = np.arange(n, dtype=np.int64) + tva  # rva of disp field


def func_of(rva):
    i = np.searchsorted(funcs[:, 0], rva, side='right') - 1
    if i >= 0 and funcs[i, 0] <= rva < funcs[i, 1]:
        return int(funcs[i, 0])


def xrefs(target_rva):
    # Instruction end = disp field + 4 + (0..4 imm bytes); accept any.
    hits = []
    for imm in (0, 1, 2, 4):
        hits += list(pos[pos + 4 + imm + disp == target_rva])
    return sorted(set(int(h) for h in hits))


def find_string(s):
    r, i = [], d.find(s.encode() + b'\0')
    while i >= 0:
        r.append(i); i = d.find(s.encode() + b'\0', i + 1)
    return r


if __name__ == '__main__':
    print(f'base {base:#x}, {len(funcs)} pdata functions')
    for a in sys.argv[2:]:
        targets = [int(a, 16) - base] if a.startswith('0x') else find_string(a)
        for t in targets:
            print(f'== {a} @ {base + t:#x}')
            for h in xrefs(t):
                f = func_of(h)
                print(f'   ref {base + h:#x}  in func {base + f:#x}' if f is not None else f'   ref {base + h:#x}')
