#!/bin/bash
# repcmp.sh <replay.rep> <frames>: play a replay in the emulator (real 1.16.1 game code via
# re/emu_game.py SCBW_REPLAY) and in scbw (`scbw reptrace`), diff the per-frame traces.
# Prints "MATCH n" or the first differing lines; full diff in $W/repdiff.txt, traces in
# $W/ra (native) / $W/rb (scbw) with unit indices blanked.
. "$(dirname "$0")/env.sh"
cd "$REPO"
"$SCBW" mapdump "$1" "$W/md/rep" >/dev/null 2>&1
"$SCBW" repdump "$1" "$W/rep.txt"
SCBW_REPLAY="$W/rep.txt" "$PY" -I re/emu_game.py "$EXE" "$DATA" "$W/md/rep" "" "" "$2" "$W/emu_rep.txt" > "$W/emu_rep_log.txt" 2>&1
grep "scenario failed\|FAILED" "$W/emu_rep_log.txt"
"$SCBW" reptrace "$1" "$2" "$W/rust_rep.txt"
sed 's/ u[0-9]* / u /' "$W/emu_rep.txt" > "$W/ra"; sed 's/ u[0-9]* / u /' "$W/rust_rep.txt" > "$W/rb"
diff "$W/ra" "$W/rb" > "$W/repdiff.txt" && echo "MATCH $(wc -l < "$W/ra")" || { echo "lines $(wc -l < "$W/ra") vs $(wc -l < "$W/rb")"; head -6 "$W/repdiff.txt" | cut -c1-400; }
