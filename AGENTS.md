# Agent notes

## Test data (local, not in the repo)

- Game install (1.16.1): `~/Downloads/StarCraft-BroodWar_1.16.1/` (default `SCBW_INSTALL`;
  its `maps/` has the Blizzard melee maps).
- Replays: `~/games/Starcraft/broodwar-reps/` (9 PvZ games, recorded by SC:R:
  they use the extended commands 0x60–0x63).
- Their map: `~/games/Starcraft/broodwar-maps/Polypoid 1.65.scx`.
- 1.16.1 replays: `~/games/Starcraft/broodwar-reps-116/` (15 pro/iCCup games from
  Mar–May 2009, after 1.16.1's January 2009 release; from https://tl.net/replay/, ids in
  the file names). Legacy format (PKWare-imploded sections); each embeds its map's
  `scenario.chk`, so `view`/`shot`/`render` take the `.rep` directly as the map.

```sh
cargo run --release -- rep ~/games/Starcraft/broodwar-reps/<file>.rep     # header and command summary
cargo run --release -- view ~/games/Starcraft/broodwar-reps/<file>.rep    # watch it
```

## Rules

- Never commit game assets, the 1.16.1 executable, decompiler output or dumps.
- Port 1.16.1 logic exactly, annotated with `/// 0x...` addresses; no simplified stand-ins.
