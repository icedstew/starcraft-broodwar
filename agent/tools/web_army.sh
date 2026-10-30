#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# web_army.sh: browsers join a campaign match in progress and share an army, headless.
# One paced match (PACE_US, default 30000 us a frame) to frame K (default 3000):
#   nat   native, joins at once, army 0; selects a worker and moves it (frames ~300); quits at
#         NAT_QUIT (default 2000)
#   web1  browser, joins at host frame W1_AT (400), army 0 too (shared with nat); selects a
#         worker, moves it (frames 900 / 920); leaves at W1_QUIT (1500): the army keeps nat
#   web2  browser, army 1; reports `frame K hash`
#   web3  browser, joins at host frame W3_AT (2200), army 0 again (the AI's since nat left);
#         reports
# Checks: host, web2 and web3 hashes at K equal; the host's side 0 humans go
# [nat] -> [nat, web1] -> [nat] -> [] -> [web3], side 1's stays [web2]; no panics. NOBUILD=1 skips web/build.sh.
# O=<dir> for logs. Headless: never a window on the user's display.
unset DISPLAY WAYLAND_DISPLAY
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-web-army}; K=${K:-3000}
MAP=${MAP:-$SCBW_INSTALL/maps/BroodWar/(2)Astral Balance.scm}
WP=${WP:-8769}; DP=${DP:-9337}
rm -rf $O; mkdir -p $O/inst $O/k
for f in "$SCBW_INSTALL"/*; do n=$(basename "$f"); [ "$n" = maps ] || ln -sf "$f" "$O/inst/$n"; done
[ -n "$NOBUILD" ] || web/build.sh > $O/build.log 2>&1 || { echo "web build failed"; tail $O/build.log; exit 1; }
CAMPAIGN_STATS=1 SCBW_NET_PACE_US=${PACE_US:-30000} timeout 600 $B campaign match "$MAP" $K 1 > $O/host.log 2>&1 & HP=$!
for i in $(seq 80); do grep -q "^MATCH 0" $O/host.log 2>/dev/null && break; sleep 0.25; done
code=$(sed -n 's/^MATCH 0 //p' $O/host.log)
# A move (0x14) of the selection to (x, y): x, y, no target, type 0xe4, not queued.
mv() { printf '14%02x%02x%02x%02x0000e40000' $(($1 & 255)) $(($1 >> 8)) $(($2 & 255)) $(($2 >> 8)); }
(cd $O; SCBW_JOIN=$code SCBW_NAME=nat SCBW_ARMY=0 SCBW_NET_HEADLESS=$K SCBW_NET_QUIT_AT=${NAT_QUIT:-2000} \
  SCBW_NET_SCRIPT="300:selw;320:$(mv 1500 1500)" SCBW_INSTALL=$O/inst SCBW_CHARACTERS=$O/k \
  timeout 500 $B glue $O/k.png > $O/nat.log 2>&1) & NP=$!
python3 -m http.server -d web $WP -b 127.0.0.1 > $O/http.log 2>&1 & SP=$!
# One browser per player: a background tab gets no animation frames, stalls the game and is
# dropped (3 s, enhanced games).
br() { timeout 560 chromium --headless=new --no-sandbox --remote-debugging-port=$1 --user-data-dir=$O/prof$1 --autoplay-policy=no-user-gesture-required about:blank > /dev/null 2>&1 & CP="$CP $!"; }
CP=; br $DP; br $((DP + 1)); br $((DP + 2))
sleep 2
enc() { printf %s "$1" | sed 's/#/%23/g;s/;/%3B/g;s/:/%3A/g'; }
page() { echo "http://127.0.0.1:$WP/?join=$(enc "$code")&name=$1&army=$2&frames=$K${3:+&script=$(enc "$3")}${4:+&quit=$4}"; }
timeout 540 node "$T/cdp_console.mjs" $DP "$(page web2 1)" > $O/web2.log 2>&1 & W2=$!
# Wait for the host's frame (its stats line, once a second).
at() { until [ "$(sed -n 's/^border 0 Stats { frame: \([0-9]*\).*/\1/p' $O/host.log | tail -1)" -ge $1 ] 2>/dev/null; do sleep 0.5; grep -q "^border 0 ended" $O/host.log && break; done; }
at ${W1_AT:-400}
timeout 520 node "$T/cdp_console.mjs" $((DP + 1)) "$(page web1 0 "900:selw;920:$(mv 600 600)" ${W1_QUIT:-1500})" > $O/web1.log 2>&1 & W1=$!
at ${W3_AT:-2200}
timeout 480 node "$T/cdp_console.mjs" $((DP + 2)) "$(page web3 0)" > $O/web3.log 2>&1 & W3=$!
for i in $(seq 500); do grep -q "frame $K hash" $O/web3.log 2>/dev/null && grep -q "frame $K hash" $O/web2.log && break; grep -q "^border 0 ended" $O/host.log && break; sleep 1; done
sleep 2; kill $HP $CP $SP $W1 $W2 $W3 $NP 2>/dev/null; wait 2>/dev/null
h=$(grep -o "border 0 frame $K hash [0-9a-f]*" $O/host.log | sed 's/border 0 //')
echo "host: $h"
for p in web2 web3; do echo "$p: $(grep -o "frame $K hash [0-9a-f]*" $O/$p.log | head -1)"; done
echo "nat:  $(grep -o 'quit at frame [0-9]*' $O/nat.log)   web1: $(grep -o 'quit at frame [0-9]*' $O/web1.log | head -1)"
echo "humans (side 0 | side 1) as the host saw them:"
sed -n 's/^border 0 Stats { frame: \([0-9]*\).*sides: \[SideStats { humans: \(\[[^]]*\]\).*SideStats { humans: \(\[[^]]*\]\).*/\1 \2 | \3/p' $O/host.log | awk '{k=$0; sub(/^[0-9]+ /,"",k); if (k!=p) print "  f" $0; p=k}'

grep -h "panicked\|exception\|desync\|Desync" $O/*.log | head -5
ok=1
for p in web2 web3; do [ -n "$h" ] && grep -q "$h" $O/$p.log || ok=0; done
[ $ok = 1 ] && echo PASS || echo FAIL
