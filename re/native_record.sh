#!/bin/bash
# native_record.sh <out dir>
#
# Play the real 1.16.1 yourself in a 640x480 Xephyr window while your input and the
# screen are recorded (re/record_glue.py record): menus, glue and games. Recording starts
# at the main menu (skip the intro first); close the game to stop. Then
# `re/record_glue.py replay <out dir>` plays the same input in `scbw glue` and diffs the
# screens. NATIVE_RECORD_HEADLESS=1: Xvfb instead (drive it with xdotool on :79).
# Needs Xephyr (xorg-server-xephyr), wine + winegcc, ImageMagick and the 1.16.1 install
# (SCBW_INSTALL).
set -e
OUT=$(realpath -m "$1")
RE=$(dirname "$(realpath "$0")")
INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
[ -f "$RE/bwmem/bwmem.exe.so" ] || (cd "$RE/bwmem" && winegcc -o bwmem.exe bwmem.c)
W=$(mktemp -d "${TMPDIR:-/tmp}/native_record.XXXXXX")
HOST_DISPLAY=$DISPLAY
export WINEPREFIX=$W/prefix WINEDEBUG=-all DISPLAY=:79 WINEDLLOVERRIDES='mscoree,mshtml='
mkdir -p "$OUT"
# The game's own replays (LastReplay.rep after each game) are kept as <out dir>/replays/ for
# agent/tools/repcmp.sh (the simulation, frame by frame).
cleanup() {
  wineserver -k 2>/dev/null || true; kill $XSRV 2>/dev/null || true
  [ -d "$G/maps/replays" ] && cp -r "$G/maps/replays" "$OUT/" 2>/dev/null
  rm -rf "$W"
}
trap cleanup EXIT
if [ -n "$NATIVE_RECORD_HEADLESS" ]; then
  Xvfb :79 -screen 0 640x480x24 >/dev/null 2>&1 &  # must be 640x480
else
  DISPLAY=$HOST_DISPLAY Xephyr :79 -screen 640x480x24 -title 'StarCraft 1.16.1 (recording)' >/dev/null 2>&1 &
fi
XSRV=$!
sleep 1
wineboot -i >/dev/null 2>&1
G=$WINEPREFIX/drive_c/StarCraft
cp -r "$INSTALL" "$G"
# A 640x480 wine virtual desktop (re/bwmem starts the game on it): the game's mode change
# stays inside it (through Xephyr's RandR, wine's 640x480 came out rotated by 90 degrees).
printf 'REGEDIT4\n\n[HKEY_LOCAL_MACHINE\\Software\\Blizzard Entertainment\\Starcraft]\n"InstallPath"="C:\\\\StarCraft"\n"Program"="C:\\\\StarCraft\\\\StarCraft.exe"\n\n[HKEY_CURRENT_USER\\Software\\Wine\\Explorer\\Desktops]\n"sc"="640x480"\n' > "$W/sc.reg"
wine regedit "$W/sc.reg" >/dev/null 2>&1
python3 -I "$RE/record_glue.py" record "$OUT" "$G"
