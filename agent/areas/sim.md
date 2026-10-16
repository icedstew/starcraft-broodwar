# Simulation (src/bw/*)

Read when working on game_step / command dispatch logic. Facts here are durable; what
is in flight lives in ../status.md.

## State of the port

- **Replays vs the emulator** (whole replay, trigger state and unit AI strengths traced):
  tl1412, 1359, 1363, 1346, 1347, 1352, 1366, 1339 MATCH to the end; tl1361 (UMS) MATCH;
  tl1330, 1340 MATCH to f30000 (after 53cd1f6); tl1412 / 1346 MATCH to f40000.
  Not yet run on the current binary: tl1341, 1349, 1354, 1356.
- The emulator runs 0x437a70(1), 0x446350 (AI areas) and 0x48a460 (triggers) at game
  start for replays and the SCBW_AI scenario, like 0x4eee00.
- **Triggers** (triggers.rs): engine, every condition, nearly every action; Melee.trg etc.
  from the game type; UMS sections.

## Gaps

- Pickup4 0x4e7b70 and hidden order 0x60 need AI target choice 0x477160 (AI, paused).
- Commands on the `todo_port!` arm of 0x4865d0 (UI / net): 5 keep-alive, 6/7 save/load,
  8 restart, 0x10/0x11 pause, 0x12 cheats, 0x37 sync, 0x39 squelch, 0x55/0x56,
  0x58 minimap ping, 0x5c chat.

## Open investigation: late-game divergence vs the real game

User report "desync around f17044, goliaths?": scbw == emulator with and without the p0
camera through f18000, but native screenshots differ from ~f17040 (unit positions; the
factory at 2464,176 gets a solid-black shadow natively). Native vision is one frame
later than the emulator from f16797. Next: `CAP_START=16700 CAP_END=17400
NATIVE_MODE=trace re/native_shots.sh` and diff against the emulator's trace.

## Unit lists (1.16.1)

Visible 0x628430 / 0x59cc9c, hidden 0x6283ec / 0x628428, free 0x628438 / 0x62843c.
Helpers: 0x49dca0 / 0x49dce0 (insert after head of visible / hidden), 0x49dea0 /
0x49de50 / 0x49de00 (unlink from visible / hidden / free). list.rs implements them
generically (documented with SC:R x64 addresses).
