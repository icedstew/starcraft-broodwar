#!/bin/bash
export SCBW_MUTE=1   # tests are silent (no sound device opened)
# dis.sh <hex addr> [n insns] [x]: disassemble 1.16.1 StarCraft.exe; "x" keeps going past ret.
. "$(dirname "$0")/env.sh"
exec "$PY" -I "$AGENT/tools/dis32.py" "$EXE" "$@"
