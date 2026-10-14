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

Committed this session: replay playback in emulator + scbw (330f38a), research and
upgrades + fog of war (4a62573), right-click marker (027eb9f), status panel training
queue / research / upgrade (b69d438). Suite: 65/65 MATCH (agent/tools/suite.sh).

**tl1412 replay**: emulator and scbw agree frame for frame until a player leaves at
frame 24642 (0x48a1c0 "player left" not ported). BUT both disagree with the real game:
in the emulator/scbw the workers jam on one mineral and get stuck (order 0x5a
ReturnMinerals in movement state 0xc, "unstack" loop), so the economy stalls (player 0
has 0 gas all game), while the native screenshots show normal mining. So the emulator's
replay setup differs from the real game's. Suspects: a game-init step re_emu_game.py's
`game_init` skips (0x4ef100 calls after the map load: 0x4a9a30, 0x4de9a0, 0x49cc40,
0x4a9d80/0x4a9250, 0x4ee180, 0x4ed2b0 -- e.g. mineral resource areas), or unit order /
unique ids differing from the real game's so command handles hit other units. Get ground
truth: extend re/native_shots_gdb.py to dump the unit table (0x59cca8, 0x150 each) and
resources (0x57f0f0) at early frames from the real game and diff against `$W/ra`.

User requests still open: screenshot tests must become exact and follow a player's
camera (lower priority: "focus on decomp"); selection target flash (sprite +0xf timer
drawing) after right-click is set but not drawn yet.

## Next steps

1. Finish/verify HoldPosition, commit, continue the tl1412 divergence chase to the end.
2. Expected next todos on this replay (commands it uses, from `scbw rep`): siege/unsiege
   (0x25/0x26), lift (0x2f), research (0x30) and upgrades (0x32), cloak (0x21/0x22),
   unload (0x28/0x29), cancel train (0x20, ported), stop (0x1a). Grep `todo_port!` for the
   address the trace stops on.
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
