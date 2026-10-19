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
2026-10-18: all functions 2182/4644 ported; the reachable sets have 0 left, so the
frontier is now `--missing rest` (every unported in-scope function, ported callers first:
dialog procs, tables and other indirect calls the reachability misses).
Coverage fix 2026-10-17: a `///` line that only mentions an address in prose no longer
counts; only a doc block's first line or a doc line starting with `/// 0x...` does
(~90 false credits removed).

## Current direction (user, 2026-10-17)

AI unpaused: port the AI ("reachable from front end (AI only)" done 2026-10-17).
No screenshot tests unless working on UI. Small units, status updated with each commit.
Delegate per agents.md: Haiku triage, Sonnet porting, Opus review / hard RE / commits.

## In flight

- Campaign (user 2026-10-18: "play through the campaign"): race click -> mission list
  (0x4b7820 / 0x4b6530) -> 0x4dbfc0 / 0x4d3fc0 game creation -> mission text works
  (`SCBW_GLUE_INPUT` path in areas/ui.md). Next blocker: the ready room / briefing
  (porter running; runner RE in areas/ui.md "Mission briefing"). Then the cinematics
  0x4f7ae0 / 0x4d4470, the UMS player shuffle 0x4a9c90 / 0x4a95d0 (todo hit by every UMS
  new game), victory -> next mission check.
- Team games (user: "support team melee"): `State.multiplayer` (0x57f0b4) replaces the
  const; team start 0x49d8e0, CTF 0x4aba20 / 0x4ab980, 0x484ff0 leaders, init_gamedata
  wired. `SCBW_TEAMS=11` on `shot` / `view` forces one (runs, unverified). In 1.16.1 team
  behaviour needs a network game; no team replay exists locally: recording one natively
  (LAN, ~/games/Starcraft/broodwar-reps-team/).

- 2026-10-18 late (Haiku triage -> Sonnet porters -> Opus review): credits / mission text
  screen (src/glue_credits.rs, 0x4d8df0 cluster; checked by `scbw glue`, not vs native),
  minimap unit passes 0x4a4650 / 0x4a47b0 / 0x4a48e0 / 0x4a4ac0 and click clamp 0x4a3d70,
  team supply caps 0x488f90 / 0x4857b0 / 0x485710 (`init_gamedata`, no caller until
  multiplayer), end dialogs 0x461640 / 0x4615b0, countdown text 0x489c30 (no on-screen
  timer yet: 0x4e5600), score 0x4b4600 / 0x4b30a0. Open sim question: trigger kill loops
  0x45f9a0.. and EBX (areas/sim.md).

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
- 2026-10-18 (subagent wave: Haiku triage, Sonnet porters on dlg.rs / scrollbar.rs /
  glue_create.rs): create game screen 0x4af5c0 / 0x4af6d0 (src/glue_create.rs: map list
  with folders, info panel, game types from templates, slot owner / race combos, the Ok
  path to 0x4d3fc0 (todo)), combo boxes (type 13, dlg.rs), list scroll bars
  (src/scrollbar.rs, auto-repeat timer todo). 13 native create-screen shots match but the
  cursor (`d`/`m`/`u` hold verbs added to SCBW_GLUE_INPUT).
- 2026-10-18 screenshot tests: creep tile variants now match native (CRT rand seeded
  with the game seed at 0x4ef100, one draw in 0x4ee2d0). Mean differing pixels per
  replay (re/repshots.sh): 1361 0.2, 1339 0.6, 1346 0.7, 1340 0.8, 1349 0.9, 1356 0.9,
  1352 1.0, 1354 1.6, 1363 2.3, 1330 3.5, 1412 6.8, 1341 8.3, 1347 8.7 (%); tl1359
  captured, not compared yet. Left in 1330: a drone's mining frame at f120. All 751
  replays (broodwar-reps-116 + broodwar-reps-tl) play to the end in scbw after fixes for
  out-of-range reads native tolerates (neutral medic AI slot, UNIT type 0xffff, full
  hotkey group, empty egg queue).
- 2026-10-18 replay drift vs the real game: found with native rng traces (areas/sim.md):
  the 100-frame counter init in 0x4d9950 (scbw and the emulator harness both lacked it).
- Glue left: dialog event procs (ready rooms 0x4f7150 / 0x4f74b0 / 0x4f7810, score
  0x4b4a30), game creation 0x4d3fc0 -> game start, mission list 0x4b7820, 0x4a8590, error popup 0x4bb300,
  OK/Cancel 0x4b5b20 (taken as OK), the glue -> game start (glue.rs `Top::Game`), list
  scroll bars (0x414c90), combo boxes (type 13), Tab focus (0x418a60).
- Don't commit README.md, .gitignore, web/*, cov.html (user's own work).

## User reports to handle (2026-10-18)

- Replays: minimap player colors wrong (purple shown green), no white border on narrow
  maps, minimap unit movement looks broken; chat lines not shown (tl1330_p2 f23040);
  "Not enough energy." not shown (tl1341_p2 f22444); cloaked observer shimmer differs by
  pixels (tl1339 f23040). Porters started on minimap / messages / cloak (rate-limited;
  resume them, partial work was reverted where it was debug only).
- Play: fixed 2026-10-18: F10 / hotkeys / Enter dropped (input between 42 ms ticks),
  psi fields and geyser highlights while placing, command button pressed look and click
  sound (Misc\Button.wav), chat caret glyph. Left: status panel tooltips (0x457350,
  0x4575a0, 0x457b10, 0x457c20: none ported; check the training queue slots).
- Leave-game desync (replay command 0x57): tl1313 f62308, tl1309 f83856, tl1363 f32442,
  tl1488 f54254, tl1320 f20320 diverge; tl1427 / 1453 / 1460 leaves match.

## Next steps

1. Re-run `agent/tools/aistress.sh 25000 <seed> agent/local/tgt/debug/scbw` with new
   seeds / longer; fix what it finds. Then 0x4a6250 and the glue event procs.
2. Glue -> game start (0x4d3fc0 then state START into view.rs), score, ready rooms.
3. Screenshot tests on more replays (user, 2026-10-18): native shots for tl1339..1363
   (`NATIVE_DISPLAY` lets captures run in parallel), more tl.net replays downloaded to
   the scratchpad; repshot each, fix the worst first.
3. Unported non-AI frontier (`--missing both`), triage then port.
4. Run tl1341 / 1349 / 1354 / 1356 replays on the current binary (areas/sim.md).
5. Workflow: porters on disjoint files run in parallel fine (shared target dir; they
   retry when another's file breaks the build). Stage the verified tree (`git add`)
   before starting the next wave, commit the index after the verifier passes.

## Always

- Background runs (suite, repcmp, native captures) with a copied binary
  (`SCBW=$W/x agent/tools/suite.sh`); build to `CARGO_TARGET_DIR=agent/local/tgt`.
- Before commit: build clean, suite has no non-MATCH lines.
