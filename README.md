# scbw

Open-source reimplementation of StarCraft: Brood War 1.16.1 in Rust: a function-by-function port of the
original game logic (about 2,800 of its 4,644 functions so far), with its own networking.

**No game assets are included or distributed.** You need a StarCraft: Brood War 1.16.1
install (the MPQs and storm.dll); scbw reads the assets from it at runtime.

Pure Rust, no platform-specific code: builds for any target Rust supports.

## Running the game

Point `SCBW_INSTALL` at the 1.16.1 install (the directory with `StarDat.mpq`; defaults to
`~/Downloads/StarCraft-BroodWar_1.16.1`) and start the menus:

    SCBW_INSTALL="$HOME/Downloads/StarCraft-BroodWar_1.16.1" cargo run --release -- glue

This opens the 1.16.1 front end (640x480, shown at 2x): pick Single Player, then
StarCraft or Brood War, make or choose an ID, and from there play the campaign, a
custom game, load a saved game, or watch a replay. IDs, saves and replays live under
`~/.local/share/scbw/` (`characters/`, `save/`, `maps/replays/`; set `SCBW_CHARACTERS`
to move the `characters` directory, and the others go next to it).

## Usage

    cargo run --release -- view "/path/to/StarCraft/maps/(4)Blood Bath.scm"   # play a melee start
    cargo run --release -- render "/path/to/map.scm" out.png                  # full-map terrain PNG

`view` runs the faithful engine below. Melee maps start you (slot 0, `SCBW_RACE` =
terran/protoss/zerg, default terran) with a town hall and four workers; other start
locations get idle computer players. Controls: left-click / drag selects (Shift adds or
removes, Ctrl or a double-click picks the same type on screen), right-click moves,
attacks, gathers or sets rallies (Shift queues), Ctrl+0-9 / 0-9 / Shift+0-9 set, select
and add to hotkey groups, the command card and hotkeys M/A/S/H/P/G/C issue Move,
Attack, Stop, Hold, Patrol, Gather and Return Cargo, and the Build menu places
buildings; arrows or the screen edge scroll, Esc quits.

Start a game on The Hunters as each race:

    SCBW_RACE=terran  cargo run --release -- view "$HOME/Downloads/StarCraft-BroodWar_1.16.1/maps/(8)The Hunters.scm"
    SCBW_RACE=protoss cargo run --release -- view "$HOME/Downloads/StarCraft-BroodWar_1.16.1/maps/(8)The Hunters.scm"
    SCBW_RACE=zerg    cargo run --release -- view "$HOME/Downloads/StarCraft-BroodWar_1.16.1/maps/(8)The Hunters.scm"

Set `SCBW_INSTALL` to the 1.16.1 install directory (the one with `StarDat.mpq`;
defaults to `~/Downloads/StarCraft-BroodWar_1.16.1`).

### In the browser (WebAssembly)

One-time setup (the wasm-bindgen CLI version must match `wasm-bindgen` in `Cargo.lock`):

    rustup target add wasm32-unknown-unknown
    cargo install wasm-bindgen-cli --version 0.2.129 --locked

Build and serve:

    web/build.sh                          # writes web/pkg/ (scbw.js, scbw_bg.wasm)
    python3 -m http.server -d web 8000    # then open http://localhost:8000

On the page pick the 1.16.1 install folder (it reads `storm.dll`, `Patch_rt.mpq`,
`BrooDat.mpq`, `StarDat.mpq`; the CD archives and so the music are left out) and a map
or replay (`.scm`, `.scx`, `.chk`, `.rep`), then Start. If keys do nothing, click the
game once so it has keyboard focus.

## Layout

- `src/storage.rs`, `src/mpq.rs`, `src/storm.rs`, `src/grid.rs` – the install's MPQ archives in 1.16.1
  priority order, the MPQ reader, storm.dll's decompressors, the font encryption
