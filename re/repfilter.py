"""Pick 1.16.1-era replays out of downloaded packs, in parallel.

usage: python -I re/repfilter.py <scbw> <out.tsv> <dir>...
Runs `scbw rep` on every .rep under the dirs and writes one row per replay:
  path, ok, start (UTC date), frames, humans, races (Z/T/P per human), map, reason.
ok = parses, Brood War, game start within 1.16.1's time as the live patch
(2009-01-21 .. 2017-04-18; 1.18 came out 2017-04-19), at least two human players.
The start time is the header's creation time (offset 0x08, also the rng seed).
"""
import concurrent.futures as cf
import datetime as dt
import os
import re
import subprocess
import sys

LO = dt.datetime(2009, 1, 21, tzinfo=dt.timezone.utc).timestamp()
HI = dt.datetime(2017, 4, 19, tzinfo=dt.timezone.utc).timestamp()
RACES = "ZTP"
HEAD = re.compile(r'^map "(.*)" \d+x\d+, (\d+) frames, .* start time (\d+)$', re.M)
SLOT = re.compile(r"^  player \d+: id \d+ storm \d+ type (\d+) race (\d+) team \d+ ", re.M)


def check(exe, path):
    try:
        p = subprocess.run([exe, "rep", path], capture_output=True, text=True, errors="replace", timeout=60)
    except subprocess.TimeoutExpired:
        return path, False, "", 0, 0, "", "", "timeout"
    m = HEAD.search(p.stdout)
    if p.returncode != 0 or not m:
        return path, False, "", 0, 0, "", "", "parse: " + (p.stderr.strip().splitlines() or ["?"])[-1][:80]
    mapname, frames, start = m.group(1), int(m.group(2)), int(m.group(3))
    # Slot type 2 = human (observers included); race 0..2 = Z / T / P.
    humans = [int(r) for t, r in SLOT.findall(p.stdout) if t == "2"]
    races = "".join(RACES[r] if r < 3 else "?" for r in humans)
    date = dt.datetime.fromtimestamp(start, dt.timezone.utc).strftime("%Y-%m-%d")
    if not LO <= start < HI:
        why = "date"
    elif len(humans) < 2:
        why = "humans"
    elif "?" in races:
        why = "race"
    elif frames < 24 * 60:
        why = "short"
    else:
        why = ""
    return path, not why, date, frames, len(humans), races, mapname, why


def main():
    exe, out, dirs = sys.argv[1], sys.argv[2], sys.argv[3:]
    files = [os.path.join(d, n) for root in dirs for d, _, fs in os.walk(root) for n in fs if n.lower().endswith(".rep")]
    with cf.ThreadPoolExecutor(os.cpu_count()) as ex, open(out, "w", encoding="utf-8") as f:
        f.write("path\tok\tdate\tframes\thumans\traces\tmap\treason\n")
        for row in ex.map(lambda p: check(exe, p), files):
            f.write("\t".join(str(x) for x in row) + "\n")
    print(f"{len(files)} replays checked -> {out}")


main()
