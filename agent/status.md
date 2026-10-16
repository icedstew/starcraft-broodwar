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
2026-10-16: no AI 959/1615 (59%), AI only 145/358 (40%), all 1104/1973 (55%).
Next targets: `--missing both` (frontier list, one line per function).

## Current direction (user, 2026-10-16)

Increase functions ported. AI stays paused. No screenshot tests unless working on UI.
Work in small units: one function or cluster per commit, status updated with it.
Delegate per agents.md: Haiku triage, Sonnet porting, Opus review / hard RE / commits.

## In flight

- Porting loop running: triaged 0x4010e0..0x4046d0 and 0x4258b0..0x437290. Skipped for
  now: save/load pointer fixups (0x402d10..0x4046d0, Rust uses indices; port with
  save/load), CRT 0x404e00..0x410000. Next porter batch (triaged): extract inline
  0x4281b0, 0x428240, 0x42d600, 0x42e0e0, 0x42d400 (view.rs heap), 0x42f8f0.

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
