# Agent notes

Start here. Detailed hand-off notes live in `agent/` (committed docs and scripts;
`agent/local/` holds Blizzard-derived and large files and is gitignored):

| File | Contents |
|---|---|
| [agent/status.md](agent/status.md) | **Read first.** Short: goal, metric, in flight, next steps. |
| [agent/areas/](agent/areas) | Durable facts per area (`sim.md`, `ui.md`, `ai.md`); read only the one you work in. |
| [agent/agents.md](agent/agents.md) | Delegation to cheaper subagents (`.claude/agents/`: triage, porter, verifier). |
| [agent/conventions.md](agent/conventions.md) | User rules: faithful ports, what never to commit, commit trailers, untrusted files. |
| [agent/tools.md](agent/tools.md) | `agent/tools/*` (decompile/disassembly lookup, scenario and replay compares), `agent/local/` layout, scbw RE commands, emulator, headless native game. |
| [agent/testing.md](agent/testing.md) | How ports are verified: oracles, 62-scenario suite, replay trace compare, screenshot tests. |
| [agent/re-notes.md](agent/re-notes.md) | 1.16.1 facts learned (addresses, register conventions, replay/init flow, orders ported). |
| [agent/history.md](agent/history.md) | What was done and why, newest first (don't read by default). |

Quick start:

```sh
cd ~/src/decomp/scbw
~/.cargo/bin/cargo build --release
agent/tools/fns.sh 479040                 # 1.16.1 decompile of a function
agent/tools/dis.sh 479040 60              # its assembly
agent/tools/repcmp.sh ~/games/Starcraft/broodwar-reps-116/tl1412*.rep 3000   # native vs scbw replay
agent/tools/suite.sh | grep -v MATCH      # regression suite (expect no output)
```

## Test data (local, not in the repo)

- Game install (1.16.1): `~/Downloads/StarCraft-BroodWar_1.16.1/` (default `SCBW_INSTALL`;
  its `maps/` has the Blizzard melee maps).
- Replays: `~/games/Starcraft/broodwar-reps/` (9 PvZ games, recorded by SC:R:
  they use the extended commands 0x60–0x63).
- Their map: `~/games/Starcraft/broodwar-maps/Polypoid 1.65.scx`.
- 1.16.1 replays: `~/games/Starcraft/broodwar-reps-116/` (15 pro/iCCup games from
  Mar–May 2009, after 1.16.1's January 2009 release; from https://tl.net/replay/, ids in
  the file names). Legacy format (PKWare-imploded sections); each embeds its map's
  `scenario.chk`, so `view`/`shot`/`render` take the `.rep` directly as the map.

- More replays: `~/games/Starcraft/broodwar-reps-tl/` (661 from tl.net ids 1290–2200,
  2026-10-18; legacy format, all parse; recording version unchecked, so a desync against
  the original game is possible, but native shots of them are still 1.16.1's).
- Maps: `~/games/Starcraft/maps-from-reps/` (190 .scx / .scm written by `scbw extractmap
  <rep>... <dir>` from the replays' scenario.chk; the real 1.16.1 loads and plays them).

All 15 load and play to their recorded end in the real 1.16.1 (checked headless:
elapsed time at "The replay has finished" equals the header's frame count).

Running the real 1.16.1 headless (Arch: `xorg-server-xvfb`, wine): use a throwaway
`WINEPREFIX`, copy the install to `drive_c/StarCraft`, set
`HKLM\Software\Blizzard Entertainment\Starcraft` `InstallPath`/`Program` with
`wine regedit`, put replays in `maps/replays/`, then `Xvfb :78 -screen 0 640x480x24`
(the screen must be 640x480 or the game exits after the intro) and
`DISPLAY=:78 wine StarCraft.exe` from the game dir. Drive menus with `xdotool`
(mousemove + mousedown/mouseup) and capture with `import -window root`. Path: Single
Player (217,120), Expansion (367,275), new ID, Ok (515,392), Load Replay (333,452);
the replay speed-up button is at (613,457) (8 clicks = Fastest x16).

### Screenshot tests against the real game

`~/games/Starcraft/broodwar-reps-116/shots/<replay>/native/` holds 1.16.1's own game
screen (the 640x480 8-bit buffer, map area only; the HUD is drawn separately) every ~120
frames plus `meta.txt` (`frame screen_x screen_y`). Made by `re/native_shots.sh <rep>
<shots dir> [every]` (gdb through `winedbg --gdb`, breakpoint after each draw; exact
frames and pixels). So far: `tl1412` (Python, TvT, 206 shots, camera fixed at the
replay's start view).

`re/repshots.sh <rep> <shots dir>` (= `scbw repshot`) plays the replay in scbw, draws the
same frames and writes `ours/`, `diff/` (native | ours | red where different) and
`report.txt`. Compare rows 0..400.

```sh
cargo run --release -- rep ~/games/Starcraft/broodwar-reps/<file>.rep     # header and command summary
cargo run --release -- view ~/games/Starcraft/broodwar-reps/<file>.rep    # watch it
```

## Rules

- Never commit game assets, the 1.16.1 executable, decompiler output or dumps.
- Port 1.16.1 logic exactly, annotated with `/// 0x...` addresses; no simplified stand-ins.
- More in [agent/conventions.md](agent/conventions.md).
