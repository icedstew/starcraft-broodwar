# History (newest first)

Short log of what was done and decided, so a new agent knows why things are the way they
are. Commit hashes refer to `git log`.

## 2026-10-13 / 14 — replays as the test driver (uncommitted, see status.md)

- User: "lets use these reps for screenshot testing ... then write a script to do the same
  with the new decomp" -> c052db9: native screenshot capture (re/native_shots.sh, gdb via
  winedbg) + `scbw repshot` / re/repshots.sh. 206 shots of tl1412 saved under
  `~/games/Starcraft/broodwar-reps-116/shots/tl1412/native/`.
- User: "continue the port" -> built the replay trace compare (emulator replay mode,
  `repdump`, `reptrace`, agent/tools/repcmp.sh), then ported along the first divergences:
  map hallucinations, timer kills, hallucination death, AttackMove, HoldPosition
  (HoldPosition unverified; a one-line fix is pending — status.md).
- User: "save everything we are doing to files in ./agent/* ... index in agents.md" ->
  this directory. The index is the existing `AGENTS.md` (loaded via CLAUDE.md).

## 2026-10-13 — 1.16.1 replays and headless real game

- 190703a / aaca775: downloaded 15 1.16.1 replays from tl.net (Mar–May 2009; 1.16.1 was
  released January 2009), added legacy replay reading, verified all 15 play to their end
  in the real 1.16.1 (Wine + Xvfb, xdotool).
- ac8b724: AGENTS.md with test data paths (user asked for it).

## 2026-10-12 / 13 — switch from SC:R data to the 1.16.1 install

- User: "would this install be better than sc:r? if so, don't worry about sc:r, and just
  use this." -> 3b2f1c7: CASC reader removed; MPQ storage, storm.dll Huffman/ADPCM,
  font decryption (grid.cpp), 1.16.1 file paths; emulator data from 1.16.1.
- Earlier the same days: cancels, hidden-unit Die, under-attack alerts (8d8725a), cursors,
  health bars, selection circles, errors, selection state and hotkeys, click/box select,
  minimap, placement box, PlayerGuard/acquisition fixes, zerg drone building, air
  repulsion; README coverage commands (user asked for full function counts and a `cd`).

## Before

Faithful engine in `src/bw/` ported function by function (pools, iscript, sprites,
pathing/SAI, movement, collision, combat, economy, Terran construction, commands,
requirements, command card, Protoss psi/warp-in, Zerg larvae/creep, ...), verified by the
emulator differential suite. See README "Faithful engine" and `re/NOTES.md`.
