#!/bin/bash
# campaign_dev.sh: dev loop for a live campaign: stop the host, delete its save, rebuild (native
# + the browser build), start it again and print the map URLs (http / https, with and without
# the admin token). The host runs in the background; log in $D/host.log, pid in $D/host.pid.
#   D=<dir>        campaign dir (default ~/games/Starcraft/campaign-test): world.txt, and
#                  drive.txt = "<API key>/<folder id>" if the page should preload your Drive folder
#                  (kept outside the repo; the key is never committed)
#   KEEP=1         keep the save (resume the campaign instead of a fresh one)
#   NOWASM=1       skip the browser build (web/pkg)
#   HOST=<name>    name in the printed URLs (default 127.0.0.1)
export SCBW_MUTE=1
unset DISPLAY WAYLAND_DISPLAY
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.." || exit 1
D=${D:-$HOME/games/Starcraft/campaign-test}; HOST=${HOST:-127.0.0.1}
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
[ -f "$D/world.txt" ] || { echo "no $D/world.txt (scbw campaign gen ... > $D/world.txt)"; exit 1; }
[ -z "$SCBW_CAMPAIGN_DRIVE" ] && [ -f "$D/drive.txt" ] && export SCBW_CAMPAIGN_DRIVE=$(cat "$D/drive.txt")

# Stop the old host: the recorded pid, and any other host on this world file (not other scbw processes).
[ -f "$D/host.pid" ] && kill "$(cat "$D/host.pid")" 2>/dev/null
pkill -f "scbw campaign host $D/world.txt" 2>/dev/null
sleep 2
[ -z "$KEEP" ] && rm -rf "$D/dir"

~/.cargo/bin/cargo build --release 2>&1 | grep -E "^(warning|error)" -A6
[ -z "$NOWASM" ] && { web/build.sh 2>&1 | grep -E "^(warning|error)" -A6; }
B=$PWD/target/release/scbw
[ -x "$B" ] || exit 1

nohup "$B" campaign host "$D/world.txt" "$D/dir" > "$D/host.log" 2>&1 < /dev/null &
echo $! > "$D/host.pid"
for i in $(seq 60); do grep -q "web map" "$D/host.log" && break; kill -0 $! 2>/dev/null || { tail -5 "$D/host.log"; exit 1; }; sleep 0.5; done
line=$(grep "web map" "$D/host.log")
http=$(echo "$line" | grep -o 'http://[^ ]*'); https=$(echo "$line" | grep -o 'https://[^ ]*')
token=$(echo "$line" | grep -o 'token=[0-9a-f]*' | cut -d= -f2)
http=${http/127.0.0.1/$HOST}; https=${https/127.0.0.1/$HOST}
grep -E "^campaign: (started|resumed)" "$D/host.log" | cut -c1-70
echo "http:         $http"
echo "https:        $https"
echo "http admin:   ${http}?token=$token"
echo "https admin:  ${https}?token=$token"
