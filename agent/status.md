# Status (updated 2026-10-16)

Read first. Kept short: what is in flight and what is next. Rewrite it in the same commit
as each step (conventions.md). Durable facts per area: [areas/sim.md](areas/sim.md),
[areas/ui.md](areas/ui.md), [areas/ai.md](areas/ai.md); read only the one you work in.
Rules: [conventions.md](conventions.md). Tools: [tools.md](tools.md). Old log:
[history.md](history.md) (don't read by default).

## Goal

Faithful Rust port of StarCraft: Brood War 1.16.1 (~all 4,644 functions, frame-exact; own
networking; no editor). Assets come from the user's install (`SCBW_INSTALL`).
Last step (after the port): optional StarCraft: Remastered assets read from the user's
SC:R install (CASC reader, HD `.anim` sprites / tiles / UI skin / audio, an RGBA
renderer at 2x/4x); the 8-bit 1.16.1 renderer stays the default and the test reference.

## Metric (the user watches this)

`python3 -I re/coverage.py agent/local/scratch/dump/bw116n.c .` "reachable from both":
2026-10-16: no AI 1021/1615 (63%), AI only 145/358 (40%), all 1166/1973 (59%).
Next targets: `--missing both` (frontier list, one line per function).

## Current direction (user, 2026-10-16)

Increase functions ported. AI stays paused. No screenshot tests unless working on UI.
Work in small units: one function or cluster per commit, status updated with it.
Delegate per agents.md: Haiku triage, Sonnet porting, Opus review / hard RE / commits.

## In flight

- Message / chat UI pass (user picked it, 2026-10-16). Done: message ring src/msgs.rs
  (0x48cd30 & co.), error line, trigger display text 0x4c5770, notices (left 0x4c4d60,
  eliminated 0x489e80) via State::notices + player_names, error helpers 0x48cf00 /
  0x48ef30 / 0x48f6c0 / energy 0x360+race, CTF 0x4e3de0 / 0x4e3e90 / 0x4e3f30.
  Left: chat entry / receive 0x48d1c0 (needs networking), names outside replays.
- Porting loop: triaged through 0x48d560 and the 0x49 / 0x4c-0x4f slices of
  `--missing both` through 0x4f5c50 (non-AI); easy equivalents extracted. What is left
  without AI: save/load (pointer <-> index fixups, section writers via 0x4c3450), UI
  overlay pools (selection circles / hp bars as image records, 0x4d4e80..0x4d7070),
  chat 0x48d1c0, placebox, dialogs, Win32 / Storm / CRT. Sim leads: pause / unpause
  0x488800 / 0x488790 (tick record, replay branch); 0x48a180 (player result slot vs
  leave_game 0x48a1c0); ported callers pass (0,0) where 0x4756e0 passes globals
  0x6d5c20 / 0x6d5c24 (identify them).

- UI / UX fixes from user play (2026-10-16): tooltips (0x481510 / 0x459030), portrait
  overlay conover.pcx, placement sound, fog sound gate, button errors, replay selection,
  1.16.1 key map (areas/ui.md "Keys"). Open question to the user: save/load as
  native-compatible .snx (all pools in 1.16.1 layout, AI data included) or scbw-only.

## Next steps

1. Triage the `--missing both` frontier (no AI) with the triage agent; port the easy
   ones, document existing equivalents (1.16.1 address on the Rust function that
   already does the same thing, verified).
2. UI leftovers in areas/ui.md "Left" (translucency, listbox, selection-circle images).
3. Run tl1341 / 1349 / 1354 / 1356 replays on the current binary (areas/sim.md).

## Always

- Background runs (suite, repcmp, native captures) with a copied binary
  (`SCBW=$W/x agent/tools/suite.sh`); build to `CARGO_TARGET_DIR=agent/local/tgt`.
- Before commit: build clean, suite has no non-MATCH lines.
