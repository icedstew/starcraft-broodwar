# Networking / rollback (own netcode)

Started 2026-10-22 (user: "implement networking / lan ... use shield battery's rollback
netcode ... play in two browsers on different machines"; then "more than 1v1", ".exe to
.exe without external servers"). Storm SNet / BN / modem are not ported (see conventions:
own networking); the 1.16.1 turn loop around them is.

## Design (agreed with the user 2026-10-22)

- **Rollback model from ShieldBattery** (github.com/ShieldBattery/ShieldBattery, MIT,
  Travis Collins): `docs/rollback.md`, `game/src/rollback/` (tick.rs, snapshot.rs,
  state_hash.rs, pacing.rs), `game/src/netcode_v2/input_table.rs`. Port the logic, credit
  it in the module docs. Their transport (rally-point2: QUIC to their relays, SB server
  credentials) is not used.
  - A step's turns are the turn index: every step consumes one turn from every slot still
    in the game; each client chooses how far ahead it sends (own input delay).
  - Missing remote turn = empty turn (prediction); a late turn that differs -> restore the
    newest snapshot at or before that step, re-simulate to the present. Prediction limit 8,
    stall past it. Rollback target ~2 frames, the rest becomes input delay.
  - Once-per-frame effects (sounds, messages, UI writes) happen on first simulation only;
    camera and local selection visuals stay out of the snapshot.
  - Replay recorder, victory / defeat, result: confirmed frames only. A leave is a fence
    no rollback crosses.
  - Native sync command (0x37) off in rollback games; state hash every 8th confirmed step
    instead, compared by the host; minority evicted, no majority = game over.
- **Snapshots = `bw::State: Clone`** (SB memcpys fixed memory ranges; we own the state).
- **Topology: star through the host**, N players (1.16.1: up to 8 slots). Clients send
  turns to the host, the host forwards them. Only the host must be reachable. The host also
  stands in for SB's relay session clock (pacing).
- **Transport: QUIC everywhere, no servers of ours.**
  - .exe <-> .exe: quinn (UDP).
  - browser -> host .exe: WebTransport with `serverCertificateHashes` (self-signed ECDSA
    cert, valid <= 14 days, made per game). Lets an https page (GitHub Pages) connect
    without mixed-content blocks or cert warnings. Server side: `wtransport` crate.
  - Join code: `ip:port#sha256-of-cert`.
  - Datagrams for turns (each packet repeats unacked turns), streams for lobby / map.
  - Reachability: LAN just works (+ LAN broadcast discovery); internet needs UPnP /
    NAT-PMP, a port forward, or IPv6 on the host. Carrier-grade NAT hosts cannot host.
  - Not covered: browser-only games (a browser cannot listen: would need signaling +
    STUN/TURN), Safari WebTransport (to test).
  - Plain `ws://` from an https page is blocked (only localhost exempt); not used.

## Phases

0. (done 2026-10-22) No network: `State: Clone`, state hash (port of SB state_hash.rs), `scbw rbtest <rep>`:
   play a replay with each player's commands arriving k frames late (empty-turn
   prediction, rollback on arrival); final hash / rng must equal straight playback. Runs
   over all local replays (suite check). Measure clone + 8-frame resim cost in wasm.
1. Turn pipeline: UI commands -> local turn buffer (0x485bd0) -> per-turn dispatch of
   every slot's buffer (0x486f50 order) instead of executing at once (view.rs
   `execute_command`). Single player = no peer, delay 0; suite / replays stay the check.
2. Rollback engine (input table, snapshot ring, prediction limit, resim without effects).
3. QUIC transport (.exe), host / join, LAN discovery, UPnP; lobby (glue join / chat
   screens or a minimal one); setup handshake: map chk, seed, slots, races.
4. Desync: hash every 8 confirmed steps, host compares; on mismatch both write a save.
5. Browser: WebTransport client in web.rs, join-code form in web/index.html.
6. (after the first networking pass; user 2026-10-22) Join in progress / AI handover:
   - a joining player takes over a computer player's slot, or shares control of a human's
     player (team melee shared control);
   - when every human of a player has left, the AI takes the player over.
   Design notes: every change of controller is a synced command at a fence step (as a
   leave: no rollback across it). A joiner needs the game's state: send a save at the
   fence and have **every** peer load the same bytes (a loaded game differs from the live
   one in free lists / unique ids, see savetest, so only "all reload" keeps peers equal).
   Not in 1.16.1 (AI only starts at game start, shared control only from the lobby), so
   this is new behaviour built on ported pieces: AI start / town setup for a player that
   already has units and buildings, stopping a player's AI (towns, scripts, requests),
   slot type changes (0x57eee0), shared control (team melee, State.multiplayer).

## 1.16.1 turn loop (facts)

- 0x4d9670 frame loop: per pass, 0x487100 (replay) then 0x4d9550 (network turn); stall /
  exit codes in 0x6d11f0; then 0x488780 / game_step, triggers 0x48a460.
