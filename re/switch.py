"""Map a byte-indexed switch (movzx idx,[base+rax+BYTES]; mov ecx,[base+rax*4+DWORDS]) to case -> called function.

usage: python3 -I switch.py dump.exe <bytes_rva> <dwords_rva> <bias> <ncases> [names.txt]
"""
import struct, subprocess, sys, re
dump, btab, dtab, bias, n = sys.argv[1], int(sys.argv[2], 16), int(sys.argv[3], 16), int(sys.argv[4]), int(sys.argv[5])
names = open(sys.argv[6]).read().split() if len(sys.argv) > 6 else []
d = open(dump, 'rb').read(); B = 0x140000000
for c in range(n):
    k = d[btab + c]
    tgt = B + struct.unpack_from('<I', d, dtab + k * 4)[0]
    out = subprocess.run(['objdump', '-d', '-M', 'intel', '--no-show-raw-insn', f'--start-address={tgt}', f'--stop-address={tgt + 40}', dump],
                         capture_output=True, text=True).stdout
    m = re.search(r'\t(call|jmp)\s+(0x[0-9a-f]+)', out)
    order = c + bias
    nm = names[order] if order < len(names) else '?'
    print(f'{order:3d} {nm:24s} case {tgt:#x} -> {m.group(1) + " " + m.group(2) if m else "inline"}')
