# Tools and local files

## Layout

```
agent/
  tools/            committed: our scripts (no Blizzard code)
    env.sh          source me: $AGENT $REPO $W $PY $EXE $DUMP $DATA $INSTALL $SCBW
    fns.sh          fns.sh 4865d0 479040 ...   -> Ghidra decompile of 1.16.1 functions
    fn_of.sh        fn_of.sh 'DAT_0051ca14 = ' -> which functions match a pattern
    dis.sh          dis.sh 478b40 200 x        -> disassembly (x = continue past ret)
    dis32.py        the disassembler behind dis.sh (capstone)
    refs.py         $PY -I refs.py $EXE 4dc4e0 -> call/jmp sites and dword refs
    sdis.py         storm.dll disassembler: `sdis.py <addr> <n>` or `sdis.py refs <addr>`
                    (STORM env = path of storm.dll; default the install's)
    storm_oracle.py run storm.dll's SCompDecompress under unicorn (sound sector oracle)
    fontdec.py      run 1.16.1's own font decryption under the emulator (font oracle):
                    $PY -I fontdec.py $EXE <enc.fnt> <font.gid> > dec.fnt
    cmp.sh          one bwsim scenario: emulator vs scbw (see testing.md)
    suite.sh        the regression scenarios (all must print MATCH). cmp.sh / suite.sh write fixed
                    files in $W (md/cur, a, b, emu_trace...): never run two at once (they
                    corrupt each other and report false diffs). repcmp.sh takes OUT=<dir>.
    repcmp.sh       replay playback: emulator vs scbw trace diff
    aistress.sh     [frames] [seed] [bin]: all (8) maps, you + 7 computers, headless via
                    `shot` (SCBW_SEED fixes the seed): panics; with a debug bin, the
                    todo_port! sites hit (the AI paths still missing, by count)
  local/            GITIGNORED (Blizzard-derived or large)
    scratch/        the whole former session scratchpad (5.8 GB), see below
re/                 committed reverse-engineering harness (emu.py, emu_game.py, ...)
```

All scripts locate files through `agent/tools/env.sh`; run them from anywhere.

## agent/local/scratch (not in git)

Copied from the session scratchpad
`/tmp/claude-1000/-home-stubob/76c93498-4528-4c2d-b789-d58b9cc39c19/scratchpad`
(which is lost on reboot). The important parts:

| Path | What |
|---|---|
| `dump/bw116n.c` | Ghidra decompile of 1.16.1 StarCraft.exe, all 4,644 functions, named where known (`// ==== 00<addr> <name>` headers). `bw116.c` is the unnamed original. `sc_x64*` are the SC:R 1.23 x64 dump/decompile (mostly superseded). |
| `sc116/StarCraft.exe`, `sc116/storm.dll` | Clean 1.16.1 exe (from the 1.16.1 patch MPQ) and storm.dll. Its code bytes equal the install's `StarCraft.exe` (they differ only in header/resources). |
| `data116/` | Files extracted from the 1.16.1 MPQs (`scbw extract`) for the emulator: arr, scripts, unit, tileset (incl. real `.vx4`), game, parallax, triggers (`triggers\\*.trg`, read by 0x4abaf0). |
| `venv/` | Python venv with capstone + unicorn (`$PY`). Moved venvs print a sys.prefix warning; harmless. To recreate: `python3 -m venv venv && venv/bin/pip install capstone unicorn`. |
| `md/` | `scbw mapdump` outputs (dims, tileset, chk, sai, ...) for emulator runs; `md/rep` = the current replay's map. |
| `rep.txt` | `scbw repdump` of the replay last compared. |
| `ra`, `rb`, `repdiff.txt` | last replay compare: native trace, scbw trace, diff. |
| `a`, `b`, `diff.txt`, `emu_log.txt` | last cmp.sh scenario. |
| `*.s`, `c_0x*.s`, `*.c` snippets | asm/decomp extracts used while porting (Blizzard-derived). |
| `dbg_*.py` | one-off emulator debugging scripts (examples of poking emulator state). |
| `f10.enc`, `font.gid`, `f10.dec` | font decryption oracle inputs/output. |
| `wp/` (not copied, 1.9 GB) | the throwaway Wine prefix; recreate with `re/native_shots.sh`. |

The SC:R CASC export that was at `scratch/data/` (672 MB) was not copied: the project no
longer uses SC:R data.

### Rebuilding the dump (Windows, 2026-10-23)

