# Status (updated 2026-10-27)

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
2026-10-23 (dump rebuilt on Windows, tools.md "Rebuilding the dump"; reachable sets equal
the old dump's): all functions 2758/4644 ported, in scope 2742/4494, `--missing rest` 771.
2026-10-24: all 2817/4644, in scope 2801/4494. Every frontier.tsv row now has a verdict
(the 278 "later" ones triaged 2026-10-24); left: 265 wait-net, ~30 partial (notes say what),
4 unsure. Ported 10-24: waves 1-2, replay vision dialog, registry stand-in
(src/registry.rs: settings / recent maps / game type; 1.16.1 never writes the options),
old-version map chunks UNIS / UPGS / TECS / UPGR / PTEC (VER 0x3b / 0x3f lists), dialog
fallback, screenshots (ScrollLock), unit speech mute triggers, old bullet records.
2026-10-23 triage of those 771: agent/frontier.tsv (addr, area, purpose, verdict, notes;
our notes only). Areas sim / hud / glue (362) triaged by Haiku, every equivalent /
not-needed / unsure checked by Sonnet. Work list: 5 fix (Rust differs: trigger give unit
0x4c8200 (skipped units count, Current Player = 13), trigger move building 0x4c8970 /
0x4c82d0 (unsigned edge clamp), command 0x08 0x4bfb10 stub (Restart), status name of
morphing units 0x425ec0 / 0x425ee0), 16 port (title screen 0x4d45a0..0x4d49a0, replay
vision dialog 0x4913d0 / 0x491310 / 0x490e90 / 0x490000, Esc skips trigger cinematics
0x489da0, Nuclear Silo panel 0x426ff0, ...), 29 partial (notes say what is missing; e.g.
game speed changes must send command 0x0f so replays record them, pause gates on
hotkeys / minimap / tooltips, quit from a multiplayer replay -> score), 5 persist (settings
and recent maps are not saved between runs: one feature, a settings file), 4 unsure (dialog
control draw details). Fixed: trigger set resources 0x4c70b0 matches any unit (0xe5).
User 2026-10-23: multiplayer functions (wait-net, 232) wait until we decide them with our
networking; other areas (later, 279) not triaged yet.
Wave 1 (2026-10-23): triggers give unit / move building / set resources, command 0x08 +
Restart, Esc skips cinematics, status name of morphing units, Nuclear Silo panel, title
screen (src/glue_title.rs). Wave 2 (2026-10-24, reviewed): game menu / end-mission / victory
dialogs by multiplayer / replay / pause state (menu.rs, `Ctx` filled from the scene), speed
changes go through command 0x0f (`State::game_speed` = 0x6cdfd4, the settings follow it),
pause gates on hotkeys / minimap / tooltips, Help / menus in replays, Alt+click ping 0x58,
paused click flashes 0x4d9460, portrait per unit + static 0x45ebc0 / 0x45e650, leaderboard
Ctrl+R 0x4bec00 (key from Local.dll's accelerators), score second shade 0x4dfa30, dialog
capture / delayed-event tables (dlg.rs `Captures`, no caller yet). Neither wave run in a
window yet. Replay vision dialog (2026-10-24): minimap button 4 -> allyfltr.bin
(`Kind::ReplayVision`, 0x4913d0 / 0x490e90 / 0x491310 / 0x490000, OK = 0x490fe0
`set_replay_vision`), button 3 / 4 tooltips 0x4a5110; `SCBW_MENU=vision SCBW_MENU_REPLAY=1
scbw shot <rep>`; not compared with native. frontier.tsv: 42 ported / 6 fixed; left 4
partial, 5 persist, 4 unsure. Suite runs on Windows now (data116 extracted, suite.sh uses $PY).
Coverage fix 2026-10-17: a `///` line that only mentions an address in prose no longer
counts; only a doc block's first line or a doc line starting with `/// 0x...` does
(~90 false credits removed).

## Current direction (user, 2026-10-17)

AI unpaused: port the AI ("reachable from front end (AI only)" done 2026-10-17).
No screenshot tests unless working on UI. Small units, status updated with each commit.
Delegate per agents.md: Haiku triage, Sonnet porting, Opus review / hard RE / commits.

## In flight

- **Enhanced games** (ours: join a running game, shared armies, AI takes armies left by
  everyone, fast drops): committed 2026-10-27 (suite, repcmp, netlive_leave, lobby, enhanced.sh pass), hand-off plan
  agent/plans/enhanced.md (read it first; open items listed there).

- Networking (user 2026-10-22: rollback netcode after ShieldBattery, N players, .exe
  hosts reachable without our servers, browsers join over WebTransport):
  agent/areas/net.md (design, phases, 1.16.1 turn loop facts). Phase 0 done: `State:
  Clone`, `State::state_hash`, `scbw rbtest <rep> [delay]` (late commands + rollback ==
  straight playback): all 90 1.16.1 replays MATCH at delay 3; wasm timing fine
  (snapshot 0.1 ms, 3-frame rollback 0.65 ms). Phase 1 done: UI commands join the local
  turn (`State::send_command` 0x485bd0), `Scene::turn_tick` dispatches them one frame
  later (measured in 1.16.1: sent at f, run at f + 1). Phase 2 done: `net::Session`
  (rollback engine) and `scbw nettest`: 20 runs over 10 replays (2-3 players, delay /
  latency / jitter 2/3/2 and 1/8/6) all MATCH straight playback. Phase 3a done (2026-10-24):
  src/net/transport.rs (wtransport 0.7.2 + tokio, localhost, cert pinned by hash) and
  `scbw netrep <rep> all [port] [delay]` (host + joins as processes): 3 replays x delay 2 / 3
  MATCH (~10 s each); 3+ player relay untested. Phase 3b done (2026-10-24): `scbw nethost
  <map> [port] [players] [delay]` / `netjoin <code>` play a live melee game over the
  Session (src/net/live.rs, view.rs NetDriver; menus do not pause net games);
  agent/tools/netlive.sh runs host + join headless (SCBW_NET_HEADLESS / SCBW_NET_SCRIPT):
  equal hashes at delay 1 / 2 / 4 with rollbacks. Not tried in a window yet. Phase 3d (2026-10-26): leaves / fence, desync hashes, confirmed-only recording: netlive_leave.sh a-f all PASS (header now taken at game start, 0x4dea10); left: UPnP. In-game chat (2026-10-27, net.md "In-game chat"): 0x4f32d0 recipient modes
  (Shift all / Ctrl allies), 0x4c2420 send through the host, 0x485d10 receive + 0x5c
  recorded into the replay (1.16.1 does), whisper / squelch slash commands 0x47f8f0;
  headless `say` scripts: hashes equal, both replays carry the lines, `repview` shows them.
  Left: message dialog 0x48ff10 (recipient choice, mode 4), labels not seen in a window.
  Lobby screens (gluConn / gluJoin / gluCreat speed, 2026-10-26/27) committed. Network lobby
  (2026-10-27, net.md "Lobby"): gluChat over Storm turns (snet.rs rounds), lobby commands
  0x486040, map transfer to joiners (in memory, browsers too), join by LOBBY code natively
  (SCBW_JOIN) and from the browser (the glue runs in the page from the lobby on);
  agent/tools/lobby.sh [web] equal hashes. Alliance dialog in games (0x490300 / 0x490570).
  Next: message dialog 0x48ff10, "[team]" chat names, 0x4a3010 team result, lobby slot
  details (0x450d90), copy-join-code UI, the 257 wait-net verdicts. Browser join done
  (2026-10-24): src/net/web_transport.rs (JS glue over WebTransport, cert pinned by hash;
  protocol core shared with native in transport.rs), web.rs `net_join`, join field in
  web/index.html (the user's file: not committed). Headless Chrome joined a headless
  `scbw nethost` (Bottleneck, 300 frames, no commands): equal hashes; with commands on both
  sides: equal hashes at delay 1 / 2 / 4 (2026-10-26, agent/tools/webjoin.sh). Determinism: wasm (node / V8) vs native rbtest hashes SAME on
  18 replays (a median and the longest per matchup, up to 218k frames). Netsuite: all 300
  bw116 replays x 3 tests = 900 MATCH.
  2026-10-23, moved to a Windows box (16 cores; no wine harness yet): ShieldBattery
  cloned for reference (`reference/shieldbattery`, fba29b3, next to the repo).
  New replay set (user: >= 200, >= 50 games per race, dated while 1.16.1 was live):
  `reference/replays/bw116/` (index bw116.tsv, list bw116.list), 300 1v1 games, 2009–2016
  by header start time (window 2009-01-21 .. 2017-04-18; 1.18 came out 2017-04-19):
  40 per matchup of 5–30 min + 10 per matchup over 30 min (up to 151 min; user: long
  games matter, desyncs grow with complexity), 140 per race. From the Webber pack and
  Synnaeve's TL/GG/ICCup pack (Wayback); tl.net now 403s scripts. `re/repfilter.py`
  (header filter), `re/netsuite.py` (rbtest 3 + nettest 2/3/2 + 1/8/6, whole replays,
  resumable). User: at most 4 scbw instances at once (14 at once gave bare exit-1
  failures; those runs MATCH alone). 2026-10-23: the 240 short ones all MATCH (720/720:
  rbtest 3, nettest 2/3/2, 1/8/6; ~4 h at 4 instances, binary from before wave 1). Next:
  resume pass for the 60 long ones (same command; it skips MATCH rows). Results:
  replays/netsuite.tsv.

- Screenshot tests on all 1.16.1 replays (user 2026-10-21: "match a PvZ 100%, with the
  minimap UI"): tl1311 (PvZ) rng + draw count match native every frame to f42049.
  Native shots for 71 more replays (`re/native_all.sh`, 4 at a time, ~3.5 h; tl2101 does
  not load in 1.16.1: post-1.18 recording). repshot over 72: map mean 0.29% on the 68
  normal ones, full screen ~2.2% (0 exact). Outliers were desyncs (tl1668 52%, tl1742 27%,
  tl2090 9.7%, tl2034 8.0%). Summary: agent/local/scratch/p1311/summary.txt.
  Desyncs vs native rng (2026-10-22), all four now match to the end: tl1668 PickupBunker
  0x4e73b0 (the target gets EnterTransport, not the bunker); tl1742 / tl2090 replay
  playback 0x4cdff0 / 0x487100 (one frame's blocks grouped into the 0x228-byte buffer
  0x6552b0, overflow writes the storm-id array 0x6554d8 so commands run as the wrong
  player; unmatched storm id keeps the previous player, group starts from 0x512684 = 0
  measured in tl1742; last group never runs); tl2034 0x49fed0 larva->egg clamp
  (0x401fa0 then 0x4eb9f0 if moved). Tools: agent/local/scratch/desync (check.sh, ucmp.py).
  Minimap border 0x4a3570 (narrow / short maps) ported: pixel-exact vs native.
  Left on the map view: native redraws dirty 16 px cells only (0x4bd580 / 0x4bcdc0 /
  0x497000): stale strips after camera jumps, mineral shadows darkened again next to
  mining drones; we redraw everything. Full screen: minimap box drawn where the camera
  already moved (capture timing?), a line above MENU, early map text / speed label.

- Save / load (user 2026-10-18: "do the deferred functions, including saving/loading"):
  done end to end (agent/areas/save.md): every part of 0x4d02d0 / 0x4cfef0 in
  src/bw/save/, file level src/saveload.rs, in-game save / load dialogs (menu.rs), glue
  load screen list, game start from a save. Checked by `scbw savetest` (testing.md 4b) on
  16 replays x 2 points (2026-10-19: all MATCH after savetest relinks the active bullet /
  thingy lists and shares the replay viewer), and headless: campaign mission saved (`SCBW_GLUE_GAME="300 save
  t1"`), listed and loaded from the glue load screen. Not yet checked against a save made
  by the real 1.16.1 (next: make one under wine, load it in scbw; and the reverse).
  Gaps listed in save.md (old formats, fields written as 0).
- Deferred functions (user 2026-10-19: "port other deferred functions" = the todo_port!
  sites). Done: implode 0x410b30 (byte-identical), replay recorder / writer
  (src/replay_rec.rs; `scbw reprecord <in> <out>` gives byte-identical files for all 15
  1.16.1 replays; LastReplay.rep written at game end), Save Replay dialog 0x4dffc0, glue
  error popups / stored errors, game mode popup 0x4db6e0, Tab focus, label font fit, profile
  save delete 0x4cf7b0, dialog timers 0x416090 (DlgTimers in dlg.rs; defeat 20 s), 0x48d0c0,
  0x4c3db0, mouse wheel, credits volume / music fade-in, trigger next scenario 0x4c50e0
  (-> GameEnd.next_scenario -> setup.next_map), AI opcode 0x3b, selection bar redraw.
  Left: networking (Storm turns, net status, slot lookup, 0x4d3910 network create), registry
  recent maps, StarEdit; crashes / heap corruption in 1.16.1 itself (0x4cb190 player >= 8,
  0x4f2a59, 0x46b6d8, AI wrong-pool kinds 0x434c90 / 0x404500 / 0x435700 / 0x435770 /
  0x45d890, trigger action ids >= 0x3c: documented at each site); in-game error dialog
  0x4e4a10 (dlgFatal.bin, modal: needs a menu.rs Kind); 0x4db280 spawn label. Done
  2026-10-19: 0x488790 timer in Scene, 0x4dbc00 `GameEnd.camp_cont`, 0x6d5a68 (= started
  from a save) `from_save`, chat cheats 0x4a62b0 radio / 0x4b1dc0 mission jump (not played
  end to end), 0x461e50 (not needed: event-driven menu); replay start from the create list runs `view::run_replay`
  (screen after it a guess: 0x4d9950 result); live replay header fields 0x4dea10 / 0x4de950
  are zeros. Untested by ear / eye: music fade-in, wheel direction, Delete button graphic.
- Campaign (user 2026-10-18: "play through the campaign"): works end to end headless
  (areas/ui.md "Campaign headless"): race click -> mission list 0x4b7820 -> mission text ->
  briefing (src/glue_ready.rs, MBRF triggers, portraits) -> Start -> game -> victory ->
  score -> next mission text / briefing; defeat restarts. Cinematics src/glue_cine.rs.
  Slide-in 0x4dd5e0 (glue_slide.rs), wav stop 0x4015f0, objectives 0x58d6c4 done; glue
  ticks in 20 ms passes (briefing takes ~30 s: scripts need `w 60000` before Start).
  Login slide-in 0x4ed810 done. Left: cinematic scaling unchecked vs native, first-start intro flags. Not yet checked in a real window by the user.
- Team games (user: "support team melee"): `State.multiplayer` (0x57f0b4) replaces the
  const; team start 0x49d8e0, CTF 0x4aba20 / 0x4ab980, 0x484ff0 leaders, init_gamedata
  wired. `SCBW_TEAMS=11` on `shot` / `view` forces one (runs, unverified). In 1.16.1 team
  behaviour needs a network game; no team replay exists locally: recording one natively
  (LAN, two clients in network namespaces: ~/games/Starcraft/broodwar-reps-team/
  tm1.rep, NOTES.txt); repcmp MATCH to the end (8375); repshot vs 34 native shots (shots/tm1) mean 0.08%,
  max 0.55% (fog edge at minerals, not team-specific). Shared control (2 humans in one
  team) is still unrecorded: needs 3+ clients.
  1.16.1 replay file names must be short ("filename is too long").

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

Rechecked 2026-10-24 on bw116 replays (`scbw repview <rep> <dir> <frames>`: full screen +
minimap PNGs): minimap colors / movement and replay chat match the decompile (not
reproduced); cloak distortion wrap 0x40b155 fixed. Open: who is the local player
(0x512684) in a native replay: `reset_slot_indices` sets 8, so native may show "Not enough
energy." and the green chat color for nobody, scbw uses the first human slot
(create.rs `local_error_line`): needs a native check.

- Replays: minimap player colors wrong (purple shown green), no white border on narrow
  maps, minimap unit movement looks broken; chat lines not shown (tl1330_p2 f23040);
  "Not enough energy." not shown (tl1341_p2 f22444); cloaked observer shimmer differs by
  pixels (tl1339 f23040). Porters started on minimap / messages / cloak (rate-limited;
  resume them, partial work was reverted where it was debug only).
- Play: fixed 2026-10-18: F10 / hotkeys / Enter dropped (input between 42 ms ticks),
  psi fields and geyser highlights while placing, command button pressed look and click
  sound (Misc\Button.wav), chat caret glyph. Left: status panel tooltips (0x457350,
  0x4575a0, 0x457b10, 0x457c20: none ported; check the training queue slots).
- Leave-game desyncs (tl1313, tl1309, tl1363, tl1488, tl1320) fixed by the 0x4cdff0 playback
  port (the leave ran as the wrong player): all match native rng to the end (2026-10-22).

## Next steps

0. Next: browser join (net.md phase 5); netsuite long replays (re/netsuite.py resumes; 62 of 180
   long runs MATCH so far, 0 non-MATCH); the 10-18 user reports below; the partial rows.
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

- Frontier porting (user 2026-10-19 "port more functions"): `--missing rest` (1279 left);
  triage in batches of ~24 top entries, port the real gaps. Done: trigger group loops
  0x45f930..0x45fdd0 (`sum_group`; EBX = count, resolved in areas/sim.md), info button
  tooltips 0x4576a0 (armor tip reads the wire type, native +0x64: differs for incomplete
  morphs), selection 0x496d30 / 0x46fa00 (Alt one-unit shortcut) / 0x468670 rally marker.
  Triaged as equivalent or not needed: 0x4017b0, 0x436a40/80, 0x4688e0, 0x4a3a00,
  0x4a8b90, 0x4dcb00, 0x42b8a0, 0x42d460, 0x4452f0, 0x447e20 (pre-v3 saves), 0x488900.
  Then: in-game error dialog 0x4e4a10 (`menu::Kind::Fatal`, `SCBW_MENU=fatal`; no native
  capture yet), scroll focus 0x4ef6d0, 0x4e1970 / 0x4e19f0 as `Ctl::press_follow` /
  `press_release` (not yet used by glue.rs hover_sound, menu.rs ~685, view.rs card_held),
  voice gate 0x4563a0 / 0x4556d0 / 0x455660 (voices.rs; 0x4564e0 `hover_cursor` ported
  but no mouse-move hook yet). Triage C: most of 0x4012c0..0x4566b0 equivalent.
  Then: leaderboard (src/bw/leaderboard.rs 0x4538b0 values / sort every 16 frames,
  src/leaderboard.rs draw statlb.bin; checked only by a forced shot, no native capture;
  countdown text 0x489c30 / 0x4e5600 not drawn; rows refresh per frame, native 1000 ms),
  placement / targeting prompt 0x4f31d0 (chat.rs, stat_txt 0x3e / 0x3f: confirmed fixed;
  rect / font unverified), notice 0x4be060 (game start / end only), 28 inline
  equivalents annotated after checking (2 panel fixes 0x425b50 / 0x4263e0), list / Vec
  replacements added to re/buckets.txt. In scope 2440 / 4494.
  2026-10-19 waves 3-4 (8 Haiku triage x 12 addresses, 12 Sonnet porters): palette color
  cycling (palette.rs Cycle: 0x4cb5b0 / 0x4d93f0 / 0x41e900; was missing entirely; per
  frame-loop pass with a 10 ms gate, native passes are more frequent: not checked by eye),
  AI town reset 0x432610, team slot rebuild 0x4a9250 (from a loaded save; `slot_order`
  0x66ff34 unfilled until the lobby exists), scenario load 0x4bf520 (`load_scenario`),
  start view slots 0x4bd3f0, volume 0x4bc320 (mute keeps the stream at 0), command senders
  0x4c0380 (right click 0x14) / 0x4c0400 / 0x4c07b0 (hotkey double tap), control dispatcher
  0x4e1a70 (`Ctl::default_event`, no caller yet), console test in cursor_kind 0x4d1460,
  cheat gating 0x4b1b50 (multiplayer, mission jump), trigger kill count 0 = all 0x4c7d80,
  template parser 0x4aae20 stops on bad quotes; ~45 inline equivalents annotated, render /
  network / registry ones bucketed. In scope 2543 / 4494.
  2026-10-19 waves 5-7: replay header copies (0x4dea10 slots / colors into LastReplay.rep;
  game-info part todo), /replay show|hide chat commands 0x4de6a0 / 0x4df3b0 / 0x4df360,
  game-start selection 0x4d0820 + 0x4eed10 copy (`start_selection`), game-start reset
  0x4eec30 (`reset_game_start`), glue start branches 0x4ee3d0 / 0x4ee520 (stub) / 0x4ee5b0
  (Zerg campaign headless flow unchanged; Terran with SCBW_GLUE_GAME exits without a PNG
  before and after: open), countdown timer drawn in the resource panel (id -10, frame-0
  icon: not checked vs native), light-up / option event dispatchers 0x4e1670 / 0x4e1bc0
  (no callers yet), cursor bounds 0x4d12a0; ~150 table-dispatched functions verified and
  annotated (button actions / conditions, trigger conditions, chk_load); dead functions
  (no exe reference, refs.py + whole-file dword scan) and Win32 dialogs bucketed. In scope
  2686+ / 4494. `scbw reprecord` differs on 14 of the 90 local 1.16.1 replays (output
  truncated; same before these changes).
  2026-10-19 wave 8: start resources 0x4aabf0 (template[9] = 1: template[0x17] minerals),
  trigger list reset 0x4893c0, statfluf dialogs 0x4f4dc0 (loaded, not drawn), map download
  protocol bucketed as network. Wave 9 (workflow): statdata / statcmd / statport / opt_dlgs
  / fd_user clusters: 0x45e320 campaign portrait swap (`portrait::unit_portrait`, wired in
  view.rs and glue_ready.rs; `State.mission` = 0x57f244 from GameSetup), slot / force
  helpers 0x45a8b0..0x45ade0 (saveload.rs), statcmd.rs tooltip cache 0x4597c0 / 0x459860,
  statdata.rs 0x457de0.. cache, 0x456e00, portrait.rs 0x45e4c0..0x45e710: all unwired
  (no caller yet). opt_dlgs: equivalents, gaps checked in wave 10.
  Open: 0x49cc40 (would fill `seat_races` 0x57f1c0 in slot order, clashes with the
  SCBW_TEAMS hack), 0x4b6810 (popup text set even when empty, proc 0x4b67f0), 0x48da30
  (placement surface from frame bytes +8/+9 vs grp size), 0x4b4a00 (0x4dd9e0 0xf4 call),
  0x4a2bf0 (0x59cc78 reader), 0x4913d0 / 0x48fb40 (find readers), 0x498c50 (needs
  0x497ce0), 0x4a5d10 (minimappreview.bin layout).
- Never run two suites at once: agent/tools/suite.sh / cmp.sh share output files in $W
  (concurrent runs gave false diffs 2026-10-19).
- Background runs (suite, repcmp, native captures) with a copied binary
  (`SCBW=$W/x agent/tools/suite.sh`); build to `CARGO_TARGET_DIR=agent/local/tgt`.
- Before commit: build clean, suite has no non-MATCH lines.
