# Status and hand-off (last updated 2026-10-14)

Read this first when picking the work up. It says what is in flight, what is uncommitted,
and what to do next. Background: [conventions.md](conventions.md) (user rules),
[tools.md](tools.md) (how to run everything), [testing.md](testing.md) (how ports are
verified), [re-notes.md](re-notes.md) (1.16.1 facts learned), [history.md](history.md).

## Goal

scbw is a faithful Rust reimplementation of StarCraft: Brood War **1.16.1**: port nearly
all of the 4,644 functions of the 1.16.1 `StarCraft.exe` (own networking instead of
Storm's, no map editor), matching the original frame for frame. Assets are read at
runtime from the user's 1.16.1 install (`~/Downloads/StarCraft-BroodWar_1.16.1`,
`SCBW_INSTALL`); none are in the repo.

## Where the work is (updated 2026-10-15)

**Replays vs the emulator (whole replay, trigger state and unit AI strengths traced):**
tl1412, tl1359, tl1363, tl1346, tl1347, tl1352, tl1366 MATCH; tl1361 (Use Map Settings,
map triggers) MATCH; tl1339 MATCH through f50700 after the transform repulse fix (the
rest was being re-diffed against `$W/tl1339/ra`). Not yet run: the other UMS replays
tl1330, 1340, 1341, 1349, 1354, 1356.

**Triggers** (src/bw/triggers.rs): engine, every condition, nearly every action (create
unit 0x4a0ad0, move unit incl. buildings, order, give, modify, kill/remove, alliance,
locations, doodads, waits, run AI script). Melee.trg etc. from the game type; UMS
sections MASK/UNIx/UPGx/TECx/PUNI/PUPx/PTEx/UPRP/MRGN/TRIG. The emulator runs 0x48a460
too (needs `$DATA/triggers/*.trg`).

**AI** (src/bw/ai.rs, ai_script.rs; memory map in re-notes.md): unit strengths
(0x431800 / 0x4313c0, traced as `str a,g`), player AI data, AI regions, pools, the
ai_update frame (0x4a2b00: scripts, region sweep + enemy strength 0x43a250, 0x4a2a40
timers, 0x43e7d0), script threads (start 0x45aef0, lookup 0x45b170, melee start
0x4ab230, RunAIScript actions) and the interpreter with its self-contained opcodes.
Next: towns (0x434220 start_town, 0x432020, 0x433dd0, 0x4626e0), AI regions state
machine (0x4390a0, 0x43a010, 0x43fc60), building/training requests (0x447230,
0x4474f0...), attacks (0x43abb0, 0x43e050), and an AI verification scenario (computer
player with its melee script in bwsim and the emulator).

**Late-game divergence vs the real game** (user report "desync around f17044, goliaths?"):
scbw == emulator with and without the p0 camera through f18000, but native screenshots
(both camera sets) differ from ~f17040 (unit positions; the factory at 2464,176 gets a
solid-black shadow natively). Native trace only covers f0-1500. Capture
`CAP_START=16700 CAP_END=17400 NATIVE_MODE=trace re/native_shots.sh` (CAP_START added)
and diff against the emulator's trace to find what the emulator harness does differently.

**Rendering fixed today**: fog shades sprites (0x4bd580 order), sprite draw-order heap
(0x42d4c0), carried resource LO offset (0x4d5a00). Gas plumes: creategasoverlays works
(seen at f2632); tl1412 p0's refinery is at y=64 so its plume leaves the map top.

**UI** (statdata.cpp panels ported): unit 0x426f50, group 0x425960, building queue /
research / upgrade, supply 0x427540, resources 0x426190, construction 0x4260c0,
transports 0x427c90, eggs 0x427260, carriers 0x4274a0; command and info tooltips;
unit voices; wireframe colors. Missing: replay progress panel 0x427a80, dark archon /
archon panel 0x4273e0, specials 0x425ee0, powerups 0x426ee0, status button tooltips,
shadows look off in screenshots (user report).

Background runs: always run replays / compares / screenshot tests in the background
with a copied binary (`SCBW=<copy>` for suite.sh / repcmp.sh) so rebuilding is safe.

## Next steps

1. Continue the tl1412 divergence chase to the end (player leave 0x48a1c0 near
   f24642). Grep `todo_port!` for the address the trace stops on.
2. Remaining todos seen on the way: lurker spines splash (0x48b680), AI paths, cloak,
   interceptor/scarab removal (0x4652a0), ghost nuke cleanup.
3. Then the other 14 replays in `~/games/Starcraft/broodwar-reps-116/` (PvZ, ZvT, 2v2 top
   vs bottom; `rep` shows game type: 2 melee, 10 top-vs-bottom). Non-melee game types
   currently get melee starting units (`view.rs::new_game`, marked ponytail): port the
   real setup if a replay needs it.
4. UI (earlier direction "focus on the UI"): status panel from `rez\statdata.bin`, the
   building panel 0x427890, info buttons and wireframes. Hallucination draw function
   (image modifier 0x10) is not drawn by the viewer yet.
5. Screenshot tests: once the sim matches, `re/repshots.sh` should drop toward 0%;
   remaining differences are rendering (fog edge smoothing, geyser smoke overlays, one
   missing doodad, palette cycling).
