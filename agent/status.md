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

## Where the work is (updated 2026-10-14)

**tl1412 replay**: scbw equals the emulator (which equals the real 1.16.1) for the whole
game (24641 frames; the diff starts only where the native trace ends). Also with a
follow camera (`SCBW_CAMERA`, see below).

**Replay camera**: 1.16.1 replays keep no camera; `view::Follow` derives one from a
player's commands (latest commanded point or the middle of a new selection).
`scbw view <rep>` plays a replay following a player (Tab / F). `scbw repcam <rep> <out>`
writes the per-frame screen positions (as 0x49c440 sets them; the screen tile feeds
creep updates, so the sim depends on it). `SCBW_CAMERA` feeds them to reptrace /
repshot / the emulator, `NATIVE_CAMERA` to re/native_shots.sh (gdb calls the game's
0x49c440 after each draw / step). tl1412_p0 shots: mean 8% differing; late frames
differ a lot: check against the native trace with camera (NATIVE_MODE=trace, running
into shots/tl1412_p0/native/trace.txt) before blaming rendering.

**tl1359 (ZvT), tl1363 (PvT), tl1346 (PvZ)** (2026-10-14): scbw == emulator for the whole
replay (36567 / 32459 / 58129 frames). Done by porting medics,
the spell system (shared cast order 0x492850, castspell, every spell explosion type but
yamato-style is done; defensive matrix; status timers/overlays), glave wurm bounces,
lurker spines (0x48b680, grdsprol), burrowed cloak, AttackFixedRange, bunker muzzle
flash / target stand-in, valkyrie spread, Move-to-unit. Full emulator trace in
`$W/e59/ra` (re-diff: `scbw reptrace <rep> 37000`, blank unit ids, diff).

**Also ported (not yet exercised by a replay)**: carriers / reavers / interceptors /
scarabs (src/bw/carrier.rs), shield batteries and arbiters (battery.rs), spider mines
(mines.rs), disruption web marking, AI alert searches (AI reaction itself todo),
hallucination hits and the Hallucination2 secondary. Still todo: nukes (0x479200 family),
CastHallucination (0x4f6c40), CastRecall (0x494470), CastMindControl (0x4f6950),
CastFeedback (0x4f6d40), AI (ai_update 0x4a2b00 and everything AI-gated).

**Creep**: the view drew terrain once from the CHK, so creep never showed. It now draws
from the live megatile map (0x628494) each frame, with the creep edge map (0x6d0e80)
and the tileset's edge GRP (0x414100 / 0x49b9f0) ported.

**Other 1.16.1 replays** (2026-10-14, 6000 frames each, `OUT=<dir>` lets repcmp.sh run
several at once): tl1363, tl1359, tl1346 MATCH the emulator. tl1339 / tl1347 / tl1352: the
emulator crashes at eip 0x41f82f (fix the harness first); tl1366: emulator CHK load fails.
Game type 10 in headers is **Use Map Settings** (not top-vs-bottom; that is 0x0f). UMS
starts need the template (0x596865, 0x20 bytes; +0xc starting units) and 0x59bda8 (replay
header +0x271) in 0x49da40 / the UNIT loader (`ScenarioModes`, `melee_map_units` exist
but callers pass defaults); where replays restore the template is not found yet, and the
emulator hardcodes the melee setup too.

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
