#!/bin/bash
# Phase 3d checks (agent/areas/net.md): headless live games on localhost.
#   netlive_leave.sh [a|b|c|d|e|f|all]      SCBW=<binary> (default target/release/scbw)
#   a  2 players, 3000 frames, delay 1: equal hashes; each peer's replay (SCBW_NET_REPLAY,
#      confirmed frames only) played back straight (`scbw rephash`) gives the live hash
#   b  3 players on (4)Dark Crystal (host relays between two clients): all hashes equal
#   c  3 players, one joiner killed mid-game: the others continue to the end with equal
#      hashes; the host log shows the Leave
#   d  forced desync (SCBW_NET_DEBUG_DESYNC on one joiner): the host reports Desync
#   e  clean quit (SCBW_NET_QUIT_AT: the Drop of the game announces the leave)
#   f  the host quits: the joiner ends the game ("host left")
. "$(dirname "${BASH_SOURCE[0]:-$0}")/env.sh"
WHAT=${1:-all}
OUT=${TMPDIR:-/tmp}/netleave.$$
mkdir -p "$OUT"
export SCBW_SEED=${SCBW_SEED:-12345} SCBW_NET_RACES=${SCBW_NET_RACES:-t,z,p}
FAIL=0
# Scripts: each peer selects a worker and sends it somewhere, at a few frames. The last entry
# is a harmless select: 1.16.1 replay playback never runs the final command group (0x4cdff0
# returns -1 at the end of the data), so the replay comparison needs a last group that does nothing.
S0="5:selw;20:14bc02f4010000000000;120:14840364020000000000;900:14a00360020000000000;2000:14c00280020000000000;2900:selw"
S1="9:selw;31:149001bc020000000000;150:14f003c4010000000000;1100:14d002c0020000000000;2100:14b00290020000000000;2910:selw"
S2="12:selw;40:14900270020000000000;300:14e00380020000000000;1300:14a001f0010000000000;2920:selw"

# game <tag> <map> <players> <frames> <delay>: starts the host, then the joiners (env
# JOIN_ENV_<i>="VAR=val" per joiner i, optional). Sets PIDS (host first). Output in $OUT/<tag>.<n>.*
game() {
    local tag=$1 map=$2 n=$3 k=$4 delay=$5
    local scripts=("${SC0:-$S0}" "${SC1:-$S1}" "$S2")
    local hextra=()
    [ -n "$HOST_ENV" ] && hextra=(env "$HOST_ENV")
    SCBW_NET_HEADLESS=$k SCBW_NET_SCRIPT=${scripts[0]} SCBW_NET_REPLAY="$OUT/$tag.0.rep" "${hextra[@]}" "$SCBW" nethost "$map" 0 "$n" "$delay" > "$OUT/$tag.0.out" 2> "$OUT/$tag.0.err" &
    PIDS=($!)
    local code=
    for _ in $(seq 150); do
        code=$(sed -n 's/^LISTEN //p' "$OUT/$tag.0.out" 2>/dev/null | head -1)
        [ -n "$code" ] && break
        sleep 0.2
    done
    [ -n "$code" ] || { echo "FAIL $tag: host did not listen"; cat "$OUT/$tag.0.err"; kill ${PIDS[0]} 2>/dev/null; return 1; }
    local i
    for ((i = 1; i < n; i++)); do
        local extra=()
        local v="JOIN_ENV_$i"
        [ -n "${!v}" ] && extra=(env "${!v}")
        SCBW_NET_HEADLESS=$k SCBW_NET_SCRIPT=${scripts[$i]} SCBW_NET_REPLAY="$OUT/$tag.$i.rep" "${extra[@]}" "$SCBW" netjoin "$code" "$delay" > "$OUT/$tag.$i.out" 2> "$OUT/$tag.$i.err" &
        PIDS+=($!)
        sleep 0.3
    done
}

hashes_equal() { # tag n... (peer indexes): all `frame` lines equal and non-empty
    local tag=$1; shift
    local first= i h
    for i in "$@"; do
        h=$(grep '^frame ' "$OUT/$tag.$i.out")
        echo "  peer $i: $h ($(grep '^player' "$OUT/$tag.$i.out"))"
        [ -n "$h" ] || return 1
        [ -z "$first" ] && first=$h
        [ "$h" = "$first" ] || return 1
    done
}

verdict() { # name ok
    if [ "$2" = 0 ]; then echo "PASS $1"; else echo "FAIL $1 (logs in $OUT)"; FAIL=1; fi
}

if [[ $WHAT == a || $WHAT == all ]]; then
    AF=${A_FRAMES:-3000}
    game a "$INSTALL/maps/(2)Bottleneck.scm" 2 $AF 1
    wait "${PIDS[@]}"
    hashes_equal a 0 1; ok=$?
    # Replay check: confirmed-only recording must replay to the live hash.
    live=$(grep '^frame ' "$OUT/a.0.out" | awk '{print $4}')
    for i in 0 1; do
        rh=$("$SCBW" rephash "$OUT/a.$i.rep" $AF 2>&1 | grep '^frame ' | awk '{print $4}')
        echo "  replay of peer $i: $rh (live $live)"
        [ "$rh" = "$live" ] || ok=1
    done
    verdict "a: 2 players 3000 frames, equal hashes, replay = live" $ok
