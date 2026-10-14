# 1.16.1 reverse-engineering notes

Facts learned while porting, with addresses (1.16.1 StarCraft.exe, image base 0x400000;
storm.dll base 0x15000000). Long-standing notes are also in `re/NOTES.md`.

## General

- Units: array 0x59cca8, stride 0x150; handle = `unique_id << 11 | (index + 1)`.
  Field offsets are in `src/bw/unit.rs` comments (first number = 1.16.1 offset).
- Player table 0x57eee0, 9 dwords (0x24) per player: +0 player id, +4 storm id,
  +8 type (1 computer, 2 human, ...), +9 race (0 zerg, 1 terran, 2 protoss, 6 random),
  +0xa team, +0xb name. Minerals 0x57f0f0, gas 0x57f120 (u32 per player).
- Current command player 0x512678 / 0x51267c (reset from 0x512684 / 0x512688 = local).
- Frame counter 0x57f23c (`st.frame`); it advances in 0x4d9670 before `game_step`
  0x4d94b0.
- Sync RNG: seed 0x51ca14, total draws 0x51ca18, per-site counters from 0x51c610
  (site = (counter addr - 0x51c610) / 4; e.g. 0x51c65c = site 0x13). Enabled flag
  0x6d11c8. `seed_rng` 0x4dc4e0, `sync_rand` 0x4dc4a0.
- Alliances 0x58d634 (12x12 bytes), `is_enemy` in scbw; owner 0xb means "use the sprite
  owner".
- units.dat flags 0x664080 (bit 1 building, 0x10 subunit/turret, 0x20000 ...); attack
  order per type 0x663320 (`ai_attack_unit` in dat.rs); air weapon 0x6616e0; weapons
  min range 0x656a18, attack angle 0x656990; sight range 0x663238.
- Ghidra hides register args: `in_EAX`, `unaff_ESI`, `unaff_EBX`... check `dis.sh`.

## Game start and replays

- Game init 0x4ef100: if replay (0x6d0f14) the seed 0x51ca10 = replay header +8
  (0x6d0f38); else time(). Then seed 0x51ca14 = 0x51ca10, srand(seed), map/game setup
  (0x4a9a30, replay players 0x4de9a0, 0x49cc40, melee start 0x49da40 mode 0x596871 = 2).
- Replay header (0x279 bytes) is loaded at 0x6d0f30: +0 engine (1 = Brood War), +1 frame
  count (0x6d0f31), +8 seed, +0x18 title, +0x34/+0x36 map size, +0x3c game type
  (2 melee, 10 top vs bottom), +0x48 host, +0x61 map name, +0xa1 the 12-entry player
  table exactly as 0x57eee0, +0x251 colors.
- Replay commands carry the **storm id**; 0x487100 maps it to the player through
  0x57eee4 (+4 of each player entry) and runs every command whose frame <= counter
  through 0x4865d0 with flags 0x6d11c8 = 1 (RNG on), 0x6d5bec = 1, 0x6556e8 = 0, before
  the counter increments.
- Command executor 0x4865d0: EAX = command bytes, [esp+4] = length, [esp+8] = 1 (when
  0 during a replay, commands are filtered). After each command it records it through the
  replay writer at [0x596bbc] (0x4cde70; returns at once when [writer] == 0).
- CHK load for replays: 0x4bf520 -> 0x4cc060 checks the VER section against a table
  (0x500560..0x5005d8); Brood War versions require 0x58f440 (Brood War game) != 0.
- Image GRPs: 0x47ae30 loads 0x2f3 (755) images, or 999 when 0x6d11e4 (expansion data)
  is set. With 755, BW doodads/units have null GRPs and crash 0x4d57b0.
- Map-placed unit state bits (CHK UNIT +0x1a with valid mask +0xc): 1 cloaked (0x491a50),
  2 burrowed (0x4ccf20), 4 lifted/in transit (0x4cc470), 8 hallucinated (0x4f6420),
  0x10 invincible.

## Game start (verified against the real game's memory, tl1412)

- 0x4ef100 seeds the RNG (0x51ca14 = 0x51ca10, which for replays is header +8) twice; the
  second time right before 0x4eee00 loads the scenario. CHK units (0x4bf520) and the melee
  start (0x49da40) are created with the RNG **on**: 133 draws each at create_unit's sites
  0xe / 0xf on Python 1.1.
- 0x4d9950 -> 0x4d9530 runs `game_step` **twice** (plus UI refresh 0x4d93f0) before the
  frame loop; the frame counter stays 0. So frame f of the loop is the f+3-th step.
- Replays: 0x4ef100 ends with vision 0x57f0b0 = 0xff, explored 0x57eeb8 = 0xff00,
  0x6d0f18 = 0xff, 0x6d0f1c = 0. All eight players have 50 minerals (empty slots too).
- With these, re/emu_game.py's replay mode equals the real game frame for frame
  (`~/games/Starcraft/broodwar-reps-116/shots/tl1412/native/trace.txt`, frames 0-1500,
  captured by `NATIVE_MODE=trace re/native_shots.sh`).

## Orders ported this session

- 0x4f6420 make hallucination: counters -1 (0x488bf0(-1), 0x488d50(ecx=-1, score 1)),
  button set 0xf4 unless disabled (status 0x400 / lockdown / stasis / maelstrom) and not
  a building, status |= 0x40000000, counters +1 (no-ops now), images -> draw function
  0x10 (0x497dc0, replay or own unit), remove timer 0x546, AI strength (todo), secondary
  order 0x95, recurse into subunit; carrier/reaver children todo (0x466170).
