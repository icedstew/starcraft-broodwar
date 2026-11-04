# gdb probe (NATIVE_MODE=trace NATIVE_GDBPY=re/probe_local.py re/native_shots.sh <rep> <dir>):
# after each game step (0x4d9780) every CAP_EVERY (default 200) frames, one line to
# $CAP_OUT/trace.txt with the local-player globals of a replay: 0x512684 / 0x512688 (local
# player / human), 0x512678 / 0x51267c (command player), 0x6d0f14 (replay), 0x59723d /
# 0x597248 (status unit), 0x5153f8 (resource panel's player), 0x57f0b4 (network game).
import gdb, os, struct

OUT = os.environ["CAP_OUT"]
END = int(os.environ.get("CAP_END", "2000"))
EVERY = int(os.environ.get("CAP_PROBE_EVERY", "200"))
inf = gdb.selected_inferior()
fo = open(os.path.join(OUT, "trace.txt"), "w")


def r32(a):
    return struct.unpack("<I", bytes(inf.read_memory(a, 4)))[0]


class Step(gdb.Breakpoint):
    def stop(self):
        f = r32(0x57f23c) - 1
        if 0 <= f <= END and f % EVERY == 0:
            g = {n: r32(a) for n, a in [("local", 0x512684), ("human", 0x512688), ("cmd", 0x512678), ("cmdh", 0x51267c), ("replay", 0x6d0f14), ("stat", 0x59723d), ("unit", 0x597248), ("respl", 0x5153f8), ("mp", 0x57f0b4)]}
            fo.write(f"f{f} " + " ".join(f"{k}={v:#x}" for k, v in g.items()) + "\n")
            fo.flush()
        return False


gdb.execute("set pagination off")
Step("*0x4d9780")
