# Status (updated 2026-10-16)

Read first. Kept short: what is in flight and what is next. Rewrite it in the same commit
as each step (conventions.md). Durable facts per area: [areas/sim.md](areas/sim.md),
[areas/ui.md](areas/ui.md), [areas/ai.md](areas/ai.md); read only the one you work in.
Rules: [conventions.md](conventions.md). Tools: [tools.md](tools.md). Old log:
[history.md](history.md) (don't read by default).

## Goal

Faithful Rust port of StarCraft: Brood War 1.16.1 (~all 4,644 functions, frame-exact; own
networking; no editor). Assets come from the user's install (`SCBW_INSTALL`).

## Metric (the user watches this)

`python3 -I re/coverage.py agent/local/scratch/dump/bw116n.c .` "reachable from both":
2026-10-16: no AI 948/1615 (58%), AI only 145/358 (40%), all 1093/1973 (55%).
Next targets: `--missing both` (frontier list, one line per function).

## Current direction (user, 2026-10-16)

Increase functions ported. AI stays paused. No screenshot tests unless working on UI.
Work in small units: one function or cluster per commit, status updated with it.
Delegate per agents.md: Haiku triage, Sonnet porting, Opus review / hard RE / commits.

## In flight

- Nothing uncommitted. Last commits: UI (text engine, dialogs, menus, portraits, group
  buttons), unit-list address docs, web build.

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
