#!/bin/bash
# campaign_e2e.sh: a whole campaign, AI only, unpaced (SCBW_NET_PACE_US=1): 4 areas in a ring
# (North Zerg, East Terran, South Protoss, West Zerg; West-North starts quiet), a map per
# border ((2)Astral Balance, Baby Steps, Crystallis, Full Circle). Waits (TIMEOUT s, default
# 1200) for "wins the campaign"; prints the campaign: lines and PASS / FAIL (on FAIL also
# `status`). Own ports (WP, default 18830). O=<dir> for logs.
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
trap "" PIPE   # the host exits by itself once a faction wins
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-campaign-e2e}; WP=${WP:-18830}; TIMEOUT=${TIMEOUT:-1200}
M=$SCBW_INSTALL/maps/BroodWar
rm -rf $O; mkdir -p $O
cat > $O/world.txt <<W
faction Z 1 Zerg
faction T 0 Terran
faction P 2 Protoss
area 1 0 0 North
area 2 1 1 East
area 1 2 2 South
area 0 1 0 West
border 0 1 $M/(2)Astral Balance.scm
border 1 2 $M/(2)Baby Steps.scm
border 2 3 $M/(2)Crystallis.scm
border 3 0 $M/(2)Full Circle.scm
eliminated out
W
mkfifo $O/in; (SCBW_NET_PACE_US=1 timeout $((TIMEOUT + 30)) $B campaign host $O/world.txt $O/dir 0 $WP < $O/in > $O/host.log 2>&1) & HP=$!; exec 3> $O/in
ok=FAIL
for i in $(seq $TIMEOUT); do
  grep -q "wins the campaign" $O/host.log && { ok=PASS; break; }
  kill -0 $HP 2>/dev/null || break
  sleep 1
done
[ $ok = FAIL ] && { echo status >&3; sleep 2; }
echo quit >&3; exec 3>&-; wait $HP
grep -E "^campaign:" $O/host.log | grep -v "web map\|started"
[ $ok = FAIL ] && grep -E "^border [0-9]+: game" $O/host.log | tail -4
echo "$ok (${i}s)"
