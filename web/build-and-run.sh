#!/bin/sh
# Build, link the local install and a map into web/auto (gitignored), serve, and open
# index.html?auto=<map>, which loads them without the file pickers.
# Usage: web/build-and-run.sh [map]   (default: Boxer from $SCBW_INSTALL/maps)
set -e
cd "$(dirname "$0")"
./build.sh
INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
MAP=$(realpath "${1:-$INSTALL/maps/(2)Boxer.scm}")
rm -rf auto
mkdir auto
for f in storm.dll patch_rt.mpq broodat.mpq stardat.mpq; do
  src=$(find "$INSTALL" -maxdepth 1 -iname "$f" | head -n1)
  [ -n "$src" ] || { echo "missing $f in $INSTALL" >&2; exit 1; }
  ln -s "$src" "auto/$f"
done
name="map.${MAP##*.}"
ln -s "$MAP" "auto/$name"
url="http://localhost:8000/?auto=$name"
echo "serving $url"
(sleep 1; xdg-open "$url" >/dev/null 2>&1) &
exec python3 -m http.server -b 127.0.0.1 8000
