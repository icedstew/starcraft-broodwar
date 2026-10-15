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
        # SNet turn info (0x4c40f0 after a player leaves): succeed, values left as passed.
        h["storm.dll#109"] = lambda e, a: (1, 3)
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


def load_replay(path):
    """`scbw repdump` output: {"seed", "players": [(p, type, race)], "cmds": [(frame, player, bytes)]}."""
    r = {"players": [], "cmds": []}
    for line in open(path):
        w = line.split()
        if w[0] == "seed":
            r["seed"] = int(w[1], 0)
        elif w[0] == "player":
            r["players"].append((int(w[1]), int(w[2]), int(w[3])))
        elif w[0] in ("template", "mapunits"):
            r[w[0]] = bytes.fromhex(w[1])
        elif w[0] == "cmd":
            r["cmds"].append((int(w[1]), int(w[2]), bytes.fromhex(w[3])))
    return r


def install_ptd(e):
    """__getptd (0x4059e4) reads TLS: return one static per-thread block instead, with
    MSVC's initial rand state (_holdrand at +0x14 = 1). Installed once, before the
    scenario loads (map-placed zerg buildings spread creep with rand)."""
    if e.r8(0x4059e4) == 0xb8:
        return
    ptd = e.alloc(0x8c)
    e.w32(ptd + 0x14, 1)
    e.write(0x4059e4, b"\xb8" + struct.pack("<I", ptd) + b"\xc3")


def game_init(e, f, md):
    """0x4eee00 with the scenario taken from memory (replay-style CHK, 0x4bf520)."""
    install_ptd(e)
    # Trigger UI: the countdown timer text (0x489c30) and its dialog (0x4e5600).
    e.skip(0x489c30)
    e.skip(0x4e5600)
    # 0x4fc7f0: CRT static constructor of the per-player lists at 0x51a280 (freed when
    # a player leaves, 0x48a1c0): empty-list sentinels, head + 8 = ~(head + 4). The
    # harness does not run the CRT init table (its atexit needs CRT state).
    for head in range(0x51a280, 0x51a2e0, 0xc):
        e.w32(head + 8, ~(head + 4) & 0xffffffff)
    rd = lambda n: open(os.path.join(md, n), "rb").read()
    if os.environ.get("SCBW_REPLAY"):
        e.w8(0x58f440, 1)  # Brood War game (BW maps need it to load)
        e.w32(0x6d11e4, 1)  # and its data (0x47ae30 loads all 999 image GRPs)
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
                rep = os.environ.get("SCBW_REPLAY")
                if rep:
                    # 0x4ef100 seeds the RNG from the replay header just before 0x4eee00
                    # loads the scenario, whose units are created with the RNG on.
                    r = load_replay(rep)
                    for pl, ty, race in r["players"]:
                        e.w8(0x57eee8 + pl * 0x24, ty)
                        e.w8(0x57eee8 + pl * 0x24 + 1, race)
                    e.w32(0x51ca14, r["seed"])
                    e.w32(0x51ca18, 0)
                    # The game's template (0x596865) and 0x59bda8 (header +0x271), which
                    # the UNIT loader and 0x49da40 consult.
                    if "template" in r:
                        e.write(0x596865, r["template"])
                    if "mapunits" in r:
                        e.write(0x59bda8, r["mapunits"])
                    e.w32(0x6d11c8, 1)
                    # 0x4eec30 -> 0x489b30: trigger state for the new game.
                    e.call(0x489b30)
                e.w32(0x6d0f14, 1)
                e.w32(0x6d0f24, p)
                e.w32(0x6d0f20, len(chk))
                r = e.call(0x4bf520)
                if rep:
                    # 0x4eee00 tail: AI regions (0x437a70(1)) and resource areas (0x446350).
                    e.call(0x437a70, stack=[1])
                    e.call(0x446350)
                    # 0x4ef100 -> 0x4abaf0: the game type's triggers (Melee.trg ...).
                    e.call(0x4abaf0)
                if not rep:
                    e.w32(0x6d0f14, 0)
                e.w32(0x6d11c8, 0)
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


