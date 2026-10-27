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
- (Fixed for net games in 3b.) While a menu is open scbw's frame loop returns before stepping, so no `turn_tick`
  runs; 1.16.1 keeps exchanging turns while paused (0x4d9550 runs before the pause
  check in 0x4d9670). Must change for network games (phase 3): turns cannot stall.
  glue_ready.rs 813..930 models the single player provider (VecDeque, window 1) for the
  lobby; the in-game 0x486f50 path is todo there.
- Today scbw runs local commands at once (view.rs `Scene::command` -> `execute_command`),
  not through a turn; glue_ready.rs has the lobby-side pieces (todo_port! at 843..929).

## In-game chat (2026-10-27)

1.16.1 chat is not a turn command: Storm messages (SNetSendMessage ord 127 per recipient,
received by 0x485f70 / SNetReceiveMessage ord 121, dispatched by the first byte through
0x50439c: 0 chat 0x485f50, 1 whisper 0x485f30, 2 0x47d170). Packet = type, an unset byte,
text + NUL. The receiver (0x485d10) checks the sender is in the game and not squelched
(0x57f1d8), then records `[0x5c, sender slot, 80 bytes]` into the replay under its own storm
id (0x4cde70, frame 0x57f23c) and shows it (0x48d1c0); whispers are shown, not recorded.
So 1.16.1 replays do carry chat, as seen by the recording player. Storm hands a message
to oneself directly (the sender's own line comes back through the receive path).
Recipients (0x4f32d0 at open): Shift = all, Ctrl = allies (alliance games, 0x4aabb0), else
0x581d60 (the message dialog 0x48ff10, not ported: 8 all at game start, 9 allies in type
0xf games). "/" lines are 0x47f8f0's and never sent (squelch / ignore / unsquelch /
unignore, whisper / msg / w / m; /r only on Battle.net).
Ours: chat.rs (`Chat::send`, `slash`, `receive`), live.rs C_CHAT control messages routed
by the host (`send_chat` / `route_chat` / `chat_in`), `Session::record_chat` records at the
frame it arrived in, flushed with that frame's confirmed turns. Masks are player bits, not
storm ids (one to one). Check: SCBW_NET_SCRIPT `say <text>`; `scbw rep` prints chat lines.
A 0x5c in the replay's last frame group is never run (0x4cdff0 returns -1 at the end).

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
- **3b done** (`src/net/live.rs`, `view::NetDriver`; native only): `scbw nethost <map> [port]
  [players] [delay]` prints `LISTEN ip:port#hash` (the join code) and waits for players - 1
  joins; `scbw netjoin <code>` takes the next free slot. The host builds a melee `GameSetup`
  (humans = the map's first N OWNR-human slots with a start location, other slots empty;
  races from SCBW_NET_RACES `t,z,p,r` or dealt from the seed; names "Player n"; FORC forces;
  colors 0..7; template melee (2, 1); `multiplayer` true = 0x57f0b4, a new `GameSetup` field),
  serializes it (`encode_setup`, no serde) and sends it with Start (lobby messages are now
  `len u32`; Start = slot, the host's delay, setup bytes: the host's delay wins). Hello's
  player 255 = any slot. Every peer builds its Scene from the same setup with its own
  `local`. `view::set_net_driver` hands a `NetGame` (Session + Transport) to the next
  `frame_loop_end`; per game frame `NetDriver::frame` = UI-queued commands (`st.turn_out`)
  as the local turn -> send, poll -> `remote_turn`, `Session::tick` replaces `turn_tick` +
  `step`. A menu does not pause a net game (`Scene::live_net`: `pause_game` is a no-op; the
  menu branch calls `frame` per 42 ms game tick, its sounds are dropped); a stall just does
  not step. Single player and replays do not touch the driver. `SCBW_NET_BIND` (host address,
  default 127.0.0.1; `0.0.0.0` for a LAN: the join code then shows 0.0.0.0, use the host's
  IP). Headless check `agent/tools/netlive.sh [map] [frames] [delay]`: SCBW_NET_HEADLESS=K
  runs no window / audio / UI (still `frame_loop_end`'s setup) and prints `frame K hash H`;
  SCBW_NET_SCRIPT="<frame>:<hex>;..." injects replay-format commands (`selw` = select a
  worker) into the local turn at that frame. 2026-10-24, (2)Bottleneck, Terran vs Zerg, one
  scripted worker move per player: MATCH at 400 f / delay 1, 1500 f / delay 1 and 4, 800 f /
  delay 2 (delay 1 unpaced: 1..3 rollbacks per peer, a few stalls).
  Left: map archive sounds do not travel (clients have no map wavs); replay recording is
  still every simulated frame (3d: confirmed only); no leaves / desync hashes (3d); no lobby
  UI (3c); drops: a peer that quits stalls the others.
- 3c (user 2026-10-26): port the 1.16.1 multiplayer glue, plus custom UI to copy a join URL /
  code for the joiner (1.16.1 has no join-by-address). Plan:
  1. `src/net/snet.rs`: Storm SNet stand-in (named after the Storm calls, ordinals noted),
     one provider 'UDPN' ("Local Area Network (UDP)") over our QUIC transport; game list
     from LAN broadcast discovery; create / join / messages / lobby turns. BN / modem / IPX /
     direct cable stay unported.
  2. gluConn (screen 2, 0x4baf30 cluster): provider list -> 0x4d3cc0 -> login (5).
  3. gluJoin (screen 10, 0x4b86c0 cluster): game list, Create -> gluCreat (11), Join; plus the
     custom join-code entry.
  4. gluCreat network branch (glue_create.rs) -> 0x4d3910 / SNetCreateGame.
  5. gluChat (screen 3, 0x4b9e70 cluster): lobby slots / races / chat over lobby turns; the
     host's custom "copy join URL"; start (0x4d4130) -> the rollback Session (live.rs).
- 3d: UPnP, leaves (fence), desync hashes every 8 confirmed frames, confirmed-only
  replay recording.
- **3d done except UPnP** (2026-10-26; code from 835ea06, verified here): leaves with a
  fence (`Session::apply_leave`, host picks `fence_for(last)`, 0x57 at the fence, reason 1
  quit / 6 dropped), desync hashes, confirmed-only recording. `agent/tools/netlive_leave.sh
  all`: a (2p, replay = live), b (3p relay), c (joiner killed, survivors equal, replay =
  live), d (forced desync reported), e (clean quit), f (host quits) all PASS.
  Fixes: the replay header is taken at game start (0x4dea10 from 0x4e0710, right after
  0x4ef100; `replay_rec::capture_header`), not when the file is written, so a leaver
  stays a human in it (was type 10 from frame 0 -> replay != live); the storm ids are
  filled in at write. Gotcha: 1.16.1 playback never runs a replay's last command group
  (0x4cdff0 returns -1 at the end), so a leave that is the last recorded command is not
  replayed (real 1.16.1 too); the tests end with a harmless select after it.
- **Browser join with commands on both sides** (2026-10-26, Linux): `agent/tools/webjoin.sh
  [frames] [delay]` = native `nethost` + headless Chromium (`?join=&frames=&script=`), the
  console read over the DevTools protocol (`agent/tools/cdp_console.mjs`; `--enable-logging`
  shows nothing with `--headless=new`). (2)Bottleneck, both peers scripted (selects + moves),
  1500 frames: equal hashes at delay 1 / 2 / 4. The browser steps ~18 frames/s headless.
  Fix: a client that loses the host after holding every turn up to its end plays them out
  (the host closes once its turns are acked, the slower browser was 1-2 frames short).
- **Browser leave** (2026-10-27): the browser's frame loop drops its state when it ends
  (`run_raf`), so `NetGame::drop` sends the leave (no blocking wait in wasm) and the
  WebTransport session closes ~200 ms later: the host sees the player lost at once. Before,
  a menu quit kept the session alive and the host stalled forever. A closed tab already gave
  "closed by peer" at once; a killed browser is seen after the 10 s idle timeout, with no
  waiting-for-players dialog yet (0x4a3380 unported).
- **Waiting dialog** (2026-10-27, 0x4a3380 cluster): `Session::waiting_on` (prediction limit
  reached, turn at `confirmed` missing) -> `NetDriver::waiting` after 500 ms held (ours: 1.16.1
  opens it at the first missing turn, its turns are buffered ahead) -> menu `Kind::TimeOut`
  (rez\timeout.bin, 0x4a3320 45 s / carry-over countdown, 0x4a2d60 rows with swatches,
  0x4a3150 Drop enabled at 0:00). Drop (0x4a3010): the host drops whom it waits for (reason
  6); a client sends `C_DROP` (7). Not done: 0x4a3010's self-leave when more lag than
  respond, the 0x4a2c20 row shuffle. Checked under Xvfb: SIGSTOPped joiner -> dialog,
  countdown, Drop -> "Player 2 was dropped from the game.", game resumes.
- **Team games / shared control** (2026-10-27): 0x486f50 runs each human's commands as two
  players: 0x512678 = 0x57eec0[id] (the slot whose units it orders, "nation") and 0x51267c =
  0x57ee7c[id] (the human: its selection 0x6284e8 row, hotkey groups, errors shown when it
  is 0x512688). `State::execute_command(nation, human, ..)`, `Scene::human_command`,
  `State::human_nation` / `local_human` / `human_names`. Ours: humans past 8 (up to 12 per
  slot, 32 in all) have no slot and get rows 8.. (selections / hotkeys / recent groups grow).
  `scbw nethost` with SCBW_NET_TEAMS=a,b[,c,d] (SCBW_NET_GAME 11/12/13): leaders on start
  slots, members on free human slots (type 2, team force, seat races 0x57f1c0), then ids 8..;
  the setup carries humans / names / leaders / seats / storm order. Chat masks are u32 of
  humans. Leave (0x4c4d60 team branch): the slot is settled only when no human of it is left.
  Teammates' selection circles (0x49a110 / 0x49af80 / 0x499a10): sprite flag bits 1..2 only,
  drawn by view.rs; 1.16.1's image 0x23b runs iscript header 0 (waitrand) on the teammates'
  clients only, which split our peers' states ("DESYNC at frame 56" in a window test).
  Checked: 2v1 (human 8 shares slot 0), 2v2 on a 4-slot map, 12v1 (13 peers): equal hashes;
  the member's move changes the hash. Not checked against native (needs 3+ real clients).
  Left: team chat filter dialog (teamfltr.bin 0x490570), "[team]" chat names, 0x4a3010 team
  result, lobby seat races (0x486040 / 0x471d00), gluChat team slots.

## Lobby (gluChat) and map transfer (2026-10-27)

- Storm turns over snet.rs (Ordinal 128 / 122 / 115): the host is the turn server: each
  player's turns queue at the host, a round (one turn of every player in the game) goes out
  once all are there; a joiner counts from its NAME message on (that is also when the host's
  Joined event fires: Storm knows names at join). Control kinds 0x80.. (live.rs's 1.. share
  the link after the start). Names (Ordinal 113), drops (106), game mode (107 / 130: bit 2
  hides the game from the LAN list).
- glue_lobby.rs: the turn exchange (moved from glue_ready.rs; network-aware), 0x486040 lobby
  commands 0x3c .. 0x5b, net_chat_cmds.cpp senders / pending buffers (0x6d5c30 .. 0x6d5c94,
  flushed by 0x471a50 in its order), 'I' .. 'P' messages (0x470a30), 0x4d4130 / 0x4d4340 /
  0x4f40f0 / 0x4719d0 (countdown: 24 turns, 0x48 seed + slot order from the creator),
  0x4c4980 / 0x4c3c90 handlers. glue_chat.rs: the gluChat screen (rows by force or in
  order, owner / race combos, chat list + entry, description, countdown).
- The game the lobby starts runs on the rollback Session over the lobby's transport
  (`lobby_net_game`, delay 2); Session::map_humans maps Storm ids to the slots.
- Map transfer ('O', 0x4eafe0 sub-messages, our wire): 1 offer (id, size, CRC32, name) sent
  on Joined, 2 request from an offset, 4 data (`O 4 id 0 off u32 len u16 bytes`, 499-byte
  chunks, 8 in flight), 5 ack, 0 state (1 = has it). The joiner checks maps\ and
  maps\download\ for the name + CRC first; what arrives stays in memory
  (`Setup::map_data`, opened by Mpq::new: browsers too). Start (0x452460 / 0x472a70)
  waits for every member. 0x3d carries the percent (shown in the row's fourth box).
- Test: two `scbw glue` processes with SCBW_GLUE_REALTIME=1 (waits take real time),
  SCBW_LAN_PORT, SCBW_NET_HEADLESS=300 (the started game runs headless), scripts: main "t m",
  Enter (expansion), "t o", New ID "t name; k 0d", login Enter, join "t o" / create "t g",
  "t o"; the creator's Ok is "c 500 393" (the entry takes the keys). Checked: join, slots,
  chat both ways, start -> equal hashes at 300; a joiner whose install has no maps folder
  downloads the map and plays (equal hashes).
- Join by code (ours: 1.16.1 lists LAN / Battle.net games only): the creator prints
  `LOBBY ip:port#hash`; the host sends a joiner the game's name + stat string (kind 0x86) so
  `Glue::join_by_code` can make the description. Native: `SCBW_JOIN=<code> SCBW_NAME=<n> scbw
  glue`. Browser: the page's join field / `?join=<code>&name=` (net_join sees a Start with no
  setup and runs `glue::web_frame`: the glue in the browser from the lobby on, the game inside
  it). Game turns that reach a peer while its lobby still owns the link are kept for the game
  (snet `early`); `Transport::set_first` lifts the lobby's ack counters to the first frame.
  Browser datagram send errors are not fatal (they had cleared the lobby's player table).
- `agent/tools/lobby.sh [web]`: creator glue + joiner glue with no maps folder (or headless
  Chromium): equal hashes at 300 (2026-10-27, both).
- Not done: slot row details of 0x450d90 (ban, team seats, Make Game Public), the minimap
  preview (0x4a5d10), saved network games (0x6d1218), the ready room's network path in a
  window, team / UMS force moves (0x42 / 0x43) from the UI, slash commands in the lobby,
  0x4a3010 when the lobby stalls 20 s, an in-lobby "copy join code" UI (the code is on
  stdout), the browser front end from the title screen (it starts at the lobby).

## Alliance dialog in games (2026-10-27)

Minimap button 4 outside a replay: 0x4913d0 takes the local alliances (0x63ff64) and shared
vision; rows 0x490300 (other players; computers only in Team CTF; boxes off with locked
alliances / computers), in a team game rez\teamfltr.bin with 0x490570 (a row per team
without a computer, member names in labels -30..); allied victory 0x490c40; Accept 0x490fe0
sends 0x0e (2 bits per player) and 0x0d. Checked in a window shot (melee rows); the team
layout opens but its member labels did not show in a SCBW_TEAMS shot: not checked vs native.
The message dialog (minimap button 3, 0x48ff10 recipients) is still not ported.

## Enhanced games (ours, 2026-10-27)

Full hand-off design: [../plans/enhanced.md](../plans/enhanced.md). Summary: create-game
option (description byte +0x2f bit 7); join a running game by code (`SCBW_JOIN`, `?join=`);
joiner picks an army (`SCBW_ARMY`), a taken army makes a second human on it (shared
control); a leaving army with no human goes to the AI (`ai_take_over`, melee script), a
joiner can take it back. Join = command 0xe0 (`src/bw/enhanced.rs`); every peer reloads
through a 1.16.1 save at the join frame so the state equals the joiner's load. State sent
as C_LATE control parts. Drops wait 3 s (no dialog). Test: `agent/tools/enhanced.sh`
(`QUIT_AT`, `ARMY`). Host leaving ends the game. Open: LAN list advert for running games,
browser late-join test, visible "AI took army" line, window check.
