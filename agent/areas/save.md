# Save / load (saveload.cpp)

Started 2026-10-18 (user: "do the deferred functions, including saving/loading").

## File layout (1.16.1, `.snx` single player / `.mlx` multiplayer BW)

0x4d02d0 writes, in order:
1. Text up to a 0x1a byte (0x4ce950: the file's description), then u32 0x690003 and the
   0xb5-byte header (0x4cf160: u16 0x69, u16 3, seed 0x51ca14, the game info 0x5967f8).
2. Packed (0x4c3450) 0x57eee0 (0x1b0, player table), 0x57f0f0 (96000 = 0x17700, game
   data), raw u32 0x512688.
3. The game state, `State::save_state` (src/bw/save.rs), read back by 0x4cfef0 from game
   init (0x4ef100 -> 0x4eed10). Parts 1-2 are read by 0x4cf5f0 when the save is opened.

"Packed" = 0x4c3450 / 0x4c3280: crc32 (0x410430), chunk count, per 0x2000-byte chunk its
stored size + bytes (imploded, or as is when that does not shrink it). `save::Out::packed`
/ `save::In::packed`. Implode 0x410b30 in src/bw/implode.rs (binary, 1 KB dictionary):
re-imploding 1.16.1 replay chunks gives the same bytes (118/118 checked 2026-10-19).

## Code

- `src/bw/save.rs`: `Out` / `In`, `State::save_state` / `load_state` (the order of
  0x4d02d0 / 0x4cfef0). One file per part under `src/bw/save/`: images, sprites,
  thingies (fog thingies), flingies, units, bullets, orders, tiles, triggers, ai, misc
  (CHK, selections, prepare / restore, tail).
- Each part: `save_x(&mut self, d, o)` writes exactly the bytes 1.16.1 writes for the same
  game state; `load_x(&mut self, d, i)` reads them into the Rust state and rebuilds what
  1.16.1 rebuilds. Native pointers are stored as the native savers store them (usually a
  1-based index, 0 = null); Rust slot indices / `NIL` convert to those. A native field the
  Rust state does not keep: write what 1.16.1 would hold if it can be derived, else 0 with
  a `// not kept: ...` comment (porters: report these; the reviewer lists them under Gaps).
- Test: `scbw savetest <rep> <frame> [frames]`: save at <frame>, load into a fresh game of
  the same replay, resave (bytes must be SAME) and play on (traces must MATCH).

- Front end: `src/saveload.rs` (paths 0x4ce5b0 / 0x4cee60 / 0x4cf2a0, text 0x4ce950, header
  0x4cf160 / 0x4cf0f0, head 0x4cf5f0, open 0x4cf8e0 / 0x4cf950); in-game dialogs
  rez\savegame.bin / loadgame.bin in src/menu.rs (0x462600 / 0x462100, Actions SaveGame /
  LoadGame handled in view.rs); glue load screen list in glue.rs (0x4b62a0 / 0x4b6010),
  `GameSetup.save` -> `Scene::for_setup` -> `Scene::from_save`.
- Game start from a save (0x4ef100 with 0x6d1218 set): `new_game` skips the scenario
  (0x4bf520), start units, 0x488f90, 0x437a70(1), 0x446350 and 0x4abaf0; the header seed
  seeds the game and CRT rands; the template comes from the header's game info (+0x6d);
  then the load, then 0x4d9530's two steps. `from_save` first reads the file into a scratch
  state for the map (the CHK is saved) and color slots.
- Headless: `SCBW_GLUE_GAME="<frames> save <name>"` saves where the in-game dialog would
  (`<characters>/../save/<profile>/<name>.snx`); the glue load screen then lists it.

## What a 1.16.1 load changes (faithful, so savetest emulates it on the original game)

Not saved and reset by game init: stagger counters 0x59cca4 / 0x6283e8, AI timers
0x59cc8c..94, the finder arrays past their count, 0x65eb10, the 100-frame counter; never
reset (zero in a fresh process): 0x63fe2c. Pools are zeroed and their free lists rebuilt in
slot order (units 0x49e1f0, sprites 0x497230, images 0x4d4ba0, bullets 0x48a720, thingies
0x487990), so later allocations differ; a unit path in pool slot 0 reads back as none
(offset 0); pylon psi field sprites are re-made (0x4ead50). Saving itself refreshes the
overlap flags 0x40000 (0x4e2e20). The active bullet / thingy / fog thingy
lists come back in slot order too (0x48a720, 0x487990); savetest relinks the original's.
savetest copies the free-list order and stale unique ids to the loaded game only so the
replay's unit handles keep resolving, and gives it the replay's viewer (`replay`,
`local_vision_mask`: the creep shown tile flag 0x40000000 follows the local vision).

## Gaps

- Old save formats (< 3): bullets 0x48abd0, AI 0x447e20 / script +0xb0 not read.
- Written as 0 (not kept in Rust): image +0x10..0x17, +0x24, +0x28, modifier 0xb's unit;
  flingy +0x14 / +0x26; bullet +0x25 / +0x26 / +0x52 / +0x54 / +0x6d..; order +9; unit
  +0x26 / +0x52 / +0x66 / +0x8c / +0x106 / +0x125 / +0x132; AI contour heap pointers, guard
  +0x1a, building +0xe / +0xf, town +0x1c4..0x1cc, resource area +0x10..0x30, SAI +2;
  player table storm ids; game data ranges listed in src/bw/save/gamedata.rs's test.
- Image +0x2c GRP index searches 999 entries; 1.16.1 searches 0x2f3 without Brood War.
- Header game info 0x5967f8 is zero but for the template and 0x4cf160's fields; text has no
  map title / path / game type name; 0x4c40f0's two values are 0.
- The dat tables in the save's tail (0x4bf390) are read and dropped; a loaded UMS map's
  UNIx / UPGx / TECx are not re-applied (glue load: no CHK before the load; in-game load:
  the running game's).
- The screen position is kept in tiles (0x62848c / 0x6284a8 lose the 8-pixel steps).
- 0x4dbc00 (campaign continue flag) todo; multiplayer saving rights (0x4c3db0) todo.
