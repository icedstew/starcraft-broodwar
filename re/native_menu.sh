#!/bin/bash
# native_menu.sh <replay.rep> <out dir>
#
# Reference screenshots of the real 1.16.1's in-game menus (headless: Xvfb + wine, no
# debugger): loads the replay, pauses it, then opens the game menu (F10 / MENU) and its
# Options / Speed / Sound / Video / Help dialogs, saving the whole 640x480 screen after
# each step as <out dir>/NN_<name>.png. Needs wine, Xvfb, xdotool, ImageMagick and the
# 1.16.1 install (SCBW_INSTALL).
set -e
REP=$(realpath "$1"); OUT=$(realpath -m "$2")
INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
W=$(mktemp -d "${TMPDIR:-/tmp}/native_menu.XXXXXX")
export WINEPREFIX=$W/prefix WINEDEBUG=-all DISPLAY=:78 WINEDLLOVERRIDES='mscoree,mshtml='
mkdir -p "$OUT"
cleanup() { wineserver -k 2>/dev/null || true; kill $XVFB 2>/dev/null || true; rm -rf "$W"; }
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
green() { shot; magick "$W/s.png" -crop "${1}x${2}+${3}+${4}" -format '%[fx:mean.g-mean.r]' info:; }
wait_for() { for _ in $(seq 60); do awk "BEGIN{exit !($(green "$@") > 0.06)}" && return; sleep 5; done; echo "timed out" >&2; exit 1; }
step=0
click() { xdotool mousemove "$1" "$2"; sleep 0.3; xdotool mousedown 1; sleep 0.1; xdotool mouseup 1; sleep "${3:-2}"; step=$((step + 1)); [ -n "$NATIVE_MENU_STEPS" ] && import -window root "$OUT/step$step.png"; true; }
n=0
save() { n=$((n + 1)); xdotool mousemove 639 0; sleep 0.5; import -window root "$OUT/$(printf %02d $n)_$1.png"; }
(cd "$G" && wine StarCraft.exe >/dev/null 2>&1 &)
wait_for 140 16 148 112          # main menu ("Single Player")
click 217 120 4                  # Single Player
click 367 275 6                  # Expansion
xdotool type --delay 100 scbw; xdotool key Return; sleep 2
click 515 392 5                  # Ok
click 333 452 4                  # Load Replay
click 150 140 1                  # the replay
click 515 392 20                 # Ok
save hud
click 447 397 2; save gamemenu   # MENU
click 315 149 2; save options    # Options
click 315 81 2; save speed       # Speed
click 375 297 2                  # Cancel
click 315 115 2; save sound      # Sound
click 375 297 2
click 315 149 2; save video      # Video
click 375 297 2
click 315 183 2; save network    # Network
xdotool key Escape; sleep 2
click 315 297 2; save back       # Previous
click 315 183 2; save help       # Help
xdotool key Escape; sleep 2; save after_help
xdotool key Escape; sleep 2; save closed
xdotool key F10; sleep 2; save f10
echo "$n shots in $OUT"
