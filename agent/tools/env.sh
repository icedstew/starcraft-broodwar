# Source me: paths shared by the agent tools. Everything Blizzard-derived or large lives
# in agent/local (gitignored); see agent/tools.md.
AGENT=$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)
REPO=$(dirname "$AGENT")
W=$AGENT/local/scratch              # the former session scratchpad, copied whole
PY=$W/venv/bin/python               # venv with capstone + unicorn
EXE=$W/sc116/StarCraft.exe          # clean 1.16.1 exe (code identical to the install's)
DUMP=$W/dump/bw116n.c               # Ghidra decompile of EXE, one "// ==== 00addr name" per function
DATA=$W/data116                     # 1.16.1 MPQ files extracted for the emulator (re/emu_game.py)
INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
SCBW=$REPO/target/release/scbw
