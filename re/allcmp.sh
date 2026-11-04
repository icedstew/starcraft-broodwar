#!/bin/bash
# allcmp.sh <out dir> [jobs] [names...]
#
# `scbw repshot` (with SCBW_REPSHOT_IDX=1: where the differing pixels are, by palette index)
# for every native shot set under ~/games/Starcraft/broodwar-reps-116/shots (<id> or <id>_pN:
# the replay is <id>*.rep in broodwar-reps-116 or -tl), <jobs> (default 8) at a time. The
# native/ and camera.txt of each set are linked into <out>/<name>/ so the real sets stay
# untouched. Writes <out>/<name>/{report.txt,idx.txt}; the summary is <out>/summary.txt.
OUT=$(realpath -m "$1"); JOBS=${2:-8}; shift 2
RE=$(dirname "$(realpath "$0")"); R=${SCBW_REPS:-$HOME/games/Starcraft}
BIN=$(mktemp "${TMPDIR:-/tmp}/scbw.XXXXXX"); cp "${SCBW:-$RE/../agent/local/tgt/release/scbw}" "$BIN"; chmod +x "$BIN"
mkdir -p "$OUT"
names=("$@")
[ ${#names[@]} = 0 ] && names=($(for d in "$R"/broodwar-reps-116/shots/tl*; do [ -s "$d/native/meta.txt" ] && basename "$d"; done | grep -v "_\(buf\|ev\|fixed\|full\|hud\|sf\|sf2\|p0\)$"))
one() {
  n=$1; id=${n%%_*}; d=$OUT/$n
  rep=$(ls "$R"/broodwar-reps-116/$id*.rep "$R"/broodwar-reps-tl/$id*.rep 2>/dev/null | head -1)
  [ -n "$rep" ] || { echo "$n: no replay"; return; }
  rm -rf "$d"; mkdir -p "$d"
  ln -s "$R/broodwar-reps-116/shots/$n/native" "$d/native"
  cp "$R/broodwar-reps-116/shots/$n/camera.txt" "$d/" 2>/dev/null
  SCBW_FOLLOW=$([ "$n" != "$id" ] && echo "${n##*_p}" || echo 0) SCBW_REPSHOT_IDX=1 SCBW_MUTE=1 timeout 3000 "$BIN" repshot "$rep" "$d" > "$d/log.txt" 2>&1
  grep "diff in cycle" "$d/log.txt" > "$d/idx.txt"
  echo "$n $(tail -1 "$d/log.txt" | cut -c1-40)"
}
export -f one; export OUT R BIN
printf '%s\n' "${names[@]}" | xargs -P "$JOBS" -I{} bash -c 'one {}' | tee "$OUT/summary.txt"
rm -f "$BIN"