fi

if [[ $WHAT == b || $WHAT == all ]]; then
    game b "$INSTALL/maps/(4)Dark Crystal.scm" 3 1500 1
    wait "${PIDS[@]}"
    hashes_equal b 0 1 2; ok=$?
    live=$(grep '^frame ' "$OUT/b.0.out" | awk '{print $4}')
    rh=$("$SCBW" rephash "$OUT/b.0.rep" 1500 2>&1 | grep '^frame ' | awk '{print $4}')
    echo "  replay of peer 0: $rh (live $live)"
    [ "$rh" = "$live" ] || ok=1
    verdict "b: 3 players, host relay, equal hashes" $ok
fi

if [[ $WHAT == c || $WHAT == all ]]; then
    # A long game; joiner 2 is killed after a few seconds (a hard kill: no goodbye, the
    # host notices through the transport timeout).
    # The leave (~frame 11000) must not be the replay's last command group (1.16.1 playback
    # never runs that one): the survivors select once more near the end.
    export SCBW_NET_TIMEOUT_MS=3000
    SC0="$S0;19900:selw" SC1="$S1;19910:selw" game c "$INSTALL/maps/(4)Dark Crystal.scm" 3 20000 1
    sleep 3
    kill -9 "${PIDS[2]}" 2>/dev/null || taskkill //F //PID "${PIDS[2]}" > /dev/null 2>&1
    wait "${PIDS[0]}" "${PIDS[1]}"
    unset SCBW_NET_TIMEOUT_MS
    hashes_equal c 0 1; ok=$?
    grep -h "LEAVE\|leave of player\|FENCE" "$OUT/c.0.err" "$OUT/c.1.err" | sed 's/^/  /'
    grep -q "LEAVE player" "$OUT/c.0.err" || ok=1
    grep -q "FENCE VIOLATION" "$OUT"/c.*.err && ok=1
    grep -q "^frame 20000" "$OUT/c.0.out" || ok=1
    # The leave is in the recorded replay (0x57 at the fence): playing it back matches too.
    live=$(grep '^frame ' "$OUT/c.0.out" | awk '{print $4}')
    rh=$("$SCBW" rephash "$OUT/c.0.rep" 20000 2>&1 | grep '^frame ' | awk '{print $4}')
    echo "  replay of peer 0: $rh (live $live)"
    [ "$rh" = "$live" ] || ok=1
    verdict "c: joiner killed mid-game, the others finish with equal hashes, replay = live" $ok
fi

if [[ $WHAT == e || $WHAT == all ]]; then
    # A clean quit (the in-game menu path): the quitter announces itself, no timeout wait.
    JOIN_ENV_2="SCBW_NET_QUIT_AT=1500"
    game e "$INSTALL/maps/(4)Dark Crystal.scm" 3 ${E_FRAMES:-4000} 1
    wait "${PIDS[@]}"
    unset JOIN_ENV_2
    hashes_equal e 0 1; ok=$?
    live=$(grep '^frame ' "$OUT/e.0.out" | awk '{print $4}')
    rh=$("$SCBW" rephash "$OUT/e.0.rep" ${E_FRAMES:-4000} 2>&1 | grep '^frame ' | awk '{print $4}')
    echo "  replay of peer 0: $rh (live $live)"
    [ "$rh" = "$live" ] || ok=1
    grep -h "LEAVE" "$OUT/e.0.err" | sed 's/^/  /'
    grep -q "LEAVE player 2 (reason 1)" "$OUT/e.0.err" || ok=1
    verdict "e: clean quit announces the leave (reason 1), the others finish equal" $ok
fi

if [[ $WHAT == f || $WHAT == all ]]; then
    # The host quits: the clients end the game (host left).
    HOST_ENV="SCBW_NET_QUIT_AT=800"
    game f "$INSTALL/maps/(2)Bottleneck.scm" 2 4000 1
    wait "${PIDS[@]}"
    unset HOST_ENV
    ok=0
    grep -q "game over: the host left" "$OUT/f.1.out" || ok=1
    grep "game over" "$OUT/f.1.out" | sed 's/^/  /'
    verdict "f: host quits, the client ends the game" $ok
fi

if [[ $WHAT == d || $WHAT == all ]]; then
    JOIN_ENV_1="SCBW_NET_DEBUG_DESYNC=125"
    game d "$INSTALL/maps/(2)Bottleneck.scm" 2 600 1
    wait "${PIDS[@]}"
    unset JOIN_ENV_1
    ok=0
    grep -q "DESYNC detected" "$OUT/d.0.err" && grep -q "DESYNC detected" "$OUT/d.1.err" || ok=1
    grep -h "DESYNC\|hash mismatch" "$OUT"/d.*.err | sed 's/^/  /'
    verdict "d: forced desync reported on both peers" $ok
fi

[ $FAIL = 0 ] && rm -rf "$OUT"
exit $FAIL