def tile_trace(e, f):
    """FNV-1a over the tile map (0x5993c4, creep) and the high half of the tile flags."""
    n = e.r16(0x57f1d4) * e.r16(0x57f1d6)
    m = bytes(e.uc.mem_read(e.r32(0x5993c4), n * 2))
    fl = bytes(e.uc.mem_read(e.r32(0x6d1260), n * 4))
    if os.environ.get("TILEDUMP") and f == int(os.environ.get("TILEDUMP_AT", "0")):
        # TILEDUMP=<file>: raw tile map + tile flags at frame TILEDUMP_AT (vs `scbw reptrace`)
        open(os.environ["TILEDUMP"], "wb").write(m + fl)
    h = 0x811c9dc5
    for i in range(n):
        for x in (m[i * 2], m[i * 2 + 1], fl[i * 4 + 2], fl[i * 4 + 3]):
            h = ((h ^ x) * 0x01000193) & 0xffffffff
    return f"f{f} tile {h:#x}"


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
    if not sp:
        return f"f{f} u{u} gone"
    return (f"f{f} u{u} pos {s16(e, sp + 0x14)},{s16(e, sp + 0x16)} order {e.r8(a + 0x4d):#x} ost {e.r8(a + 0x4e)} "
            f"mv {e.r8(a + 0x97):#x} mt {s16(e, a + 0x10)},{s16(e, a + 0x12)} nw {s16(e, a + 0x18)},{s16(e, a + 0x1a)} "
            f"ntw {s16(e, a + 0x1c)},{s16(e, a + 0x1e)} fl {e.r8(a + 0x20):#x} dir {e.r8(a + 0x21)} vdir {e.r8(a + 0x23)} "
            f"ddir {e.r8(a + 0x4b)} spd {s32(e, a + 0x38)} nspd {s32(e, a + 0x3c)} top {s32(e, a + 0x34)} "
            f"exact {s32(e, a + 0x2c)},{s32(e, a + 0x30)} vel {s32(e, a + 0x40)},{s32(e, a + 0x44)} "
            f"status {e.r32(a + 0xdc):#x} path {int(e.r32(a + 0x100) != 0)} ot {e.r8(a + 0x85)} mot {e.r8(a + 0x54)} "
            f"hp {s32(e, a + 8)} sh {s32(e, a + 0x60)} gcd {e.r8(a + 0x55)} tgt {tgt(e, e.r32(a + 0x5c))} vis {e.r8(sp + 0xc)} str {e.r16(a + 0x138)},{e.r16(a + 0x13a)}")


def pcx(path):
    """(w, h, pixels) of an 8-bit PCX (asset decode only)."""
    d = open(path, "rb").read()
    x0, y0, x1, y1 = struct.unpack_from("<4H", d, 4)
    w, h, bpl = x1 - x0 + 1, y1 - y0 + 1, struct.unpack_from("<H", d, 66)[0]
    out, i = bytearray(), 128
    while len(out) < bpl * h:
        c = d[i]
        i += 1
        if c >= 0xc0:
            out += bytes([d[i]]) * (c & 0x3f)
            i += 1
        else:
            out.append(c)
    return w, h, b"".join(out[r * bpl:r * bpl + w] for r in range(h))


TILESETS = ["badlands", "platform", "install", "ashworld", "jungle", "desert", "ice", "twilight"]


def minimap_init(e, data):
    """The minimap state 0x4a5a80 sets up, with its asset loads done here."""
    name = TILESETS[e.r16(0x57f1dc)]
    wpe = open(os.path.join(data, "tileset", name + ".wpe"), "rb").read()
    e.write(0x5994e0, wpe)  # palette
    e.write(0x5973a0, pcx(os.path.join(data, "tileset", name, "dark.pcx"))[2][:0x2000])
    e.write(0x581dd6, pcx(os.path.join(data, "game", "tminimap.pcx"))[2][:12])
    e.call(0x4bdd60)  # color-cycling exclusions 0x6d125c
    for i in range(256):  # 0x59cb60 remap, as 0x4a5a80
        if e.call(0x4cb560, regs={"ecx": i}):
            v = e.call(0x4bdb30, regs={"ecx": 0x5994e0}, stack=[struct.unpack_from("<I", wpe, i * 4)[0]]) & 0xff
        else:
            v = i
        e.w8(0x59cb60 + i, v)
    e.call(0x41e450, regs={"ebx": 0x4bdb30}, stack=[0x5994e0])  # interface colors 0x6ceb20
    e.w32(0x59cc60, e.alloc(0x40))  # the minimap dialog control
    e.w16(0x59cc74, 0x80)
    e.w16(0x59cc64, 0x80)
    e.call(0x4a4400)


