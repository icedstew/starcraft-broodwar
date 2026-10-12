"""Run 1.16.1 game setup under the emulator with game files served from a directory
(`scbw export <dir> arr/ scripts/ unit/ tileset/`).

    python -I re/emu_game.py <StarCraft.exe> <data dir> <mapdump dir>
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from emu import Emu  # noqa: E402
import emu_map  # noqa: E402


class Files:
    """Storm SFile* over a directory of exported CASC files."""

    def __init__(self, e, root):
        self.e, self.root, self.open, self.next = e, root, {}, 1
        self.missing = []
        h = e.handlers
        h["storm.dll#268"] = self.sfile_open        # SFileOpenFileEx(mpq, name, flags, *handle)
        h["storm.dll#267"] = lambda e, a: self.sfile_open(e, [0, a[0], 0, a[1]])[0:1] + (2,)  # SFileOpenFile(name, *handle)
        h["storm.dll#265"] = self.sfile_size        # SFileGetFileSize(handle, *high)
        h["storm.dll#269"] = self.sfile_read        # SFileReadFile(handle, buf, len, *read, overlapped)
        h["storm.dll#253"] = self.sfile_close       # SFileCloseFile(handle)
        k = "kernel32.dll!"
        h[k + "createeventa"] = lambda e, a: (0x1000, 4)
        h[k + "waitforsingleobject"] = lambda e, a: (0, 2)
        h[k + "setevent"] = lambda e, a: (1, 1)
        h[k + "resetevent"] = lambda e, a: (1, 1)
        h[k + "closehandle"] = lambda e, a: (1, 1)
        h[k + "getlasterror"] = lambda e, a: (0, 0)
        u = "user32.dll!"
        h[u + "peekmessagea"] = lambda e, a: (0, 5)
        h[u + "setrect"] = lambda e, a: (e.write(a[0], struct.pack("<4i", *a[1:5])) or 1, 5)
        h[k + "waitformultipleobjects"] = lambda e, a: (0, 4)
        h[k + "sleep"] = lambda e, a: (0, 1)
        h[k + "gettickcount"] = lambda e, a: (0, 0)
        # Graphics: report success without doing anything.
        h["storm.dll#323"] = lambda e, a: (1, 7)    # SBmpLoadImage
        h["storm.dll#321"] = self.bmp_decode        # SBmpDecodeImage
        h["storm.dll#494"] = lambda e, a: (e.write(a[0], bytes(a[1])) or 0, 2)  # SMemZero
        h["storm.dll#465"] = lambda e, a: (0, 1)    # SErrSetLastError

    def path(self, name):
        return os.path.join(self.root, name.replace("\\", "/").lower())

    def sfile_open(self, e, a):
        name = e.cstr(a[1])
        self.opened = getattr(self, "opened", []) + [name]
        p = self.path(name)
        data = None
        if os.path.exists(p):
            data = open(p, "rb").read()
        elif p.endswith(".vx4") and os.path.exists(p + "ex"):
            # SC:R ships only 32-bit vx4ex; graphics-only, so truncate to vx4.
            ex = open(p + "ex", "rb").read()
            data = b"".join(ex[i:i + 2] for i in range(0, len(ex), 4))
        if data is None:
            self.missing.append(name)
            return 0, 4
        hd = self.next
        self.next += 1
        self.open[hd] = [data, 0]
        e.w32(a[3], hd)
        return 1, 4

    def sfile_size(self, e, a):
        if a[1]:
            e.w32(a[1], 0)
        return len(self.open[a[0]][0]), 2

    def sfile_read(self, e, a):
        data, pos = self.open[a[0]]
        chunk = data[pos:pos + a[2]]
        e.write(a[1], chunk)
        self.open[a[0]][1] = pos + len(chunk)
        if a[3]:
            e.w32(a[3], len(chunk))
        return int(len(chunk) == a[2]), 5

    def bmp_decode(self, e, a):
        for p in (a[6], a[7]):
            if p:
                e.w32(p, 0)
        return 1, 9

    def sfile_close(self, e, a):
        self.open.pop(a[0], None)
        return 1, 1


def game_init(e, f, md):
    """0x4eee00 with the scenario taken from memory (replay-style CHK, 0x4bf520)."""
    rd = lambda n: open(os.path.join(md, n), "rb").read()
    w, h = struct.unpack("<HH", rd("dims"))
    e.w16(0x57f1d4, w)
    e.w16(0x57f1d6, h)
    e.w16(0x57f1dc, rd("tileset")[0])
    e.w16(0x628450, w * 32)
    e.w16(0x6284b4, h * 32)
    e.call(0x49bb90)  # map pixel size and the map box 0x6284a0
    chk = rd("chk")
    p = e.alloc(len(chk))
    e.write(p, chk)
    steps = [
        ("startup images", 0x4d7390), ("path pool 42f4f0", 0x42f4f0), ("48ce90", 0x48ce90), ("447680", 0x447680), ("462a50", 0x462a50),
        ("437a70(0)", (0x437a70, [0])), ("45b5f0", 0x45b5f0), ("432610", 0x432610),
        ("consts", None), ("map 4bd6f0", 0x4bd6f0), ("images", 0x4d6930), ("sprites", 0x499900),
        ("thingies", 0x488550), ("flingy.dat", None), ("bullets", None), ("48a690", 0x48a690),
        ("orders", 0x48c590), ("4cb5b0", 0x4cb5b0), ("41e450 palette (skipped)", "skip"), ("4c99c0 color cycling (skipped)", "skip"),
        ("488f90", 0x488f90), ("493360", 0x493360), ("4ccc80", 0x4ccc80), ("49f380", 0x49f380),
        ("4313c0", 0x4313c0), ("497110", 0x497110), ("chk 4bf520", None),
    ]
    for name, what in steps:
        try:
            if name == "consts":
                e.w32(0x59cc94, 0x3c)
                e.w32(0x59cc8c, 0xf)
                e.w32(0x59cc90, 6)
            elif name == "flingy.dat":
                e.call(0x4d2e80, regs={"eax": 0x515a38}, stack=[0x504274])
            elif name == "bullets":
                e.write(0x63fee0, bytes(0x13 * 4))
                e.w32(0x63ff3c, 0x63fee0)
                e.w32(0x63ff38, 0x63fee0)
                e.w32(0x63fec8, 0)
                e.w32(0x63ff34, 0)
            elif name == "chk 4bf520":
                e.w32(0x6d0f14, 1)
                e.w32(0x6d0f24, p)
                e.w32(0x6d0f20, len(chk))
                r = e.call(0x4bf520)
                e.w32(0x6d0f14, 0)
                print("  chk parse ->", r)
            elif what == "skip":
                pass
            elif isinstance(what, tuple):
                e.call(what[0], stack=what[1])
            else:
                e.call(what)
            print(name, "ok")
        except Exception as ex:
            print(name, "FAILED:", ex, "missing", f.missing[-3:])
            return False
    return True


def check_sai(e, md):
    import emu_map
    got = emu_map.sai_layout(e, e.r32(emu_map.SAI))
    want = open(os.path.join(md, "sai"), "rb").read()
    print("sai after real CHK parse:", "ok" if got == want else "MISMATCH")


UNITS = 0x59cca8


def s16(e, a):
    return struct.unpack("<h", e.read(a, 2))[0]


def s32(e, a):
    return struct.unpack("<i", e.read(a, 4))[0]


def fog_trace(e, f):
    """FNV-1a over the per-tile fog bytes (low byte of each 0x6d1260 entry)."""
    n = e.r16(0x57f1d4) * e.r16(0x57f1d6)
    b = bytes(e.uc.mem_read(e.r32(0x6d1260), n * 4))[0::4]
    h = 0x811c9dc5
    for x in b:
        h = ((h ^ x) * 0x01000193) & 0xffffffff
    return f"f{f} fog {h:#x}"


def bullet_trace(e, f):
    out, b = [], e.r32(0x64dec4)
    while b:
        out.append(f"w{e.r8(b + 0x60)},st{e.r8(b + 0x4d)},{s16(e, b + 0x28)},{s16(e, b + 0x2a)},t{tgt(e, e.r32(b + 0x5c))},"
                   f"tm{e.r8(b + 0x61)},fl{e.r8(b + 0x62)},s{int(e.r32(b + 0xc) != 0)}")
        b = e.r32(b + 4)
    return f"f{f} bul" + "".join(" " + x for x in out)


def thingy_trace(e, f):
    out, t = [], e.r32(0x654874)
    while t:
        sp = e.r32(t + 0xc)
        im = e.r32(sp + 0x18)
        out.append(f"{e.r16(sp + 8)},{s16(e, sp + 0x14)},{s16(e, sp + 0x16)},{e.r16(im + 0x12)},{e.r8(im + 0x17)},{e.r16(im + 0x18)}")
        t = e.r32(t + 4)
    return f"f{f} th " + " ".join(out)


def tgt(e, p):
    return (p - UNITS) // 0x150 if p else "-"


def unit_trace(e, f, u):
    a = UNITS + u * 0x150
    sp = e.r32(a + 0xc)
    return (f"f{f} u{u} pos {s16(e, sp + 0x14)},{s16(e, sp + 0x16)} order {e.r8(a + 0x4d):#x} ost {e.r8(a + 0x4e)} "
            f"mv {e.r8(a + 0x97):#x} mt {s16(e, a + 0x10)},{s16(e, a + 0x12)} nw {s16(e, a + 0x18)},{s16(e, a + 0x1a)} "
            f"ntw {s16(e, a + 0x1c)},{s16(e, a + 0x1e)} fl {e.r8(a + 0x20):#x} dir {e.r8(a + 0x21)} vdir {e.r8(a + 0x23)} "
            f"ddir {e.r8(a + 0x4b)} spd {s32(e, a + 0x38)} nspd {s32(e, a + 0x3c)} top {s32(e, a + 0x34)} "
            f"exact {s32(e, a + 0x2c)},{s32(e, a + 0x30)} vel {s32(e, a + 0x40)},{s32(e, a + 0x44)} "
            f"status {e.r32(a + 0xdc):#x} path {int(e.r32(a + 0x100) != 0)} ot {e.r8(a + 0x85)} mot {e.r8(a + 0x54)} "
            f"hp {s32(e, a + 8)} sh {s32(e, a + 0x60)} gcd {e.r8(a + 0x55)} tgt {tgt(e, e.r32(a + 0x5c))} vis {e.r8(sp + 0xc)}")


def scenario(e, units, move, frames, out):
    """Same steps as `scbw bwsim`: create + finish units, order Move, step frames."""
    for p in range(12):
        e.w8(0x57eee8 + p * 0x24, 2)
    e.call(0x4c3ed0)  # alliances and shared vision at game start
    # 0x49e530 shows a placement error to the owning player (UI only): skip it.
    e.write(0x49e530, b"\xc2\x04\x00")
    made = []
    for spec in units:
        t, x, y, p = spec
        e.w32(0x6d11c8, 1)
        u = e.call(0x4a09d0, regs={"ecx": t, "eax": x}, stack=[y, p])
        if u:
            e.call(0x4a01f0, regs={"eax": u})
            if e.call(0x49ec30, regs={"esi": u}):
                e.call(0x49fa40, regs={"eax": u})
            made.append((u - UNITS) // 0x150)
        e.w32(0x6d11c8, 0)
    if move:
        for i, u in enumerate(made):
            m = move[i % len(move)]
            if m[0] == "o":  # order m[1] targeting the unit at position m[2]
                t = UNITS
                a = e.r32(0x628430)
                while a:
                    sp = e.r32(a + 0xc)
                    if (s16(e, sp + 0x14), s16(e, sp + 0x16)) == m[2]:
                        t = a
                    a = e.r32(a + 4)
                sp = e.r32(t + 0xc)
                e.call(0x474c30, regs={"esi": UNITS + u * 0x150}, stack=[m[1], e.r32(sp + 0x14), t])
                continue
            if m[0] == "@":  # attack made unit k (AttackUnit, order 0xa)
                t = UNITS + made[m[1]] * 0x150
                sp = e.r32(t + 0xc)
                e.call(0x474c30, regs={"esi": UNITS + u * 0x150}, stack=[0xa, e.r32(sp + 0x14), t])
                continue
            mx, my = m
            e.call(0x474c30, regs={"esi": UNITS + u * 0x150}, stack=[6, (my << 16) | (mx & 0xffff), 0])
    with open(out, "w") as fo:
        for f in range(frames):
            e.w32(0x57f23c, e.r32(0x57f23c) + 1)  # 0x4d9670 advances the frame first
            e.call(0x4d94b0)
            for u in made:
                fo.write(unit_trace(e, f, u) + "\n")
            a = e.r32(0x628430)  # other units on the map (map-placed critters etc.)
            while a:
                u = (a - UNITS) // 0x150
                if u not in made:
                    fo.write(unit_trace(e, f, u) + "\n")
                a = e.r32(a + 4)
            fo.write(thingy_trace(e, f) + "\n")
            fo.write(fog_trace(e, f) + "\n")
            fo.write(bullet_trace(e, f) + "\n")
            fo.write(f"f{f} res " + " ".join(f"{e.r32(0x57f0f0 + p * 4)},{e.r32(0x57f120 + p * 4)}" for p in range(2)) + "\n")
            fo.write(f"f{f} rng {e.r32(0x51ca14):#x} {e.r32(0x51ca18)}\n")


def main():
    exe, data, md = sys.argv[1:4]
    e = Emu(exe)
    f = Files(e, data)
    if not game_init(e, f, md):
        return
    check_sai(e, md)
    if len(sys.argv) > 4:
        units = [tuple(int(v) for v in u.split(",")) for u in sys.argv[4].split(";") if u]
        # "x,y" for every unit, or "x,y;x,y;..." cycled over the units.
        # "@k" attacks the k-th created unit.
        # "o<order>:<x>,<y>" orders the unit at (x, y) as target.
        def parse(m):
            if m[0] == "@":
                return ("@", int(m[1:]))
            if m[0] == "o":
                o, xy = m[1:].split(":")
                return ("o", int(o, 0), tuple(int(v) for v in xy.split(",")))
            return tuple(int(v) for v in m.split(","))
        move = [parse(m) for m in sys.argv[5].split(";") if m]
        frames, out = int(sys.argv[6]), sys.argv[7]
        try:
            scenario(e, units, move, frames, out)
        except Exception as ex:
            print("scenario failed:", ex)


if __name__ == "__main__":
    main()
