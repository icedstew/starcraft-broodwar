#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# webjoin.sh [frames] [delay]: net phase 5 check on Linux. A native `scbw nethost` and a headless
# Chromium joining it (web/pkg built with web/build.sh, the install in web/auto/), each with
# scripted commands; prints both peers' `frame K hash H` (must be equal). O=<dir> for the logs.
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
K=${1:-1500}; D=${2:-2}; O=${O:-/tmp/webjoin}; mkdir -p $O
S0="5:selw;20:14bc02f4010000000000;120:14840364020000000000;900:14a00360020000000000;1400:selw"
S1="9:selw;31:149001bc020000000000;150:14f003c4010000000000;1100:14d002c0020000000000;1410:selw"
python3 -m http.server -d web 8765 -b 127.0.0.1 > $O/http.log 2>&1 & HP=$!
SCBW_NET_HEADLESS=$K SCBW_NET_SCRIPT="$S0" SCBW_NET_REPLAY=$O/host.rep ./target/release/scbw nethost "$SCBW_INSTALL/maps/(2)Bottleneck.scm" 0 2 $D > $O/host.out 2> $O/host.err & NP=$!
for i in $(seq 100); do code=$(sed -n 's/^LISTEN //p' $O/host.out); [ -n "$code" ] && break; sleep 0.2; done
url="http://127.0.0.1:8765/?join=$(printf %s "$code" | sed 's/#/%23/')&frames=$K&script=$(printf %s "$S1" | sed 's/;/%3B/g')"
echo "$url" > $O/url
timeout 300 chromium --headless=new --no-sandbox --remote-debugging-port=9333 --user-data-dir=$O/prof --autoplay-policy=no-user-gesture-required about:blank > $O/chrome.out 2> $O/chrome.err &
CP=$!
timeout 300 node "$T/cdp_console.mjs" 9333 "$url" > $O/console.txt 2>&1 &
NJ=$!
wait $NP
sleep ${WAIT:-30}
kill $CP $HP $NJ 2>/dev/null
echo "host:  $(grep '^frame' $O/host.out)"
echo "web:   $(grep -o 'frame [0-9]* hash [0-9a-f]*' $O/console.txt | head -1)"
grep -i -E 'rollback|player|error|panic|exception' $O/console.txt | head -5