`re/ghidra/dump.ps1 -Install <1.16.1 dir>` recreates `sc116/`, the Ghidra project
`ghidra_proj/`, `dump/bw116.c` and `dump/bw116n.c` (~2 min auto-analysis, ~30 s
ParallelDecompiler on all cores; Ghidra 12.1.4, JDK 21). Names come from
`re/src_names.py`: each `/// 0x...` doc block names its address after the Rust fn
(dispatcher fns porting several functions get `_<addr>` suffixes so names stay unique),
plus `re/data/names116.txt`. Output is deterministic (DumpAll sorts each batch). Its
coverage.py reachable sets match the original Linux dump. `analyzeHeadless.bat` fails on
paths with spaces: run through a junction (here `C:\src\scbwre` -> `C:\src\scbw reverse`).
`env.sh` falls back to `venv/Scripts/python.exe` (Windows venv).
`data116/` on a new box: `agent/tools/fill_data116.py "$PY" "$EXE" "$DATA" "$W/md/cur" "$SCBW" "$W"`
(after one `scbw mapdump` into `$W/md/cur`; reruns the emulator, extracting what it misses: 746 files).

## scbw commands useful for RE

```sh
S=target/release/scbw
$S rep <file.rep>                    # header: seed, game type, players (index/storm id/type/race), command histogram
$S repdump <rep> <out.txt>           # replay as text for re/emu_game.py SCBW_REPLAY
$S reptrace <rep> <frames> <trace>   # scbw's per-frame trace of the replay
$S repshot <rep> <shots dir>         # draw frames listed in <dir>/native/meta.txt, diff vs native
$S mapdump <map|rep> <dir>           # map files for the emulator
$S bwsim <frames>                    # scenario harness (env SCBW_MAP, SCBW_UNITS, SCBW_MOVE, SCBW_MELEE, SCBW_TRACE, ...; see README)
$S extract <mpq path> <out>          # a file from the install's MPQs (font\font.gid etc.)
$S shot <map|rep> out.png x y [frames] [n_select]
$S view <map|rep>
```

Played by hand: `re/native_record.sh <dir>` (you play 1.16.1 in a 640x480 Xephyr window,
menus and games; NATIVE_RECORD_HEADLESS=1: Xvfb :79 for xdotool). Input is polled every
4 ms and the screen grabbed every 250 ms, each stamped with the game's in-game flag, frame
and seed (re/bwmem: a winegcc helper that starts the game and keeps the CreateProcess
handle, as the game denies OpenProcess). `re/record_glue.py replay <dir>` turns it into
SCBW_GLUE_INPUT steps (glue in ms; game steps at frames, src/script.rs: the headless game
loop never steps past a frame with events left) and writes `diff/` and `report.txt`.

## Emulator (re/emu.py, re/emu_game.py)

`re/emu.py` is a unicorn x86 emulator for 1.16.1 functions with Storm/kernel32 shims
(bump heap at 0x10000000). `e.call(addr, regs={...}, stack=[...])` returns EAX;
`e.r8/r16/r32/w8/w16/w32/read/write/alloc`. `re/emu_game.py <exe> <data dir> <mapdump
dir> <units> <moves> <frames> <trace out>` runs 1.16.1 game init and `game_step` 0x4d94b0
and writes the trace format that `scbw bwsim` / `scbw reptrace` write (unit lines, `th`
thingies, fog, tiles every 25 frames, bullets, `res`, `rng`). Env: `SCBW_REPLAY`,
`SCBW_MELEE`, `SCBW_MOVE2`/`SCBW_AT`, `SCBW_SELTRACE`, `SCBW_CLICKTEST`, `SCBW_SELTEST`,
`SCBW_MINIMAP`. See testing.md.

Quirks it patches: `__getptd` 0x4059e4 (TLS), 0x49e530 (placement error UI), local
player 0x512684 = 8 during the sim (0 only during UI tests), explored mask 0x57eeb8 =
0x100, turn counter 0x57eebc = frame counter each frame.

## Native game (headless)

`re/native_shots.sh <rep> <shots dir> [every]` builds a throwaway prefix, runs the real
1.16.1 under `Xvfb :78` (640x480) and `winedbg --gdb`, and gdb (`re/native_shots_gdb.py`)
dumps the 8-bit game screen buffer after each draw. `NATIVE_SHOTS_KEEP=1` keeps the temp
dir with a screenshot per menu step. ptrace_scope is 1 on this machine, so gdb cannot
attach to a running wine process; winedbg's gdb proxy (Windows debug API) works.
The same breakpoint mechanism can read any game state at an exact frame (frame counter
0x57f23c) — useful for checks the emulator cannot do. `NATIVE_GDBPY=<script.py>` runs
your own gdb script instead (gdb.Breakpoint subclasses whose `stop` logs memory and
returns False); `CAP_END=<frame>` with `every` limits a shots run.
