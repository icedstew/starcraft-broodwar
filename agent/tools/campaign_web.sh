#!/bin/bash
# campaign_web.sh: the campaign's web map, headless (src/campaign/web.rs). Starts a host on
# a 3-area world, then over HTTP: the page, /state without a name (no stats, no codes), a
# name registers for faction 0 (/join), /state with it (its faction's stats and code only),
# another faction refused for that name, /minimap (a PNG), faction chat, admin with and
# without the token. SHOT=1 also screenshots the page with headless chromium ($O/page.png).
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-campaign-web}; WP=${WP:-18781}
MAP=${MAP:-$SCBW_INSTALL/maps/BroodWar/(2)Astral Balance.scm}
rm -rf $O; mkdir -p $O
cat > $O/world.txt <<EOF
faction Z 1 Zerg
faction T 0 Terran
faction P 2 Protoss
area 0 0 0 Char
area 2 0 1 Mar Sara
area 1 2 2 Aiur
border 0 1
border 1 2
border 2 0
pool $MAP
eliminated pick
EOF
mkfifo $O/in; (timeout 120 $B campaign host $O/world.txt $O/dir 0 $WP < $O/in > $O/host.log 2>&1) & HP=$!; exec 3> $O/in
for i in $(seq 40); do grep -q "web map" $O/host.log 2>/dev/null && break; sleep 0.25; done
sleep 4
U=http://127.0.0.1:$WP
tok=$(grep -o 'token=[0-9a-f]*' $O/host.log | head -1 | cut -d= -f2)
c() { curl -s -o $O/out -w '%{http_code}' "$@"; }
echo "page: $(c $U/) $(grep -c 'Campaign' $O/out) lines with Campaign"
echo "anon state: $(c "$U/state")"; grep -o '"code"' $O/out | wc -l | sed 's/^/  codes: /'; grep -o '"workers"' $O/out | wc -l | sed 's/^/  stats: /'
echo "join ann 0: $(c -X POST "$U/join?name=ann&faction=0") $(cat $O/out)"
echo "join ann 1: $(c -X POST "$U/join?name=ann&faction=1") $(cat $O/out)"
echo "bad name: $(c -X POST "$U/join?name=a%20b&faction=0") $(cat $O/out)"
echo "ann state: $(c "$U/state?name=ann")"; grep -o '"code"' $O/out | wc -l | sed 's/^/  codes: /'; grep -o '"workers"' $O/out | wc -l | sed 's/^/  stats: /'
echo "minimap b0: $(c "$U/minimap?border=0&name=ann") $(file -b $O/out | cut -c1-40)"
echo "minimap b1 (not ann's): $(c "$U/minimap?border=1&name=ann")"
echo "chat: $(c -X POST --data 'hello faction' "$U/chat?name=ann")"; c "$U/state?name=ann" > /dev/null; grep -o '"chat":\[[^]]*\]\]*' $O/out
echo "admin no token: $(c -X POST "$U/admin?cmd=status")"
echo "admin: $(c -X POST "$U/admin?token=$tok&cmd=pause") $(cat $O/out)"
if [ -n "$SHOT" ]; then
  timeout 30 chromium --headless=new --no-sandbox --user-data-dir=$O/prof --window-size=1200,800 --virtual-time-budget=4000 --screenshot=$O/page.png "$U/?name=ann&border=0" > /dev/null 2>&1
  echo "shot: $(ls -s $O/page.png 2>/dev/null)"
fi
echo quit >&3; exec 3>&-; wait $HP
grep -E "^campaign:" $O/host.log | cut -c1-120
# The .exe chooser (scbw campaign join <web map> <name> <faction>): lists bob's faction's
# matches, plays the first headless to frame 700 (saying "!hi from the game" at 400: a
# faction line), lists again, quits. The line must reach the faction chat on the map.
if [ -n "$CHOOSER" ]; then
  mkfifo $O/in2; (timeout 150 $B campaign host $O/world.txt $O/dir2 0 $WP < $O/in2 > $O/host2.log 2>&1) & HP=$!; exec 3> $O/in2
  for i in $(seq 40); do grep -q "web map" $O/host2.log 2>/dev/null && break; sleep 0.25; done
  sleep 4; mkdir -p $O/inst $O/k
  for f in "$SCBW_INSTALL"/*; do n=$(basename "$f"); [ "$n" = maps ] || ln -sf "$f" "$O/inst/$n"; done
  printf '1\nq\n' | (cd $O; SCBW_NET_HEADLESS=700 SCBW_NET_SCRIPT="400:say !hi from the game" SCBW_INSTALL=$O/inst SCBW_CHARACTERS=$O/k timeout 120 $B campaign join $U bob 1 > $O/chooser.log 2>&1)
  c "$U/state?name=bob" > /dev/null; echo "bob chat: $(grep -o '"chat":\[[^]]*\]\]*' $O/out)"
  grep -E "matches|\) |frame 700|ended" $O/chooser.log | head -8
  echo quit >&3; exec 3>&-; wait $HP
fi
