#!/bin/bash
# native_game.sh <out dir>
#
# Reference screenshots of the real 1.16.1's in-game interface (headless: Xvfb + wine):
# starts a custom game on (2)Astral Balance as Terran against the computer, then saves
# the whole 640x480 screen as <out dir>/NN_<name>.png: the HUD with nothing selected,
# the command center selected, training an SCV, a group of SCVs, a command button's
# tooltip and the game menu. Needs wine, Xvfb, xdotool, ImageMagick and the 1.16.1
# install (SCBW_INSTALL).
set -e
OUT=$(realpath -m "$1")
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
click 322 414 6                  # Play Custom
# Race: hold the drop-down open and release on Terran.
xdotool mousemove 268 323; sleep 0.3; xdotool mousedown 1; sleep 1; xdotool mousemove 216 353; sleep 0.5; xdotool mouseup 1; sleep 1
click 515 392 25                 # Ok
click 200 262 3                  # the tips dialog's OK (shown at startup)
save hud
click 127 380 2                  # the minimap at our base
click 420 200 2; save cc         # the command center
xdotool key s; sleep 1; save train
xdotool mousemove 250 120; sleep 0.3; xdotool mousedown 1; sleep 0.2; xdotool mousemove 630 320; sleep 0.3; xdotool mouseup 1; sleep 1; save group
xdotool mousemove 523 375; sleep 2; import -window root "$OUT/$(printf %02d $((n + 1)))_tooltip.png"; n=$((n + 1))
xdotool key F10; sleep 2; save gamemenu
echo "$n shots in $OUT"