- 0x4d9550: counter 0x51cea0 counts down frames; at 0 calls 0x487070 (receive + dispatch a
  turn); failure = waiting (0x51ce9d set, GetTickCount kept in 0x6d6370; on resume the
  next-frame time 0x51ce94 is adjusted). Reload value 0x51ce70[speed] (frames per turn).
- 0x487070: 0x486580 (receive) -> 0x485aa0 (send) -> 0x486f50 (dispatch; 0x486530 when
  0x6556e0 == 0) -> turn counter 0x57eebc += 1, 0x47cfc0.
- 0x486f50: for storm id 7..0 with flag 0x20000 in 0x57f0b8[id]: player 0x512678 =
  0x57eec0[id], 0x51267c = 0x57ee7c[id], run 0x4865d0 on buffer 0x654a80[id]; a failed
  run (0x6556e8 != 1, not a replay) marks the id 0x10000 and calls 0x4a3010. 0x486530 is
  the same loop with 0x486040 (0x6556e0 == 0).
- 0x486580 receive poll (provider call 0x4bb530): sets 0x57ee78; no turn for 2500 ms ->
  0x4a3380, the waiting-for-players loop (SNetReceiveTurns shape, Storm ord 122).
- 0x485aa0 send: while the in-flight count (Storm ord 115) < 0x57f090 (+0x6556e4) sends
  the local turn 0x654880, length 0x654aa0 (starts at 5). 0x485bd0 (ECX = bytes) appends
  a command to 0x654880 (0x485a40 flushes when full).
- Frames per turn 0x51ce70[s] (0x4d92a0 at init): 1 when not networked, else
  max(1, 1000 / (ms[s] * L)), ms = 0x5124d8 = 167 111 83 67 56 48 42, L from Storm ord
  114 (0 or > 20 -> 20). Probably turns per second (provider caps): L = 8 gives 2 frames
  per turn at Fastest (not verified). Our netcode: one turn per frame (SC:R TR24 / SB).
- 0x47cfc0 after each network turn: a 16-entry ring of 0x10c-byte records (copies
  0x63fd30): the sync data SB lists as `sync_data` (desync check), unported.
- 0x4d3cc0 (EBX = provider id; 0x4d3fc0 passes 0 = single player): 0x57f090 (turns in
  flight) = 1 single player, else Storm caps +0x20 clamped 2..8; 0x57f0d8 (turn buffer
  size) = min(caps +0x08, 0x200). L above = caps +0x1c. 0x6556e4 (extra in flight) from a
  command via 0x485e60 (<= 2). 0x6556e0 = 1 during any game (0x485bb0 from 0x4d9950).
  0x485a40 = one SNetSendTurn (ord 128) flush, no window check.
- Single player runs a command 1 frame after it is sent: measured 2026-10-22 under
  wine + gdb (agent/local/scratch/spdelay/: bp.py breakpoints 0x485bd0 / 0x4865d0 printing
  0x57f23c; 8 of 8 commands, two game speeds): sent at counter f, run at f + 1. Every
  frame also sends the 7-byte sync command 0x37 (`37 <seed u32> 00 00 00`); 0x4865d0 gets
  EAX = bytes, [esp+4] = length, [esp+8] = flag. scbw: `Scene::turn_tick`,
  LOCAL_TURNS = 1. The 0x37 sync command is not sent (SB turns it off in rollback too).
- While a menu is open scbw's frame loop returns before stepping, so no `turn_tick`
  runs; 1.16.1 keeps exchanging turns while paused (0x4d9550 runs before the pause
  check in 0x4d9670). Must change for network games (phase 3): turns cannot stall.
  glue_ready.rs 813..930 models the single player provider (VecDeque, window 1) for the
  lobby; the in-game 0x486f50 path is todo there.
- Today scbw runs local commands at once (view.rs `Scene::command` -> `execute_command`),
  not through a turn; glue_ready.rs has the lobby-side pieces (todo_port! at 843..929).

## Rollback oracle (scbw rbtest)

`scbw rbtest <rep> [delay] [frames]` (main.rs): straight playback vs one where all but the
first player's commands arrive `delay` frames late (empty prediction, restore the
`State` clone before that frame, re-simulate). 2026-10-22: tl1307 whole game (61725
frames) MATCH at delay 1 / 3 / 8 (14560 rollbacks); all 90 replays of broodwar-reps-116
MATCH at delay 3 (final `State` Debug dump and `state_hash` equal). Native cost:
tl1307 whole game: step 0.108 ms, snapshot (`State` clone) 0.086 ms, a delay-3 rollback
0.72 ms. Wasm in headless Chromium (`index.html?auto=r.rep&rbtest=3,20000`, web.rs
`rbtest`; first 20000 frames): step 0.084 ms, snapshot 0.097 ms, rollback 0.65 ms. So a
snapshot every frame and 8-frame rollbacks (~2 ms) fit a 42 ms tick easily.
Headless run: serve web/ (auto/ links as build-and-run.sh), `chromium --headless=new
--disable-extensions --user-data-dir=<tmp> --enable-logging=stderr --v=0 <url>`, grep
'"rbtest '. Both games start from one cloned
`State`: only a process's first game draws the music variant from the CRT rand.