- `src/map.rs`, `src/dat.rs`, `src/grp.rs`, `src/iscript.rs`, `src/palette.rs`, `src/fog.rs` – CHK
  terrain, `arr/*.dat` tables, GRP sprites, iscript, the palette pipeline, fog of war shading
- `src/view.rs`, `src/ui.rs`, `src/dlg.rs`, `src/menu.rs`, `src/statdata.rs`, `src/audio.rs` – the in-game
  screen, HUD, dialogs and sound
- `src/glue*.rs`, `src/setup.rs`, `src/saveload.rs`, `src/registry.rs` – the front end (menus, lobby,
  campaign flow, score screens), game setup, save / load, settings
- `src/bw/` – the faithful engine port (below); `src/bw/save/` is the 1.16.1 save format
- `src/net/` – our own networking: rollback netcode over QUIC / WebTransport (see `agent/areas/net.md`)
- `src/campaign/` – the campaign mode (a Risk-style world where every border is a live game)
- `src/screen.rs` – the game screen as 1.16.1 keeps it (dirty cells), used by the screenshot tests
- `src/game/` – an older approximate game layer, only used for a few debug commands
- `re/` – reverse-engineering and test tools (`re/NOTES.md`); `agent/` – working notes and scripts

## Faithful engine (`src/bw/`)

Game logic ported function by function from the classic 1.16.1 StarCraft.exe, each routine
annotated with its 1.16.1 address (`/// 0x...`). Ported: the frame loop and RNG, units, orders,
pathing (SAI regions, A*, contours), movement and collision, vision, combat, all three races'
economy, production, construction, morphs, add-ons, research and upgrades, spells, transports,
creep, triggers, the computer-player AI (scripts, town and military planning), melee and team
games, commands as the game receives them, replay playback and recording, and save / load.

The front end follows the original too: login, main menus, campaign flow with briefings and
cinematics, create / join game and lobby screens, in-game menus, score screens.

Where 1.16.1 itself is unfinished or crashes (a handful of AI and trigger corner cases) the port
notes it at the site (`todo_port!`) instead of reproducing the crash.

## Verification

Ports are checked against the original, not against expectations:

- **Differential testing**: `re/emu.py` and friends run 1.16.1 code under Unicorn (with Storm shims)
  and compare byte for byte: tile flags, SAI regions, region A*, and whole-game traces
  (`re/emu_game.py`) in 62 scenarios (`agent/tools/suite.sh`, expect no non-MATCH line).
- **Replays**: `agent/tools/repcmp.sh <rep> <frames>` compares scbw's RNG and unit traces with the
  real game's on real 1.16.1 replays; ~900 replays from several years play to their end and match.
  `scbw rbtest` / `nettest` check that rollback and late commands equal straight playback.
- **Screenshots**: `re/native_shots.sh` plays a replay in the real 1.16.1 (wine, Xvfb, gdb), and
  `scbw repshot` draws the same frames; with `CAP_DRAWLOG=1` the capture also logs every draw, the
  palette cycling steps and the game's dirty cells, which `SCBW_DIRTY=1 scbw repshot` replays
  through `src/screen.rs`. `re/dirtycmp.sh` compares plain and dirty-cell runs over many captures.
  Most replays differ from the real screen in well under 1% of the map pixels; closing the rest is
  the current work (`agent/areas/ui.md`, "Dirty-cell redraw").

The 1.16.1 executable and decompiler output are not part of this repository.

## Status

Working: the whole single-player game on the faithful engine (campaign, custom games, replays,
save / load), our own multiplayer (rollback netcode, N players, lobby, browser join over WebTransport,
joining a running game), the campaign mode, and the browser build.

In progress: pixel-exact screens against the real game (dirty-cell redraw), the remaining ~1000
unported functions (mostly front-end plumbing; see `agent/frontier.tsv`), and the optional
StarCraft: Remastered asset path planned for after the port. Details: `agent/status.md`.

### Port coverage

