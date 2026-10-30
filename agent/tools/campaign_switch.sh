#!/bin/bash
# campaign_switch.sh: AI <-> human army handoff sweep (`scbw campaign switch`, runner.rs).
# Every map x seed x mode; prints each run with a BUG line and a summary; exit 1 if any.
# MAPS / SEEDS / MODES / CYCLES override the defaults; J parallel runs (default nproc).
# Modes: plain, rb (rollback replay of each join / leave), shared (two humans on the army),
# busy (the human trains workers), quick (24-frame human / 48-frame AI phases, 20 cycles).
export SCBW_MUTE=1
unset DISPLAY WAYLAND_DISPLAY
T=$(cd "$(dirname "$0")" && pwd); cd "$T/../.."
export SCBW_INSTALL=${SCBW_INSTALL:-$HOME/Downloads/StarCraft-BroodWar_1.16.1}
B=${SCBW:-$PWD/target/release/scbw}
M=$SCBW_INSTALL/maps
MAPS=${MAPS:-"(2)Bottleneck.scm|(4)Lost Civilization.scm|(2)Boxer.scm|(2)Challenger.scm"}
SEEDS=${SEEDS:-"1 2 3 4 5 6"}
MODES=${MODES:-"plain rb shared busy quick"}
O=${O:-/tmp/scbw-switch}; mkdir -p "$O"; rm -f "$O"/*.log
run() {  # mode seed map
  local env="" c=${CYCLES:-3}
  case $1 in
    rb) env="SWITCH_RB=1" ;; shared) env="SWITCH_SHARED=1" ;; busy) env="SWITCH_BUSY=1" ;;
    quick) env="SWITCH_HUMAN=24 SWITCH_AI=48"; c=20 ;;
  esac
  local log="$O/$1-$2-$(basename "$3" .scm | tr -c 'A-Za-z0-9\n' _).log"
  env $env SEED=$2 timeout 900 "$B" campaign switch "$M/$3" $c > "$log" 2>&1
  local r=$(grep -E '^BUG' "$log" | head -3 | tr '\n' ' ')
  grep -q ' bugs$' "$log" || r="did not finish: $(tail -1 "$log")"
  [ -n "$r" ] && echo "$1 seed $2 $3: $r"
}
export -f run; export B M O CYCLES
IFS='|' read -ra maps <<< "$MAPS"
for mode in $MODES; do for seed in $SEEDS; do for map in "${maps[@]}"; do
  printf '%s\0%s\0%s\0' "$mode" "$seed" "$map"
done; done; done | xargs -0 -n3 -P"${J:-$(nproc)}" bash -c 'run "$0" "$1" "$2"' > "$O/summary.txt"
cat "$O/summary.txt"
n=$(ls "$O"/*.log | wc -l); bad=$(wc -l < "$O/summary.txt")
echo "$((n - bad)) / $n runs clean (logs: $O)"
[ "$bad" = 0 ]
