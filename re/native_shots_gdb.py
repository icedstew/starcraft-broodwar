# gdb script (via winedbg --gdb): after each screen draw (0x41cb28, the return from
# 0x41e280 in 0x41ca00) dump 1.16.1's 640x480 8-bit game screen (*0x6ceff4) and palette
# (0x6ce320) once every CAP_EVERY frames (counter 0x57f23c), with the screen position
# (0x62848c, 0x6284a8), into CAP_OUT. See re/native_shots.sh.
# CAP_CAMERA (a `scbw repcam` file, "frame x y"): after each frame's draw the screen is
# moved there by calling the game's own 0x49c440 (eax = x, ecx = y): the registers are
# saved, the return address pushed and execution resumes at 0x49c440; when it returns
# to the breakpoint the registers are restored.
import gdb, os, struct
OUT = os.environ["CAP_OUT"]
EVERY = int(os.environ.get("CAP_EVERY", "120"))
END = int(os.environ.get("CAP_END", "1000000"))
inf = gdb.selected_inferior()
u32 = lambda a: struct.unpack("<I", bytes(inf.read_memory(a, 4)))[0]
state = {"next": EVERY, "saved": None}
CAM = {}
if os.environ.get("CAP_CAMERA"):
    for line in open(os.environ["CAP_CAMERA"]):
        f_, x_, y_ = map(int, line.split())
        CAM[f_] = (x_, y_)
REGS = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi", "eip", "eflags"]
meta = open(os.path.join(OUT, "meta.txt"), "a")

def move_screen(f):
    """Call 0x49c440(x, y) now, returning to this breakpoint (handled in stop())."""
    if f not in CAM:
        return
    x, y = CAM[f]
    regs = {r: int(gdb.parse_and_eval("$" + r)) & 0xffffffff for r in REGS}
    state["saved"] = regs
    esp = regs["esp"] - 4
    inf.write_memory(esp, struct.pack("<I", regs["eip"]))
    gdb.execute("set $esp = %d" % esp)
    gdb.execute("set $eax = %d" % x)
    gdb.execute("set $ecx = %d" % y)
    gdb.execute("set $eip = 0x49c440")

class Draw(gdb.Breakpoint):
    def stop(self):
        if state["saved"] is not None:  # back from 0x49c440: restore and go on
            for r, v in state["saved"].items():
                gdb.execute("set $%s = %d" % (r, v))
            state["saved"] = None
            return False
        f = u32(0x57f23c)  # frame counter
        try:
            self.capture(f)
        finally:
            move_screen(f - 1)  # repcam frames are the step's (counter - 1)
        return False

    def capture(self, f):
        if f < state["next"] or f > END:
            if f < state["next"] - EVERY:  # new game started: reset
                state["next"] = EVERY
            return False
        buf = bytes(inf.read_memory(u32(0x6ceff4), 640 * 480))
        pal = bytes(inf.read_memory(0x6ce320, 1024))
        sx, sy = u32(0x62848c), u32(0x6284a8)
        open(os.path.join(OUT, "f%06d.raw" % f), "wb").write(buf + pal)
        meta.write("%d %d %d\n" % (f, sx, sy)); meta.flush()
        state["next"] = (f // EVERY + 1) * EVERY
        return False

gdb.execute("set pagination off")
Draw("*0x41cb28")
