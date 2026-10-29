#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# campaign_page.sh: the campaign map page as one app, headless chromium driven over CDP
# (agent/tools/cdp_drive.mjs). The host (localhost: a secure context; SCBW_CAMPAIGN_AUTO=1 so
# ?files=auto can stand in for the player's own folder) runs a 3-area world; ann (faction 0)
# loads the files once, joins her first match in the page, box-selects her units (green
# circles), opens the game menu (F10: keys reach the game), leaves (End Mission -> Leave): the map is back; then the second match,
# leave again. Screenshots of every step in $O (look at them). STEPS overrides the steps.
# NOBUILD=1 skips web/build.sh. O=<dir> for logs.
unset DISPLAY WAYLAND_DISPLAY
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-campaign-page}; WP=${WP:-18880}
# The files: the player's own Google Drive folder when SCBW_TEST_DRIVE_KEY / _FOLDER are set
# (never printed; the key's referrer rule allows only port 8000, so the web map runs there),
# else the host's own copy (?files=auto, SCBW_CAMPAIGN_AUTO=1: tests only).
if [ -n "$SCBW_TEST_DRIVE_KEY" ] && [ -n "$SCBW_TEST_DRIVE_FOLDER" ]; then
  WP=8000; FILES=drive
  ss -ltn | grep -q ':8000 ' && { echo "port 8000 is in use: the Drive check needs it"; exit 1; }
else
  echo "(SCBW_TEST_DRIVE_KEY / _FOLDER unset: files from the host's auto/ copy, not Drive)"; FILES=auto; AUTO=1
fi
MAP=${MAP:-$SCBW_INSTALL/maps/BroodWar/(2)Astral Balance.scm}
rm -rf $O; mkdir -p $O
[ -n "$NOBUILD" ] || web/build.sh > $O/build.log 2>&1 || { echo "web build failed"; tail $O/build.log; exit 1; }
printf 'faction Z 0 Zerg\nfaction T 1 Terran\nfaction P 2 Protoss\narea 0 0 0 A\narea 2 0 1 B\narea 1 2 2 C\nborder 0 1\nborder 1 2\nborder 2 0\npool %s\n' "$MAP" > $O/world.txt
(env ${AUTO:+SCBW_CAMPAIGN_AUTO=1} SCBW_NET_PACE_US=${PACE_US:-70000} timeout 600 $B campaign host $O/world.txt $O/dir 0 $WP < /dev/null > $O/host.log 2>&1) & HP=$!
for i in $(seq 40); do grep -q "web map" $O/host.log 2>/dev/null && break; sleep 0.25; done
curl -s -o /dev/null "http://127.0.0.1:$WP/join?name=ann&faction=0"
timeout 590 chromium --headless=new --no-sandbox --remote-debugging-port=9350 --user-data-dir=$O/prof --autoplay-policy=no-user-gesture-required about:blank > /dev/null 2>&1 & CP=$!
sleep 2
MAPUP="!document.querySelector('main').hidden && document.getElementById('game').hidden && !!window.campaignPlay"
INGAME="!!document.querySelector('#scbw canvas') && !document.getElementById('game').hidden"
CODES="(async () => { const s = await (await fetch('/state?name=ann')).json(); window.codes = s.borders.filter(b => b.code).map(b => b.code); return window.codes.length; })()"
DEFAULT=("wait:$MAPUP" "shot:1-map" "eval:$CODES" "eval:(campaignPlay(window.codes[0]), 1)" "wait:$INGAME" "sleep:9000" "shot:2-game" "drag:5,30,635,320" "sleep:500" "shot:2a-selected" "key:F10" "sleep:800" "shot:3-menu" "click:317,252" "sleep:800" "shot:4-end" "click:317,151" "wait:$MAPUP" "sleep:1500" "shot:5-map-again"
  "eval:$CODES" "eval:(campaignPlay(window.codes[1]), 1)" "wait:$INGAME" "sleep:9000" "shot:6-game2" "key:F10" "sleep:600" "shot:6b-menu2" "click:317,252" "sleep:600" "click:317,151" "wait:$MAPUP" "sleep:1500" "shot:7-map-end")
eval "STEPS_ARR=(${STEPS:-})"; [ ${#STEPS_ARR[@]} -eq 0 ] && STEPS_ARR=("${DEFAULT[@]}")
if [ $FILES = drive ]; then
  # A fresh profile: no files kept yet, so the page shows its load step.
  STEPS_ARR=("wait:!document.getElementById('load').hidden" "shot:0-load" "drive" "${STEPS_ARR[@]}")
  URL="http://127.0.0.1:$WP/?name=ann"
else
  URL="http://127.0.0.1:$WP/?name=ann&files=auto"
fi
timeout 580 node "$T/cdp_drive.mjs" 9350 "$URL" $O "${STEPS_ARR[@]}" > $O/drive.log 2>&1
kill $CP $HP 2>/dev/null; wait 2>/dev/null
grep -E "^(step|FAIL|DONE|exception|value)" $O/drive.log | cut -c1-140
grep -h "net: player\|refused" $O/host.log | head -5
ls $O/*.png
