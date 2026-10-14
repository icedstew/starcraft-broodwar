#!/bin/bash
# fn_of.sh <pattern>: list the decompiled functions whose body matches <pattern> (grep -E),
# e.g. fn_of.sh 'DAT_0051ca14 = ' to find who writes a global.
. "$(dirname "$0")/env.sh"
grep -nE "$1" "$DUMP" | while IFS=: read -r l rest; do
  awk -v L="$l" 'NR<=L && /^\/\/ ==== /{f=$3" "$4} NR==L{print f}' "$DUMP"
done | sort | uniq -c
