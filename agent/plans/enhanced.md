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

All of it is committed (2026-10-27). It builds natively and for wasm with no warnings
(`cargo build --release`, `cargo build --release --target wasm32-unknown-unknown`).

Verified with `agent/tools/enhanced.sh` (headless, real network over localhost):

| Run | Result |
|---|---|
| host + lobby joiner start; third player joins by code mid-game, its install has no maps | all 3 equal hash at frame 60000 / 80000 |
| `QUIT_AT=15000`: 2nd player quits, its army goes to the AI; 3rd joins, takes it back (human id reused) | host and joiner equal at 40000 |
| `QUIT_AT=15000 ARMY=<host's army>`: 3rd joins the host's army as a second human (id 8) | equal at 60000 |

| `LAN=1 K=300000`: 3rd finds the running game on the join screen's LAN list | all 3 equal at 300000 |

Regression set passed after these changes. Not verified: the browser late join, anything in
a window,
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
| `agent/tools/enhanced.sh` | the end-to-end test (env: K, QUIT_AT, ARMY, LAN, LATE_AFTER, LATE_DEBUG, P, O) |

## Open items, in order

Done 2026-10-27 (committed): the full pre-commit run (cargo test 27, suite, repcmp tl1412
MATCH, netlive_leave a..f, lobby.sh, enhanced.sh plain / QUIT_AT), the LAN list for
running enhanced games (`snet::take_transport(keep_advert)`, `KEPT_ADVERT`, native only),
the glue.rs native late-start branch moved before `begin_game` (it used to exit silently),
the "army is played by the computer" line (`NOTICE_AI` = 0x1000, view.rs notices loop).

Rules for every task below: build both targets with no warnings
(`~/.cargo/bin/cargo build --release` and `... --target wasm32-unknown-unknown`; native-only
code such as `Advert` / `UdpSocket` needs `#[cfg(not(target_arch = "wasm32"))]`), run the
"Regression set" before each commit, one commit per task (trailers: conventions.md), update
this file's task to DONE in the same commit. Do not push; ask the user.

Regression set (each must pass; ~10 min):
```sh
~/.cargo/bin/cargo test --release 2>&1 | grep "^test result"          # 27 passed
agent/tools/suite.sh | grep -v MATCH                                   # no output
agent/tools/repcmp.sh ~/games/Starcraft/broodwar-reps-116/tl1412*.rep 3000 | tail -1   # MATCH
agent/tools/netlive_leave.sh all 2>&1 | grep -E "^(PASS|FAIL)"         # a..f PASS
agent/tools/lobby.sh | tail -2                                         # equal hashes
K=60000 LATE_AFTER=1 agent/tools/enhanced.sh | head -3                 # 3 equal hashes
```

### Task 1: list box border colour on the Games screen (user report, UI) - half done

User (2026-10-27) circled the Games list of our join screen: "This looks wrong, the border
color maybe?" (a bright blue outline when the mouse was over the list).

Done (2026-10-27): native captures with `re/native_join.sh <dir>` (Xvfb + wine; keys need the
pointer inside the window; `NATIVE_COLORS=1` also reads the colour table 0x6ceb20 through
gdb into `<dir>/colors.txt`). Findings, all measured against 1.16.1's own screens:
- The colour table is right: native `[0xc]=0xab, [0xf]=0x96, [0x10]=0x65` = ours for the
  Games screen's palette.
- The blue came from the focus flag: native glue dialogs have dialog flag 0x20000000
  (0x4dd9e0), and 0x4180d0 then keeps the dialog's active control at 0, so a hovered list is
  never focused. Our `Glue::mouse_to` set `dlg.active = hit`; now `None` for such dialogs
  (glue.rs). Ours vs native Games screen with the mouse over the list: identical except
  the list's outer outline.
