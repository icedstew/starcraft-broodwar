# Status (updated 2026-10-17)

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
2026-10-17: no AI 1145/1615 (70%), AI only 229/358 (63%), all 1374/1973 (69%).
Front end AI only: 99/100 (1 deferred: save/load 0x4044c0).
Frontier lists: `--missing both` (non-AI), `--missing ai`, `--missing ai-front`.

## Current direction (user, 2026-10-17)

AI unpaused: port the AI ("reachable from front end (AI only)" done 2026-10-17).
No screenshot tests unless working on UI. Small units, status updated with each commit.
Delegate per agents.md: Haiku triage, Sonnet porting, Opus review / hard RE / commits.

## In flight

- Committed 2026-10-17 (suite, 6 replays to 20000, AI Boxer 8000 / 20000): last
  session's x9 work (AI spells 0x4a13c0, ai_script opcodes, path search 0x4e29b0, UI
  training panel / minimap terrain / button tooltips) plus the AI front-end cluster:
  target choice 0x477160 (attack.rs), reaction to attacks 0x43f320 (ai_military.rs),
  0x43c580 / 0x436e70 / 0x462ea0 (ai_region.rs), unit joins AI 0x4a2830 / 0x435770 /
  0x435db0 / 0x462960 (ai_town.rs). Fixed: interceptor parent is +0xc0 (union), not
  connected_unit +0x80, in 0x476fc0 / 0x43b790.
- AI todo_port! left (grep `todo_port!("0x4` in src/bw/ai*.rs attack.rs): 0x476fc0
  siege-tank part (0x442460, 0x476870), 0x4a2450, 0x440770, 0x463040, guard requests
  0x4630c0, region requests 0x43f990, defense 0x43ebb0 (needs 0x43e2e0), town request
  0x43aae0, attack timers 0x43b9e0, military step 0x43d910, ComputerReturn 0x478490.
  Next: `--missing ai` frontier (127 left), triage then clusters per agents.md.
- Front end (not started in code): src/glue.rs top loop 0x4e0820, glue loop 0x4de200,
  title 0x4d49a0, main menu 0x4db930 / 0x4db7a0, single player 0x4dc320; glue popups
  0x4b7180 / 0x4b59a0 / 0x4b5cc0 / 0x4b7a70. Save / load deferred (areas/saveload.md).
- Triaged non-AI frontier 2026-10-17: music stream 0x4a5e40..0x4a6250 and 0x4aad40 are
  Win32 / Miles (bucket them replaced; 0x4a5f50 fade-out has ~14 callers, audio.rs has
  no fade); 0x402140 / 0x48eae0 / 0x4c3db0 claimed equivalents, not yet verified;
  0x463a10 port-small (0x48e1e0 in death.rs may be missing its call).
- Don't commit README.md, .gitignore, web/*, cov.html (user's own work).

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
