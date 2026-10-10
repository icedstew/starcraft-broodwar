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
- `src/view.rs` – map viewer (window + headless screenshot)
- `re/` – reverse-engineering tools and notes (see re/NOTES.md)

## Status

Working: asset loading (CASC, MPQ maps, replays), terrain, sprites/iscript animation,
shadows/remaps/player colors, console HUD, minimap, cursor, selection, sound/music.

Not yet: game simulation (unit movement, orders, combat, pathing), command card,
triggers, AI, networking. Game logic is being reverse-engineered from the
StarCraft: Remastered binary; see `re/NOTES.md`.

Commands: `view <map|chk|rep>`, `shot <map> out.png x y [frames] [n_select]`,
`render`, `walk`, `rep <file.rep>`, `dat`, `ls`, `extract <path> <out>`.
