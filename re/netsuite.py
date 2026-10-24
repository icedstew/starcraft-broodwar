"""Run the rollback / netcode oracles over many replays in parallel.

usage: python -I re/netsuite.py <scbw> <list.txt> <out.tsv> [jobs, default 4]
An existing out.tsv is resumed: its MATCH rows are kept, the rest rerun.
list.txt: one replay path per line. For each replay runs
  rbtest <rep> 3                 (late commands + rollback == straight playback)
  nettest <rep> 2 3 2            (rollback sessions over a simulated link)
  nettest <rep> 1 8 6            (latency + jitter past the prediction limit)
and writes path, test, result (first word: MATCH / DIFF / INVALID / ERROR / TIMEOUT),
seconds, first line of output. SCBW_INSTALL must be set.
"""
import concurrent.futures as cf
import os
import subprocess
import sys
import time

TESTS = [("rbtest3", ["rbtest", None, "3"]), ("net232", ["nettest", None, "2", "3", "2"]),
         ("net186", ["nettest", None, "1", "8", "6"])]


def run(exe, rep, name, argv):
    argv = [exe] + [rep if a is None else a for a in argv]
    t = time.time()
    try:
        p = subprocess.run(argv, capture_output=True, text=True, errors="replace", timeout=7200)
        out = (p.stdout.strip().splitlines() or p.stderr.strip().splitlines() or ["?"])
        line = next((l for l in out if l.split()[:1] and l.split()[0] in ("MATCH", "DIFF", "INVALID")), out[-1])
        res = line.split()[0] if line.split()[:1] and line.split()[0] in ("MATCH", "DIFF", "INVALID") else "ERROR"
        if p.returncode != 0:
            res, line = "ERROR", (p.stderr.strip().splitlines() or ["exit %d" % p.returncode])[-1]
    except subprocess.TimeoutExpired:
        res, line = "TIMEOUT", ""
    return rep, name, res, round(time.time() - t, 1), line[:200].replace("\t", " ")


def main():
    exe, lst, out = sys.argv[1:4]
    exe = os.path.abspath(exe)
    jobs = int(sys.argv[4]) if len(sys.argv) > 4 else 4
    reps = [l.strip() for l in open(lst, encoding="utf-8") if l.strip()]
    # Resume: keep the MATCH rows of an earlier run, redo everything else.
    kept = []
    if os.path.exists(out):
        kept = [l for l in open(out, encoding="utf-8").read().splitlines()[1:] if l.split("\t")[2:3] == ["MATCH"]]
    done = {tuple(l.split("\t")[:2]) for l in kept}
    work = [(r, n, a) for r in reps for n, a in TESTS if (r, n) not in done]
    counts = {}
    with cf.ThreadPoolExecutor(jobs) as ex, open(out, "w", encoding="utf-8") as f:
        f.write("path\ttest\tresult\tseconds\tline\n")
        for l in kept:
            f.write(l + "\n")
        for row in ex.map(lambda w: run(exe, *w), work):
            f.write("\t".join(str(x) for x in row) + "\n")
            f.flush()
            counts[(row[1], row[2])] = counts.get((row[1], row[2]), 0) + 1
    for k in sorted(counts):
        print(k[0], k[1], counts[k])


main()
