"""Regenerate data/names.txt from the Ghidra C dump.

Order handler names come from the `switch(order)` statements inside the known
dispatchers; fixed names are listed below.
usage: python3 -I gen_names.py all.c
"""
import re, sys

FIXED = {
    0x140305d10: "load_dat", 0x14039aa10: "sync_rand", 0x140190f38: "iscript_run_body",
    0x1401e7650: "update_unit", 0x1401e6f30: "update_unit_timers", 0x1401e5c60: "execute_main_order",
    0x1401a1d60: "update_unit_post", 0x1402791b0: "order_process_tick",
    0x1401c6280: "unit_type", 0x1401bfc40: "first_active_unit", 0x1401e7860: "update_hidden_unit",
    0x1401e3340: "update_units", 0x1401e6ab0: "set_move_target_pos", 0x1401e6b00: "set_move_target_unit",
    0x1401cc660: "is_at_move_target", 0x1401c9ce0: "order_done", 0x140189c10: "flingy_step",
    0x140189e50: "flingy_update_desired_direction", 0x14018a1b0: "flingy_update_speed",
    0x14018a060: "flingy_update_moving_flag", 0x140189f70: "flingy_next_position", 0x14018a0f0: "flingy_turn",
    0x1401888b0: "flingy_halt_distance", 0x140188680: "flingy_accelerate", 0x1401899c0: "flingy_set_speed",
    0x14021d820: "xy_length", 0x1401881b0: "atan_dir", 0x140187b80: "frame_reset_buffers",
    0x1401b3bd0: "order_Die", 0x1401cb270: "order_IncompleteWarping", 0x1401e01c0: "order_NukeTrack",
}
ORDERS = open(__file__.replace('gen_names.py', 'data/orders.txt')).read().split()
# Handlers shared by many orders get descriptive names.
OVERRIDE = {0x1401c4970: "order_MoveToTargetThenOrder", 0x1401d9d40: "order_CastSpell", 0x14014ed40: "order_AttackTileOrHover"}
DISPATCHERS = [0x1401e5c60, 0x1401e7650]
CASE = re.compile(r'((?:case (?:0x[0-9a-f]+|\d+):\s*)+)(?:return\s+)?(?:thunk_)?(?:FUN_|func_0x0*)([0-9a-f]+)\(')

src = open(sys.argv[1]).read()
funcs = {int(m.group(1), 16): m.end() for m in re.finditer(r'^// ==== ([0-9a-f]+) ', src, re.M)}
names = dict(FIXED)
by_addr = {}
for d in DISPATCHERS:
    start = funcs[d]
    end = src.find('\n// ==== ', start)
    body = src[start:end]
    for m in CASE.finditer(body):
        addr = int(m.group(2), 16)
        for c in re.findall(r'case (0x[0-9a-f]+|\d+):', m.group(1)):
            if int(c, 0) < len(ORDERS):
                by_addr.setdefault(addr, []).append(int(c, 0))
for addr, orders in by_addr.items():
    names.setdefault(addr, OVERRIDE.get(addr, f"order_{ORDERS[min(orders)]}"))
with open(__file__.replace('gen_names.py', 'data/names.txt'), 'w') as f:
    f.write("# address name  (applied to the Ghidra C dump by rename.py; regenerate with gen_names.py)\n")
    for a in sorted(names):
        f.write(f"{a:#x} {names[a]}\n")
print(len(names), "names")
