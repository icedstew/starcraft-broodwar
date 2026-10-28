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

### Task 1: list box border colour on the Games screen (DONE 2026-10-27)

User report: a bright blue outline around our Games list. Two causes, both fixed, checked
pixel-exact against 1.16.1 (`re/native_join.sh`, gdb on the real game with
`NATIVE_COLORS=1 NATIVE_GDB_EX=<gdb script>`):
- Glue dialogs have dialog flag 0x20000000 and 0x4180d0 keeps their active control at 0, so a
  hovered list is never focused: `Glue::mouse_to` no longer sets `active` for them.
- Each glue list's own event proc sets flag 0x20000 (no frame) on event 10: the join list
  (0x4b84a0 / 0x4b84c3) and the provider list (0x4bad10 / 0x4bad41) were missing it; the
  login and registry lists already had it. The native list draws all had flags 0x20818.
Games screen ours vs native with the mouse over the list: 0 differing pixels. Battle.net
screens are out of scope (user, 2026-10-27).

### Task 2: browser late join (DONE 2026-10-27: `WEB=1 K=2500 LATE_AFTER=1 agent/tools/enhanced.sh`, three equal hashes; `SCBW_NET_PACE_US` paces native headless peers because a headless browser runs ~18 fps; not run with ARMY)

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

### Task 3: edge cases (DONE 2026-10-27)

- Two joins queued: `FOURTH=1` (human 8 / army 0, human 9 / army 1): equal hashes.
- A leaver's army taken back plus a fourth player: `QUIT_AT=15000 FOURTH=1`: equal.
- Full game: `PACE_US=4000 EXTRA=5 K=12000` (8 players) equal; `EXTRA=6`: the 8th joiner exits
  with "cannot join ..." ("game full" from `transport.rs host_conn`), the rest unaffected.
  Unthrottled 8 native peers starve the CPU and time out: always pace them (PACE_US).
- Link id reuse: found and fixed a real bug: a reused link id kept the old occupant's control
  sequence / ack state in `Transport` (`ctl_in` etc.), so the joiner was never heard
  ("no answer from the game"). Reset on `Inbound::Joined`. Test (needs the old link to time
  out, ~30 s): `PACE_US=2000 KILL2=1 QUIT_AT=3000 K=60000 LATE_AFTER=45 agent/tools/enhanced.sh`
  -> "player 1 joins as human 1, army 1", equal hashes.
- A defeated player's army: not run, by code reading `enhanced_pick` skips it (`alive` needs
  `player_left == 0`). With no army at all the host logs "cannot join: no army" and the joiner
  times out with "no answer from the game" (no explicit refusal message yet).

### Task 4: window check (mostly done 2026-10-27)

`agent/tools/enhanced_win.sh <dir>` runs host / joiner / late joiner in real windows (three
Xvfb displays 1280x960 because the window is 2x) driven by xdotool and saves screenshots.
Seen: hotkeys m / e / o work, the Enhanced box on the create screen, the lobby, the game, and
"three has joined the game." on the late joiner's screen. It found a real bug: DESYNC at the
join frame when that frame is a multiple of 8 (the others hashed the state before their
save / load pass, the joiner the loaded one): `Session::joins_at` now skips that hash
(`due_hashes`). Regression for it: `PACE_US=40000 K=1500 LATE_AFTER=0 agent/tools/enhanced.sh`
(must print no "hash mismatch" / DESYNC).
Harness gotchas: do not send Escape (a second one quits from the main menu; a click skips
the title); keys must be held ~0.04 s (the window samples key state per frame); never
`pkill -f` a pattern that also appears in your own command line (it kills your shell:
exit 144) - anchor it with ^.
Not verified: the "army is played by the computer" line in a window (the last block of
enhanced_win.sh kills display :92 and shoots host / third 8 s later; it was never run to the
end because of the pkill problem above), the Enhanced box text cut-off at other widths.

### Task 5: announce the join with the army and colour (user request 2026-10-27, DONE 2026-10-27)

User: "Announce for all players which team (slot) the new player has joined. Also style the
player's name as the colour they are joining." Ours (no 1.16.1 counterpart), so no address
to port; keep it small and keep the sim deterministic (it only pushes to an output queue).

Facts checked 2026-10-27 (do not re-derive):
- `State::enhanced_join` (src/bw/enhanced.rs ~174) pushes `(0x49, h)`; `notices` is
  `Vec<(u16, u8)>` (src/bw/mod.rs ~406), listed in rollback.rs `outs!` (no change needed
  there if the tuple type stays the same).
- view.rs ~762 drains `st.notices`: network.tbl line `fmt` with `%s` = `st.human_name(p)`,
  printed by `msgs.print(.., sound 0x17)` (line colour 3). `NOTICE_AI` (0x1000) is the
  precedent for our own text.
- network.tbl has no "joined <army>" line (only 0x49 "%s has joined the game."): use our
  own text.
