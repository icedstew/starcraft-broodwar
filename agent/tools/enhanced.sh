#!/bin/bash
# enhanced.sh: an enhanced game end to end, headless. A scripted glue creates it
# (SCBW_ENHANCED), one player joins its lobby by code and the game starts; a third joins by
# code while it runs (its install has no maps: the state comes from the host). All play to
# frame K (default 12000) and print the state hash (must be equal). O=<dir> for logs.
# WEB=1: the third player is headless chromium (needs web/build.sh); the others run paced
# (PACE_US, default 70000 us a frame, about game speed Normal) so use a small K (2500).
# LAN=1: the third player joins through the LAN game list instead of SCBW_JOIN.
# FOURTH=1: a fourth player (native, by code) joins FOURTH_AFTER (default 2) s after the third.
# QUIT_AT=<frame>: the second player quits there (its army goes to the AI); ARMY=<slot>: the
# third asks for that army (taking it back from the AI).
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-enhanced}; P=${P:-16400}; K=${K:-12000}
mkdir -p $O/h $O/j $O/k $O/inst
for f in "$SCBW_INSTALL"/*; do n=$(basename "$f"); [ "$n" = maps ] || ln -sf "$f" "$O/inst/$n"; done
[ -n "$WEB" ] && export SCBW_NET_PACE_US=${PACE_US:-70000}   # a browser is slower than an unthrottled host
export SCBW_GLUE_REALTIME=1 SCBW_LAN_PORT=$P SCBW_NET_HEADLESS=$K
(cd $O; NETREP_DEBUG=1 SCBW_ENHANCED=1 SCBW_CHARACTERS=$O/h SCBW_GLUE_INPUT="w 2500;t m;w 1500;k 0d;w 1500;t o;w 2000;t host;k 0d;w 1500;k 0d;w 2500;t g;w 2500;t o;w 8000;c 500 393;w 40000" timeout 300 $B glue $O/h.png > $O/host.log 2>&1) & HP=$!
for i in $(seq 80); do grep -q LOBBY $O/host.log 2>/dev/null && break; sleep 0.5; done
code=$(sed -n 's/^LOBBY //p' $O/host.log | sed 's/0\.0\.0\.0/127.0.0.1/')
(cd $O; SCBW_NET_QUIT_AT=${QUIT_AT:-} SCBW_JOIN=$code SCBW_NAME=two SCBW_CHARACTERS=$O/j SCBW_GLUE_INPUT="w 60000" timeout 300 $B glue $O/j.png > $O/join.log 2>&1) & JP=$!
for i in $(seq 120); do grep -q "net player" $O/host.log 2>/dev/null && break; sleep 0.5; done
sleep ${LATE_AFTER:-3}
if [ -n "$WEB" ]; then
  # The third player is a headless browser (web build: web/build.sh first).
  python3 -m http.server -d web 8767 -b 127.0.0.1 > $O/http.log 2>&1 & SP=$!
  timeout 400 chromium --headless=new --no-sandbox --remote-debugging-port=9335 --user-data-dir=$O/prof --autoplay-policy=no-user-gesture-required about:blank > /dev/null 2>&1 & CP=$!
  sleep 2
  timeout 400 node "$T/cdp_console.mjs" 9335 "http://127.0.0.1:8767/?join=$(printf %s "$code" | sed 's/#/%23/')&frames=$K&name=three${ARMY:+&army=$ARMY}" > $O/late.log 2>&1 & LP=$!
else
if [ -n "$LAN" ]; then J=(SCBW_GLUE_INPUT="w 2500;t m;w 1500;k 0d;w 1500;t o;w 2000;t three;k 0d;w 1500;k 0d;w 6000;c 200 100;w 500;t o;w 60000"); else J=(SCBW_JOIN=$code); fi
(cd $O; env NETREP_DEBUG=${LATE_DEBUG:-} SCBW_ARMY=${ARMY:-} "${J[@]}" SCBW_NAME=three SCBW_INSTALL=$O/inst SCBW_CHARACTERS=$O/k timeout 300 $B glue $O/k.png > $O/late.log 2>&1) & LP=$!
fi
if [ -n "$FOURTH" ]; then
  mkdir -p $O/m; sleep ${FOURTH_AFTER:-2}
  (cd $O; SCBW_ARMY=${ARMY4:-} SCBW_JOIN=$code SCBW_NAME=four SCBW_INSTALL=$O/inst SCBW_CHARACTERS=$O/m timeout 300 $B glue $O/m.png > $O/four.log 2>&1) & FP=$!
fi
wait $HP $JP; [ -n "$WEB" ] && sleep 5; kill $CP $SP $LP 2>/dev/null; wait 2>/dev/null
for f in host join late ${FOURTH:+four}; do echo "$f: $(grep -o "frame $K hash [0-9a-f]*" $O/$f.log)"; done
grep -h "net:" $O/*.log | grep -v "net player" | head -12
