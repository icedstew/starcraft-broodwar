"""Run 1.16.1 StarCraft.exe functions under unicorn for differential tests.

Usage (as a module):
    e = Emu("StarCraft.exe")
    e.w16(0x57f1d4, 128)
    e.call(0x484290, regs={"ebx": ptr}, stack=[...])

Imports are bound to stubs; Storm SMemAlloc (401) / SMemFree (403) are emulated
with a bump heap, any other import stops the run.
"""
import struct
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_INVALID
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_EBX, UC_X86_REG_ECX, UC_X86_REG_EDX, \
    UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EBP, UC_X86_REG_ESP, UC_X86_REG_EIP

REGS = {"eax": UC_X86_REG_EAX, "ebx": UC_X86_REG_EBX, "ecx": UC_X86_REG_ECX, "edx": UC_X86_REG_EDX,
        "esi": UC_X86_REG_ESI, "edi": UC_X86_REG_EDI, "ebp": UC_X86_REG_EBP}
STUBS = 0x7f000000
HEAP = 0x10000000
HEAP_SIZE = 0x08000000
STACK = 0x7e000000
STACK_SIZE = 0x100000
RET = STUBS + 0xfff0


class Emu:
    def __init__(self, path):
        exe = open(path, "rb").read()
        pe = struct.unpack_from("<I", exe, 0x3c)[0]
        nsec, = struct.unpack_from("<H", exe, pe + 6)
        opt, = struct.unpack_from("<H", exe, pe + 20)
        self.base = base = struct.unpack_from("<I", exe, pe + 24 + 28)[0]
        img_size = struct.unpack_from("<I", exe, pe + 24 + 56)[0]
        self.uc = uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.mem_map(base, (img_size + 0xfff) & ~0xfff)
        uc.mem_write(base, exe[:0x1000])
        o = pe + 24 + opt
        for _ in range(nsec):
            vsz, va, rsz, raw = struct.unpack_from("<IIII", exe, o + 8)
            uc.mem_write(base + va, exe[raw:raw + min(rsz, vsz)])
            o += 40
        uc.mem_map(STUBS, 0x10000)
        uc.mem_write(STUBS, b"\xc3" * 0x10000)
        uc.mem_map(HEAP, HEAP_SIZE)
        uc.mem_map(STACK, STACK_SIZE)
        self.heap = HEAP
        self.imports = {}
        self._bind_imports(exe, pe)
        uc.hook_add(UC_HOOK_CODE, self._stub, begin=STUBS, end=STUBS + 0xffff)
        uc.hook_add(UC_HOOK_MEM_INVALID, self._bad)
        self.snaps = {}

    def at(self, addr, fn):
        """Run fn(emu) every time execution reaches addr."""
        self.uc.hook_add(UC_HOOK_CODE, lambda uc, a, size, _: fn(self), begin=addr, end=addr)

    def _bind_imports(self, exe, pe):
        rva = lambda r: r  # image is mapped at base; read through uc
        imp, = struct.unpack_from("<I", exe, pe + 24 + 104)
        n = 0
        d = self.base + imp
        while True:
            ilt, _, _, name_rva, iat = struct.unpack("<IIIII", self.uc.mem_read(d, 20))
            if name_rva == 0:
                break
            dll = self.cstr(self.base + name_rva)
            src = ilt or iat
            i = 0
            while True:
                th, = struct.unpack("<I", self.uc.mem_read(self.base + src + i * 4, 4))
                if th == 0:
                    break
                sym = f"{dll}#{th & 0xffff}" if th & 0x80000000 else f"{dll}!{self.cstr(self.base + th + 2)}"
                addr = STUBS + n * 4
                self.imports[addr] = sym
                self.w32(self.base + iat + i * 4, addr)
                n += 1
                i += 1
            d += 20

    def cstr(self, a):
        b = bytes(self.uc.mem_read(a, 256))
        return b.split(b"\0")[0].decode()

    def _bad(self, uc, access, addr, size, value, _):
        eip = uc.reg_read(UC_X86_REG_EIP)
        raise RuntimeError(f"invalid mem access {access} at {addr:#x} (eip {eip:#x})")

    def _stub(self, uc, addr, size, _):
        if addr == RET:
            uc.emu_stop()
            return
        sym = self.imports.get(addr)
        esp = uc.reg_read(UC_X86_REG_ESP)
        arg = lambda i: self.r32(esp + 4 + 4 * i)
        if sym and sym.lower().startswith("storm.dll#401"):
            size, flags = arg(0), arg(3)
            p = self.alloc(size)
            uc.reg_write(UC_X86_REG_EAX, p)
            self._ret(4)
        elif sym and sym.lower().startswith("storm.dll#403"):
            uc.reg_write(UC_X86_REG_EAX, 1)
            self._ret(4)
        else:
            raise RuntimeError(f"unhandled import {sym} at {addr:#x}")

    def _ret(self, nargs):
        esp = self.uc.reg_read(UC_X86_REG_ESP)
        ra = self.r32(esp)
        self.uc.reg_write(UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        self.uc.reg_write(UC_X86_REG_EIP, ra)

    def alloc(self, size):
        # Zero-filled bump allocation (fresh memory is zero).
        p = self.heap
        self.heap = (self.heap + size + 0xf) & ~0xf
        assert self.heap < HEAP + HEAP_SIZE
        return p

    def r8(self, a): return self.uc.mem_read(a, 1)[0]
    def r16(self, a): return struct.unpack("<H", self.uc.mem_read(a, 2))[0]
    def r32(self, a): return struct.unpack("<I", self.uc.mem_read(a, 4))[0]
    def w8(self, a, v): self.uc.mem_write(a, bytes([v & 0xff]))
    def w16(self, a, v): self.uc.mem_write(a, struct.pack("<H", v & 0xffff))
    def w32(self, a, v): self.uc.mem_write(a, struct.pack("<I", v & 0xffffffff))
    def read(self, a, n): return bytes(self.uc.mem_read(a, n))
    def write(self, a, b): self.uc.mem_write(a, bytes(b))

    def call(self, addr, regs=None, stack=()):
        """Call `addr` with register and stack (cdecl/stdcall order) args; returns eax."""
        esp = STACK + STACK_SIZE - 0x100
        for v in reversed(list(stack)):
            esp -= 4
            self.w32(esp, v)
        esp -= 4
        self.w32(esp, RET)
        self.uc.reg_write(UC_X86_REG_ESP, esp)
        for k, v in (regs or {}).items():
            self.uc.reg_write(REGS[k], v)
        self.uc.emu_start(addr, RET)
        return self.uc.reg_read(UC_X86_REG_EAX)