- 0x475710 kill unit (`kill_unit`): used when the remove timer runs out and for
  self-destructing bullet sources.
- Die 0x479480, hallucination branch: sound 0x26b (0x48ed50, EBX = sound, ESI = unit),
  thingy 0x488210(0x176, x, y=EDI, owner 0), its sprite elevation = unit's + 1 and
  visibility = unit sprite's (0x497480), then 0x4e6340 (hide), subunit, 0x4a0740
  remove, 0x497b40 free sprite.
- AttackMove 0x479040: order id = unit +0x4d. 0x478370(order) moves to the order position
  (state 0 -> 1, next target waypoint = position) and, if the auto target (+0x7c) is a
  visible non-invincible enemy, halts, queues `order` again in front (0x474af0: insert
  before head with type 0xe4, or append), queues the unit's attack order (0x663320) on
  the target before the head (0x4747e0 -> 0x474540, type 0xe4), clears +0x7c, starts the
  next order (0x475420) and sets status 0x8000000 on unit and subunit (0x4026a0). Then, if
  the main order timer (+0x54) is 0: timer = 15; if the move target equals the position
  -> 0x4753a0 (done); else 0x477820: if acquisition range (0x476000, low byte) != 0 and
  not AI, the best target 0x443080 gets the same treatment. (The `status & 0x80000`
  test in 0x479040 is always true: setne + inc.)
- HoldPosition 0x478d10: state 0: halt (0x4eb290), next target waypoint = move target,
  status 0x8000000 (and subunit), state 1. Then 0x478b40 (attack in place) else, when the
  main order timer is 0: timer 15, order target = 0x442fc0, order process timer 0 when
  found. When attacking and the unit has no turret subunit (or is type 3 / 0x11): next
  target waypoint = order position.
- 0x478b40 attack in place: needs an order target; 0x477510 pick better target;
  0x4e5db0 target visible; 0x476730(1) can attack; order position = target position;
  turret subunit -> true; weapon/slot by target air status (0x6616e0 / 0x475ad0);
  cooldown (+0x55 ground, +0x56 air) running -> clamp order process timer (+0x85), true;
  flingy flags (+0x20) & 8 -> true; distance 0x430d30 (ECX = target, EAX = unit) vs min
  range / 0x475870 range -> false when outside; facing: |heading (+0x21) - direction to
  target| (0x495300) > attack angle -> if status 0x10000 then timer 0 and true, else
  false; status 0x8000 -> false; in bunker (status 0x20) 0x477fd0 muzzle flash (todo);
  fire: flingy flags |= 8, 0x64deb0 = 0, cooldown = 0x475dc0 + (rand site 0xd & 3) - 1
  into both cooldowns, images play anim 4 + slot (0x4d8470).
- 0x442fc0 target in range: acquisition range * 32 (+0x40 in a bunker), finder box
  +-(range + 0x40), filter 0x442da0 with min range 0x6955dc = 0, buckets 0x6955ec by
  priority; 0x4401b0 picks a random entry (rng site 0x13) when the RNG is on, else the
  first. 0x443080 instead uses 0x440520 for the min range, AI sight range, and the
  nearest (0x4405e0).
- 0x4e5db0: order target visible to the owner (status 0x300 + detection mask +0xe4),
  or, without a target, the order position's tile is not fogged for the owner.
- 0x430d30 box gap: ECX = unit whose plain box is used, EAX = unit adjusted for turrets.

## Screen and drawing (for native captures)

- Draw: 0x41ca00 calls 0x41e280 (layers) at 0x41cb23; return address 0x41cb28. Game
  screen buffer pointer [0x6ceff4] (640x480 8-bit, map area; HUD drawn separately),
  palette 0x6ce320 (256 PALETTEENTRY), screen position 0x62848c (x) / 0x6284a8 (y).
- Status/console UI facts from earlier sessions: dialog control helpers 0x4258b0 (text +
  show), 0x457200 (value + show), 0x418700 (hide), 0x4186a0 (show), 0x457250 (show
  range), 0x457310 (hide after -7); status dispatcher 0x458120 with per-type table
  0x5193a0 (type, condition, draw): generic 0x424980/0x426f50, buildings
  0x425180/0x427890, eggs 0x427260, transports 0x427c90/0x427d30, carriers 0x4274a0,
  specials 0x425ee0; multi-selection 0x425960. Control ids: -5 name, -7 hp text,
  -8 energy, -20 status line, -21 kills, 9..12 info buttons.

## Assets (1.16.1 install)

- MPQ priority: Patch_rt, BrooDat, StarDat, BroodWar, Starcraft (`src/storage.rs`).
- storm.dll compression (SCompDecompress 0x1501d160): implode (8), Huffman (1, tables
  0x15050030 + type * 0x102), stereo/mono ADPCM (0x80/0x40; steps 0x150509c8, index table
  0x15050948). Ported in `src/storm.rs` reading the tables from the user's storm.dll.
- Fonts are encrypted (grid.cpp, `src/grid.rs`): key = decrypt(`font\font.gid`, key1)
  where key1 = 19 nonzero MSVC rand bytes after srand(0x150b) (0x480a40); decrypt 0x4818c0
  with init 0x4816b0 = IDEA (Colin Plumb's code on little-endian words; expand 0x44e420,
  invert 0x44e780, mulinv 0x44e4a0, cipher 0x44e550) + a SHA-0 transform 0x42ae90 whose
  rotates use `sar` (sign smearing). Verified byte-exact against the game.
- Replays from 1.16.1 are "legacy": PKWare-imploded section chunks (`src/replay.rs`).
