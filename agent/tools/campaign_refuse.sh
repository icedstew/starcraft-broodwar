#!/bin/bash
# campaign_refuse.sh: a refused campaign joiner gets the reason (C_REFUSED, or the hub's
# refusal before the Start) and prints it. Host with SCBW_CAMPAIGN_SLOTS=1 (one joiner per
# match) on a 3-area world (borders 0: F0-F1, 1: F1-F2, 2: F2-F0). ann (faction 0) plays
# border 0 (after: zed (banned) -> banned; cat (faction 2) on border 0 -> another faction);
# a second ann on border 2 -> name in use; bob on border 0 -> full; after `end 1 draw` the
# old code of border 1 -> the match ended. O=<dir> for logs.
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=${O:-/tmp/scbw-campaign-refuse}; WP=${WP:-18785}
MAP=${MAP:-$SCBW_INSTALL/maps/BroodWar/(2)Astral Balance.scm}
rm -rf $O; mkdir -p $O/inst
for f in "$SCBW_INSTALL"/*; do n=$(basename "$f"); [ "$n" = maps ] || ln -sf "$f" "$O/inst/$n"; done
printf 'faction Z 1 Zerg\nfaction T 0 Terran\nfaction P 2 Protoss\narea 0 0 0 A\narea 2 0 1 B\narea 1 2 2 C\nborder 0 1\nborder 1 2\nborder 2 0\npool %s\n' "$MAP" > $O/world.txt
mkfifo $O/in; (SCBW_CAMPAIGN_SLOTS=1 timeout 150 $B campaign host $O/world.txt $O/dir 0 $WP < $O/in > $O/host.log 2>&1) & HP=$!; exec 3> $O/in
for i in $(seq 40); do grep -q "web map" $O/host.log 2>/dev/null && break; sleep 0.25; done
sleep 3; U=http://127.0.0.1:$WP
curl -s -o /dev/null "$U/join?name=ann&faction=0"; curl -s -o /dev/null "$U/join?name=cat&faction=2"; curl -s -o /dev/null "$U/join?name=zed&faction=1"
echo "ban zed" >&3; echo status >&3; sleep 1
code() { grep "^border $1: game" $O/host.log | tail -1 | grep -o '127[^ ]*'; }
c0=$(code 0); c1=$(code 1); c2=$(code 2)
join() { # $1 log, $2 code, $3 name, [$4 frames]
  (cd $O; SCBW_NET_HEADLESS=${4:-100} SCBW_INSTALL=$O/inst SCBW_CHARACTERS=$O timeout ${5:-40} $B campaign join "$2" $3 > $O/$1.log 2>&1)
}
join banned $c0 zed; sleep 3; join faction $c0 cat
sleep 3   # their links close 2 s after the refusal
join ann $c0 ann 2000 120 & AP=$!
sleep 8
join inuse $c2 ann; join full $c0 bob
# The glue: a second ann picks border 2 (row 2) on the Join Game screen while ann plays:
# the reason shows in a popup, Enter closes it, the list is back, Cancel quits.
(cd $O; SCBW_GLUE_REALTIME=1 SCBW_GLUE_INPUT="w 2500;c 200 120;w 300;t o;w 5000;s $O/refused.png;k 0d;w 1500;s $O/back.png;t c;w 500" SCBW_INSTALL=$O/inst SCBW_CHARACTERS=$O timeout 60 $B campaign join $U ann 0 $O/end.png > $O/glue.log 2>&1)
echo "end 1 draw" >&3; sleep 2
join ended $c1 bob
wait $AP
echo quit >&3; exec 3>&-; wait $HP
for r in banned faction inuse full ended; do echo "$r: $(grep -ah 'refused:' $O/$r.log | head -1)"; done
echo "ann: $(grep -ao 'frame 2000 hash [0-9a-f]*' $O/ann.log)"
echo "glue: $(grep -ah 'refused:' $O/glue.log | head -1) (shots refused.png, back.png)"
