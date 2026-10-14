"""refs.py <exe> <hex addr>...: find .text call/jmp sites to each address and raw dword
references to it (globals). Run with the agent venv python and -I."""
import sys, struct
d = open(sys.argv[1], "rb").read()
pe = struct.unpack_from("<I", d, 0x3c)[0]
so = pe + 24 + struct.unpack_from("<H", d, pe + 20)[0]
va, rs, raw = struct.unpack_from("<III", d, so + 12)
base = 0x400000 + va
code = d[raw:raw + rs]
for t in (int(a, 16) for a in sys.argv[2:]):
    for i in range(len(code) - 5):
        if code[i] in (0xe8, 0xe9) and (base + i + 5 + struct.unpack_from("<i", code, i + 1)[0]) & 0xffffffff == t:
            print(hex(t), "call" if code[i] == 0xe8 else "jmp", hex(base + i))
    p = struct.pack("<I", t)
    i = code.find(p)
    while i >= 0:
        print(hex(t), "dword", hex(base + i - 2), code[i - 2:i].hex())
        i = code.find(p, i + 1)
