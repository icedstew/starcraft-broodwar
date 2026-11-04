#!/bin/bash
# dirtycmp.sh <sets dir> <out dir> [jobs]
#
# For every capture <sets dir>/<id>/ made with CAP_DRAWLOG=1 (native/draws.txt next to its
# shots): `scbw repshot` plain (a whole frame per shot) and with SCBW_DIRTY=1 (the game's own
# draws replayed on a persistent screen, screen.rs), the mean differing share and how many
# dirty sets equal native's. The replay is <id>*.rep in broodwar-reps-116. Summary: <out>/summary.txt.
SETS=$(realpath "$1"); OUT=$(realpath -m "$2"); JOBS=${3:-4}
RE=$(dirname "$(realpath "$0")"); R=${SCBW_REPS:-$HOME/games/Starcraft/broodwar-reps-116}
BIN=$(mktemp "${TMPDIR:-/tmp}/scbw.XXXXXX"); cp "${SCBW:-$RE/../agent/local/tgt/release/scbw}" "$BIN"; chmod +x "$BIN"
mkdir -p "$OUT"
one() {
  id=$1; rep=$(ls "$R"/$id*.rep | head -1)
  for mode in plain dirty; do
    d=$OUT/$id.$mode; mkdir -p "$d"; cp -rs "$SETS/$id/native" "$d/native" 2>/dev/null; cp "$SETS/$id/camera.txt" "$d/" 2>/dev/null
    if [ $mode = dirty ]; then export SCBW_DIRTY=1; else unset SCBW_DIRTY; fi
    env -u DISPLAY -u WAYLAND_DISPLAY SCBW_REPSHOT_IDX=1 SCBW_MUTE=1 timeout 6000 "$BIN" repshot "$rep" "$d" > "$d/log.txt" 2>&1
  done
  echo "$id plain $(tail -1 $OUT/$id.plain/log.txt | cut -c1-34) | dirty $(tail -1 $OUT/$id.dirty/log.txt | cut -c1-34) | $(grep 'dirty cells' $OUT/$id.dirty/log.txt)"
}
export -f one; export SETS OUT R BIN
ls -d "$SETS"/tl*/ | while read d; do b=$(basename $d); [ -s "$d/native/draws.txt" ] && [ -s "$d/native/meta.txt" ] && echo "$b"; done | xargs -P "$JOBS" -I{} bash -c 'one {}' | tee "$OUT/summary.txt"
rm -f "$BIN"
