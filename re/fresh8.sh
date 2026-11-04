#!/bin/bash
# fresh8.sh <out dir> <rep>...   native_shots.sh on each replay, one Xvfb display each (83..),
# all at once (<= 8). Sets land in <out dir>/<id>/ (id = file name up to the first space).
OUT=$(realpath -m "$1"); shift
RE=$(dirname "$(realpath "$0")"); k=0
mkdir -p "$OUT"
for r in "$@"; do
  id=$(basename "$r" .rep); id=${id%% *}
  SCBW=${SCBW:-$RE/../agent/local/tgt/release/scbw} NATIVE_DISPLAY=$((83 + k)) "$RE/native_shots.sh" "$r" "$OUT/$id" > "$OUT/native_$id.log" 2>&1 &
  k=$((k + 1))
done
wait
