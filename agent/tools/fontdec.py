"""Decrypt a 1.16.1 .fnt with the game's own routines (0x4d9ba0 key, 0x4818c0 data)."""
import sys, os, struct
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "re"))
from emu import Emu
e = Emu(sys.argv[1])
e.handlers['kernel32.dll!getsystemtimeasfiletime'] = lambda e, a: (e.write(a[0], bytes(8)) or 0, 1)
ptd = e.alloc(0x8c); e.w32(ptd + 0x14, 1)
e.write(0x4059e4, b"\xb8" + struct.pack("<I", ptd) + b"\xc3")  # __getptd: one static block
gid = open(sys.argv[3], 'rb').read()
n = len(gid); blob = e.alloc(n); e.write(blob, gid)
key = e.alloc(n + 16)
e.write(key, e.read(blob, n))
ok = e.call(0x4d9ba0, regs={"esi": key, "edi": n})
k = e.read(key, 64).split(b"\0")[0]
sys.stderr.write("key ok %d %r\n" % (ok, k))
data = open(sys.argv[2], 'rb').read()
buf = e.alloc(len(data) + 16)
e.write(buf, data)
r = e.call(0x4818c0, regs={"eax": key, "ecx": buf}, stack=[len(data)])
sys.stderr.write("len %d -> %d\n" % (len(data), r))
sys.stdout.buffer.write(e.read(buf, r))
