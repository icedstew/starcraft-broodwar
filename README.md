# scbw

Open-source reimplementation of StarCraft: Brood War in Rust.

**No game assets are included or distributed.** You must own StarCraft (Remastered,
free from Battle.net). scbw reads the assets from your local install at runtime.

Pure Rust, no platform-specific code: builds for any target Rust supports.

## Usage

    SCBW_INSTALL="/path/to/StarCraft" cargo run --release -- render "Maps/(4)Blood Bath.scm" out.png

`SCBW_INSTALL` is the directory containing `.build.info` and `Data/`.

## Layout

- `src/casc.rs` – local CASC storage reader (SC:R asset store)
- `src/mpq.rs`  – MPQ reader (maps)
- `src/map.rs`  – CHK parsing, tileset terrain rendering
