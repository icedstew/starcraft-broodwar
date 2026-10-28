#!/bin/bash
# native_join.sh <out dir>
#
# Reference screenshots of the real 1.16.1's Multiplayer screens (LAN): Select Connection and
# the Games list, mouse off and over the list. Headless like native_menu.sh. Keys follow
# our glue script (m = Multiplayer, Return, o = Ok).
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
n=0
save() { n=$((n + 1)); sleep 1; import -window root "$OUT/$(printf %02d $n)_$1.png"; }
mv_() { xdotool mousemove "$1" "$2"; sleep 1; }
if [ -n "$NATIVE_COLORS" ]; then
  # Under gdb (winedbg --gdb), to read the interface color table 0x6ceb20 at the end.
  (cd "$G" && winedbg --gdb --no-start 'C:\StarCraft\StarCraft.exe' > "$W/wdbg.txt" 2>&1 &)
  for _ in $(seq 30); do grep -q 'localhost:' "$W/wdbg.txt" && break; sleep 1; done
  PORT=$(grep -o 'localhost:[0-9]*' "$W/wdbg.txt" | cut -d: -f2)
  gdb -q -batch -ex 'set debuginfod enabled off' -ex 'set architecture i386' -ex "target remote localhost:$PORT" ${NATIVE_GDB_EX:+-x "$NATIVE_GDB_EX"} -ex continue \
    -ex 'x/26xb 0x6ceb20' -ex 'x/8xw 0x6ceb20' > "$OUT/colors.txt" 2>&1 &
  GDBP=$!
else
  (cd "$G" && wine StarCraft.exe >/dev/null 2>&1 &)
fi
wait_for 140 16 148 112          # main menu
# No window manager under Xvfb: keys go to the window under the pointer, so keep it inside.
key() { xdotool mousemove 320 400; sleep 0.3; xdotool key "$@"; sleep "${KS:-3}"; }
key m; save multi                             # Multiplayer
key e; save conn_out                          # Expansion; Select Connection, mouse off the list
mv_ 150 150; save conn_over
xdotool mousemove 150 182; sleep 0.3; xdotool mousedown 1; sleep 0.1; xdotool mouseup 1; sleep 1; save conn_udp
key o; save id                                # Ok
xdotool type --delay 100 scbw; xdotool key Return; sleep 2; save newid
key Return; sleep 3; save games_in            # the ID's Ok
mv_ 600 20; save games_out
mv_ 200 150; save games_over
mv_ 200 250; save games_over2
mv_ 110 345; xdotool mousedown 1; sleep 0.1; xdotool mouseup 1; sleep 4; mv_ 600 20; save create
if [ -n "$GDBP" ]; then kill -INT $GDBP; sleep 5; fi
echo "$n shots in $OUT"
