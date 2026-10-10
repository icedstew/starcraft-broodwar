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
