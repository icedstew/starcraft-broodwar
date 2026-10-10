"""Differential test: run 1.16.1 map terrain + region creation in the emulator and
compare with `scbw mapdump` output.

    python -I re/emu_map.py <StarCraft.exe> <mapdump dir>
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from emu import Emu  # noqa: E402

W, H = 0x57f1d4, 0x57f1d6
MTXM, MEGATILES, TILE_FLAGS = 0x5993c4, 0x628494, 0x6d1260
VF4, VF4_COUNT, MEGA_FLAGS, CV5 = 0x5993d0, 0x5998e0, 0x5993cc, 0x6d5ec8
MINITILES, SAI = 0x6ca54c, 0x6d5bfc


def load(e, d):
    rd = lambda n: open(os.path.join(d, n), "rb").read()
    w, h = struct.unpack("<HH", rd("dims"))
    e.w16(W, w)
    e.w16(H, h)
    for g, n in ((MTXM, 0x20000), (MEGATILES, 0x20000), (0x628454, 0x49800), (TILE_FLAGS, 0x40000)):
        e.w32(g, e.alloc(n))
    e.w32(0x6d5cd8, e.alloc(0x7239))
    vf4, cv5 = rd("vf4"), rd("cv5")
    p = e.alloc(len(vf4))
    e.write(p, vf4)
    e.w32(VF4, p)
    e.w16(VF4_COUNT, len(vf4) >> 5)
    e.call(0x4bcf50)
    p = e.alloc(len(cv5))
    e.write(p, cv5)
    e.w32(CV5, p)
    mtxm = rd("mtxm")
    data = e.alloc(len(mtxm) + 4)
    e.write(data, mtxm)
    chunk = e.alloc(16)
    for i, v in enumerate((data + len(mtxm), data, 0, len(mtxm))):
        e.w32(chunk + 4 * i, v)
    return w, h, e.call(0x4cd0b0, stack=[chunk, min(len(mtxm), 0x20000), 0])


def check(name, want, got):
    if want == got:
        print(f"{name}: ok ({len(want)} bytes)")
        return True
    i = next(i for i in range(min(len(want), len(got))) if want[i] != got[i]) if len(want) == len(got) else -1
    print(f"{name}: MISMATCH at byte {i:#x} (len {len(want)} vs {len(got)})")
    if i >= 0:
        print("  exe :", want[i & ~15:(i & ~15) + 32].hex(" "))
        print("  rust:", got[i & ~15:(i & ~15) + 32].hex(" "))
    return False


def main():
    exe, d = sys.argv[1], sys.argv[2]
    e = Emu(exe)
    snap = {}
    e.at(0x483d2f, lambda e: snap.setdefault("minitiles", e.read(e.r32(MINITILES), 0x100000)))
    w, h, ok = load(e, d)
    print("MTXM handler returned", ok)
    rd = lambda n: open(os.path.join(d, n), "rb").read()
    check("tile_flags", e.read(e.r32(TILE_FLAGS), w * h * 4), rd("tile_flags"))
    check("megatiles", e.read(e.r32(MEGATILES), w * h * 2), rd("megatiles"))
    check("minitiles", snap["minitiles"], rd("minitiles"))


if __name__ == "__main__":
    main()
