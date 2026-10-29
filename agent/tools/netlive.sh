#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# Phase 3b check (agent/areas/net.md): host + join a live melee game headless on localhost,
# each peer with its own scripted commands; both must print the same `frame K hash H`.
#   netlive.sh [map] [frames] [delay]      SCBW=<binary> (default target/release/scbw)
# Peer scripts: host selects a worker and sends it to (700,500) / (900,700) (right click
# 0x14 x y 0 0 0); the joiner does the same from its own base. Delay 1 on localhost with no
# pacing makes the peers drift apart, so late turns force rollbacks; the rollbacks line is
# printed for each peer.
. "$(dirname "${BASH_SOURCE[0]:-$0}")/env.sh"
MAP=${1:-$INSTALL/maps/(2)Bottleneck.scm}
K=${2:-400}
DELAY=${3:-1}
OUT=${TMPDIR:-/tmp}/netlive.$$
mkdir -p "$OUT"
export SCBW_SEED=${SCBW_SEED:-12345} SCBW_NET_HEADLESS=$K SCBW_NET_RACES=${SCBW_NET_RACES:-t,z}
HS="5:selw;20:14bc02f4010000000000;120:14840364020000000000"
JS="9:selw;31:149001bc020000000000;150:14f003c4010000000000"

SCBW_NET_SCRIPT=$HS "$SCBW" nethost "$MAP" 0 2 "$DELAY" > "$OUT/host.out" 2> "$OUT/host.err" &
HP=$!
CODE=
for _ in $(seq 100); do
    CODE=$(sed -n 's/^LISTEN //p' "$OUT/host.out" 2>/dev/null | head -1)
    [ -n "$CODE" ] && break
    sleep 0.2
done
[ -n "$CODE" ] || { echo "FAIL host did not listen"; cat "$OUT/host.err"; kill $HP 2>/dev/null; exit 1; }
SCBW_NET_SCRIPT=$JS "$SCBW" netjoin "$CODE" "$DELAY" > "$OUT/join.out" 2> "$OUT/join.err" &
JP=$!
wait $HP; HR=$?
wait $JP; JR=$?
HH=$(grep '^frame ' "$OUT/host.out")
JH=$(grep '^frame ' "$OUT/join.out")
echo "host: $HH ($(grep '^player' "$OUT/host.out"))"
echo "join: $JH ($(grep '^player' "$OUT/join.out"))"
if [ -n "$HH" ] && [ "$HH" = "$JH" ] && [ $HR = 0 ] && [ $JR = 0 ]; then
    echo "MATCH netlive $K delay $DELAY"
else
    echo "DIFF netlive $K delay $DELAY (exit $HR $JR; logs in $OUT)"
    exit 1
fi
rm -rf "$OUT"
