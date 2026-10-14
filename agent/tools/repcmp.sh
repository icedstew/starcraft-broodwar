#!/bin/bash
# repcmp.sh <replay.rep> <frames>: play a replay in the emulator (real 1.16.1 game code via
# re/emu_game.py SCBW_REPLAY) and in scbw (`scbw reptrace`), diff the per-frame traces.
# Prints "MATCH n" or the first differing lines; full diff in $O/repdiff.txt, traces in
# $O/ra (native) / $O/rb (scbw) with unit indices blanked.
. "$(dirname "$0")/env.sh"
cd "$REPO"
O=${OUT:-$W}; mkdir -p "$O/md"   # OUT=<dir> to run several compares at once
"$SCBW" mapdump "$1" "$O/md/rep" >/dev/null 2>&1
"$SCBW" repdump "$1" "$O/rep.txt"
SCBW_REPLAY="$O/rep.txt" "$PY" -I re/emu_game.py "$EXE" "$DATA" "$O/md/rep" "" "" "$2" "$O/emu_rep.txt" > "$O/emu_rep_log.txt" 2>&1
grep "scenario failed\|FAILED" "$O/emu_rep_log.txt"
"$SCBW" reptrace "$1" "$2" "$O/rust_rep.txt"
sed 's/ u[0-9]* / u /' "$O/emu_rep.txt" > "$O/ra"; sed 's/ u[0-9]* / u /' "$O/rust_rep.txt" > "$O/rb"
diff "$O/ra" "$O/rb" > "$O/repdiff.txt" && echo "MATCH $(wc -l < "$O/ra")" || { echo "lines $(wc -l < "$O/ra") vs $(wc -l < "$O/rb")"; head -6 "$O/repdiff.txt" | cut -c1-400; }
