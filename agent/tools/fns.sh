#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# fns.sh <hex addr without 0x>...: print the Ghidra decompile of 1.16.1 functions.
. "$(dirname "$0")/env.sh"
for a in "$@"; do
  n=$(grep -n "^// ==== 00$a" "$DUMP" | cut -d: -f1)
  [ -z "$n" ] && { echo "// $a not found"; continue; }
  awk -v s="$n" 'NR>=s { if (NR>s && /^\/\/ ====/) exit; if ($0 !~ /^$/) print }' "$DUMP"
done
