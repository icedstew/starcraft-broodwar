"""Decompress sector files with storm.dll's SCompDecompress (ordinal 552) under unicorn."""
import sys, os, struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX, UC_X86_REG_EIP
dll = open(sys.argv[1], 'rb').read()
pe = struct.unpack_from('<I', dll, 0x3c)[0]
n = struct.unpack_from('<H', dll, pe + 6)[0]
so = pe + 24 + struct.unpack_from('<H', dll, pe + 20)[0]
base = 0x15000000
uc = Uc(UC_ARCH_X86, UC_MODE_32)
uc.mem_map(base, 0x80000)
for i in range(n):
    nm, vs, va, rs, ra = struct.unpack_from('<8sIIII', dll, so + i * 40)
    uc.mem_write(base + va, dll[ra:ra + rs])
# Imports: point every IAT slot at a stub page; a few kernel32 calls are emulated.
def off(rva):
    for i in range(n):
        nm, vs, va, rs, ra = struct.unpack_from('<8sIIII', dll, so + i * 40)
        if va <= rva < va + max(vs, rs): return rva - va + ra
STUB = 0x6000000
uc.mem_map(STUB, 0x10000)
stubs = {}
irva = struct.unpack_from('<I', dll, pe + 24 + 104)[0]; o = off(irva)
while True:
    oft, ts, fc, name, ft = struct.unpack_from('<5I', dll, o)
    if name == 0: break
    i = 0
    while True:
        t = struct.unpack_from('<I', dll, off(oft or ft) + i * 4)[0]
        if t == 0: break
        nm = ('#%d' % (t & 0xffff)) if t & 0x80000000 else dll[off(t) + 2:dll.index(b'\0', off(t) + 2)].decode()
        a = STUB + len(stubs) * 4
        stubs[a] = nm
        uc.mem_write(base + ft + i * 4, struct.pack('<I', a))
        uc.mem_write(a, b'\xc3')
        i += 1
    o += 20
ARGS = {'FormatMessageA': 1, 'FindClose': 1, 'HeapAlloc': 3, 'GetProcessHeap': 0, 'FindFirstFileA': 2, 'GetModuleFileNameA': 3, 'InitializeCriticalSection': 1, 'DeleteCriticalSection': 1, 'EnterCriticalSection': 1, 'LeaveCriticalSection': 1, 'SetLastError': 1, 'InterlockedIncrement': 1, 'InterlockedDecrement': 1, 'GetLastError': 0}
def hook(uc, addr, size, _):
    nm = stubs.get(addr)
    if nm is None: return
    esp = uc.reg_read(UC_X86_REG_ESP)
    arg = struct.unpack('<I', uc.mem_read(esp + 4, 4))[0]
    r = 0
    if nm in ('InterlockedIncrement', 'InterlockedDecrement'):
        v = struct.unpack('<i', uc.mem_read(arg, 4))[0] + (1 if nm == 'InterlockedIncrement' else -1)
        uc.mem_write(arg, struct.pack('<i', v)); r = v & 0xffffffff
    elif nm == 'HeapAlloc':
        global brk
        size = struct.unpack('<I', uc.mem_read(esp + 12, 4))[0]
        r = brk; brk = (brk + size + 15) & ~15
    elif nm == 'FindFirstFileA':
        r = 0xffffffff
    elif nm == 'GetProcessHeap':
        r = 1
    elif nm not in ARGS:
        raise RuntimeError('import ' + nm)
    ret = struct.unpack('<I', uc.mem_read(esp, 4))[0]
    uc.reg_write(UC_X86_REG_EAX, r)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + 4 * ARGS[nm])
    uc.reg_write(UC_X86_REG_EIP, ret)
brk = 0x8800000
def smem(uc, addr, size, _):
    global brk
    esp = uc.reg_read(UC_X86_REG_ESP)
    ret = struct.unpack('<I', uc.mem_read(esp, 4))[0]
    if addr == 0x15020ed0:
        n = struct.unpack('<I', uc.mem_read(esp + 4, 4))[0]
        uc.reg_write(UC_X86_REG_EAX, brk); uc.mem_write(brk, bytes(n)); brk = (brk + n + 15) & ~15
    else:
        uc.reg_write(UC_X86_REG_EAX, 1)
    uc.reg_write(UC_X86_REG_ESP, esp + 4 + 16)
    uc.reg_write(UC_X86_REG_EIP, ret)
uc.hook_add(UC_HOOK_CODE, smem, begin=0x15020ed0, end=0x15020ed0)
uc.hook_add(UC_HOOK_CODE, smem, begin=0x150205d0, end=0x150205d0)
uc.hook_add(UC_HOOK_CODE, hook, begin=STUB, end=STUB + 0x10000)
STACK, HEAP, RET = 0x7000000, 0x8000000, 0x6fff000
uc.mem_map(STACK, 0x100000); uc.mem_map(HEAP, 0x1000000); uc.mem_map(RET, 0x1000)
def call(addr, args, regs={}):
    esp = STACK + 0x100000 - 0x100
    for v in reversed(args):
        esp -= 4; uc.mem_write(esp, struct.pack('<I', v))
    esp -= 4; uc.mem_write(esp, struct.pack('<I', RET))
    uc.reg_write(UC_X86_REG_ESP, esp)
    from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EDX
    for k, v in regs.items(): uc.reg_write({'ecx': UC_X86_REG_ECX, 'edx': UC_X86_REG_EDX}[k], v)
    try:
        uc.emu_start(addr, RET)
    except Exception as e:
        esp=uc.reg_read(UC_X86_REG_ESP)
        sys.stderr.write('fault eip %x ret %x\n' % (uc.reg_read(UC_X86_REG_EIP), struct.unpack('<I', uc.mem_read(esp,4))[0]))
        raise
    return uc.reg_read(UC_X86_REG_EAX)
d = sys.argv[2]
for f in sorted(os.listdir(d), key=lambda x: int(x.split('_')[0])):
    want = int(f.split('_')[1])
    src = open(os.path.join(d, f), 'rb').read()
    mask, data = src[0], src[1:]
    pos = globals().get('pos', 0)
    if len(src) >= want:
        sys.stdout.buffer.write(src[:want]); pos += want; continue
    def run(fn, data):
        uc.mem_write(HEAP, data)
        dst = HEAP + 0x100000; lenp = HEAP + 0x200000
        uc.mem_write(lenp, struct.pack('<I', want))
        call(fn, [HEAP, len(data)], {'ecx': dst, 'edx': lenp})
        got = struct.unpack('<I', uc.mem_read(lenp, 4))[0]
        return bytes(uc.mem_read(dst, got))
    if mask & 8: data = run(0x1501d500, data)
    if mask & 1: data = run(0x1501d090, data)
    if mask & 0x80: data = run(0x1501c660, data)
    if mask & 0x40: data = run(0x1501c6e0, data)
    sys.stdout.buffer.write(data); pos += len(data)