Share of the 4,644 1.16.1 functions that are ported. A function counts as ported when
`src/` has a `/// 0x...` doc comment with its address, and as todo when only a
`todo_port!` names it. The input is a Ghidra export of StarCraft.exe, which is not
part of this repo:

```sh
cd ~/src/decomp/scbw
python3 -I re/coverage.py agent/local/scratch/dump/bw116n.c . | tail -11     # summary: in scope / reachable from game_step
python3 -I re/coverage.py agent/local/scratch/dump/bw116n.c . --html cov.html  # per source file, as HTML
python3 -I re/coverage.py agent/local/scratch/dump/bw116n.c . --missing both  # non-AI functions left to port, reachable from the sim
python3 -I re/coverage.py agent/local/scratch/dump/bw116n.c . --missing ai    # computer-player AI functions left to port
python3 -I re/coverage.py agent/local/scratch/dump/bw116n.c . --missing both --all  # also the replaced / deferred ones, tagged
```

Unported functions are split by `re/buckets.txt` (addresses and ranges, each with its
reason) into three groups:

- **replaced**: the Rust port has its own equivalent, so there is nothing to port:
  the C runtime, Storm / Win32 imports (any function Ghidra did not name `FUN_*`), the
  UI engine's dirty-rect / DirectDraw plumbing, the Win32 shell, the sound loader cache,
  and the selection-circle / health-bar image pools that scbw draws each frame.
- **deferred**: planned for later: save / load (`agent/areas/saveload.md`) and
  networking.
- **left**: still to port. `--missing` lists only these unless `--all` is given.

Each "reachable from" row is also split into "(no AI)" and "(AI only)": the walk from
the simulation roots stops at the computer players' AI entry points (`AI_ROOTS` in
`re/coverage.py`), and what is only reachable through them is the AI bucket.

Each "reachable from" line ends with `unported: N replaced by Rust, N deferred, N left`.

Summary as a table (`column -N` is util-linux):

```sh
cd ~/src/decomp/scbw
python3 -I re/coverage.py agent/local/scratch/dump/bw116n.c . | sed -nE \
  's/^all functions: ([0-9]+) functions, ([0-9]+) ported \(([0-9]+%)\).*/All functions|\1|\2 (\3)/p;
   s/^in scope: ([0-9]+) functions, ([0-9]+) ported \(([0-9]+%)\).*/In scope|\1|\2 (\3)/p;
   s/^reachable from (.*): ([0-9]+) functions, ([0-9]+) ported \(([0-9]+%)\).*unported: ([0-9]+) replaced by Rust, ([0-9]+) deferred, ([0-9]+) left/Reachable from \1|\2|\3 (\4)|\5|\6|\7/p' \
  | column -t -s'|' -N Scope,Functions,Ported,Replaced,Deferred,Left
```

## Commands

Run `cargo run --release -- <command>`; the main ones:

- Play and watch: `glue` (front end), `view <map|chk|rep>`, `repview`, `rep <file.rep>`
- Images: `shot <map> out.png x y [frames] [n_select]`, `render <map> out.png`, `smk`
- Networking: `nethost` / `netjoin <code>` (live games), `netrep`, `nettest`, `rbtest <rep> [delay]`
- Campaign: `campaign gen | check | host | join | switch`
- Tests: `repshot <rep> <shots dir>` (screens vs the real game), `reptrace`, `rephash`, `savetest`,
  `reprecord`, `sndcheck`, `bwsim <frames>` (headless engine; env `SCBW_MAP`, `SCBW_UNITS`,
  `SCBW_MOVE`, `SCBW_MELEE`, `SCBW_TRACE`)
- Data: `dat`, `units`, `mpqls`, `mpqx`, `extract`, `extractmap`, `mapdump`

Environment: `SCBW_INSTALL` (the 1.16.1 install), `SCBW_MUTE=1` (no audio), `SCBW_RACE`;
`nosound` as an argument also turns the sound off. Test runs should be started detached and
without a display (`env -u DISPLAY -u WAYLAND_DISPLAY SCBW_MUTE=1 ...`).
