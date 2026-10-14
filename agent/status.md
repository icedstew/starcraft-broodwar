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

## Where the work is

The current driver is **replay playback**: the 1.16.1 replay `tl1412 Python 1.1.rep`
(TvT, 24,769 frames) is played both by the real 1.16.1 game code under the emulator and by
scbw, and the per-frame traces are diffed (`agent/tools/repcmp.sh <rep> <frames>`). Each
first divergence names the next function to port. The same replay also has 206 native
screenshots for image-level comparison (`re/repshots.sh`, see testing.md).

Progress on tl1412 (trace diff, emulator vs scbw):

| Frames | Result |
|---|---|
| 0-3000 | MATCH (after porting map hallucinations, timer kill, hallucination death) |
| 0-5819 | MATCH (after porting AttackMove) |
| 5882 | first diff: Command Center gets HoldPosition (order 0x6b, command 0x2b) -> HoldPosition was a todo; now ported but **not yet verified** (see below) |

## Uncommitted work in the tree (all builds; verify, then commit)

`git status` shows these modified files. Everything except the last item was verified by
`repcmp.sh` before the next change was made; the 62-scenario regression suite
(`agent/tools/suite.sh`) was all MATCH after the hallucination/kill/death changes (before
AttackMove/HoldPosition; rerun it).

1. **Replay harness**
   - `src/main.rs`: `scbw repdump <rep> <out>` (header seed, player table, commands as
     text for the emulator), `scbw reptrace <rep> <frames> <trace>` (play the replay in
     scbw and write the same trace format as the emulator); `rep` prints seed, game type,
     per-player index/storm id/type/race and commands per player.
   - `src/replay.rs`: `Header.engine` (byte 0: 0 StarCraft, 1 Brood War).
   - `src/view.rs`: replays set `st.is_expansion = (engine == 1)`.
   - `re/emu_game.py`: `SCBW_REPLAY=<repdump file>` mode: players/races/50 minerals from
     the header table, Brood War flags 0x58f440 = 1 and 0x6d11e4 = 1 (else BW maps fail
     the VER check in 0x4cc060 and only 755 of 999 image GRPs load), replay flag
     0x6d0f14 = 1, a zeroed replay-writer struct at [0x596bbc] (0x4865d0 records every
     command through it), seed 0x51ca14 = header seed before the melee start 0x49da40,
     alliances 0x4c3ed0 after the player table, and each frame the commands of frame F
     run through 0x4865d0 (eax = bytes, stack = [len, 1]) with 0x512678/0x51267c = player
     before the counter advances (as 0x487100 does). Also writes `res` for 8 players.
2. **0x4f6420 make_hallucination** (`src/bw/chk_load.rs`, called for map-placed units
   with the hallucinated state bit; Python 1.1 has a hallucinated critter).
3. **Timer expiry kills** (`src/bw/frame.rs`: remove_timer reaching 0 -> `kill_unit`
   0x475710; `src/bw/bullet.rs`: self-destructing source -> `kill_unit`).
4. **Hallucination death in Die 0x479480** (`src/bw/death.rs`): sound 0x26b, thingy sprite
   0x176 one elevation up with the unit sprite's visibility, then 0x4e6340 (`hide_unit`)
   before `remove_unit` for all non-returning paths.
5. **AttackMove 0x479040** with 0x478370 (`attack_move_step`), 0x477820
   (`attack_move_acquire`) and their shared tail (`attack_move_switch`) in
   `src/bw/order_exec.rs`; `harvest.rs::queue_front` and `attack.rs::cloaked_from` made
   `pub(super)`.
6. **HoldPosition 0x478d10** (`order_exec.rs`), with new `attack.rs` functions
   `attack_in_place` (0x478b40), `find_target_in_range` (0x442fc0, random pick 0x4401b0
   with rng site 0x13), `order_target_visible` (0x4e5db0); `find_auto_target` (0x443080)
   was split so both share `target_buckets` (finder box + filter 0x442da0 + priority
   buckets 0x440160).

### Pending fix (the edit was interrupted, NOT applied)

In `attack.rs::attack_in_place`, the distance call must be
`self.unit_box_gap(d, t, u)` (0x430d30 takes ECX = the plain-box unit = target, EAX = the
turret-adjusted unit = `u`; see re-notes.md). The tree currently has
`self.unit_box_gap(d, u, t)`. Apply it, rebuild, then:

```sh
cd ~/src/decomp/scbw
~/.cargo/bin/cargo build --release
agent/tools/repcmp.sh ~/games/Starcraft/broodwar-reps-116/tl1412*.rep 8000
agent/tools/suite.sh | grep -v MATCH      # expect nothing; 62 lines MATCH
```

If both pass, commit (trailers in conventions.md), e.g. "Replay playback in the emulator
and scbw; hallucinations, AttackMove, HoldPosition". Then keep extending the frame count
(1000 frames of emulator take ~10 s; the whole replay is ~25k frames).

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
