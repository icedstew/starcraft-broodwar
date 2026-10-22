#!/bin/bash
# native_all.sh [replay dir] [jobs]
#
# re/native_shots.sh for every replay in <replay dir> (default ~/games/Starcraft/
# broodwar-reps-116) that has no <replay dir>/shots/<id>/native/meta.txt yet (<id> = the
# file name up to the first space), <jobs> (default 4) at a time, each on its own Xvfb
# display (NATIVE_DISPLAY 83..). Logs: <replay dir>/shots/native_<id>.log.
DIR=$(realpath "${1:-$HOME/games/Starcraft/broodwar-reps-116}"); JOBS=${2:-4}
RE=$(dirname "$(realpath "$0")")
# A copy of the binary (repcam), so rebuilding meanwhile does not disturb the runs.
BIN=$(realpath "$(mktemp "${TMPDIR:-/tmp}/scbw.XXXXXX")"); cp "$RE/../target/release/scbw" "$BIN"; chmod +x "$BIN"
todo=()
for r in "$DIR"/*.rep; do
  id=$(basename "$r" .rep); id=${id%% *}
  [ -s "$DIR/shots/$id/native/meta.txt" ] || todo+=("$r")
done
echo "${#todo[@]} replays to capture"
worker() {
  local k=$1 i
  for ((i = k; i < ${#todo[@]}; i += JOBS)); do
    local r=${todo[i]} id; id=$(basename "$r" .rep); id=${id%% *}
    echo "== $id $(date +%T) (display $((83 + k)))"
    SCBW=$BIN NATIVE_DISPLAY=$((83 + k)) "$RE/native_shots.sh" "$r" "$DIR/shots/$id" \
      > "$DIR/shots/native_$id.log" 2>&1 && echo "done: $id $(date +%T)" || echo "native failed: $id"
  done
}
for ((k = 0; k < JOBS; k++)); do worker $k & done
wait
rm -f "$BIN"
