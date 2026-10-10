"""Differential test of the path search against `scbw mapdump` output.

    python -I re/emu_path.py <StarCraft.exe> <mapdump dir>
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from emu import Emu  # noqa: E402
import emu_map  # noqa: E402

WAYPOINTS = 0x42efa0


def main():
    exe, d = sys.argv[1], sys.argv[2]
    e = Emu(exe)
    emu_map.load(e, d)
    rd = lambda n: open(os.path.join(d, n), "rb").read()
    tests = rd("path_tests")
    want_all = rd("path_regions")
    unit = e.alloc(0x150)
    ctx = e.alloc(0x16c)
    snap = {}

    def grab(e):
        snap["ctx"] = e.read(ctx, 0x16c)
        e.stop()
    e.at(WAYPOINTS, grab)
    bad = 0
    n = len(tests) // 8
    for i in range(n):
        sx, sy, tx, ty = struct.unpack_from("<hhhh", tests, i * 8)
        e.write(ctx, bytes(0x16c))
        e.w32(ctx, unit)
        e.write(ctx + 4, struct.pack("<hhhh", sx, sy, tx, ty))
        snap.clear()
        try:
            e.call(0x4e25f0, regs={"eax": ctx})
        except Exception as ex:  # later stages may touch state the harness lacks
            if "ctx" not in snap:
                print(f"test {i}: emu error {ex}")
                bad += 1
                continue
        got = snap.get("ctx") or e.read(ctx, 0x16c)
        got = bytearray(got)
        got[0:4] = bytes(4)
        want = want_all[i * 0x16c:(i + 1) * 0x16c]
        if bytes(got) != want:
            bad += 1
            j = next(j for j in range(0x16c) if got[j] != want[j])
            print(f"test {i} ({sx},{sy})->({tx},{ty}) differs at +{j:#x}")
            print("  exe :", bytes(got[j & ~15:(j & ~15) + 32]).hex(" "))
            print("  rust:", want[j & ~15:(j & ~15) + 32].hex(" "))
            if bad > 3:
                break
    print(f"{n - bad}/{n} region searches match")


if __name__ == "__main__":
    main()
