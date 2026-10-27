# Enhanced games: hand-off plan

Status 2026-10-27. Written for a model taking this over cold. Read this whole file, then
`agent/areas/net.md` sections "Lobby (gluChat) and map transfer" and "Team games / shared
control", then the files listed under "Where the code is". Do not re-derive the design: it
is settled and tested; the open items are at the end.

## What the user asked for (verbatim intent)

1. "Add an enhanced option when creating a game."
2. "Allow players to join an ongoing game, and if another player is in that army slot, join
   that army as team melee" (shared control).
3. "If players leave, continue the game, and if no players are controlling an army, give it
   to AI."
4. Follow-up: "In the enhanced option, if a player drops don't have a long timer to wait for
   them, and they should have a chat message when they drop."

This is **our own feature**, not 1.16.1 behavior. The repo rule "port 1.16.1 exactly" does
not apply to it, but keep the code style: module docs saying it is ours, `///` docs, no
fake `0x...` addresses on our functions (cite the 1.16.1 functions we imitate, e.g.
0x4ab230 for the AI start).

## State of the work

**All of it is uncommitted** in the working tree (18 modified files + `src/bw/enhanced.rs`
+ `agent/tools/enhanced.sh`). It builds natively and for wasm with no warnings
(`cargo build --release`, `cargo build --release --target wasm32-unknown-unknown`).

Verified with `agent/tools/enhanced.sh` (headless, real network over localhost):

| Run | Result |
|---|---|
| host + lobby joiner start; third player joins by code mid-game, its install has no maps | all 3 equal hash at frame 60000 / 80000 |
| `QUIT_AT=15000`: 2nd player quits, its army goes to the AI; 3rd joins, takes it back (human id reused) | host and joiner equal at 40000 |
| `QUIT_AT=15000 ARMY=<host's army>`: 3rd joins the host's army as a second human (id 8) | equal at 60000 |

Not verified: the regression suite / net tests after these changes (run them, see
"Before committing"), the browser late join, the LAN-list late join, anything in a window,
whether the AI actually plays well after taking over (only that its script starts).

## Design (settled)

### Sim side (`src/bw/enhanced.rs`, deterministic, runs on every peer)

- `State.enhanced: bool` (set from `GameSetup.enhanced` in `view::new_game`), `State.ai_taken:
  u8` (bit per slot the AI took from humans).
