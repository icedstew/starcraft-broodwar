"""Remove SC:R's junk-byte opaque predicates from the dumped image so Ghidra
decompiles cleanly.

The obfuscator inserts conditional jumps whose outcome is fixed by flags set
just before them (`or r,imm` / `test` clear CF+OF, `stc` sets CF, shifts by 0
keep flags, `je X; jne X` pairs, ...), followed by junk bytes on the dead path.
We track known flag values along straight-line code (and across resolved
jumps, which is how the chains are built) and rewrite each decided jcc to a
plain jmp (always taken) or NOPs (never taken). Both rewrites are exact.

Only code reached by recursive descent from .pdata function starts is touched,
so jump tables and data embedded in .text are left alone.

usage: venv/bin/python deobf.py dump.exe out.exe
"""
import struct, sys
import capstone
from capstone import x86

src, dst = sys.argv[1], sys.argv[2]
d = bytearray(open(src, 'rb').read())
BASE = 0x140000000
pe = struct.unpack_from('<I', d, 0x3c)[0]
nsec = struct.unpack_from('<H', d, pe + 6)[0]
opt = struct.unpack_from('<H', d, pe + 20)[0]
secs = {}
for i in range(nsec):
    o = pe + 24 + opt + i * 40
    vs, va = struct.unpack_from('<II', d, o + 8)
    secs[d[o:o + 8].rstrip(b'\0').decode()] = (va, vs)
TVA, TVS = secs['.text']
PVA, PVS = secs['.pdata']
starts = sorted({struct.unpack_from('<I', d, PVA + i * 12)[0] for i in range(PVS // 12)} - {0})

md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
md.detail = True
in_text = lambda a: TVA <= a - BASE < TVA + TVS

FLAGS = {  # name -> (set, reset, modify, undefined) capstone bits
    f: tuple(getattr(x86, f'X86_EFLAGS_{k}_{f}') for k in ('SET', 'RESET', 'MODIFY', 'UNDEFINED'))
    for f in ('CF', 'OF', 'ZF', 'SF', 'PF')
}
SHIFTS = {'shl', 'sal', 'shr', 'sar', 'rol', 'ror', 'rcl', 'rcr'}


def apply_flags(i, st):
    """Update known-flag dict `st` for instruction `i`."""
    if i.mnemonic in SHIFTS and len(i.operands) == 2 and i.operands[1].type == x86.X86_OP_IMM and i.operands[1].imm & 0x3F == 0:
        return st  # shift/rotate by 0 leaves flags alone
    ef = i.eflags
    if not ef:
        return st
    st = dict(st)
    for f, (s, r, m, u) in FLAGS.items():
        if ef & s:
            st[f] = 1
        elif ef & r:
            st[f] = 0
        elif ef & (m | u):
            st.pop(f, None)
    return st


def decide(m, st):
    """True = always taken, False = never, None = unknown."""
    g = st.get
    def known(*fs):
        return all(f in st for f in fs)
    if m == 'jo' and known('OF'): return g('OF') == 1
    if m == 'jno' and known('OF'): return g('OF') == 0
    if m in ('jb', 'jc', 'jnae') and known('CF'): return g('CF') == 1
    if m in ('jae', 'jnb', 'jnc') and known('CF'): return g('CF') == 0
    if m in ('je', 'jz') and known('ZF'): return g('ZF') == 1
    if m in ('jne', 'jnz') and known('ZF'): return g('ZF') == 0
    if m in ('js',) and known('SF'): return g('SF') == 1
    if m in ('jns',) and known('SF'): return g('SF') == 0
    if m in ('jp', 'jpe') and known('PF'): return g('PF') == 1
    if m in ('jnp', 'jpo') and known('PF'): return g('PF') == 0
    if m in ('jbe', 'jna'):
        if g('CF') == 1 or g('ZF') == 1: return True
        if known('CF', 'ZF'): return False
    if m in ('ja', 'jnbe'):
        if g('CF') == 1 or g('ZF') == 1: return False
        if known('CF', 'ZF'): return True
    if m in ('jl', 'jnge') and known('SF', 'OF'): return g('SF') != g('OF')
    if m in ('jge', 'jnl') and known('SF', 'OF'): return g('SF') == g('OF')
    if m in ('jle', 'jng'):
        if g('ZF') == 1: return True
        if known('ZF', 'SF', 'OF'): return g('SF') != g('OF')
    if m in ('jg', 'jnle'):
        if g('ZF') == 1: return False
        if known('ZF', 'SF', 'OF'): return g('SF') == g('OF')
    return None


COMPLEMENT = {'je': 'jne', 'jne': 'je', 'jb': 'jae', 'jae': 'jb', 'jo': 'jno', 'jno': 'jo', 'js': 'jns', 'jns': 'js',
              'jp': 'jnp', 'jnp': 'jp', 'jl': 'jge', 'jge': 'jl', 'jle': 'jg', 'jg': 'jle', 'jbe': 'ja', 'ja': 'jbe'}

seen = set()
patches = {}


def jmp_bytes(off, a, size, tgt):
    if size == 2:
        return bytes([0xEB, d[off + 1]])
    return bytes([0xE9]) + struct.pack('<i', tgt - (a + 5)) + b'\x90' * (size - 5)


for s in starts:
    work = [(BASE + s, {})]
    while work:
        a, st = work.pop()
        while a not in seen and in_text(a):
            off = a - BASE
            i = next(md.disasm(bytes(d[off:off + 16]), a), None)
            if i is None:
                break
            seen.add(a)
            m = i.mnemonic
            tgt = i.operands[0].imm if i.operands and i.operands[0].type == x86.X86_OP_IMM else None
            if m.startswith('ret') or m in ('int3', 'ud2', 'hlt'):
                break
            if m == 'jmp':
                if tgt is not None:
                    work.append((tgt, st))
                break
            if m.startswith('j') and tgt is not None and m != 'jrcxz' and m != 'jecxz':
                verdict = decide(m, st)
                if verdict is None and m in COMPLEMENT:
                    nxt = next(md.disasm(bytes(d[off + i.size:off + i.size + 16]), a + i.size), None)
                    if nxt is not None and nxt.mnemonic == COMPLEMENT[m] and nxt.operands and nxt.operands[0].type == x86.X86_OP_IMM and nxt.operands[0].imm == tgt:
                        verdict = True
                if verdict is True:
                    patches[off] = jmp_bytes(off, a, i.size, tgt)
                    work.append((tgt, st))
                    break
                if verdict is False:
                    patches[off] = b'\x90' * i.size
                    a += i.size
                    continue
                work.append((tgt, {}))
                a += i.size
                continue
            st = {} if m == 'call' else apply_flags(i, st)
            a += i.size

for off, b in patches.items():
    d[off:off + len(b)] = b
open(dst, 'wb').write(d)
print(f'{len(starts)} functions, {len(seen)} instructions reached, {len(patches)} opaque predicates patched')
