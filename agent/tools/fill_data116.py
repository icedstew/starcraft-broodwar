"""fill_data116.py <py> <exe> <data> <mapdump dir> <scbw> <W>: run re/emu_game.py until it's
missing no file, extracting each missing one with `scbw extract` (data116/ on a new box)."""
import os, re, subprocess, sys, ast
py, exe, data, md, scbw, W = sys.argv[1:7]
tried = set()
for i in range(2000):
    r = subprocess.run([py, "-I", "re/emu_game.py", exe, data, md, "0,1500,1500,0", "600,700", "5", W + "/emu_trace.txt"], capture_output=True, text=True)
    log = r.stdout + r.stderr
    names = set()
    for m in re.findall(r"missing (\[.*?\])", log):
        try: names |= set(ast.literal_eval(m))
        except Exception: pass
    names -= tried
    if not names:
        print("done after", i, "runs"); print(log[-600:]); break
    for f in names:
        tried.add(f)
        out = os.path.join(data, f.replace("\\", "/").lower())
        os.makedirs(os.path.dirname(out), exist_ok=True)
        ok = subprocess.run([scbw, "extract", f, out], capture_output=True).returncode == 0 and os.path.exists(out)
        print(("+ " if ok else "! ") + f, flush=True)
