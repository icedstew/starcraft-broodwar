"""Recover .dat file layouts from the dumped exe's load_dat(name, table) call sites.

Descriptor entry (24 bytes): u64 dest_ptr; i32 elem_size; i32 count; u8 flags (bit0: 4-byte
pointer in file widened to 8 in memory). Table ends at dest_ptr == 0.
"""
import struct, sys
sys.argv = sys.argv[:2]
exec(open('xref.py').read().split("if __name__")[0])
LOAD_DAT = 0x140305d10 - base


def lea(at, reg):  # lea reg,[rip+disp] at `at`; returns target rva or None
    op = {'rcx': b'\x48\x8d\x0d', 'rdx': b'\x48\x8d\x15'}[reg]
    if d[at:at + 3] == op:
        return at + 7 + struct.unpack_from('<i', d, at + 3)[0]


calls = pos[pos + 4 + disp == LOAD_DAT] - 1  # e8 opcode position
for c in sorted(set(int(x) for x in calls)):
    if d[c] != 0xE8:
        continue
    name = tbl = None
    for back in range(5, 40):
        name = name or lea(c - back, 'rcx')
        tbl = tbl or lea(c - back, 'rdx')
    if not (name and tbl):
        continue
    nm = d[name:d.index(b'\0', name)].decode(errors='replace')
    fields, o = [], tbl
    while struct.unpack_from('<Q', d, o)[0]:
        ptr, sz, cnt, fl = struct.unpack_from('<QiiB', d, o)
        fields.append((ptr, 4 if fl & 1 else sz, cnt, fl))
        o += 24
    print(f'{nm} table={base + tbl:#x} fields={len(fields)} filesize={sum(s * n for _, s, n, _ in fields)}')
    print('   ' + ' '.join(f'{s}x{n}' + ('p' if fl & 1 else '') for _, s, n, fl in fields))
