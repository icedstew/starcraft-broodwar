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
2026-10-16: no AI 974/1615 (60%), AI only 145/358 (40%), all 1119/1973 (56%).
Next targets: `--missing both` (frontier list, one line per function).

## Current direction (user, 2026-10-16)

Increase functions ported. AI stays paused. No screenshot tests unless working on UI.
Work in small units: one function or cluster per commit, status updated with it.
Delegate per agents.md: Haiku triage, Sonnet porting, Opus review / hard RE / commits.

## In flight

- Porting loop running: triaged 0x4010e0..0x4046d0 and 0x4258b0..0x437290. Skipped for
  now: save/load pointer fixups (0x402d10..0x4046d0, Rust uses indices; port with
  save/load), CRT 0x404e00..0x410000. Triaged up to 0x47da80. Next porter batch: triage F extractions
  (0x463310, 0x463470, 0x4634e0, 0x465200, 0x465270, 0x466910, 0x466a30 (check order
  0x1e/0x21/0x22 branch: possible bug in spells.rs), 0x466b40, 0x466ee0, 0x4688b0,
  0x468a60), H (0x4756e0, 0x475ec0, 0x47b8f0/0x47b5f0), and G suspects: doodads.rs
  cites 0x474d10 (looks like 0x474d60), order_exec.rs 0x475350 branches mismatch,
  battery.rs 0x4749d0 always queues front (native: front only if queue non-empty).

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
