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

Harness setup for replays (re/emu_game.py, fixed 2026-10-26): `repdump` gives the header's
player table (+0xa1 = 0x57eee0, forces decide 0x4c3ed0's team alliances), the template
(0x4aabf0 starting resources: Top vs Bottom starts at 0), and the CRT rand is srand(seed) +
one music-variant draw (creep tile variants). tl1341 / tl1672 (TvB) and tl1362 MATCH to the end.

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

## 4b. Save / load round trip (savetest)

`scbw savetest <rep> <frame> [frames]`: plays the replay to <frame>, saves the whole file,
loads it into a new game (`Scene::from_save`), applies to the original what a 1.16.1 load
changes (agent/areas/save.md), then both play on and the per-frame traces (units, bullets,
thingies, fog, tiles, rng, selections, hotkeys; slot numbers ignored) must MATCH.
`SCBW_SAVETEST_DUMP=<p>` writes both full states (Debug) after the load,
`SCBW_SAVETEST_DUMPAT=<f>` at frame f (as `<f>.da` / `<f>.db` in the current directory),
`SCBW_SAVEOUT=<p>` both save files.
Resave: SAME, except with pylons alive (DIFF in the images, ~0x17b00: the load re-makes
the psi field sprites in other slots). 2026-10-19: 16 replays x f3000 / f9000 all MATCH.

## 4c. AI <-> human army handoff (campaign_switch.sh)

Ours (enhanced games, campaign matches): `agent/tools/campaign_switch.sh` runs `scbw campaign
switch <map> [cycles]` (src/campaign/runner.rs) over maps x seeds x modes in parallel (~1 min for
480 runs on 16 cores) and prints each run with a `BUG:` line; exit 1 if any. Army 0 goes AI ->
human -> AI through the live commands (join with its save and load, Stop / move / train, leave
0x57). Checks: a human's army runs no AI script, builds nothing by itself, its stopped workers
stay idle; one frame after a take-over every unit (but ones a building trains, dying units,
subunits, units in transports) has AI state; the AI's phase builds or trains something and
does not bank while under the supply cap. Modes: plain, rb (each join / leave replayed from a
copy 60 frames, hashes equal), shared (two humans, one leaves: the army stays human), busy (the
human trains workers), quick (24 / 48 frame phases, 20 cycles). Debug: SWITCH_DEBUG=1 (scripts,
towns and their build lists, requests, buildings, what a save and load changed),
SWITCH_TRACE=1 (stopped workers), SWITCH_WATCH=<unit> (one unit every frame). The save and load
at a join changes `state_hash` natively (0x4e2e20 overlap status, 0x493f70 pylon order).
Live check of a takeover over the network: `K=6000 QUIT_AT=3000 PACE_US=4000
agent/tools/enhanced.sh` (host and late joiner hashes equal; unpaced, the host ends before the
late joiner connects).

Shared mode also checks that two humans on one army each have their own selection: A selects
two mining workers, B two others, A's Stop and B's Move (to the start location) reach only
their own pair (tracked over 150 frames: mining steps finish before an order takes).

Browsers: `agent/tools/web_army.sh` (one paced campaign match, PACE_US 30000, K 3000): a native
player and a browser share army 0 and give commands (`selw`, a move), the browser leaves at 1500
(the army stays the native's), the native at 2000 (the AI's), a third browser takes it at 2200;
a browser plays army 1. Host, army-1 browser and late browser hashes at K must be equal, and the
host's humans per side are printed. One headless Chromium per player: a background tab gets no
animation frames (the page's), but the game runs in a Web Worker: `ONE_BROWSER=1` runs all three as
tabs of one browser, two in the background, and must pass too. The page takes `script=` (as SCBW_NET_SCRIPT) and `quit=`
(as SCBW_NET_QUIT_AT) for a running game it joins too (`NetGame::script_next`). Also
`campaign_browser.sh` and `WEB=1 K=2500 enhanced.sh`. A campaign host's `frame K hash` for K a
multiple of 8 is the confirmed frame's (the desync check's), not the state it has run to.

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
