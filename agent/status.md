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
