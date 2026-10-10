# RE notes — StarCraft: Remastered 1.23.10.13515 (x64)

On-disk `.text` is encrypted (entropy 8.0); a TLS callback decrypts at startup.
We analyze a runtime dump:

1. Launch SC:R (Battle.net is fine).
2. `sudo sysctl kernel.yama.ptrace_scope=0` (restore to 1 afterward).
3. `python3 -I re/dump.py <x86_64/StarCraft.exe> sc_x64.exe <pid> 0x140000000`

The dump is a PE with raw layout == virtual layout and ImageBase 0x140000000.
`.pdata` gives exact function bounds (38,378 functions). The dump is never committed.

Tools: `re/xref.py` (RIP-relative xrefs, numpy), `re/datlayout.py` (dat layouts).

## Addresses

| Address | What |
|---|---|
| 0x140305d10 | `load_dat(const char* name, DatDesc* table)`; DatDesc = {u64 dest; i32 size; i32 count; u8 flags(bit0: ptr widened 4->8)}, 24 bytes, ends at dest==0 |
| 0x1401c01e0 | init units/orders dat, clears unit array (stride 0x1e8 = x64 CUnit size) |
| 0x140190440 | references `scripts\iscript.bin` |
| 0x140188db0 | loads flingy.dat |
| 0x14035c630 | loads upgrades/weapons/techdata/portdata |
| 0x140da9820 | units.dat descriptor table (field dest ptrs = runtime column arrays) |
| 0x140da9000 | flingy.dat table |
| 0x140da9100 | images.dat table |
| 0x140da95d0 | sprites.dat table |
| 0x140da92a0 | orders.dat table |
| 0x140daa0a0 | weapons.dat table |
| 0x140da9f00 | upgrades.dat table |
| 0x140da9700 | techdata.dat table |
| 0x140190f38 | iscript interpreter body; opcode `cmp eax,0x44` (69 ops); jump table 0x1401922c0 |
| 0x140e61c30 | iscript.bin buffer pointer |
| 0x14039aa10 | `rand(call_site_id)`: returns 0 if rng disabled (0x1410936ac); per-id counters at 0x1410936b0; seed 0x1410936a0; `seed=seed*0x15A4E35+1; (seed>>16)&0x7FFF` |

