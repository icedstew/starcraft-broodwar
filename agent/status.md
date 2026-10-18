# Status (updated 2026-10-17, evening)

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
2026-10-17 (after the coverage fix below): no AI 1140/1615 (70%), AI only 350/358 (97%),
all 1490/1973 (75%); front end 683/1186. Frontier lists: `--missing both` (non-AI),
`--missing ai`, `--missing front`. The "left" column is what is still to port.
Coverage fix 2026-10-17: a `///` line that only mentions an address in prose no longer
counts; only a doc block's first line or a doc line starting with `/// 0x...` does
(~90 false credits removed).

## Current direction (user, 2026-10-17)

AI unpaused: port the AI ("reachable from front end (AI only)" done 2026-10-17).
No screenshot tests unless working on UI. Small units, status updated with each commit.
Delegate per agents.md: Haiku triage, Sonnet porting, Opus review / hard RE / commits.

## In flight

- 2026-10-17 session (subagent waves: Haiku triage, Sonnet porters per file, Opus
  review): AI "left" 126 -> 0 and front end 32 -> 0 under the old count. Ported: AI
  script opcodes (all of 0x45b850 but 0x3b = 0x4355c0), attack planning ai_attack.rs
  (0x43abb0, 0x43b0b0, 0x43b570, 0x43e050), region / guard / defense requests
  (ai_military.rs 0x43f990, 0x4630c0, 0x43ebb0), building spots 0x443ac0 / 0x4442e0 /
  0x4448d0 / 0x440420, building-round helpers (ai_town.rs: 0x4361a0 / 0x435f10 /
  0x4348c0 / 0x4a2450 callees, 0x445aa0, 0x43a8f0, 0x43aae0), military step 0x43d910,
  attack timers 0x43b9e0, medic 0x436ae0, stim 0x43ffd0, worker bonus 0x447a70,
  ComputerReturn 0x478490; glue: campaign select / start / lookup chain, load screen,
  ready rooms, login / create / score open-close (event procs todo), 0x6d1220 is
  network.tbl; unit names from the map's STR (0x47b090), "unit unplaceable" line
  0x48d140. Review fixes: 0x4a2450 arbiter returns 1; 0x4a2130 calls 0x432760.
- Coverage-fix list done (left: 0x4a6250 music end check only).
- 2026-10-17 late: user's crash (protoss vs 7 computers) fixed: three panics, all
  reproduced with `agent/tools/aistress.sh` (guard request value was an `ai_ref`;
  `u8_at(0x93)` outside the union; probe's empty queue slot 0xe4 indexing the cost
  tables). 20 x 30k and 10 x 60k frames now clean. Hot todo_port! sites the stress
  (debug build) showed are ported: 0x4548b0 / 0x4546a0 AI tail, 0x463900 medic,
  0x4f39a0 AI budget, 0x4e9f90 AI larva, 0x475b90 guard post, 0x45d890 drone,
  0x4e6790 addon, 0x463040 take unit; 0x4355c0 (opcode 0x3b / town workers). Second
  round: 0x4e78e0 SCV loaded, 0x477b0e tank turret sieges, 0x476fc0 / 0x43d5d0 siege
  tank parts (0x477160's stack arg 1 = `help`, set only from 0x43cd40 and 0x476fc0),
  0x4795d0 on a miss, 0x4a1e50 on owner change (unconditional), 0x4e7700 early unload,
  0x492850 spell cooldown, 0x4c779c guard post from triggers. Bugs the stress found:
  `town_add_unit` cleared only the low byte of +0xc8 (stale harvest target), more
  `u8_at` / `u32_at` outside the union, cloak detector bit as a u8 shift.
- Creep (user report 2026-10-17, "too small"): the sim was right; the shown terrain
  missed 0x49c780 (on-screen refresh each draw). Fixed (areas/ui.md); tl1354 frames
  10..130 vs native now match the creep extent. Left: creep tile variants differ from
  native (CRT rand seed for creep variations), top-left fog shading.
- path_req: `Path::region` past the 0x60 data bytes reads the next pool entry
  (`path_region`), found by aistress.
- AI todo_port! left: 0x434c90 / 0x404500 / 0x435700 / 0x435770 / 0x435db0 / 0x45d890
  / 0x4e78e0 non-matching AI kinds (1.16.1 reads the entry as the other struct; never
  hit in aistress), 0x45b850 unknown script opcode. UI 0x49eb70 ported 2026-10-18
  (`portrait_dropped`: a talking unit that dies hides the portrait until the talk ends).
- 2026-10-18: login screen 0x4edaa0 ported (profile list 0x4ed720 / 0x4ed640, Ok
  0x4ed3f0, New ID 0x4ed940 with the edit popup 0x4b6e50, Delete 0x4ed840 with the
  Ok/Cancel popup 0x4b73b0); list boxes (dlg/lbox.cpp) and edits (dlg/edit.cpp) in
  dlg.rs; glue keyboard (0x418eb0 / 0x418ab0: hotkeys, Enter / Esc, keys to lists and
  edits) and list mouse. Fixed: glue fonts are font10/14/16/16x (0x4daec0), image and
  popup backgrounds with flag 0x40000 blend only the shade color (0x416f30), force names
  and the profile name check read network.tbl (not gluAll). Checked against native
  (9 login steps, only the cursor differs): `SCBW_GLUE_INPUT` drives `scbw glue <png>`.
- Glue left: dialog event procs (ready rooms 0x4f7150 / 0x4f74b0 / 0x4f7810, create
  0x4af5c0, score 0x4b4a30), mission list 0x4b7820, 0x4a8590, error popup 0x4bb300,
  OK/Cancel 0x4b5b20 (taken as OK), the glue -> game start (glue.rs `Top::Game`), list
  scroll bars (0x414c90), combo boxes (type 13), Tab focus (0x418a60).
- Don't commit README.md, .gitignore, web/*, cov.html (user's own work).

## Next steps

1. Re-run `agent/tools/aistress.sh 25000 <seed> agent/local/tgt/debug/scbw` with new
   seeds / longer; fix what it finds. Then 0x4a6250 and the glue event procs.
2. Glue event procs: create game 0x4af5c0 next (map list = list box, combos), then the
   glue -> game start, score, ready rooms.
3. Unported non-AI frontier (`--missing both`), triage then port.
4. Run tl1341 / 1349 / 1354 / 1356 replays on the current binary (areas/sim.md).
5. Workflow: porters on disjoint files run in parallel fine (shared target dir; they
   retry when another's file breaks the build). Stage the verified tree (`git add`)
   before starting the next wave, commit the index after the verifier passes.

## Always

- Background runs (suite, repcmp, native captures) with a copied binary
  (`SCBW=$W/x agent/tools/suite.sh`); build to `CARGO_TARGET_DIR=agent/local/tgt`.
- Before commit: build clean, suite has no non-MATCH lines.
