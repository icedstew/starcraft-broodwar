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

**tl1412 replay** (`agent/tools/repcmp.sh <rep> <frames>`): the emulator equals the real
1.16.1 (native traces, `re/native_trace_gdb.py`), and scbw equals the emulator through
frame ~10379. Ported along the way this session: addons (addon.rs), transports / bunkers
and Unload (transport.rs), siege mode (siege.rs), turret orders (TurretGuard /
TurretAttack), StayInRange, WatchTarget, BunkerGuard, subunit death, damage overlays,
splash damage (0x48b570), Repair, Patrol, worker resource release on death, resource
holder removal (0x49f4a0), and the turret mode switch (0x49f6a0). Replay commands now
run with the synced RNG on (as the turn loop 0x487191 does).

Debug aids: `SCBW_WATCH=<unit>` (per-frame line + main image anim/pc on stderr),
`SCBW_FINDPOS=f,x,y` (unit indices at a position), `SCBW_RNGSITES_AT=f`, and
`SCBW_RNGLOG_AT=f` (every RNG draw on frame f; the emulator also prints the iscript unit,
image id, anim and pc for each).

User requests still open: screenshot tests must become exact and follow a player's
camera (lower priority: "focus on decomp"); selection target flash drawing.

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
