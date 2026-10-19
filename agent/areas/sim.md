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
- Trigger kill / remove for player groups (0x4c8570 via 0x45f9a0 / 0x45fa10 / 0x45fa80 /
  0x45faf0 / 0x45f930): resolved 2026-10-19 from the disassembly: 0x4c8570 loads EBX with
  its count argument ([ebp+0xc]) before each call (Ghidra shows it as unaff_EBX), and the
  loops also pass EBX to the callback. The callback 0x4c8570 returns 0 and the count is
  never 0 (0 -> 999999 at 0x4c888e), so the early exit never fires: every player of the
  group is hit, as `for_group` (triggers.rs) does.

## Replays vs the real game (not the emulator)

The emulator harness (re/emu_game.py) can share our mistakes in what it sets up around
game_step. Check against the real 1.16.1 itself: `NATIVE_MODE=trace CAP_START=999999
CAP_END=<n> NATIVE_CAMERA=<shots>/camera.txt re/native_shots.sh <rep> <dir>` writes its
per-frame rng ("f<n> rng <seed> <draws>"; CAP_C100=1 adds the 100-frame counter,
CAP_SITES=1 with a CAP_START window the per-site counters 0x51c610); `SCBW_TRACE_START=
999999 scbw reptrace <rep> <n> <out>` writes ours. First differing line = divergence.
- Fixed 2026-10-18: 0x4d9950 sets 0x51ce98 = 1 before the two pre-loop steps (vision
  resets one frame later than we had). tl1412 / 1347 / 1341 now match the real game's
  rng to the end (they diverged at f16798 / 26183 / 33015: a unit at a sight edge at a
  reset frame).
- Fixed 2026-10-18: Infesting / InfestedCommandCenter (0x4ea290 / 0x4ea4c0) count on
  +0xac (remaining build time), not the order timer: tl1313 matched to f62308.
- Open: a replay's leave command 0x57 (tl1313 f62308, tl1309 f83856, tl1363 f32442):
  ours sends the leaver's units to Guard, the real game draws far less.
- Debug: reptrace SCBW_TYPES_AT=<f> (unit index / type / player / order, visible and
  hidden lists), SCBW_RNGLOG_AT=<f> (each draw's site).

## Unit lists (1.16.1)

Visible 0x628430 / 0x59cc9c, hidden 0x6283ec / 0x628428, free 0x628438 / 0x62843c.
Helpers: 0x49dca0 / 0x49dce0 (insert after head of visible / hidden), 0x49dea0 /
0x49de50 / 0x49de00 (unlink from visible / hidden / free). list.rs implements them
generically (documented with SC:R x64 addresses).