- **Join command** `CMD_JOIN = 0xe0`, fixed length `JOIN_LEN = 4 + 0x18`:
  `[0xe0, human h, army n, link id p, name 0x18 bytes NUL padded]`. Built by
  `join_command(h, n, p, name)`. Length is special-cased in `replay::split_commands`
  (0xe0 is outside 1.16.1's table). Dispatched in `command.rs receive_command` to
  `State::enhanced_join`: human `h` controls army `n` (grows `human_nation` /
  selections / hotkeys via `set_humans`), sets its name (slot name below 8, else
  `human_names[h-8]`), clears its `humans_gone` bit, releases an AI-held army
  (`ai_release`: type back to 2, the player's AI scripts get flag 4 = killed), and pushes
  notice 0x49 ("%s has joined the game.", network.tbl).
- **Leave**: `command.rs leave_notice` — when no other human controls the army and the game
  is enhanced, `ai_take_over(n)` instead of `leave_game` / `player_dropped`: type 1, bit in
  `ai_taken`, AI flag 0x20 cleared, `start_melee_ai_for(p)` (split out of `ai.rs
  start_melee_ai`, 0x4ab230: the race's melee script at the start location). The leave /
  drop notice (0x4a / 0x4b) still shows: that is the user's "chat message when they drop".
- **Army choice** `State::enhanced_pick(want)` (host only, on its current scene):
  `want` if valid; else first AI-taken army; else the human army with fewest humans. Human
  id = the army's own slot id when that human is gone / the army is AI's; else first free id
  >= 8 (up to 31).
- `Extras` (in enhanced.rs): what a 1.16.1 save does not hold — human_nation, humans_gone,
  ai_taken, human_names, the 8 slot names; `of`, `apply` (also sets multiplayer/enhanced),
  `encode` / `decode`.

### Why every peer reloads at the join frame (important, non-obvious)

A joiner gets the game as a **1.16.1 saved game** of the join frame's start state. Saving
mutates a game (0x4e3410 / 0x4e2e20 re-link units in the finder) and loading rebuilds pools,
free lists, unit handles, psi sprites, etc. (see `main.rs savetest` comments). So a loaded
game is NOT equal to the game it was saved from. Fix: at the start of the join frame
**every peer** runs `Scene::reload_through_save()` (view.rs): save itself, load that save
(`Scene::from_save_steps(.., false)`), `restore_sim` the loaded sim state, re-apply
`Extras` and `local_human`. The joiner, whose state *is* that load, skips it
(`Session.start == f`). Result: identical states everywhere (verified by hashes). A
rollback across the join frame redoes it from the ring snapshot: deterministic.

### Net side

Message flow (host = star center; transport ids = Storm ids; lobby link reused for the game):

1. Joiner connects to the host's lobby endpoint (still accepting after the start) by
   code (`SCBW_JOIN`, browser `?join=`), or the LAN list (see open items). It gets a Start
   with an empty setup, then `snet::adopt` sends snet NAME (kind 0x83): name, then
   optionally `NUL + army` (`snet::want_army`, from `SCBW_ARMY` / `?army=`).
2. Host `NetGame` (live.rs): transport `Event::Joined{p}` -> `tr.hold_peer(p)` (no turns
   relayed yet). `ctl` 0x83 from a link that is not a live session player, enhanced game ->
   `join_queue`.
3. `NetGame::enhanced_host` (each frame, host, enhanced): one join at a time: pick (h, n),
   `scene.st.send_command(join_command(...))`. When the local turn carrying it is made,
   its frame `F` is recorded and `se.keep = Some((F, None))` (Session keeps the start
   state of F: `keep_snap` in `tick_inner`, also on rollback re-sims).
4. When `se.kept(F)` is final (confirmed >= F, no rollback below F pending): `late_bytes`
   = `F u32, p, h, n, delay u32, nplayers, (link, storm, human)*, Extras, zlib(save_file of
   the kept state)`; sent as C_LATE (=8, `snet::LATE`) control messages of 500 bytes
   (`LATE_PART`, under the transport's 600-byte control budget per datagram);
   `tr.start_peer(p, F)` (relay every player's turns from F); `joined_at` grace starts.
5. All peers' `Session::run_frame(F)`: sees CMD_JOIN in a turn -> `reload_through_save`
   (except a joiner at its start frame), runs commands, then `add_player(p, storm=p, h,
   from = F + JOIN_LAG)` (`JOIN_LAG = 2*PRED_LIMIT`: no peer can have confirmed that frame
   when it learns of the join) and `added.push((p, F))`. NetGame then calls
   `tr.player_from(p, F)` so the link counts p's turns from F (without this the ack
   counters sat at frame 2 and the same 64 turns were resent forever — a real bug found
   in testing). Turns of a player not yet added are buffered in `Session.orphans`.
6. Joiner: snet assembles C_LATE parts (`snet::late()`); `live::late_game(a, bytes)`:
   decode, unzip, `Scene::from_save_steps`, `Extras::apply`, local player n / human h,
   `snet::take_transport`, `tr.set_first(F)`, `Session::new_at(p, players, delay, F)`
   (frames < F known empty), `map_humans`, `add_player(p, p, h, F+JOIN_LAG)`,
   `NetGame::new` (next_send = frame + delay), recording off (`se.record = false`), and
   **empty turns for F..F+delay** so the host's counters for p start at F.
7. Fast drops (user's follow-up): `NetGame.drops_fast = scene.st.enhanced`; `waiting()`
   returns nothing (no waiting dialog); host drops a player holding the game for
   `ENH_DROP` = 3 s (`decide_leave(p, 6)`), except joiners within `JOIN_GRACE` = 60 s of
   being sent their start (they download ~180 KB and load ~2.5 s).

### UI / setup plumbing

- `GameSetup.enhanced` (view.rs); `encode_setup` bit 3 of the flags byte; `nethost` /
  `melee_setup` read `SCBW_NET_ENHANCED` (no late join through nethost: its transport has a
  fixed player list, only the glue lobby accepts late links).
- Create screen (glue_create.rs): `Create.enhanced`, our check box `ENHANCED_BOX` (0x7e0)
  "Enhanced game" added under the speed row in network create (`add_enhanced_box`, a copy
  of label 38 made type 4); toggled in `create_activate`; `SCBW_ENHANCED=1` presets it
  (tests). `net_create_game`: enhanced sets bit 7 of description byte +0x2f (the version
  hash dword +0x2c; ours is 0) and lets 8 links connect (`max` = 8). Joiners read it back
  (`glue_join.rs`, both join paths) into `Create.enhanced`; `begin_game` sets
  `GameSetup.enhanced = multiplayer && create.enhanced`.
- Joiner entry points: native `SCBW_JOIN` loop in `glue.rs run` (checks `snet::late()`
  and runs `late_game` + `view::run_scene`); lobby screen `glue_chat.rs chat_tick` (a
  non-creator that receives a late start ends the dialog, `Glue.late`, then glue.rs
  `Top::Game` runs it natively, `web_frame` in the browser); browser `web.rs net_join` ->
  `glue_lobby` loop sets `glue.late` and state START.
- Save loading of either kind: `Scene::from_save_steps` retries with
  `is_expansion = false`, remembers it (`SAVE_EXPANSION` thread-local) for the second load
  in `Scene::start`. (The test host was an Original game; the old code refused it.)
- Debug aids added: `scbw loadsave <file>`, `SCBW_SAVE_DEBUG=1` (which part of a load
  fails), `SCBW_LATE_DUMP=<path>` (joiner writes the received save), joiner prints
  "the game's state came (N bytes, T ms)" / "loaded (T ms)".

## Where the code is

| File | What |
|---|---|
| `src/bw/enhanced.rs` | new: command, pick, join, AI take over / release, Extras |
| `src/bw/ai.rs` | `start_melee_ai_for` split out |
| `src/bw/command.rs` | 0xe0 dispatch; `leave_notice` AI branch |
| `src/bw/mod.rs` | `enhanced`, `ai_taken` fields; `pub mod enhanced` |
| `src/replay.rs` | `split_commands` length of 0xe0 |
| `src/net/mod.rs` | Session: `new_at`, `add_player`, `orphans`, `keep`/`kept`/`keep_snap`, `start`, `added`, `live_players`, `JOIN_LAG`, join detection in `run_frame` |
| `src/net/live.rs` | `enhanced_host`, join queue, C_LATE, `late_bytes`, `late_game`, fast drops, grace |
| `src/net/transport.rs` | `hold_peer`, `start_peer`, `player_from` |
| `src/net/snet.rs` | C_LATE assembly (`late()`), NAME with army, `want_army` |
| `src/view.rs` | `GameSetup.enhanced`, `run_scene`, `reload_through_save`, save kind retry |
| `src/glue.rs`, `glue_chat.rs`, `glue_join.rs`, `glue_create.rs`, `web.rs`, `web/index.html` | entry points and the option |
| `src/bw/save.rs`, `save/gamedata.rs`, `main.rs` | debug aids |
| `agent/tools/enhanced.sh` | the end-to-end test (env: K, QUIT_AT, ARMY, LATE_AFTER, LATE_DEBUG, P, O) |

## Open items, in order

1. **Before committing** (do these first, in this order; each command must pass):
   - `cargo test --release` (27 pass before).
   - `agent/tools/suite.sh | grep -v MATCH` -> no output.
   - `agent/tools/repcmp.sh ~/games/Starcraft/broodwar-reps-116/tl1412*.rep 3000` -> MATCH.
   - `agent/tools/netlive_leave.sh all` -> a..f PASS (they use non-enhanced games: the
     fast-drop / Session changes must not affect them).
   - `agent/tools/lobby.sh` and `agent/tools/lobby.sh web` -> equal hashes (needs
     `web/build.sh` first for web).
   - `agent/tools/enhanced.sh` (K=60000 LATE_AFTER=1), and with `QUIT_AT=15000`.
   - Then commit (trailers from conventions.md), update `agent/areas/net.md` (new section
     "Enhanced games" summarizing the design above, shorter) and `agent/status.md`.
     Ask the user before pushing.
2. **LAN list for running enhanced games** (was in progress when interrupted). The LAN
   advert is owned by snet's `Game` and dies in `snet::take_transport` when the game
   starts. Plan: give `take_transport` a `keep_advert: bool` (glue passes
   `self.create.enhanced` from `lobby_net_game`); when true, move `g.advert` into a
   thread-local `KEPT_ADVERT: RefCell<Option<Arc<Mutex<Advert>>>>`, dropped by
   `snet::leave_game` / `destroy`. Then a joiner on the join screen sees the game; joining
   it goes to the lobby screen, where `chat_tick` already switches to the late start.
   Test: like `enhanced.sh`, but the 3rd player uses the join screen script (`"$L;t
   three;k 0d;w 1500;k 0d;w 3000;t o"` as in `lobby.sh`) instead of SCBW_JOIN.
3. **Browser late join test**: `lobby.sh web` style: host via `enhanced.sh` steps, then
   headless Chromium `?join=<code>&frames=K&name=web` after the game started (see
   `lobby.sh` web branch and `agent/tools/cdp_console.mjs`). Expect the console line
   `frame K hash H` equal to the host's. Watch for: `late_game` uses `Scene::from_save_steps`
   (fine in wasm), the `web_frame` `Top::Game` late branch (glue.rs), and that
   `NetGame::report_next` is applied (it is consumed in `NetGame::new`).
4. **Visible feedback**: when the AI takes an army, show a line (no network.tbl string
   exists; use `scene.msgs` with our own text, e.g. "%s's army is played by the computer").
   Push a notice kind from `ai_take_over` (e.g. a new `State` queue like `notices`) and
   print it in view.rs next to the notices loop (~line 740). Keep sim deterministic: only
   push to an output queue (listed in `outs!` in bw/rollback.rs so rollbacks trim it).
5. **Host leaving ends the game** (star topology). Host migration is out of scope; tell the
   user if asked. A non-host leaving works (AI takeover).
6. **Edge cases to check**: two players asking to join at once (queue serializes; second
   pick happens after the first's frame ran on the host scene — verify with two late
   joiners); a joiner reusing a link id of a player that left earlier (`add_player`
   handles a gone row; untested); joiner asking for an army of a defeated player
   (`enhanced_pick` refuses: `player_left != 0`); more than 7 links (transport ids 1..7
   only: an 8th link is refused "game full").
7. **Throughput**: the start is ~180 KB in 500-byte control messages (~2 s on localhost).
   If it is slow over real networks, raise the control budget in `transport.rs flush`
   (the `> 600` check) for a peer in the C_LATE phase, or send several datagrams per
   flush. Do not exceed the browser's `maxDatagramSize` (~1200).
8. **Window check** (user-visible, see memory "Visual checks need native"): run a host in
   a window (`scbw glue`, Multiplayer, LAN, Create with the Enhanced box), join from a
   second window mid-game, check: the "has joined" line, shared selection rows, the
   Enhanced box look (its text was cut off at a wider label; now "Enhanced game").

## Gotchas learned

- Headless glue waits fast-forward unless `SCBW_GLUE_REALTIME=1`.
- Headless games run unthrottled: a late joiner needs a long game (K >= 60000) to arrive in
  time. `enhanced.sh` waits for the host's `net player` debug line (NETREP_DEBUG) before
  starting the late joiner.
- Start positions are random in melee: "army 1" is not always the second player.
- `restore_sim` keeps viewer-local fields; `local_human` is not in its keep list, so
  `reload_through_save` saves and restores it by hand.
- `state_hash` covers units/bullets/resources, not everything: a desync can hide; the
  C_HASH check every 8 frames is the first signal (`net: hash mismatch at frame ...`).
