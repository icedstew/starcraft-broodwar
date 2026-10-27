#!/bin/bash
# lobby.sh [web]: the network lobby end to end. A scripted `scbw glue` creates a game (real
# time, SCBW_GLUE_REALTIME) and starts it once a player joined; the joiner is a second glue
# whose install has no maps folder (it joins by the printed LOBBY code and downloads the map),
# or with "web" a headless Chromium (web/pkg built, the install in web/auto/, ?join=<code>).
# Both play to frame 300 headless and print the state hash (must be equal). O=<dir> for logs.
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-lobby}; P=${P:-16300}
mkdir -p $O/h $O/j $O/inst
for f in "$SCBW_INSTALL"/*; do n=$(basename "$f"); [ "$n" = maps ] || ln -sf "$f" "$O/inst/$n"; done
export SCBW_GLUE_REALTIME=1 SCBW_LAN_PORT=$P SCBW_NET_HEADLESS=300
# Main menu: Multiplayer, Expansion, LAN Ok; New ID; login Ok; Create Game; Ok; wait; the
# lobby's Ok (a click: the chat entry takes the keys).
WJ=$([ "$1" = web ] && echo 30000 || echo 8000)
(cd $O; SCBW_CHARACTERS=$O/h SCBW_GLUE_INPUT="w 2500;t m;w 1500;k 0d;w 1500;t o;w 2000;t host;k 0d;w 1500;k 0d;w 2500;t g;w 2500;t o;w $WJ;c 500 393;w 40000" timeout 200 $B glue $O/h.png > $O/host.log 2>&1) & HP=$!
for i in $(seq 80); do grep -q LOBBY $O/host.log 2>/dev/null && break; sleep 0.5; done
code=$(sed -n 's/^LOBBY //p' $O/host.log | sed 's/0\.0\.0\.0/127.0.0.1/')
if [ "$1" = web ]; then
  python3 -m http.server -d web 8767 -b 127.0.0.1 > $O/http.log 2>&1 & SP=$!
  timeout 200 chromium --headless=new --no-sandbox --remote-debugging-port=9335 --user-data-dir=$O/prof --autoplay-policy=no-user-gesture-required about:blank > /dev/null 2>&1 & CP=$!
  timeout 200 node "$T/cdp_console.mjs" 9335 "http://127.0.0.1:8767/?join=$(printf %s "$code" | sed 's/#/%23/')&frames=300&name=web" > $O/join.log 2>&1 & NJ=$!
  wait $HP; sleep 10; kill $CP $SP $NJ 2>/dev/null
else
  (cd $O; SCBW_JOIN=$code SCBW_NAME=join SCBW_INSTALL=$O/inst SCBW_CHARACTERS=$O/j SCBW_GLUE_INPUT="w 60000" timeout 200 $B glue $O/j.png > $O/join.log 2>&1)
  wait $HP
fi
echo "host: $(grep -o 'frame 300 hash [0-9a-f]*' $O/host.log)"
echo "join: $(grep -o 'frame 300 hash [0-9a-f]*' $O/join.log | head -1)"