Notes:
- Game-state globals are often pointer-obfuscated: `ptr = [global] ^ 0x3703db00831bdfe2`.
- Junk-byte opaque predicates (`and dh,0xff; jno ...`) also appear inside decrypted functions.
| 0x1401e7650 | `update_unit(u)`: disable checks, sprite flash timer, timers (0x1401e6f30), main order (0x1401e5c60), secondary order switch, subunit recursion (u+0xa0), tail 0x1401a1d60 |
| 0x1401e5c60 | `execute_main_order`: every-frame switch (order-4, 180 cases) then every-9th-frame switch (order-1, 187 cases) gated by u+0xc9 timer; see re/data/*_orders.txt |
| 0x140dd9e80 | button sets: 250 x {u64 count, Button* buttons, u64 linked(0xffff)}; Button = 32 B {u16 pos, u16 icon, pad, cond*, act*, u16 cond_var, u16 act_var, u16 enabled_str, u16 disabled_str}; re/buttons.py |
| 0x140ad4528 | remap table names (images.dat remapping 1..7): ofire gfire bfire bexpl trans50 red green |

CUnit (x64) fields seen: +0x18 sprite*, +0x68 player, +0x69 order, +0x6a order_state, +0xa0 subunit*, +0xc9 order_process_timer, +0xd6 ?, +0xea secondary_order.

## Deobfuscation

`re/deobf.py` rewrites the obfuscator's opaque predicates (flag-tracked: logic ops,
stc/clc, shifts by 0, complementary jcc pairs) into jmp/NOPs, exactly, only in code
reachable from .pdata starts. 12k predicates patched. Ghidra on the cleaned dump
decompiles functions that previously failed. Remaining oddities in some functions are
anti-tamper timing checks reading KUSER_SHARED_DATA (0x7ffe0000) — not game logic.

## CUnit layout (x64, SC:R 1.23.10)

BWAPI's x86 BW::CUnit layout, with pointers widened to 8 and 8-byte alignment, plus
SC:R insertions (4 bytes after velocityDirection2 at 0x64; 0x10 bytes before isBeingHealed).

| x64 | x86 | field |
|---|---|---|
| 0x00/0x08 | 0x00/0x04 | prev/next (active unit list) |
| 0x10 | 0x08 | hitPoints (24.8) |
| 0x18 | 0x0C | sprite* |
| 0x20/0x28 | 0x10/0x14 | moveTarget pos / unit* |
| 0x30, 0x34 | 0x18, 0x1C | nextMovementWaypoint, nextTargetWaypoint |
| 0x38..0x3f | 0x20..0x27 | movementFlags, heading, turnRate, velocityDir1, flingyID(u16), ?, movementType |
| 0x40, 0x44, 0x48 | 0x28, 0x2C, 0x30 | position, exact x, exact y |
| 0x4c..0x63 | 0x34..0x4B | topSpeed, speed, nextSpeed, velocity x/y, acceleration, velocityDir2, desiredDir |
| 0x68 | 0x4C | playerID |
| 0x69, 0x6a, 0x6b | 0x4D..0x4F | orderID, orderState, orderSignal |
| 0x6c | 0x50 | orderUnitType |
| 0x70..0x73 | 0x54..0x57 | mainOrderTimer, groundCooldown, airCooldown, spellCooldown |
| 0x78, 0x80 | 0x58, 0x5C | orderTarget pos, unit* |
| 0x88 | 0x60 | shieldPoints (24.8) |
| 0x8c | 0x64 | unitType |
| 0x90, 0x98 | 0x68, 0x6C | prev/next player unit |
| 0xa0 | 0x70 | subUnit* |
| 0xa8, 0xb0 | 0x74, 0x78 | orderQueue head/tail |
| 0xb8, 0xc0 | 0x7C, 0x80 | autoTargetUnit*, connectedUnit* |
| 0xc8, 0xc9 | 0x84, 0x85 | orderQueueCount, orderProcessTimer |
| 0xd6, 0xd7 | 0x92, 0x93 | AIActionFlag, userActionFlags |
| 0xdc..0xe5 | 0x98 | buildQueue[5] |
| 0xe6 | 0xA2 | energy |
| 0xe8 | 0xA4 | buildQueueSlot |
| 0xea | 0xA6 | secondaryOrderID |
| 0xf0 | 0xAC | remainingBuildTime |
| 0x110..0x138 | worker union | target resource pos/unit, isCarrying(0x122), harvestTarget(0x128), prev/next harvester(0x130/0x138) |
| 0x140 | 0xDC | statusFlags |
| 0x147 | 0xE3 | recentOrderTimer |
| 0x193 | 0x107 | isBeingHealed |
| 0x19c | 0x110 | removeTimer |
| 0x1b2 | | 8-frame status timer counter |
| 0x1c8 | 0x134 | AI* |

## Classic 1.16.1 reference binary

SC:R protects core game routines (unit loop, bullets, speed, sprite init) with
anti-cheat code (rdtsc checks, encrypted state); they decompile as junk. The classic
1.16.1 `StarCraft.exe` has the same game logic, unprotected. It ships inside the
Blizzard patch installer (an MPQ appended to the exe): `scbw mpqx SC-1161.exe StarCraft.exe out`,
then strip the 24-byte patch header (`tail -c +25`). Ghidra decompiles all 4644 functions.

| 1.16.1 | SC:R x64 | what |
|---|---|---|
| 0x4dc4a0 | 0x14039aa10 | sync rand (site id in EAX); 0x4dc550 rand_between |
| 0x6d11c8 | 0x1410936ac | RNG enabled flag; seed 0x51ca14 |
| 0x4ed000 | 0x1401e3340 | update_units |
| 0x4ecf70 | 0x1401e7650 | update_unit |
| 0x4ec290 | 0x1401e6f30 | update_unit_timers |
| 0x4ec4d0 | 0x1401e5c60 | execute_main_order |
| 0x4ec170 | (inline) | secondary orders |
| 0x492eb0 / 0x493790 | | bullets / thingies update (called from update_units) |