- Left: our list draws the 1 px outer outline (`list_frame`, colour 0xab = #303030) and
  native draws nothing there (native pixel (64,200) = the background; same for the Select
  Connection list). The inner outline is invisible either way (colour 0x96 = bg). Not found
  who suppresses it natively: the draw proc is 0x41b950 (data table 0x501504, type 12),
  its frame call is skipped only with control flag 0x20000; the join list's flags are 0x818
  (bin) and neither 0x4b8340 / 0x4b8460 / 0x4b8560 nor the slide-in table (ids 1-4 only)
  set it. Ideas: read the list control's flags from native memory (gdb: the control
  struct is on the heap; find it through the dialog list 0x6d5e..), or find which function
  clears/sets flags +0x18 of type 12 controls during glue dialog creation (0x41a080 path).
  Once the cause is known, port it (do not special-case Games); check the Select Connection,
  Games and lobby lists against native shots from `re/native_join.sh`.

### Task 2: browser late join

Goal: a browser joins a running enhanced game by code and ends with the same hash.
1. `web/build.sh` (builds wasm into `web/pkg`). It must succeed.
2. Add `WEB=1` to `agent/tools/enhanced.sh` for the third player, copying `lobby.sh`'s web
   branch: `python3 -m http.server -d web 8767 -b 127.0.0.1`, headless chromium with
   `--remote-debugging-port=9335 --user-data-dir=$O/prof`, and
   `node agent/tools/cdp_console.mjs 9335 "http://127.0.0.1:8767/?join=<code with # as %23>&frames=$K&name=web"`
   writing `$O/late.log`; kill chromium / server / node after the host exits. Add `&army=N`
   when `ARMY` is set.
3. Run `WEB=1 K=300000 LATE_AFTER=1 agent/tools/enhanced.sh`. Expect `frame 300000 hash H`
   in all three logs, equal. (The browser runs slower; if the game ends before it joins,
   raise K. A long K is fine: headless games are unthrottled.)
4. If it fails, look in order at: the console log (`late.log`) for panics; `web.rs net_join`
   -> `glue_lobby` loop setting `glue.late` and state START; the wasm `Top::Game` branch in
   glue.rs (~line 3359, it checks `glue.late` before `begin_game`: keep it that way);
   `live::late_game`; that `NetGame::report_next` is applied. Datagrams must stay under the
   browser's `maxDatagramSize` (~1200; C_LATE parts are 500 bytes).

### Task 3: edge cases (headless, extend enhanced.sh)

Each as an env option of enhanced.sh, all three (or four) hashes equal:
- `FOURTH=1`: a fourth player joins by code 2 s after the third (two joins queued; the
  second is picked after the first's frame ran on the host).
- `QUIT_AT=15000` then the third joins with no `ARMY`: it should take the AI-held army
  (already passes) — also check a fourth player joining after the third quits reuses its
  link id (`add_player` with a gone row).
- Asking for a defeated player's army (`ARMY=<that slot>` after it lost): `enhanced_pick`
  must refuse and pick another army; check the host log line "joins as human h, army n".
- 8 links: the 8th is refused "game full" (transport ids 1..7); the joiner must show an
  error, not hang.

### Task 4: window check (user-visible; needs the user or a native-style capture)

Run a host in a window (`target/release/scbw glue`, Multiplayer, LAN, Create Game with the
"Enhanced game" box ticked), join from a second window mid-game (join screen list, or
`SCBW_JOIN=<code>`). Check: the "has joined the game." line, shared selection, the
"army is played by the computer" line after a player quits, the Enhanced box text not cut.
Take screenshots (ScrollLock) and send them to the user.

### Known limits (tell the user if asked, do not fix)

- Host leaving ends the game (star topology; no host migration).
- The start state is ~180 KB in 500-byte control messages (~2 s on localhost). If slow on
  real networks: raise the per-flush control budget in `transport.rs flush` (the `> 600`
  check) for a peer receiving C_LATE, never above ~1200 bytes per datagram.

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
