#!/bin/bash
# aistress.sh [frames] [seed base] [binary]: every (8) melee map, the local player plus 7
# computers (races cycle), played headless through `scbw shot` (the view path) in
# parallel. Prints each run's exit and any panic; with a debug binary, also counts the
# todo_port! sites hit (AI paths still missing). Logs in $W/aistress/.
. "$(dirname "$0")/env.sh"
FRAMES=${1:-30000}; BASE=${2:-1}; BIN=${3:-$SCBW}
OUT=$W/aistress; mkdir -p "$OUT"; cp "$BIN" "$OUT/x"
i=0
for map in "$INSTALL"/maps/\(8\)*.scm; do
    i=$((i+1)); r=$(echo protoss terran zerg | cut -d' ' -f$((i%3+1)))
    ( SCBW_SEED=$((BASE+i)) SCBW_RACE=$r RUST_BACKTRACE=1 "$OUT/x" shot "$map" "$OUT/o$i.png" 0 0 "$FRAMES" > "$OUT/log$i.txt" 2>&1
      echo "exit $? seed $((BASE+i)) $r $(basename "$map")" >> "$OUT/log$i.txt" ) &
done
wait
for f in "$OUT"/log*.txt; do grep -E "panicked|^exit" -A5 "$f" | grep -v "^ *[0-9]*: \(__rustc\|core::\)" | head -8; done
cat "$OUT"/log*.txt | grep todo_port | sort | uniq -c | sort -rn
