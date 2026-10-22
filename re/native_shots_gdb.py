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
state = {"next": EVERY, "saved": None, "full": None, "after": None}
# CAP_FULL=1: also the whole screen as the player sees it (HUD, minimap, console).
# The game buffer above has only the map layer; the screen gets the frame together with
# the interface at the next draw's blit (0x41c200), so the X screen is grabbed just
# before that draw's layer pass (0x41cb23), named after the captured frame.
FULL = bool(os.environ.get("CAP_FULL"))
# CAP_FULL=surf: instead read the screen surface itself at the same draw as the buffer
# dump (fNNNNNN_full.raw; exact, no X lag): the game's own SDrawLockSurface (Storm 350,
# thunk 0x411e4e: 0, 0, &ptr, &pitch, 0) is called from the breakpoint, the surface read,
# then SDrawUnlockSurface (356, 0x411e48: 0, ptr, 0, 0), the registers restored.
SURF = os.environ.get("CAP_FULL") == "surf"
FULL = FULL and not SURF
surf = {"step": None}
# CAP_FULL_PROBE=1: grab the X screen both before (fNNNNNN_fullA.png) and after
# (fNNNNNN_fullB.png) the next draw's layer pass, to measure which one shows the frame.
PROBE = bool(os.environ.get("CAP_FULL_PROBE"))
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

# events.txt, in order: "T <frame> <id>" for each minimap timer firing (they run on real
# time: 0x4a4e00 id 1, 0x4a4a70 id 2, 0x4a4e40 id 3, 0x4a4f10 id 6, set by 0x4a5770; id 256
# is a palette cycling step 0x41e900) and
# "G <frame> <file> <cursor counter 0x597390> <mouse x y 0x6cddc4/c8> <screen x y>" for
# each X grab, so `scbw repshot` fires the same timers at the same points and draws the
# same cursor.
events = open(os.path.join(OUT, "events.txt"), "a")

def grab(name):
    import subprocess
    subprocess.run(["import", "-window", "root", os.path.join(OUT, name)], check=False)
    events.write("G %d %s %d %d %d %d %d\n" % (u32(0x57f23c), name, u32(0x597390), u32(0x6cddc4), u32(0x6cddc8), u32(0x62848c), u32(0x6284a8)))
    events.flush()

class Timer(gdb.Breakpoint):
    def __init__(self, addr, tid):
        super().__init__("*%#x" % addr)
        self.tid = tid
    def stop(self):
        events.write("T %d %d\n" % (u32(0x57f23c), self.tid))
        return False

def stdcall_here(target, args, regs):
    """Call target(args) and come back to this breakpoint; a scratch area below the
    saved stack holds out-values (returned: its address)."""
    scratch = regs["esp"] - 0x100
    esp = scratch - 4 * (len(args) + 1)
    inf.write_memory(esp, struct.pack("<%dI" % (len(args) + 1), regs["eip"], *[a(scratch) if callable(a) else a for a in args]))
    gdb.execute("set $esp = %d" % esp)
    gdb.execute("set $eip = %d" % target)
    return scratch

def surf_step():
    """The lock / read / unlock calls in turn; True while one is under way."""
    st = surf["step"]
    if st is None:
        return False
    regs = st["regs"]
    if st["at"] == "lock":
        ok = int(gdb.parse_and_eval("$eax")) & 0xffffffff
        ptr, pitch = u32(st["scratch"]), u32(st["scratch"] + 4)
        if ok and ptr and pitch >= 640:
            px = b"".join(bytes(inf.read_memory(ptr + y * pitch, 640)) for y in range(480))
            open(os.path.join(OUT, "f%06d_full.raw" % surf["f"]), "wb").write(px + surf["pal"])
            st["at"] = "unlock"
            stdcall_here(0x411e48, [0, ptr, 0, 0], regs)
            return True
    for r, v in regs.items():  # unlocked (or the lock failed): back to the draw
        gdb.execute("set $%s = %d" % (r, v))
    surf["step"] = None
    move_screen(surf["f"] - 1)
    return True

class Draw(gdb.Breakpoint):
    def stop(self):
        if surf["step"] is not None:
            surf_step()
            return False
        if state["after"] is not None and state["saved"] is None:
            grab("f%06d_fullB.png" % state["after"])
            state["after"] = None
        if state["saved"] is not None:  # back from 0x49c440: restore and go on
            for r, v in state["saved"].items():
                gdb.execute("set $%s = %d" % (r, v))
            state["saved"] = None
            return False
        f = u32(0x57f23c)  # frame counter
        took = False
        try:
            took = self.capture(f)
        finally:
            if took and SURF:  # the screen move follows the surface read (surf_step)
                regs = {r: int(gdb.parse_and_eval("$" + r)) & 0xffffffff for r in REGS}
                surf["step"] = {"at": "lock", "regs": regs}
                surf["step"]["scratch"] = stdcall_here(0x411e4e, [0, 0, lambda sc: sc, lambda sc: sc + 4, 0], regs)
            else:
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
        if FULL:
            state["full"] = f
        if SURF:
            events.write("G %d f%06d_full.png %d %d %d %d %d\n" % (f, f, u32(0x597390), u32(0x6cddc4), u32(0x6cddc8), sx, sy))
            events.flush()
            surf["f"], surf["pal"] = f, pal
        state["next"] = (f // EVERY + 1) * EVERY
        return True

class Blitted(gdb.Breakpoint):
    def stop(self):
        f = state["full"]
        if f is not None:
            state["full"] = None
            grab("f%06d_fullA.png" % f if PROBE else "f%06d_full.png" % f)
            if PROBE:
                state["after"] = f
        return False


gdb.execute("set pagination off")
Draw("*0x41cb28")
if SURF:
    # id 256: a palette cycling step (0x41e900, every 10 ms of real time from 0x4d93f0).
    for a, t in ((0x4a4e00, 1), (0x4a4a70, 2), (0x4a4e40, 3), (0x4a4f10, 6), (0x41e900, 256)):
        Timer(a, t)
if FULL:
    Blitted("*0x41cb23")
    for a, t in ((0x4a4e00, 1), (0x4a4a70, 2), (0x4a4e40, 3), (0x4a4f10, 6)):
        Timer(a, t)
