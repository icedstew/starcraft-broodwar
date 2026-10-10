"""Recursive-descent disassembler that sees through SC:R's junk-byte opaque predicates.

Pattern: `or/and/xor/test reg, imm` leaves OF=CF=0, so a following `jno`/`jnc`/`jae`
is always taken and `jo`/`jc`/`jb` never; the untaken side is junk bytes.

usage: venv/bin/python rdis.py dump.exe 0xENTRY [--calls]
Prints reachable instructions in address order (or just the call targets).
"""
import struct, sys
import capstone
from capstone import x86

path, entry = sys.argv[1], int(sys.argv[2], 16)
# Optional --end 0xADDR: jumps outside [entry, end) are tail calls, not followed.
END = int(sys.argv[sys.argv.index('--end') + 1], 16) if '--end' in sys.argv else entry + 0x4000
inside = lambda t: t is not None and entry <= t < END
d = open(path, 'rb').read()
BASE = 0x140000000
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
md.detail = True

LOGIC = {'or', 'and', 'xor', 'test'}
ALWAYS = {'jno', 'jae', 'jnb', 'jnc'}
NEVER = {'jo', 'jb', 'jc', 'jnae'}
# jcc X followed directly by its complement to the same X = unconditional jump.
COMPLEMENT = {'je': 'jne', 'jne': 'je', 'jb': 'jae', 'jae': 'jb', 'jo': 'jno', 'jno': 'jo', 'js': 'jns', 'jns': 'js',
              'jp': 'jnp', 'jnp': 'jp', 'jl': 'jge', 'jge': 'jl', 'jle': 'jg', 'jg': 'jle', 'jbe': 'ja', 'ja': 'jbe'}


def insn_at(a):
    off = a - BASE
    return next(md.disasm(d[off:off + 16], a), None)


seen = {}
work = [entry]
while work:
    a = work.pop()
    prev = None
    while a not in seen:
        i = insn_at(a)
        if i is None:
            break
        seen[a] = i
        m = i.mnemonic
        tgt = None
        if i.operands and i.operands[0].type == x86.X86_OP_IMM:
            tgt = i.operands[0].imm
        if m in ('ret', 'int3', 'ud2', 'hlt'):
            break
        if m == 'jmp':
            if inside(tgt):
                work.append(tgt)
            break
        if m in COMPLEMENT and tgt is not None:
            nxt = insn_at(a + i.size)
            if nxt is not None and nxt.mnemonic == COMPLEMENT[m] and nxt.operands and nxt.operands[0].type == x86.X86_OP_IMM and nxt.operands[0].imm == tgt:
                if inside(tgt):
                    work.append(tgt)
                break
        if m.startswith('j') and m != 'jmp':
            opaque = prev is not None and prev.mnemonic in LOGIC
            if opaque and m in ALWAYS:
                if inside(tgt):
                    work.append(tgt)
                break
            if opaque and m in NEVER:
                a += i.size
                prev = i
                continue
            if inside(tgt):
                work.append(tgt)
        a += i.size
        prev = i

names = {}
try:
    for l in open(__file__.replace('rdis.py', 'data/names.txt')):
        if l.strip() and not l.startswith('#'):
            k, n = l.split()[:2]
            names[int(k, 16)] = n
except OSError:
    pass

for a in sorted(seen):
    i = seen[a]
    ops = i.op_str
    if i.mnemonic == 'call' and i.operands and i.operands[0].type == x86.X86_OP_IMM:
        ops += f'    ; {names.get(i.operands[0].imm, "")}'
    if '--calls' in sys.argv:
        if i.mnemonic == 'call':
            print(f'{a:#x} call {ops}')
    else:
        print(f'{a:#x}: {i.mnemonic} {ops}')