## State hash

`State::state_hash` (src/bw/state_hash.rs, after SB's state_hash.rs): frame, rng, minerals
/ gas, supplies, upgrades / techs, units (visible then hidden lists) and active bullets;
units referred to by handle. Viewer-local state stays out (camera, local player / vision,
selections, sounds), so peers can compare it; a whole-`State` compare (rbtest's Debug
dump) is only valid inside one process. Not covered yet: crt_seed (creep tiles), fog,
AI and trigger state (SB's list doesn't have them either).

## Phase 2 notes (prep)

- `replay_rec::record` runs inside `execute_command` (0x4865d0 -> writer): a re-simulated
  frame would record its commands again. Record confirmed frames only (SB does the same).
- `State` fields a restore must keep (viewer-local / UI): local_player, replay*,
  selected (UI selection), selection_emptied, key_alt, marker_thingy / marker_shown,
  ui_hl_pool / ui_circle_pool / ui_box_pool, last_error / shown_error, shown_text,
  transmissions, center_view*, notices, chat, alert_*, local_vision_mask, screen_tile
  (feeds the creep display only, else 1.16.1 games would desync), turn_out.
  Once-per-frame outputs drained by the frame loop (sounds, ui_sounds, trigger_wavs,
  speech, unit_sounds, talked, portrait_dropped, minimap_pings): discard what a
  re-simulated frame produces (shortcut; SB matches them against the first run).
  Check each against its readers before relying on this list.

## Phase 2: rollback session (src/net.rs)

`Session` (one peer): input table per player by frame (frames < delay known empty),
`local_turn` / `remote_turn` / `tick(&mut Scene)`, PRED_LIMIT 8, a snapshot per frame
(ring of PRED_LIMIT + 1: a late turn is never older than that), dispatch in storm id
order 7..0 (0x486f50). `State::restore_sim` (src/bw/rollback.rs) keeps the viewer-local
fields (not the UI image pools / order marker: they index pools the snapshot restores;
a rollback can leave a stale circle / ghost until redrawn). Re-simulated frames' output
queues are cut back (`out_lens` / `truncate_outs`); Scene-side voices / pings /
show_error still run twice (ponytail). Replay recording of confirmed frames only: todo.
`scbw nettest <rep> [delay] [latency] [jitter]`: one Session + Scene per replay player in
one process, turns over a simulated link; all final hashes must equal straight playback.
2026-10-22: every 8th 1.16.1 replay at 2/3/2 and 1/8/6: 20 MATCH (latency + jitter past
PRED_LIMIT stalls, ~30% more ticks); tl2071 INVALID (a command at frame 0 < delay).

## Phase 3 plan (transport, .exe first)

- Crates (native only, cfg(not wasm32)): `wtransport` (WebTransport over quinn: the same
  server takes .exe and browser clients; the .exe client uses it too), `rcgen` (per-game
  self-signed ECDSA cert, <= 14 days), later `igd-next` (UPnP). tokio runtime on a
  thread; the game loop talks to it through channels (try_recv per tick).
- Host relays (star): datagrams `{from, frame, bytes}` x the last unacked turns
  (redundancy), plus per sender the newest frame received (ack) and the sender's current
  frame (pacing: a peer ahead of the slowest by more than its lead skips a tick). Reliable
  stream for the lobby: hello / GameSetup (chk, slots, races, names, forces, colors,
  template, seed; each client sets `local`) / start / leave.
- 3a: transport + `scbw netrep <rep> host <port> | join <addr#hash>`: two (or more)
  headless processes each play one replay player's commands as local input over real
  QUIC on localhost; final hashes must equal straight playback (the nettest check
  across processes).
- **3a done** (`src/net/transport.rs`, `src/net/netrep.rs`; native only): wtransport 0.7.2
  (ring backend, no cmake/nasm) + tokio 1.48 current-thread runtime on a `net` thread;
  star topology, host also plays; per-run self-signed cert pinned by SHA-256 (`addr#hash`).
  Lobby = Hello/Start on a reliable bi stream; turns = redundant datagrams with acks (wire
  format in the transport.rs module doc). `scbw netrep <rep> all [port] [delay]` spawns a
  host + one `join` per other player and prints MATCH only if every peer's hash equals
  straight playback (exit code 1 otherwise); manual: `netrep <rep> host <port> [delay]`,
  `netrep <rep> join <addr#hash> <map player> [delay]`. `NETREP_DEBUG=1` prints progress.
  Needs delay >= 1. Short 2-player replays take ~10 s wall. Gotcha: events delivered with
  or before Start must all be applied (turns are not redelivered).
- 3b: live game: frame loop drives `Session::tick` instead of `turn_tick` + step, turns
  keep flowing while a menu is open; game start from the received GameSetup.
- 3c: minimal lobby (host: map, slots; join: address field), LAN broadcast discovery.
- 3d: UPnP, leaves (fence), desync hashes every 8 confirmed frames, confirmed-only
  replay recording.
