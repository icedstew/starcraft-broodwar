# Computer AI (src/bw/ai*.rs) — paused by the user

- ai.rs: requests heap, budget, player tick, town tick. ai_town.rs: towns, worker /
  building rounds, requests, supply / gas, unit removal. ai_place.rs: building spot
  search 0x4465c0. ai_areas.rs: resource areas 0x446350 / 0x445610. ai_region.rs: region
  state machine 0x43fc60, targets 0x431990, military moves 0x43d5d0, region flood
  0x437e70.
- Check: `SCBW_MELEE="1,1" SCBW_AI="1" agent/tools/cmp.sh "$INSTALL/maps/(2)Boxer.scm"
  "" "" 8000` must MATCH.
- Left: guard / region / defense requests (0x4630c0, 0x43f990, 0x43ebb0, 0x43aae0),
  attack timers 0x43b9e0, target choice 0x477160, military AI 0x43d910, script opcodes,
  AI orders AtkMoveEP 0x478de0, HarassMove 0x478ec0, GuardPost 0x4778e0, ComputerReturn
  0x478490, AIPatrol 0x4a1d80.
