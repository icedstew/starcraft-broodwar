#!/bin/bash
# campaign_host.sh: the campaign host end to end, headless (src/campaign/host.rs). A world
# of 4 areas in a line (A B C D, owners 0 1 2 0) has 3 live borders; the host runs their
# matches. Commands on stdin: `end 0 0` (faction 0 wins A-B: takes B, so B-C restarts as
# 0 v 2), a joiner by code (takes an army of B-C's match, its name registers), `ban`,
# `save`, `quit`; then the host starts again on its directory and must resume the same
# world with the saved matches. O=<dir> for logs.
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-campaign-host}
MAP=${MAP:-$SCBW_INSTALL/maps/BroodWar/(2)Astral Balance.scm}
rm -rf $O; mkdir -p $O/k $O/inst
for f in "$SCBW_INSTALL"/*; do n=$(basename "$f"); [ "$n" = maps ] || ln -sf "$f" "$O/inst/$n"; done
cat > $O/world.txt <<EOF
faction Z 1 Zerg
faction T 0 Terran
faction P 2 Protoss
area 0 0 0 A
area 1 0 1 B
area 2 0 2 C
area 3 0 0 D
border 0 1
border 1 2
border 2 3
pool $MAP
eliminated pick
EOF
run() { # $1: log; stdin script from the rest
  mkfifo $O/in.$1; (timeout 200 $B campaign host $O/world.txt $O/dir < $O/in.$1 > $O/$1.log 2>&1) & HP=$!; exec 3> $O/in.$1
}
step() { echo "$1" >&3; sleep ${2:-1}; }
code_of() { grep "^border $1:" $O/host1.log | tail -1 | grep -o '127[^ ]*'; }
run host1
sleep 6; step status
step "end 0 0" 3; step status 1
c=$(code_of 1)
(cd $O; SCBW_JOIN=$c SCBW_NAME=three SCBW_NET_HEADLESS=400 SCBW_INSTALL=$O/inst SCBW_CHARACTERS=$O/k timeout 100 $B glue $O/k.png > $O/late.log 2>&1) &
sleep 20; step status; step "ban three" 2; step save 3; step quit 2
exec 3>&-; wait $HP
run host2
sleep 6; step status; step quit 2
exec 3>&-; wait $HP
echo "== first run"; grep -E "^campaign:|^live|^faction|^border [0-9]+:" $O/host1.log | cut -c1-160
echo "== joiner"; grep -E "^frame|net: joined" $O/late.log
echo "== files"; ls $O/dir; cat $O/dir/players.txt
echo "== second run"; grep -E "^campaign:|^live|^faction|^border [0-9]+:" $O/host2.log | cut -c1-160
