#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# campaign_browser.sh: a browser joins a campaign match, headless (as enhanced.sh WEB=1).
# `scbw campaign match` runs one match (paced PACE_US, default 70000 us a frame, so the
# headless browser keeps up); headless chromium opens the web build (web/build.sh, served
# from web/, game files from web/auto/) with ?join=<code>&frames=K&name=web and takes an
# army: its console's `frame K hash H` must equal the host's. Then a second page joins a
# match id that does not exist: its status line must show "refused: the match ended".
# NOBUILD=1 skips web/build.sh. O=<dir> for logs.
# Headless: never a window on the user's display (a campaign host refuses one anyway).
unset DISPLAY WAYLAND_DISPLAY
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-campaign-browser}; K=${K:-1500}
MAP=${MAP:-$SCBW_INSTALL/maps/BroodWar/(2)Astral Balance.scm}
rm -rf $O; mkdir -p $O
[ -n "$NOBUILD" ] || web/build.sh > $O/build.log 2>&1 || { echo "web build failed"; tail $O/build.log; exit 1; }
SCBW_NET_PACE_US=${PACE_US:-70000} timeout 400 $B campaign match "$MAP" $K 1 > $O/host.log 2>&1 & HP=$!
for i in $(seq 80); do grep -q "^MATCH 0" $O/host.log 2>/dev/null && break; sleep 0.25; done
code=$(sed -n 's/^MATCH 0 //p' $O/host.log)
python3 -m http.server -d web 8768 -b 127.0.0.1 > $O/http.log 2>&1 & SP=$!
timeout 420 chromium --headless=new --no-sandbox --remote-debugging-port=9336 --user-data-dir=$O/prof --autoplay-policy=no-user-gesture-required about:blank > /dev/null 2>&1 & CP=$!
sleep 2
enc() { printf %s "$1" | sed 's/#/%23/g'; }
timeout 400 node "$T/cdp_console.mjs" 9336 "http://127.0.0.1:8768/?join=$(enc "$code")&frames=$K&name=web" > $O/web.log 2>&1 & LP=$!
for i in $(seq 380); do grep -q "frame $K hash" $O/web.log 2>/dev/null && break; grep -q "^border 0 ended" $O/host.log && break; sleep 1; done
# A match id nobody hosts: refused before the start.
bad=$(printf %s "$code" | sed 's/#0$/#77/')
timeout 40 node "$T/cdp_console.mjs" 9336 "http://127.0.0.1:8768/?join=$(enc "$bad")&name=web2" > $O/web2.log 2>&1
sleep 1; kill $HP $CP $SP $LP 2>/dev/null; wait 2>/dev/null
echo "host: $(grep -o "border 0 frame $K hash [0-9a-f]*" $O/host.log | sed 's/border 0 //')"
echo "web:  $(grep -o "frame $K hash [0-9a-f]*" $O/web.log | head -1)"
echo "refused page: $(grep -o 'status: refused[^|]*' $O/web2.log | head -1)"
grep -h "panicked\|exception" $O/web.log $O/web2.log | head -3
