# Status and hand-off (last updated 2026-10-15)

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

**Metric the user watches:** `re/coverage.py ... ` "reachable from both" (game_step +
command dispatch), now split into "(no AI)" and "(AI only)" (README has the command).
2026-10-15: no AI 57% (931/1615), AI only 40%, all 54%. The user said: skip AI for now (port the
non-AI frontier, `--missing both`); do not run screenshot tests until ~90% is ported.

**Replays vs the emulator (whole replay, trigger state and unit AI strengths traced):**
tl1412, tl1359, tl1363, tl1346, tl1347, tl1352, tl1366, tl1339 MATCH to the end;
tl1361 (Use Map Settings) MATCH. Not yet run: UMS replays tl1330, 1340, 1341, 1349,
1354, 1356. The emulator now runs 0x437a70(1) and 0x446350 at game start for replays
and the SCBW_AI scenario, like 0x4eee00.

**Triggers** (src/bw/triggers.rs): engine, every condition, nearly every action. Melee.trg
etc. from the game type; UMS sections. The emulator runs 0x48a460 too.

**AI** (paused by the user): ai.rs (requests heap, budget, player tick, town tick),
ai_town.rs (towns, worker / building rounds, requests, supply / gas, unit removal),
ai_place.rs (building spot search 0x4465c0), ai_areas.rs (resource areas 0x446350 /
0x445610), ai_region.rs (region state machine 0x43fc60, targets 0x431990, military
moves 0x43d5d0, region flood 0x437e70). Check: `SCBW_MELEE="1,1" SCBW_AI="1"
agent/tools/cmp.sh "$INSTALL/maps/(2)Boxer.scm" "" "" 8000` MATCHes. Left: guard /
region / defense requests (0x4630c0, 0x43f990, 0x43ebb0, 0x43aae0), attack timers
0x43b9e0, target choice 0x477160, military AI 0x43d910, script opcodes still todo.

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

**UI work (2026-10-15, user: "focus on UI and UX")**: the HUD is now drawn 8-bit in the
game palette (view.rs `draw_hud`), like 1.16.1. Ported: the text engine (ui.rs
`Font::draw_text` = 0x4202b0 / 0x4200d0 / 0x41f920 / 0x41fd70 / 0x41f7c0: glyph advance
w + xoff + 1, word wrap, alignment flags 0x6ce110, color codes 0x41f610); dialogs
(src/dlg.rs: .bin parsing, control draws by type 0x501504, wide buttons 0x4ef710, check /
option boxes, tick sliders 0x4153b0, tile.grp menu backgrounds 0x4c9cc0); the console's
MENU button (0x4f4fb0), minimap buttons (0x4a5230), command buttons (0x458900), status
icon buttons (0x456c30 / 0x456d30); the in-game menus (src/menu.rs: game menu, options,
speed, sound, video, help, end mission, restart / quit confirmations) with F10 / MENU,
Esc / Enter / hotkeys, mouse, sliders, Cancel restoring settings, pause while open; game
speed setting drives single-player frame time. Checks: `scbw repshot` of tl1412_full
frames (HUD text / buttons pixel-exact vs native), `SCBW_MENU=game|options|speed|sound|
video [SCBW_MENU_REPLAY=1] [SCBW_MENU_INPUT="k o;c x y;..."] scbw shot ...` vs native
captures from `re/native_menu.sh <rep> <dir>` (all five dialogs pixel-exact except the
4 transparent corners). Left: dialog translucency (0x416f30 blend on the key color),
save / load, keystroke help / tips, objectives text, scroll speed tables, gamma, sound
flags wired to voices, the native in-game (non-replay) HUD reference.

Background runs: always run replays / compares / screenshot tests in the background
with a copied binary (`SCBW=<copy>` for suite.sh / repcmp.sh) so rebuilding is safe.

## Next steps

1. The non-AI frontier (`--missing both`) is now mostly UI: selection circle / health
   bar images (own pools 0x57d768 / 0x5244b8 set up by 0x4d6930; 0x4d7070, 0x4d4fa0,
   0x4975d0, 0x497590, 0x4ebbd0 flash, 0x4e6180 / 0x4e6290 select / deselect), status
   panel refresh flags (0x4c36c0), placement box (placebox.cpp 0x48e310 ...), glue
   menus, replay UI (0x4deb90), chat (0x48d1c0, command 0x5c). Commands still on the
   `todo_port!` arm of 0x4865d0: 5, 6/7 save/load, 8 restart, 0x10/0x11 pause, 0x12
   cheats, 0x37 sync, 0x39 squelch, 0x55/0x56, 0x58 minimap ping, 0x5c chat.
2. Sim gaps left: Pickup4 0x4e7b70 and hidden order 0x60 (need AI target choice
   0x477160), AI orders (AtkMoveEP, HarassMove, GuardPost, ComputerReturn, AIPatrol).
3. AI (paused by the user): see the AI paragraph above.
4. Screenshot tests: not until ~90% of the sim functions are ported (user rule).
