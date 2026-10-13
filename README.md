# scbw

Open-source reimplementation of StarCraft: Brood War in Rust.

**No game assets are included or distributed.** You must own StarCraft (Remastered,
free from Battle.net). scbw reads the assets from your local install at runtime.

Pure Rust, no platform-specific code: builds for any target Rust supports.

## Usage

    cargo run --release -- view "/path/to/StarCraft/Maps/(4)Blood Bath.scm"   # play a melee start
    cargo run --release -- render "/path/to/map.scm" out.png                  # full-map terrain PNG

`view` runs the faithful engine below. Melee maps start you (slot 0, `SCBW_RACE` =
terran/protoss/zerg, default terran) with a town hall and four workers; other start
locations get idle computer players. Controls: left-click / drag selects (Shift adds or
removes, Ctrl or a double-click picks the same type on screen), right-click moves,
attacks, gathers or sets rallies (Shift queues), Ctrl+0-9 / 0-9 / Shift+0-9 set, select
and add to hotkey groups, the command card and hotkeys M/A/S/H/P/G/C issue Move,
Attack, Stop, Hold, Patrol, Gather and Return Cargo, and the Build menu places
buildings; arrows or the screen edge scroll, Esc quits.

Set `SCBW_INSTALL` to the directory containing `.build.info` and `Data/` (defaults to the Lutris/Wine path).

## Layout

- `src/casc.rs` – local CASC storage reader (SC:R asset store)
- `src/mpq.rs`  – MPQ reader (maps)
- `src/map.rs`  – CHK parsing, tileset terrain rendering
- `src/dat.rs`  – arr/*.dat tables (layouts recovered from the binary)
- `src/grp.rs`  – GRP sprite decoding/drawing
- `src/iscript.rs` – iscript interpreter, images, sprites, BW RNG
- `src/replay.rs` – replay parsing
- `src/view.rs`, `src/ui.rs`, `src/audio.rs` – map viewer, HUD, sound
- `src/bw/` – faithful engine port (see below)
- `src/game/` – older approximate game layer (still used for the command card button tables and the `walk`/`path` debug commands)
- `re/` – reverse-engineering tools and notes (see re/NOTES.md)

## Faithful engine (`src/bw/`)

Game logic ported function by function from the classic 1.16.1 StarCraft.exe (same
logic as Remastered, but unprotected), with each routine annotated by its 1.16.1
address. Ported so far:

- Core: BW intrusive lists, pools (in 1.16.1 allocation order), sync RNG, iscript
  interpreter, images/sprites/thingies, x86 CUnit layout, frame loop
  (`game_step`, unit updates, timers, secondary orders).
- Units: `create_unit` chain, unit finder, player tables, finish/show/place, order
  queue, main order dispatch (Stop, Guard, Move, HoldPosition, ...).
- Map: tile flags, minitile walkability, CHK `MTXM` and `UNIT` sections.
- Pathing: SAI region creation (regions, split tiles, contours), region A*,
  waypoint search with contour walkers, path pool and path requests, units as
  path obstacles.
- Movement: flingy predict/apply, movement state machine, unit/terrain collision,
  collision resolution, sidestepping, predicted-blocker search, slide direction,
  unstacking.
- Vision and fog of war, map sprites (THG2 doodads).
- Combat: AttackUnit, auto-targeting and target priorities, weapons and bullets,
  damage, retaliation, death.
- Economy: worker mining and gas, training (build queue, supply, costs), Terran
  construction (placement checks, PlaceBuilding, ConstructingBuilding).
- Commands as the game receives them: right-click actions, targeted orders with
  group formation, stop, hold, return cargo, train, build, with order and unit
  requirement checks and Shift-queueing.
- Melee starting units.
- Interface logic: minimap image, drag-box and click selection (pixel-exact unit
  picking), selections and hotkey groups as the simulation keeps them, select
  commands, building placement box.

Not yet: Protoss/Zerg building, zerg larvae and creep, add-ons, research and
upgrades, spells, transports, triggers, AI, networking.

## Verification

Ports are checked against the original by differential testing: `re/emu.py` and
friends run 1.16.1 code under Unicorn (with Storm shims) and compare its output
byte for byte with the Rust port.

- Tile flags, minitiles, SAI regions and contours: exact on 40 maps.
- Region A* and full path searches: 8,000 searches exact.
- Whole game: `re/emu_game.py` runs 1.16.1 game init and frames; unit, sprite, fog,
  bullet and resource traces match `scbw bwsim` frame for frame in 42 scenarios:
  group movement, fights, mining and gas, training, construction, player commands
  and melee starts.

The 1.16.1 executable and decompiler output are not part of this repository.

## Status

Working: asset loading (CASC, MPQ maps, replays), terrain, sprites/iscript animation,
shadows/remaps/player colors, console HUD, minimap, cursor, selection, sound/music;
`view` plays on the faithful engine: move, fight, mine, train and build (Terran).

Not yet: see the engine list above.

### Port coverage

Share of the 4,644 1.16.1 functions that are ported. A function counts as ported when
`src/` has a `/// 0x...` doc comment with its address, and as todo when only a
`todo_port!` names it. The input is a Ghidra export of StarCraft.exe, which is not
part of this repo:

```sh
python3 -I re/coverage.py <decomp.c> . | tail -5      # summary: in scope / reachable from game_step
python3 -I re/coverage.py <decomp.c> . --html cov.html  # per source file, as HTML
```

Summary as a table (`column -N` is util-linux):

```sh
python3 -I re/coverage.py <decomp.c> . | sed -nE \
  's/^all functions: ([0-9]+) functions, ([0-9]+) ported \(([0-9]+%)\).*/All functions|\1|\2 (\3)/p;
   s/^in scope: ([0-9]+) functions, ([0-9]+) ported \(([0-9]+%)\).*/In scope|\1|\2 (\3)/p;
   s/^reachable from (.*): ([0-9]+) functions, ([0-9]+) ported \(([0-9]+%)\).*/Reachable from \1|\2|\3 (\4)/p' \
  | column -t -s'|' -N Scope,Functions,Ported
```

## Commands

- `view <map|chk|rep>` – map viewer
- `shot <map> out.png x y [frames] [n_select]` – headless screenshot
- `render <map> out.png` – full-map terrain PNG
- `rep <file.rep>`, `dat`, `units`, `ls`, `extract <path> <out>`, `walk`, `path`
- `bwsim <frames>` – run the faithful engine headless. Env: `SCBW_MAP` (map file),
  `SCBW_UNITS="type,x,y,player;..."`, `SCBW_MOVE` (`;`-separated, cycled over the
  created units: `x,y` move, `@k` attack unit k, `o<order>:<x>,<y>` / `o<order>:@k`,
  `t<type>[x<n>]` train, `b<order>:<type>:<tx>,<ty>` build; group commands with all
  player-0 units selected: `R<x>,<y>` / `R@k` right-click, `T<order>:...` targeted
  order, `q` suffix queues), `SCBW_MELEE=race,...` (melee start),
  `SCBW_TRACE=<file>` (per-frame trace).
- `mapdump <map> <dir>` – dump tile flags, minitiles, SAI and contours for diffing.
- `export <dir> <paths...>` – copy CASC data (e.g. `arr/ scripts/ unit/`) for the
  emulator harness.
