# gdb script (via winedbg --gdb, see re/native_shots.sh with NATIVE_MODE=trace): after
# every game_step call in 0x4d9670 (return address 0x4d9780) write the real 1.16.1's
# per-frame unit trace in re/emu_game.py's format to $CAP_OUT/trace.txt, for frames up to
# $CAP_END. Frame f is the state after the f+1-th step, as the emulator numbers it.
import gdb, os, sys, struct

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
# emu_game imports the unicorn-based emulator, which gdb's Python lacks: its trace
# functions only need the read interface below.
import types  # noqa: E402
for _m in ("emu", "emu_map"):
    sys.modules.setdefault(_m, types.SimpleNamespace(Emu=None))
import emu_game  # noqa: E402

OUT = os.environ["CAP_OUT"]
END = int(os.environ.get("CAP_END", "2000"))
inf = gdb.selected_inferior()


class Mem:
    """emu.Emu's read interface over the debuggee, cached per 4 KiB page per frame."""

    def __init__(self):
        self.pages = {}

    def read(self, a, n):
        out = b""
        while n > 0:
            p = a & ~0xfff
            if p not in self.pages:
                self.pages[p] = bytes(inf.read_memory(p, 0x1000))
            k = min(n, p + 0x1000 - a)
            out += self.pages[p][a - p:a - p + k]
            a += k
            n -= k
        return out

    def r8(self, a):
        return self.read(a, 1)[0]

    def r16(self, a):
        return struct.unpack("<H", self.read(a, 2))[0]

    def r32(self, a):
        return struct.unpack("<I", self.read(a, 4))[0]


fo = open(os.path.join(OUT, "trace.txt"), "w")


# CAP_CAMERA (a `scbw repcam` file): after each step the screen moves like a player's
# camera, through the game's own 0x49c440 (as in native_shots_gdb.py).
CAM = {}
if os.environ.get("CAP_CAMERA"):
    for line in open(os.environ["CAP_CAMERA"]):
        f_, x_, y_ = map(int, line.split())
        CAM[f_] = (x_, y_)
REGS = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi", "eip", "eflags"]
saved = [None]


def move_screen(f):
    if f not in CAM:
        return
    x, y = CAM[f]
    regs = {r: int(gdb.parse_and_eval("$" + r)) & 0xffffffff for r in REGS}
    saved[0] = regs
    esp = regs["esp"] - 4
    gdb.selected_inferior().write_memory(esp, struct.pack("<I", regs["eip"]))
    gdb.execute("set $esp = %d" % esp)
    gdb.execute("set $eax = %d" % x)
    gdb.execute("set $ecx = %d" % y)
    gdb.execute("set $eip = 0x49c440")


class Step(gdb.Breakpoint):
    def stop(self):
        if saved[0] is not None:  # back from 0x49c440
            for r, v in saved[0].items():
                gdb.execute("set $%s = %d" % (r, v))
            saved[0] = None
            return False
        e = Mem()
        k = e.r32(0x57f23c)
        f = k - 1
        try:
            self.trace(e, f)
        finally:
            move_screen(f)
        return False

    def trace(self, e, f):
        if f < 0 or f > END:
            return False
        a = e.r32(0x628430)  # visible units, in list order like the emulator's trace
        while a:
            fo.write(emu_game.unit_trace(e, f, (a - emu_game.UNITS) // 0x150) + "\n")
            a = e.r32(a + 4)
        fo.write(emu_game.thingy_trace(e, f) + "\n")
        fo.write(f"f{f} res " + " ".join(f"{e.r32(0x57f0f0 + p * 4)},{e.r32(0x57f120 + p * 4)}" for p in range(8)) + "\n")
        fo.write(f"f{f} rng {e.r32(0x51ca14):#x} {e.r32(0x51ca18)}\n")
        if f == 0:  # per call-site draw counts so far (0x51c610)
            sites = struct.unpack("<256I", e.read(0x51c610, 0x400))
            fo.write("f0 rngsites " + " ".join(f"{i:#x}:{c}" for i, c in enumerate(sites) if c) + "\n")
        fo.flush()
        return False


gdb.execute("set pagination off")
Step("*0x4d9780")