- After the join, `st.nation_of(h)` is the army `n` (`human_nation[h] = n` is set in
  `enhanced_join`), so the view can find the army from `h` alone: no new tuple field.
- Inline colour bytes in a line: `ui.rs color_row` (0x41f610 `Color::set`). Code 1 =
  back to the previous colour. Valid codes: 2..=8, 0x0e..=0x11, 0x15..=0x19, 0x1b..=0x1f
  (0x1a has no row in 1.16.1).
- `Scene.colors[n]` (view.rs ~192 / 416) = army n's tunit.pcx colour row (0x57f21c through
  the map's COLR). That row decides which text code to use.

Steps:
1. enhanced.rs: add `pub const NOTICE_JOIN: u16 = 0x1001;` next to `NOTICE_AI` (doc line:
   "Notice id (not a network.tbl line): human `p` joined the army `nation_of(p)`."). In
   `enhanced_join` replace the 0x49 push with `self.notices.push((NOTICE_JOIN, h));` and
   fix the comment above it.
2. Text colour for a tunit row: a small pure fn, e.g. in msgs.rs,
   `pub fn player_text_code(row: u8) -> u8` with the table
   `[0x08, 0x0e, 0x0f, 0x10, 0x11, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1b, 0x1f]`
   (red, blue, teal, purple, orange, brown, white, yellow, green, pale yellow, tan,
   aqua), anything else -> 2. This table is a best guess (BWAPI's text colour names):
   VERIFY it before trusting it. Write a throwaway script in the scratchpad that reads
   game/tfontgam.pcx and game/tunit.pcx (extract them with `scbw extract
   'game\tunit.pcx' <out>`) and prints, for each tunit row 0..11, its first entry's RGB
   and the RGB of the brightest entry of the tfontgam row `color_row(code)`. Fix any row
   whose colours clearly differ. Note in the doc comment that it was checked this way.
3. view.rs notice loop: add a branch for `NOTICE_JOIN` before the network.tbl lookup.
   Build the line as bytes (names may hold non-UTF-8):
   `code, name, 0x01, rest` where `n = st.nation_of(p)`, `code =
   player_text_code(self.colors[n])`, and `rest` is
   - `" has joined <owner>'s army."` with `owner = st.player_names[n]` when `p != n` and
     that name is non-empty (the slot's own player, may be a computer's name),
   - else `" has joined as Player <n+1>."` (the joiner took the slot itself: its
     `player_names[n]` is now the joiner's own name).
   Print it with the same `self.msgs.print(&ui.font, &line, now, 0, 0x17, ..)` call. Keep
   the `NOTICE_AI` branch as is (optionally colour its name the same way; ask nothing,
   just do it only if it is one line).
   Strip control bytes (< 0x20) from the names as `msgs::chat_line` does, so a name cannot
   change colours.
4. One unit test (msgs.rs `mod tests` has the pattern): the line builder gives
   `[0x0e, b'x', 1, ..]` for row 1, and the "as Player" form for `p == n`. Put the line
   building in a pure fn so it is testable without a Scene.
5. Window check: agent/tools/enhanced_win.sh shoots 25 s after the late joiner starts;
   the line lasts 7 s, so it is gone. Change that step to a loop that shoots :91 and :92
   every 2 s for ~30 s (`join_host_$i`, `join_two_$i`), look at the PNGs (Read tool) and
   confirm the line, the colour of the name, and that both see it. Note whether the late
   joiner (:93) sees it too (it starts from the join frame's saved state; seeing it is
   not required, write down which).
6. Regression set (above) before the commit; `cargo test` count becomes 28. Then mark this
   task DONE here with what was checked, update agent/status.md "In flight", one commit
   (trailers: conventions.md). Do not push.

Done: `NOTICE_JOIN` (enhanced.rs), `msgs::player_text_code` / `join_line` (+ test),
view.rs notice branch; `enhanced_win.sh` now shoots all three screens every 2 s.
- Colour table checked with a script over game/tunit.pcx and tfontgam.pcx: rows 0..10 match
  (first tunit entry == brightest font entry); row 11 (64,104,212) is font row 19 = code
  0x1c, not 0x1f (0x1f is aqua 0,228,252): table fixed.
- Window run: the line "three has joined <owner>'s army." shows on host AND joiner at the
  join, name in red (army colour) in all three windows; the late joiner sees it too (about
  20 s later, when its state arrives: the notice is in the transferred state).
- Found: the joiner never saw the line: `Session::tick_inner` drops what a rollback's
  re-simulation pushes to the output queues, and the join command arrives late there. Fix:
  `Session::announced` / `new_announcements` keep the join / AI lines a re-simulation makes
  for the first time (not the repeats). Same loss hit the old 0x49 line.
- Not fixed, seen once: in one run the host's own colour for army 0 was white (row 6, dealt
  by 0x49b360) while the late joiner drew the same army red: joiners may not get the host's
  dealt colours (`GameSetup.colors`), so unit and name colours can differ per viewer.
  Check before relying on colours across peers.

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
