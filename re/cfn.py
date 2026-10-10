"""Print decompiled C for functions by address or name from the named dump.
usage: python3 -I cfn.py named.c <addr|name>..."""
import re, sys
src = open(sys.argv[1]).read()
for q in sys.argv[2:]:
    q = q.lower().replace('0x', '') if re.fullmatch(r'(0x)?[0-9a-fA-F]{9,}', q) else q
    pat = rf'^// ==== ({q}) .*$' if re.fullmatch(r'[0-9a-f]{9,}', q) else rf'^// ==== ([0-9a-f]+) .*\n(?:.*\n){{0,4}}.*\b{re.escape(q)}\('
    m = re.search(pat, src, re.M)
    if not m:
        print(f'// {q}: not found'); continue
    end = src.find('\n// ==== ', m.end())
    print(src[m.start():end if end > 0 else None])
