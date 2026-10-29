#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# campaign_sweep.sh [pool dir or maps...]: every map of a campaign pool played headless as a
# campaign match (`scbw campaign match <map> K 1`: 2 computer sides, unpaced) to frame K
# (default 3000), JOBS (default nproc / 2) at once. Prints one line per map that fails
# (load error, panic, no hash by the timeout) and a summary; logs per map in $O/logs.
# Default pool: ~/games/Starcraft/mappacks (not the maps that ship with the game).
unset DISPLAY WAYLAND_DISPLAY
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-campaign-sweep}; K=${K:-3000}
JOBS=${JOBS:-$(( $(nproc) / 2 ))}
rm -rf $O; mkdir -p $O/logs
[ $# -eq 0 ] && set -- "$HOME/games/Starcraft/mappacks"
find "$@" -type f \( -iname '*.scm' -o -iname '*.scx' \) | sort > $O/maps.txt
echo "$(wc -l < $O/maps.txt) maps, $JOBS at once, to frame $K"
one() { # $1: map
  local log=$O/logs/$(printf %s "$1" | md5sum | cut -c1-12).log
  echo "map $1" > $log
  SCBW_CAMPAIGN_STOP_AT_HASH=1 SCBW_NET_PACE_US=1 timeout ${TIMEOUT:-180} $B campaign match "$1" $K 1 >> $log 2>&1
  if grep -q "^border 0 frame $K hash" $log; then echo "ok	$1"
  elif grep -q "panicked" $log; then echo "PANIC	$1	$(grep -m1 -o 'panicked at [^ ]*' $log) $(grep -m1 -A1 'panicked at' $log | tail -1 | cut -c1-120)"
  elif grep -q "^campaign: border 0" $log; then echo "ERROR	$1	$(grep -m1 '^campaign: border 0' $log | sed 's/^campaign: border 0: [^:]*: //' | cut -c1-120)"
  elif grep -q "ended: Won\|ended: Draw" $log; then echo "ok	$1	(ended before frame $K)"
  else echo "STUCK	$1	(no hash by the timeout)"
  fi
}
export -f one; export B O K
tr '\n' '\0' < $O/maps.txt | xargs -0 -P $JOBS -I{} bash -c 'one "$1"' _ {} > $O/results.txt
grep -v "^ok" $O/results.txt | sort
echo "== $(grep -c '^ok' $O/results.txt) ok, $(grep -c '^ERROR' $O/results.txt) load errors, $(grep -c '^PANIC' $O/results.txt) panics, $(grep -c '^STUCK' $O/results.txt) stuck (of $(wc -l < $O/results.txt))"
