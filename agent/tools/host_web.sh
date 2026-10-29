#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# host_web.sh [map] [players] [delay]: host a networked game in a scbw window, serve the
# browser page, print the join link. LAN=1 listens on every interface (the link gets this
# machine's LAN IP); PORT (4433) / WEBPORT (8000) override the ports. Ctrl+C or closing the
# game stops the page server too. The browser must load the page from localhost
# (WebTransport needs a secure context): on another machine, serve web/ there.
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
MAP=${1:-(2)Bottleneck.scm}; [ -f "$MAP" ] || MAP="$SCBW_INSTALL/maps/$MAP"
PORT=${PORT:-4433}; WEBPORT=${WEBPORT:-8000}
IP=127.0.0.1
if [ -n "$LAN" ]; then
    export SCBW_NET_BIND=0.0.0.0
    IP=$(ip -4 route get 1.1.1.1 | sed -n 's/.* src \([0-9.]*\).*/\1/p')
fi
python3 -m http.server -d web "$WEBPORT" -b 127.0.0.1 > /dev/null 2>&1 & HP=$!
trap 'kill $HP 2>/dev/null' EXIT
./target/release/scbw nethost "$MAP" "$PORT" "${2:-2}" "${3:-2}" | while read -r l; do
    case $l in LISTEN\ *)
        c=${l#LISTEN }; c=$IP:${c#*:}
        echo "JOIN: http://localhost:$WEBPORT/?join=${c/\#/%23}" ;;
    esac
done
