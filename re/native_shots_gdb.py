# gdb script (via winedbg --gdb): after each screen draw (0x41cb28, the return from
# 0x41e280 in 0x41ca00) dump 1.16.1's 640x480 8-bit game screen (*0x6ceff4) and palette
# (0x6ce320) once every CAP_EVERY frames (counter 0x57f23c), with the screen position
# (0x62848c, 0x6284a8), into CAP_OUT. See re/native_shots.sh.
import gdb, os, struct
OUT = os.environ["CAP_OUT"]
EVERY = int(os.environ.get("CAP_EVERY", "120"))
END = int(os.environ.get("CAP_END", "1000000"))
inf = gdb.selected_inferior()
u32 = lambda a: struct.unpack("<I", bytes(inf.read_memory(a, 4)))[0]
state = {"next": EVERY}
meta = open(os.path.join(OUT, "meta.txt"), "a")

class Draw(gdb.Breakpoint):
    def stop(self):
        f = u32(0x57f23c)  # frame counter
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
