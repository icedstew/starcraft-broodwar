# Testing: how ports are verified

All checks compare scbw with the original 1.16.1 code. Ordered from smallest to largest.

## 1. Function oracles

Run one 1.16.1 function under the emulator and compare its output with the Rust port.
Examples kept in `agent/tools`: `fontdec.py` (font decryption; the Rust `src/grid.rs` has
a `cargo test` that compares with its output when `SCBW_GRID_TEST="font.gid enc.fnt
dec.fnt"` is set), `storm_oracle.py` (storm.dll Huffman/ADPCM vs `src/storm.rs`, checked
on 40 sounds). `re/emu_map.py` and `re/emu_path.py` check map/SAI/pathing (40 maps).

## 2. Scenario suite (cmp.sh / suite.sh)

`agent/tools/cmp.sh <map> "<units>" "<moves>" <frames>` creates units in the emulator and
in `scbw bwsim`, applies the same commands, steps both and diffs the traces.
`agent/tools/suite.sh` runs the 62 scenarios (movement, fights, mining, gas, training,
construction, selection/hotkeys, cancels, melee starts, minimap...) on the install's
Blizzard maps. **Every line must say MATCH.** Takes ~10 minutes; run in the background
and do not rebuild meanwhile. Move syntax: see README `bwsim`, plus harness extras:
S/A/D select/add/remove (`#n` = raw unit index), `H<action>:<group>` hotkeys,
`K<op>[:<u16>]` cancels, M (morph larvae), R (right-click), T (targeted order);
`ML=race,...` env for melee starts.

## 3. Replay playback (repcmp.sh) — the main driver now

`agent/tools/repcmp.sh <rep> <frames>`:
1. `scbw mapdump` the replay's embedded map, `scbw repdump` the setup and commands.
2. `re/emu_game.py` with `SCBW_REPLAY` plays it with the real game code.
3. `scbw reptrace` plays it in scbw.
4. Diff (unit indices blanked). First diff lines show the frame and unit.

Debug loop: find the frame `fN` of the first diff; list the replay's commands around it
(`awk '$1=="cmd" && $2>=N-20 && $2<=N' $W/rep.txt`: `cmd <frame> <player> <hex>`, byte 0
= command id); compare the unit's lines one frame earlier in `$W/ra`/`$W/rb`; find the
order/function in `src/bw/order_exec.rs` / `todo_port!`; port it from
`fns.sh`+`dis.sh`; rebuild; rerun. Emulator speed ~1000 frames per 10 s.

Order of command execution (0x4d9670 / 0x487100): commands recorded at frame F run while
the frame counter equals F, then the counter increments and `game_step` runs.

## 4. Screenshot tests against the real game

`~/games/Starcraft/broodwar-reps-116/shots/tl1412/native/` (206 PNGs + `meta.txt`) were
captured from the real game (re/native_shots.sh; pixel-exact; reruns identical except
time-based palette cycling). `re/repshots.sh <rep> <shots dir>` draws the same frames with
scbw (camera from meta.txt, all players' vision as a replay shows) and writes
`ours/`, `diff/` (native | ours | red), `report.txt`. First baseline (before the replay
fixes): 9% of pixels differ at the start, 26% at the end (simulation divergence + fog
edge smoothing + missing geyser smoke overlay). Compare rows 0..400 (the buffer has no
HUD). Camera is fixed at the replay's start view; capture more passes with other camera
positions for coverage.

## 5. Visual checks

`scbw shot <map|rep> out.png x y [frames] [n]` renders a frame with the HUD for eyeballing
(e.g. text after the font decryption port).

## Before committing

```sh
~/.cargo/bin/cargo build --release   # no warnings
~/.cargo/bin/cargo test --release    # 16+ tests
agent/tools/suite.sh | grep -v MATCH # nothing
agent/tools/repcmp.sh ~/games/Starcraft/broodwar-reps-116/tl1412*.rep <N>   # MATCH
```
