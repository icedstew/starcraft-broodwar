# Computer AI (src/bw/ai*.rs)

- ai.rs: requests heap, budget, player tick, town tick. ai_town.rs: towns, worker /
  building rounds, requests, supply / gas, unit removal. ai_place.rs: building spot
  search 0x4465c0. ai_areas.rs: resource areas 0x446350 / 0x445610. ai_region.rs: region
  state machine 0x43fc60, targets 0x431990, military moves 0x43d5d0, region flood
  0x437e70.
- Check: `SCBW_MELEE="1,1" SCBW_AI="1" agent/tools/cmp.sh "$INSTALL/maps/(2)Boxer.scm"
  "" "" 8000` must MATCH.
- attack.rs: AI target choice 0x477160 `ai_choose_target(u, p3, p4)` (its stack arg 2
  only feeds 0x476fc0's siege part; dropped), 0x43b790, 0x431de0 / 0x431e90 / 0x431ee0.
  ai_military.rs: reaction to attack 0x43f320 `ai_react_to_target`. ai_region.rs:
  0x43c580 support call, 0x462ea0 guard return. ai_town.rs: units joining the AI
  0x4a2830 / 0x435700 / 0x435770 / 0x435db0 / 0x43e280 / 0x462960.
- Interceptor parent is the union field +0xc0 (`ptr_at(0xc0)`), not `connected_unit`
  (+0x80).
- PlayerAi `flags` is at +0x218 (0x690100 for player 0).
- ai_attack.rs: attack planning 0x43abb0 `ai_choose_attack_region` (stack: player,
  x, y, force, retry; 0x437770 returns region index + 1), 0x43b0b0, 0x43b570, all-in
  0x43e050 (AL = mode). ai_military.rs: region / guard / defense requests. Script
  opcodes 0x0d / 0x0e / 0x64 / 0x49 / 0x42 take the thread's `center` (+0x24 / +0x28).
- 0x4a2450 per-type AI: the arbiter case returns 1 after its order (asm 0x4a274a).
- Request value of type 2 (guard) is the raw guard index (0x448630), not `ai_ref`.
- Out-of-range dat reads (e.g. cost of the empty queue slot 0xe4): the units.dat array
  table at 0x513c30 (12-byte entries: address, element size, count; in units.dat field
  order) tells which array the read lands in (mineral_cost[0xe4] = ai_attack_move's
  first u16, gas_cost[0xe4] = armor's); see `ai_pending_build_costs`.
- `u8_at` / `ptr_at` are the +0xc0 union only; +0x93 is `order_flags`.
- Left: see status.md "In flight" (AI todo_port! list) and `--missing ai`.
