#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# enhanced_win.sh <out dir>: an enhanced game in real windows (two Xvfb displays :91 host,
# :92 joiner, :93 late joiner; 1280x960: the window is 2x), driven by xdotool; screenshots of both screens into <out dir>. The host
# creates an enhanced game and starts it alone with the joiner (SCBW_JOIN, lobby), a third
# window (:93) joins the running game by code. Needs Xvfb, xdotool, ImageMagick.
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}; O=$(realpath -m "${1:-/tmp/scbw-enhanced-win}"); P=${P:-16450}
rm -rf $O; mkdir -p $O/h $O/j $O/k
for d in 91 92 93; do (Xvfb :$d -screen 0 1280x960x24 >/dev/null 2>&1 &); done; sleep 2
cleanup() { pkill -f "^Xvfb :9[123]" ; pkill -f "^$B glue"; }
trap cleanup EXIT
shot() { DISPLAY=:$1 import -window root $O/$2.png; }
# Keys are held 0.04 s (about two frames: the window samples key state once per frame).
key() { DISPLAY=:$1 xdotool mousemove 640 800; sleep 0.3; DISPLAY=:$1 xdotool keydown "$2"; sleep 0.04; DISPLAY=:$1 xdotool keyup "$2"; [ -n "$STEPS" ] && { sleep 1; n=$((n+1)); shot $1 step$n; }; }
click() { DISPLAY=:$1 xdotool mousemove $(($2*2)) $(($3*2)); sleep 0.3; DISPLAY=:$1 xdotool mousedown 1; sleep 0.1; DISPLAY=:$1 xdotool mouseup 1; }
export SCBW_LAN_PORT=$P
(cd $O; DISPLAY=:91 SCBW_ENHANCED=1 SCBW_CHARACTERS=$O/h $B glue > host.log 2>&1 &)
sleep 12; click 91 320 240; sleep 3; key 91 m; sleep 3; key 91 e; sleep 3; key 91 o; sleep 3
DISPLAY=:91 xdotool type --delay 100 host; key 91 Return; sleep 2; key 91 Return; sleep 4
click 91 110 345; sleep 4; shot 91 create
for i in $(seq 40); do grep -q LOBBY $O/host.log 2>/dev/null && break; sleep 0.5; done
key 91 o; sleep 5; shot 91 lobby
code=$(sed -n 's/^LOBBY //p' $O/host.log | sed 's/0\.0\.0\.0/127.0.0.1/')
echo "code $code"
(cd $O; DISPLAY=:92 SCBW_JOIN=$code SCBW_NAME=two SCBW_CHARACTERS=$O/j $B glue > join.log 2>&1 &)
sleep 15; shot 92 joiner_lobby; shot 91 lobby2
click 91 500 393; sleep 15; shot 91 game_host; shot 92 game_two
(cd $O; DISPLAY=:93 SCBW_JOIN=$code SCBW_NAME=three SCBW_CHARACTERS=$O/k SCBW_INSTALL=$SCBW_INSTALL $B glue > late.log 2>&1 &)
# The join line lasts 7 s: shoot every 2 s (join_host_N, join_two_N, join_three_N).
for i in $(seq 15); do sleep 2; shot 91 join_host_$i; shot 92 join_two_$i; shot 93 join_three_$i; done
shot 93 game_three; shot 91 game_host2; shot 92 game_two2
# The second player's display goes away: its window ends, the game drops it (3 s) and the
# computer takes its army.
pkill -f "^Xvfb :92"; sleep 8; shot 91 host_after_quit; shot 93 three_after_quit
echo done
