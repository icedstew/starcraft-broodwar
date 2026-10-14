#!/bin/sh
# repshots.sh <replay.rep> <shots dir>
#
# Draw the replay's frames listed in <shots dir>/native/meta.txt (from native_shots.sh)
# with scbw and compare them with 1.16.1's: writes <shots dir>/ours/, <shots dir>/diff/
# (native | ours | red where they differ) and report.txt (share of differing pixels
# per frame).
set -e
cd "$(dirname "$(realpath "$0")")/.."
cargo build --release -q
./target/release/scbw repshot "$1" "$2"
