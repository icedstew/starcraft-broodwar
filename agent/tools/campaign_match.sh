#!/bin/bash
# campaign_match.sh: campaign matches end to end, headless (src/campaign/runner.rs). One
# process runs COUNT (default 2) matches on MAP at Fastest, both armies computers; a joiner
# (its install has no maps) joins match 0 by code JOIN_AFTER (default 3) s in and takes an
# army from the AI. Both print the state hash at frame K (default 900): must be equal.
# CHAT=1: a second joiner takes the other army; the first says "!secret plan" (a faction
# line) and "hello all": the second must get only "hello all", the host the faction line.
# O=<dir> for logs; ARMY=<slot> asks for that army.
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-campaign}; K=${K:-900}
MAP=${MAP:-$SCBW_INSTALL/maps/BroodWar/(2)Astral Balance.scm}
mkdir -p $O/k $O/inst
for f in "$SCBW_INSTALL"/*; do n=$(basename "$f"); [ "$n" = maps ] || ln -sf "$f" "$O/inst/$n"; done
CAMPAIGN_STATS=1 timeout 300 $B campaign match "$MAP" $K ${COUNT:-2} > $O/host.log 2>&1 & HP=$!
for i in $(seq 80); do grep -q "^MATCH 0" $O/host.log 2>/dev/null && break; sleep 0.25; done
code=$(sed -n 's/^MATCH 0 //p' $O/host.log)
sleep ${JOIN_AFTER:-3}
[ -n "$CHAT" ] && S3="400:say !secret plan;420:say hello all"
(cd $O; SCBW_JOIN=$code SCBW_NAME=three SCBW_ARMY=${ARMY:-} SCBW_NET_HEADLESS=$K SCBW_NET_SCRIPT="$S3" SCBW_INSTALL=$O/inst SCBW_CHARACTERS=$O/k timeout 200 $B glue $O/k.png > $O/late.log 2>&1) & LP=$!
if [ -n "$CHAT" ]; then
  # A joiner of the other side: must get "hello all", never the faction line.
  sleep 2; mkdir -p $O/m
  (cd $O; NETREP_DEBUG=1 SCBW_JOIN=$code SCBW_NAME=four SCBW_NET_HEADLESS=$K SCBW_INSTALL=$O/inst SCBW_CHARACTERS=$O/m timeout 200 $B glue $O/m.png > $O/four.log 2>&1) & FP=$!
  wait $FP
fi
wait $LP; sleep 1; kill $HP 2>/dev/null; wait 2>/dev/null
echo "host: $(grep -o "border 0 frame $K hash [0-9a-f]*" $O/host.log | sed 's/border 0 //')"
echo "late: $(grep -o "frame $K hash [0-9a-f]*" $O/late.log)"
grep -o "border 1 frame $K hash [0-9a-f]*" $O/host.log
grep "humans: \[\"three\"\]" $O/host.log | tail -1 | cut -c1-200
grep -h "net:\|campaign:" $O/*.log | head -8
if [ -n "$CHAT" ]; then
  echo "four: $(grep -o "frame $K hash [0-9a-f]*" $O/four.log)"
  echo "enemy got hello: $(grep -c 'hello all' $O/four.log), enemy got secret: $(grep -c 'secret' $O/four.log)"
  grep "faction . chat" $O/host.log
fi