def minimap_trace(e, f):
    """0x4a4ce0's redraw: terrain, fog, unit boxes, drawn for local player 0 (the
    simulation runs with 0x512684 = 8, which keeps the UI side effects of player 0 off)."""
    local = e.r32(0x512684)
    e.w32(0x512684, 0)
    e.call(e.r32(0x59c1a0))
    e.call(0x4a4150)
    e.call(0x4a4ac0)
    e.w32(0x512684, local)
    w, h = e.r16(0x59cc74), e.r16(0x59cc64)
    return f"f{f} minimap {w}x{h} {e.r32(0x59c2b8)} " + e.read(e.r32(0x59c198), w * h).hex()


def select_test(e, f, tests):
    """Drag boxes through 0x46fa40 for local player 0, from no selection; 0x49aef0
    (apply + send) is replaced by storing its list as the client selection 0x6284b8."""
    from unicorn.x86_const import UC_X86_REG_ESP
    if not hasattr(e, "sel_hooked"):
        def apply(e):
            esp = e.uc.reg_read(UC_X86_REG_ESP)
            n, lst = e.r32(esp + 4), e.r32(esp + 8)
            e.write(0x6284b8, e.read(lst, n * 4) + bytes(4 * (12 - n)))
        e.at(0x49aef0, apply)
        e.skip(0x49aef0, 0, 4)
        e.sel_hooked = True
    local = e.r32(0x512684)
    e.w32(0x512684, 0)
    e.write(0x6284b8, bytes(48))
    out = []
    for b in tests.split(";"):
        if not b:
            continue
        v = b.split(",")
        r = e.alloc(8)
        e.write(r, struct.pack("<4h", *[int(x) for x in v[:4]]))
        e.w8(0x596a28, 1 if v[4:] == ["s"] else 0)
        e.w8(0x596a2a, 0)
        e.call(0x46fa40, stack=[r])
        sel = [(a - UNITS) // 0x150 for a in struct.unpack("<12I", e.read(0x6284b8, 48)) if a]
        out.append(f"f{f} sel {b} -> " + " ".join(map(str, sel)))
    e.w8(0x596a28, 0)
    e.w32(0x512684, local)
    return "\n".join(out)


def parse_move(m):
    if m[0] == "@":
        return ("@", int(m[1:]))
    if m[0] == "t":  # "t<type>[x<count>]"
        t, _, n = m[1:].partition("x")
        return ("t", int(t, 0), int(n) if n else 1)
    if m[0] == "P":  # "P<hex>": raw command packet through 0x4865d0 as player 0
        return ("P", bytes.fromhex(m[1:]))
    if m[0] == "M":  # "M<type>": every player-0 larva morphs into <type>
        return ("M", int(m[1:], 0))
    if m[0] in "SAD":  # "S<i>,<j>,...": select / add / remove created units i, j, ... (0x09-0x0b)
        # "#n" is unit array index n rather than the n-th created unit.
        return (m[0], tuple(("#", int(v[1:])) if v.startswith("#") else int(v) for v in m[1:].split(",") if v))
    if m[0] == "K":  # "K<op>[:<u16>]": cancel command 0x18 / 0x19 / 0x20 with the current selection
        op, _, arg = m[1:].partition(":")
        return ("K", int(op, 0), int(arg, 0) if arg else 0)
    if m[0] == "H":  # "H<action>:<group>": hotkey command 0x13
        a, g = m[1:].split(":")
        return ("H", int(a), int(g))
    if m[0] in "RT":  # "R<x>,<y>[q]" / "R@k[q]", "T<order>:<x>,<y>[q]" / "T<order>:@k[q]"
        q = 1 if m.endswith("q") else 0
        body = m[1:].rstrip("q")
        o = None
        if m[0] == "T":
            o, body = body.split(":")
            o = int(o, 0)
        if body[0] == "@":
            xy, k = (0, 0), int(body[1:])
        else:
            xy, k = tuple(int(v) for v in body.split(",")), None
        return (m[0], o, xy[0], xy[1], k, q) if o is not None else (m[0], xy[0], xy[1], k, q)
    if m[0] == "b":  # "b<order>:<type>:<tx>,<ty>"
        o, t, xy = m[1:].split(":")
        return ("b", int(o, 0), int(t, 0), tuple(int(v) for v in xy.split(",")))
    if m[0] == "o":
        o, xy = m[1:].split(":")
        if xy[0] == "@":
            return ("o", int(o, 0), ("@", int(xy[1:])))
        return ("o", int(o, 0), tuple(int(v) for v in xy.split(",")))
    return tuple(int(v) for v in m.split(","))


def scenario(e, units, move, frames, out):
    """Same steps as `scbw bwsim`: create + finish units, order Move, step frames."""
    for p in range(12):
        e.w8(0x57eee8 + p * 0x24, 2)
    for p in range(8):  # starting money for training/building scenarios
        e.w32(0x57f0f0 + p * 4, 10000)
        e.w32(0x57f120 + p * 4, 10000)
    replay = None
    if os.environ.get("SCBW_REPLAY"):
        # `scbw repdump` output: play the replay's game (0x4ef100 seeds the RNG from the
        # header, 0x487100 feeds each frame's commands to 0x4865d0).
        replay = load_replay(os.environ["SCBW_REPLAY"])
        for p, ty, race in replay["players"]:
            e.w8(0x57eee8 + p * 0x24, ty)
            e.w8(0x57eee8 + p * 0x24 + 1, race)
            e.w32(0x57f0f0 + p * 4, 50)
            e.w32(0x57f120 + p * 4, 0)
    if replay:
        for p in range(8, 12):
            e.w8(0x57eee8 + p * 0x24, 0)
    e.call(0x4c3ed0)  # alliances and shared vision at game start
    # 0x49e530 shows a placement error to the owning player (UI only): skip it.
    e.write(0x49e530, b"\xc2\x04\x00")
    e.w32(0x57f0b0, 1)  # local player 0's vision mask
    e.w32(0x57eeb8, 0x100)  # and explored mask
    install_ptd(e)
    if replay:
        e.w32(0x6d0f14, 1)
        # The end of 0x4ef100 for replays: everyone's vision and exploration shown.
        e.w32(0x6d0f18, 0xff)
        e.w32(0x57f0b0, 0xff)
        e.w32(0x6d0f1c, 0)
        e.w32(0x57eeb8, 0xff00)
        e.w32(0x596bbc, e.alloc(0x20))  # replay writer, not recording while playing back
        # 0x48cd30: add a line to the on-screen message list (UI only; it lays the text
        # out with fonts that are not loaded here).
        e.write(0x48cd30, b"\xc2\x0c\x00")
        for tbl in (0x6d1238, 0x6d1220):  # string tables (stat_txt, network): empty, so
            if not e.r32(tbl):              # messages fall back to the default string
                e.w32(tbl, e.alloc(0x10))
        if "template" not in replay:  # old repdumps: a melee game
            e.w8(0x596871, 2)
        e.w32(0x6d11c8, 1)
        e.call(0x49da40)
        e.w32(0x6d11c8, 0)
        if os.environ.get("SCBW_RNGDBG"):
            sites = struct.unpack("<256I", e.read(0x51c610, 0x400))
            print("rngsites", " ".join(f"{i:#x}:{c}" for i, c in enumerate(sites) if c))
    melee = os.environ.get("SCBW_MELEE")
    if melee:  # "race,race,...": those players start a melee game (0x49da40, mode 2)
        races = [int(r) for r in melee.split(",")]
        for p in range(8):
            e.w8(0x57eee8 + p * 0x24, 2 if p < len(races) else 0)
            e.w8(0x57eee8 + p * 0x24 + 1, races[p] if p < len(races) else 0)
        e.w8(0x596871, 2)
        e.w32(0x6d11c8, 1)
        e.call(0x49da40)
        e.w32(0x6d11c8, 0)
        if os.environ.get("SCBW_RNGDBG"):
            print("melee rng total", e.r32(0x51ca18))
    ai = os.environ.get("SCBW_AI")
    if ai:  # "p,p,...": computers running their race's melee script
        for p in (int(v) for v in ai.split(",")):
            e.w8(0x57eee8 + p * 0x24, 1)
        e.call(0x437a70, stack=[1])
        e.call(0x446350)  # 0x4eee00: resource areas right after
        e.w8(0x596873, 1)
        e.w8(0x596871, 2)
        e.call(0x4ab230)
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
    def apply_moves(move):
        if move:
            # Group commands ("R..." right-click, "T..." targeted order) go through the
            # command receivers with every created player-0 unit selected.
            sel = [u for u in made if e.r8(UNITS + u * 0x150 + 0x4c) == 0][:12]
            if not made:  # melee start: player 0's workers
                sel = [u for u in range(1700) if e.r32(UNITS + u * 0x150 + 0xc) and e.r8(UNITS + u * 0x150 + 0x4c) == 0
                       and e.r16(UNITS + u * 0x150 + 0x64) in (7, 0x29, 0x40)][:12]
            pkt = e.alloc(16)
            for m in move:
                if m[0] == "M":  # unit morph receiver 0x4c1990 with player 0's larvae selected
                    lv = [u for u in range(1700) if e.r32(UNITS + u * 0x150 + 0xc) and e.r8(UNITS + u * 0x150 + 0x4c) == 0
                          and e.r16(UNITS + u * 0x150 + 0x64) == 0x23][:12]
                    for k in range(12):
                        e.w32(0x6284e8 + k * 4, UNITS + lv[k] * 0x150 if k < len(lv) else 0)
                    e.w32(0x512678, 0)
                    e.w32(0x51267c, 0)
                    e.write(pkt, bytes([0x23]) + struct.pack("<H", m[1]))
                    e.call(0x4c1990, stack=[pkt])
                    continue
                if m[0] == "P":
                    if not e.r32(0x596bbc):  # 0x4865d0 hands every command to the replay writer
                        e.w32(0x596bbc, e.alloc(0x20))
                    e.w32(0x512678, 0)
                    e.w32(0x51267c, 0)
                    e.write(pkt, m[1])
                    e.w32(0x6d11c8, 1)
                    e.call(0x4865d0, regs={"eax": pkt}, stack=[len(m[1]), 1])
                    e.w32(0x6d11c8, 0)
                    continue
                if m[0] == "K":
                    e.w32(0x512678, 0)
                    e.w32(0x51267c, 0)
                    e.write(pkt, bytes([m[1]]) + struct.pack("<H", m[2]))
                    if m[1] == 0x20:
                        e.call(0x4c0100, stack=[pkt])
                    else:
                        e.call({0x18: 0x4c2ef0, 0x19: 0x4c2ec0}[m[1]])
                    continue
                if m[0] in ("S", "A", "D", "H"):
                    e.w32(0x512678, 0)
                    e.w32(0x51267c, 0)
                    if m[0] == "H":
                        e.write(pkt, bytes([0x13, m[1], m[2]]))
                        e.call(0x4c2870, regs={"ecx": pkt})
                        continue
                    units = made or sel
                    ids = [i[1] if isinstance(i, tuple) else units[i] for i in m[1]]
                    hs = [(e.r8(UNITS + u * 0x150 + 0xa5) << 11) | (u + 1) for u in ids]
                    e.write(pkt, bytes([{"S": 9, "A": 10, "D": 11}[m[0]], len(hs)]) + struct.pack(f"<{len(hs)}H", *hs))
                    e.call({"S": 0x4c2750, "A": 0x4c2560, "D": 0x4bfb40}[m[0]], stack=[pkt])
                    continue
                if m[0] not in ("R", "T"):
                    continue
                for k in range(12):
                    e.w32(0x6284e8 + k * 4, UNITS + sel[k] * 0x150 if k < len(sel) else 0)
                e.w32(0x512678, 0)
                e.w32(0x51267c, 0)
                x, y, k, q = m[-4], m[-3], m[-2], m[-1]
                h = 0
                if k is not None:
                    t = made[k]
                    h = (e.r8(UNITS + t * 0x150 + 0xa5) << 11) | (t + 1)
                    sp = e.r32(UNITS + t * 0x150 + 0xc)
                    x, y = s16(e, sp + 0x14), s16(e, sp + 0x16)
                b = bytes([0x14 if m[0] == "R" else 0x15]) + struct.pack("<HHHH", x & 0xffff, y & 0xffff, h, 0xe4)
                if m[0] == "R":
                    e.write(pkt, b + bytes([q]))
                    e.call(0x4c2370, regs={"eax": pkt})
                else:
                    e.write(pkt, b + bytes([m[1], q]))
                    e.call(0x4c2320, regs={"eax": pkt})
            for i, u in enumerate(made or sel):
                m = move[i % len(move)]
                if m[0] in ("R", "T", "M", "S", "A", "D", "H", "K", "P"):
                    continue
                if m[0] == "t":  # train unit type m[1] (0x4c1c20 without the selection checks)
                    a = UNITS + u * 0x150
                    for _ in range(m[2]):
                        if m[1] < 0x6a and e.call(0x467250, regs={"edi": a}, stack=[m[1]]):
                            e.call(0x4743d0, regs={"eax": a, "ecx": 0x26})
                    continue
                if m[0] == "b":  # build type m[2] at tile m[3] with order m[1] (0x48e010 without the UI)
                    a = UNITS + u * 0x150
                    tx, ty = m[3]
                    ok = e.call(0x473fb0, stack=[a, e.r8(a + 0x4c), tx, ty, m[2], 1, 0, 0, 0])
                    if ok == 0:
                        pw, ph = s16(e, 0x662860 + m[2] * 4), s16(e, 0x662862 + m[2] * 4)
                        x, y = int(pw / 2) + tx * 32, int(ph / 2) + ty * 32
                        e.call(0x48de70, regs={"eax": a}, stack=[m[1], ((y & 0xffff) << 16) | (x & 0xffff), m[2]])
                    continue
                if m[0] == "o" and m[2][0] == "@":  # order m[1] targeting created unit k
                    t = UNITS + made[m[2][1]] * 0x150
                    sp = e.r32(t + 0xc)
                    e.call(0x474c30, regs={"esi": UNITS + u * 0x150}, stack=[m[1], e.r32(sp + 0x14), t])
                    continue
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

    # 0x4d9950 -> 0x4d9530: the game steps twice before the frame loop starts counting.
    e.call(0x4d94b0)
    e.call(0x4d94b0)
    apply_moves(move)
    move2, at = os.environ.get("SCBW_MOVE2"), int(os.environ.get("SCBW_AT", "0"))
    move2 = [parse_move(m) for m in move2.split(";") if m] if move2 else None
    minimap = bool(os.environ.get("SCBW_MINIMAP"))
    if minimap:
        minimap_init(e, e.data_root)
    camera = {}
    if os.environ.get("SCBW_CAMERA"):  # a `scbw repcam` file: "frame x y"
        for line in open(os.environ["SCBW_CAMERA"]):
            f_, x_, y_ = map(int, line.split())
            camera[f_] = (x_, y_)
    rnglog_at = os.environ.get("SCBW_RNGLOG_AT")
    cur = [0]
    if rnglog_at:
        # Debug: every sync_rand call on that frame: site, caller, iscript unit (0x6d11fc).
        def rnglog(e):
            if rnglog_at in ("all", str(cur[0])):
                from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ESP
                su = e.r32(0x6d11fc)
                from unicorn.x86_const import UC_X86_REG_ESI
                print(f"f{cur[0]} rng site {e.uc.reg_read(UC_X86_REG_EAX):#x} from {e.r32(e.uc.reg_read(UC_X86_REG_ESP)):#x} unit {(su - UNITS) // 0x150 if su else '-'} img {e.r16(e.uc.reg_read(UC_X86_REG_ESI) + 8):#x} anim {e.r8(e.uc.reg_read(UC_X86_REG_ESI) + 0x16):#x} pc {e.r16(e.uc.reg_read(UC_X86_REG_ESI) + 0x12):#x}", file=sys.stderr)
        e.at(0x4dc4a0, rnglog)
        e.uc.ctl_flush_tb()  # hooks added after code was translated need a flush
    thlog_at = os.environ.get("SCBW_THLOG_AT")
    if thlog_at:
        # Debug: thingy creations (0x488210) on that frame: stack args, caller, regs.
        def thlog(e):
            if str(cur[0]) == thlog_at:
                from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESI, UC_X86_REG_EDI
                sp = e.uc.reg_read(UC_X86_REG_ESP)
                regs = " ".join(f"{n}={e.uc.reg_read(r):#x}" for n, r in (("eax", UC_X86_REG_EAX), ("ecx", UC_X86_REG_ECX), ("edx", UC_X86_REG_EDX), ("esi", UC_X86_REG_ESI), ("edi", UC_X86_REG_EDI)))
                su = e.r32(0x6d11fc)
                print(f"f{cur[0]} thingy from {e.r32(sp):#x} args {[hex(e.r32(sp + 4 * k)) for k in (1, 2, 3)]} {regs} unit {(su - UNITS) // 0x150 if su else '-'}", file=sys.stderr)
        e.at(0x488210, thlog)
        def thimg(e):
            if str(cur[0]) == thlog_at:
                from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EAX
                im = e.uc.reg_read(UC_X86_REG_EAX)
                sp = e.uc.reg_read(UC_X86_REG_ESP)
                print(f"f{cur[0]} sprite-at-image from {e.r32(sp):#x} id {e.r32(sp + 4):#x} img {e.r16(im + 8):#x} anim {e.r8(im + 0x16):#x} pc {e.r16(im + 0x12):#x}", file=sys.stderr)
        e.at(0x4d7120, thimg)
        e.uc.ctl_flush_tb()
    with open(out, "w") as fo:
        ci = 0
        pkt = e.alloc(0x400)
        for f in range(frames):
            cur[0] = f
            e.w32(0x57eebc, e.r32(0x57f23c))  # net turns: one per frame
            while replay and ci < len(replay["cmds"]) and replay["cmds"][ci][0] <= f:
                _, p, data = replay["cmds"][ci]
                ci += 1
                e.write(pkt, data)
                e.w32(0x512678, p)
                e.w32(0x51267c, p)
                e.w32(0x6d11c8, 1)
                e.w32(0x6d5bec, 1)
                e.w32(0x6556e8, 0)
                e.call(0x4865d0, regs={"eax": pkt}, stack=[len(data), 1])
                e.w32(0x6d5bec, 0)
                e.w32(0x6d11c8, 0)
                e.w32(0x512678, e.r32(0x512684))
                e.w32(0x51267c, e.r32(0x512688))
            if move2 and f == at:
                apply_moves(move2)
            e.w32(0x57f23c, e.r32(0x57f23c) + 1)  # 0x4d9670 advances the frame first
            e.call(0x4d94b0)
            if replay:
                # 0x4d9670: the trigger pass (0x48a460) with the RNG on, at the fastest
                # speed's frame time (0x5124d8[6]).
                e.call(0x4dc540, regs={"ecx": 1})
                e.call(0x48a460, stack=[0x2a])
                e.call(0x4dc540, regs={"ecx": 0})
            if f in camera:
                # 0x49c440 (its 0x49bfd0 scroll redraw is UI only): the screen position
                # set after this frame's draw.
                x, y = camera[f]
                e.w32(0x62848c, x & ~7)
                e.w16(0x57f1d0, x >> 5)
                e.w32(0x6284a8, y & ~7)
                e.w16(0x57f1d2, y >> 5)
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
            if f % 25 == 0:
                fo.write(tile_trace(e, f) + "\n")
            if os.environ.get("SCBW_TILEDUMP") and f == int(os.environ.get("SCBW_TILEDUMP_AT", "0")):
                n = e.r16(0x57f1d4) * e.r16(0x57f1d6)
                with open(os.environ["SCBW_TILEDUMP"] + ".emu", "wb") as td:
                    td.write(bytes(e.uc.mem_read(e.r32(0x5993c4), n * 2)) + bytes(e.uc.mem_read(e.r32(0x6d1260), n * 4)))
            fo.write(bullet_trace(e, f) + "\n")
            fo.write(f"f{f} res " + " ".join(f"{e.r32(0x57f0f0 + p * 4)},{e.r32(0x57f120 + p * 4)}" for p in range(8 if replay else 2)) + "\n")
            fo.write(f"f{f} rng {e.r32(0x51ca14):#x} {e.r32(0x51ca18)}\n")
            if replay:  # first free unit (0x628438): unit allocation order
                fr, ids = e.r32(0x628438), []
                while fr and len(ids) < 3:
                    ids.append(str((fr - UNITS) // 0x150))
                    fr = e.r32(fr + 4)
                fo.write(f"f{f} free {' '.join(ids)}\n")
                fo.write(f"f{f} trg {list(e.read(0x58d700, 8))} {e.r32(0x58d6f8)} {e.r16(0x6509a0)} {e.r32(0x58dc40):x}\n")
            if os.environ.get("SCBW_IMGWATCH"):  # "unit,from": the unit's images each frame
                wu, wf = (int(v) for v in os.environ["SCBW_IMGWATCH"].split(","))
                if f >= wf:
                    ua = UNITS + wu * 0x150
                    sp, im, parts = e.r32(ua + 0xc), 0, []
                    if sp:
                        im = e.r32(sp + 0x1c)
                    while im:
                        parts.append(f"{e.r16(im + 8):#x}:a{e.r8(im + 0x16):#x}:pc{e.r16(im + 0x12):#x}:w{e.r8(im + 0x17)}")
                        im = e.r32(im + 4)
                    print(f"f{f} img u{wu} st {e.r32(ua + 0xdc):#x} ord {e.r8(ua + 0x4d):#x} " + " ".join(parts), file=sys.stderr)
            if os.environ.get("SCBW_RNGSITES_AT") in (str(f), str(f - 1)):
                sites = struct.unpack("<256I", e.read(0x51c610, 0x400))
                fo.write(f"f{f} rngsites " + " ".join(f"{i:#x}:{c}" for i, c in enumerate(sites) if c) + "\n")
            if os.environ.get("SCBW_SELTRACE"):
                row = [(a - UNITS) // 0x150 for a in struct.unpack("<12I", e.read(0x6284e8, 48)) if a]
                fo.write(f"f{f} psel " + " ".join(map(str, row)) + "\n")
                for g in range(18):
                    hk = [h for h in struct.unpack("<12I", e.read(0x57fe60 + g * 48, 48)) if h]
                    if hk:
                        fo.write(f"f{f} hk{g} " + " ".join(f"{h:#x}" for h in hk) + "\n")
            if os.environ.get("SCBW_CLICKTEST") and f % 50 == 0:
                local = e.r32(0x512684)
                e.w32(0x512684, 0)
                # Image map positions (+0x1c) are refreshed as sprites are drawn: 0x4983a0
                # for every unit's sprite, as if all were on screen.
                a = e.r32(0x628430)
                while a:
                    e.call(0x4983a0, regs={"eax": e.r32(a + 0xc)})
                    a = e.r32(a + 4)
                for pt in filter(None, os.environ["SCBW_CLICKTEST"].split(";")):
                    x, y = (int(v) for v in pt.split(","))
                    a = e.call(0x46f3a0, stack=[x, y])
                    fo.write(f"f{f} click {pt} -> {(a - UNITS) // 0x150 if a else '-'}\n")
                e.w32(0x512684, local)
            if os.environ.get("SCBW_SELTEST") and f % 50 == 0:
                fo.write(select_test(e, f, os.environ["SCBW_SELTEST"]) + "\n")
            if minimap and f % 50 == 0:
                fo.write(minimap_trace(e, f) + "\n")


def main():
    exe, data, md = sys.argv[1:4]
    e = Emu(exe)
    f = Files(e, data)
    e.data_root = data
    if not game_init(e, f, md):
        return
    check_sai(e, md)
    if len(sys.argv) > 4:
        units = [tuple(int(v) for v in u.split(",")) for u in sys.argv[4].split(";") if u]
        # "x,y" for every unit, or "x,y;x,y;..." cycled over the units.
        # "@k" attacks the k-th created unit.
        # "o<order>:<x>,<y>" orders the unit at (x, y) as target.
        move = [parse_move(m) for m in sys.argv[5].split(";") if m]
        frames, out = int(sys.argv[6]), sys.argv[7]
        try:
            scenario(e, units, move, frames, out)
        except Exception as ex:
            print("scenario failed:", ex)


if __name__ == "__main__":
    main()
