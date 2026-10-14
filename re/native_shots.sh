#!/bin/bash
# native_shots.sh <replay.rep> <shots dir> [every]
#
# Play a replay in the real 1.16.1 (headless: Xvfb + wine) and save its game screen
# every <every> frames (default 120) as <shots dir>/native/fNNNNNN.png, plus meta.txt
# ("frame screen_x screen_y"). `scbw repshot <replay> <shots dir>` (re/repshots.sh)
# then draws the same frames with scbw and diffs them.
#
# The screen comes straight from the game's 8-bit buffer through a gdb breakpoint after
# each draw (re/native_shots_gdb.py), so frames and pixels are exact. Needs wine, Xvfb,
# gdb, xdotool and ImageMagick, and the 1.16.1 install (SCBW_INSTALL).
set -e
REP=$(realpath "$1"); OUT=$(realpath -m "$2"); EVERY=${3:-120}
RE=$(dirname "$(realpath "$0")")
INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
W=$(mktemp -d "${TMPDIR:-/tmp}/native_shots.XXXXXX")
# No Mono/Gecko: their install prompts stall wineboot in a fresh prefix.
export WINEPREFIX=$W/prefix WINEDEBUG=-all DISPLAY=:78 WINEDLLOVERRIDES='mscoree,mshtml='
mkdir -p "$OUT/native" "$W/cap"
# NATIVE_SHOTS_KEEP=1 keeps the temp dir (with a screenshot after each menu step).
cleanup() { wineserver -k 2>/dev/null || true; kill $XVFB 2>/dev/null || true; [ -n "$NATIVE_SHOTS_KEEP" ] && echo "kept $W" || rm -rf "$W"; }
trap cleanup EXIT

Xvfb :78 -screen 0 640x480x24 >/dev/null 2>&1 &  # must be 640x480
XVFB=$!
wineboot -i >/dev/null 2>&1
G=$WINEPREFIX/drive_c/StarCraft
cp -r "$INSTALL" "$G"
mkdir -p "$G/maps/replays"
cp "$REP" "$G/maps/replays/"
printf 'REGEDIT4\n\n[HKEY_LOCAL_MACHINE\\Software\\Blizzard Entertainment\\Starcraft]\n"InstallPath"="C:\\\\StarCraft"\n"Program"="C:\\\\StarCraft\\\\StarCraft.exe"\n' > "$W/sc.reg"
wine regedit "$W/sc.reg" >/dev/null 2>&1

shot() { import -window root "$W/s.png"; }
# How green a screen region is (menu text is green): w h x y.
green() { shot; magick "$W/s.png" -crop "${1}x${2}+${3}+${4}" -format '%[fx:mean.g-mean.r]' info:; }
step=0
click() { xdotool mousemove "$1" "$2"; sleep 0.3; xdotool mousedown 1; sleep 0.1; xdotool mouseup 1; sleep "${3:-3}"; step=$((step + 1)); import -window root "$W/step$step.png"; }
wait_for() { for _ in $(seq 60); do awk "BEGIN{exit !($(green "$@") > 0.06)}" && return; sleep 5; done; echo "timed out" >&2; exit 1; }

(cd "$G" && winedbg --gdb --no-start 'C:\StarCraft\StarCraft.exe' > "$W/wdbg.txt" 2>&1 &)
for _ in $(seq 30); do grep -q 'localhost:' "$W/wdbg.txt" && break; sleep 1; done
PORT=$(grep -o 'localhost:[0-9]*' "$W/wdbg.txt" | cut -d: -f2)
CAP_OUT=$W/cap CAP_EVERY=$EVERY gdb -q -batch -ex 'set debuginfod enabled off' -ex 'set architecture i386' \
  -ex "target remote localhost:$PORT" -ex "source $RE/native_shots_gdb.py" -ex continue > "$W/gdb.log" 2>&1 &

wait_for 140 16 148 112         # main menu ("Single Player")
click 217 120 4                  # Single Player
click 367 275 6                  # Expansion
xdotool type --delay 100 scbw; xdotool key Return; sleep 2   # new ID
click 515 392 5                  # Ok
click 333 452 4                  # Load Replay
click 150 140 1                  # the replay
click 515 392 15                 # Ok
for _ in $(seq 8); do click 613 457 0.3; done   # speed: Fastest x16

# Done when no new shot for a minute (the replay has finished).
last=-1
while :; do
  sleep 60
  n=$(wc -l < "$W/cap/meta.txt" 2>/dev/null || echo 0)
  [ "$n" = "$last" ] && break
  last=$n; echo "$n shots"
done
[ -s "$W/cap/meta.txt" ] || { echo "no shots captured" >&2; exit 1; }
for f in "$W"/cap/*.raw; do python3 -I "$RE/raw2png.py" "$f" "$OUT/native/$(basename "$f" .raw).png"; done
cp "$W/cap/meta.txt" "$OUT/native/"
echo "$(wc -l < "$OUT/native/meta.txt") shots in $OUT/native"
