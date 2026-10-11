# scbw

Open-source reimplementation of StarCraft: Brood War in Rust.

**No game assets are included or distributed.** You must own StarCraft (Remastered,
free from Battle.net). scbw reads the assets from your local install at runtime.

Pure Rust, no platform-specific code: builds for any target Rust supports.

## Usage

    cargo run --release -- view "/path/to/StarCraft/Maps/(4)Blood Bath.scm"   # arrows/WASD scroll, Esc quits
    cargo run --release -- render "/path/to/map.scm" out.png                  # full-map terrain PNG

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
- `src/game/` – older approximate game layer, being replaced by `src/bw/`
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
  collision resolution, sidestepping, predicted-blocker search, slide direction.

Not yet: placement ring search for overlapping spawns, unstacking units stuck in
terrain, doodads (THG2), the rest of game init, combat and bullets, remaining
orders, triggers, AI, networking.

## Verification

Ports are checked against the original by differential testing: `re/emu.py` and
friends run 1.16.1 code under Unicorn (with Storm shims) and compare its output
byte for byte with the Rust port.

- Tile flags, minitiles, SAI regions and contours: exact on 40 maps.
- Region A* and full path searches: 8,000 searches exact.
- Whole game: `re/emu_game.py` runs 1.16.1 game init and frames; unit traces match
  `scbw bwsim` frame for frame in eight movement scenarios (up to 12 units, head-on,
  crossing, converging groups, marines/zerglings/zealots, 300–500 frames).

The 1.16.1 executable and decompiler output are not part of this repository.

## Status

Working: asset loading (CASC, MPQ maps, replays), terrain, sprites/iscript animation,
shadows/remaps/player colors, console HUD, minimap, cursor, selection, sound/music;
faithful engine core, pathing and ground movement (above).

Not yet: combat, command card, triggers, AI, networking.

## Commands

- `view <map|chk|rep>` – map viewer
- `shot <map> out.png x y [frames] [n_select]` – headless screenshot
- `render <map> out.png` – full-map terrain PNG
- `rep <file.rep>`, `dat`, `units`, `ls`, `extract <path> <out>`, `walk`, `path`
- `bwsim <frames>` – run the faithful engine headless. Env: `SCBW_MAP` (map file),
  `SCBW_UNITS="type,x,y,player;..."`, `SCBW_MOVE="x,y"` or `"x,y;x,y;..."` (per unit,
  cycled), `SCBW_TRACE=<file>` (per-frame unit trace).
- `mapdump <map> <dir>` – dump tile flags, minitiles, SAI and contours for diffing.
- `export <dir> <paths...>` – copy CASC data (e.g. `arr/ scripts/ unit/`) for the
  emulator harness.
