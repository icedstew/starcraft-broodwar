#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# campaign_https.sh: a browser joins a campaign match from the host's HTTPS web map on a
# non-localhost address (src/campaign/web.rs serve_tls). The host binds 0.0.0.0
# (SCBW_NET_BIND), paces its matches (PACE_US, default 70000 us a frame) and prints each
# match's hash at frame K. Headless chromium (--ignore-certificate-errors) opens
# https://<ADDR>:<https port>/play/index.html?join=<code>&frames=K&name=web (the page, the
# wasm and web/auto's game files all over HTTPS): its `frame K hash` must equal the host's.
# FIREFOX=1: the same with headless Firefox (WebDriver BiDi, acceptInsecureCerts; profile
# pref network.http.http3.enable). CLICK=1: chromium without the flag, its warning page passed
# by typing "thisisunsafe". Also: a restarted host keeps the same certificate.
# Needs web/build.sh done (web/pkg) and web/auto. O=<dir> for logs.
# Headless: never a window on the user's display (a campaign host refuses one anyway).
unset DISPLAY WAYLAND_DISPLAY
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-campaign-https}; K=${K:-1500}; WP=${WP:-18812}; SP=$((WP + 1))
ADDR=${ADDR:-$(ip -4 -o addr show scope global | awk '{print $4}' | cut -d/ -f1 | grep -v '^127\.' | head -1)}
MAP=${MAP:-$SCBW_INSTALL/maps/BroodWar/(2)Astral Balance.scm}
rm -rf $O; mkdir -p $O
printf 'faction Z 1 Zerg\nfaction T 0 Terran\narea 0 0 0 A\narea 1 0 1 B\nborder 0 1\npool %s\n' "$MAP" > $O/world.txt
start() { mkfifo $O/in.$1; (SCBW_CAMPAIGN_AUTO=1 SCBW_NET_BIND=0.0.0.0 SCBW_NET_PACE_US=${PACE_US:-70000} SCBW_CAMPAIGN_HASH_AT=$K timeout 500 $B campaign host $O/world.txt $O/dir 0 $WP < $O/in.$1 > $O/$1.log 2>&1) & HP=$!; exec 3> $O/in.$1
  for i in $(seq 40); do grep -q "web map" $O/$1.log 2>/dev/null && break; sleep 0.25; done; sleep 2; }
fp() { echo | openssl s_client -connect $ADDR:$SP 2>/dev/null | openssl x509 -noout -fingerprint -sha256 2>/dev/null; }
start host
FP1=$(fp)
curl -sk -o /dev/null "https://$ADDR:$SP/join?name=web&faction=0"
code=$(curl -sk "https://$ADDR:$SP/state?name=web" | grep -o '"code":"[^"]*"' | head -1 | cut -d'"' -f4 | sed "s/^0\.0\.0\.0:/$ADDR:/")
url="https://$ADDR:$SP/play/index.html?join=$(printf %s "$code" | sed 's/#/%23/g')&frames=$K&name=web"
echo "page: $url" > $O/url.txt
if [ -n "$FIREFOX" ]; then
  mkdir -p $O/ffprof; echo 'user_pref("network.http.http3.enable", true);' > $O/ffprof/user.js
  timeout 450 firefox --headless --no-remote --profile $O/ffprof --remote-debugging-port 9338 > $O/ff.log 2>&1 & CP=$!
  sleep 3; timeout 440 node "$T/bidi_console.mjs" 9338 "$url" > $O/web.log 2>&1 & LP=$!
else
  # CLICK=1: no flag; the certificate warning page is passed as a user would (cdp_console.mjs
  # types "thisisunsafe").
  timeout 450 chromium --headless=new --no-sandbox ${CLICK:---ignore-certificate-errors} --remote-debugging-port=9339 --user-data-dir=$O/prof --autoplay-policy=no-user-gesture-required about:blank > /dev/null 2>&1 & CP=$!
  sleep 2; timeout 440 node "$T/cdp_console.mjs" 9339 "$url" ${CLICK:+proceed} > $O/web.log 2>&1 & LP=$!
fi
for i in $(seq 420); do grep -q "frame $K hash" $O/web.log 2>/dev/null && break; sleep 1; done
sleep 2; kill $CP $LP 2>/dev/null
echo quit >&3; exec 3>&-; wait $HP
start host2; FP2=$(fp); echo quit >&3; exec 3>&-; wait $HP
echo "host: $(grep -o "border 0 frame $K hash [0-9a-f]*" $O/host.log | sed 's/border 0 //')"
echo "web:  $(grep -o "frame $K hash [0-9a-f]*" $O/web.log | head -1)"
grep -o "status: [^|]*| [a-z]* | secure [a-z]*" $O/web.log | tail -1
[ -n "$FP1" ] && [ "$FP1" = "$FP2" ] && echo "certificate kept on restart: yes" || echo "certificate kept on restart: NO ($FP1 / $FP2)"
grep -h "net: player\|net: hub" $O/host.log | head -4
