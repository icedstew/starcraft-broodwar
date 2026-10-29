#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# cmp.sh <map> <units> <moves> <frames>: one differential scenario, emulator (1.16.1 code)
# vs `scbw bwsim`. Prints "MATCH n lines" or the first diff (full diff in $W/diff.txt).
. "$(dirname "$0")/env.sh"
cd "$REPO"
"$SCBW" mapdump "$1" "$W/md/cur" >/dev/null 2>&1
"$PY" -I re/emu_game.py "$EXE" "$DATA" "$W/md/cur" "$2" "$3" "$4" "$W/emu_trace.txt" > "$W/emu_log.txt" 2>&1 || tail -3 "$W/emu_log.txt"
SCBW_MAP="$1" SCBW_UNITS="$2" SCBW_MOVE="$3" SCBW_TRACE="$W/rust_trace.txt" "$SCBW" bwsim "$4" >/dev/null 2>&1
sed 's/ u[0-9]* / u /' "$W/emu_trace.txt" > "$W/a"; sed 's/ u[0-9]* / u /' "$W/rust_trace.txt" > "$W/b"
diff "$W/a" "$W/b" > "$W/diff.txt" && echo "MATCH $(wc -l < "$W/a") lines" || { echo "first diff:"; head -4 "$W/diff.txt"; }
