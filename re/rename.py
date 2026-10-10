"""Apply data/names.txt to the Ghidra C dump: FUN_1401e7650 -> update_unit, etc.
usage: python3 -I rename.py in.c out.c"""
import re, sys
names = {}
nf = 'data/names.txt'
if '--names' in sys.argv:
    i = sys.argv.index('--names')
    nf = sys.argv[i + 1]
    del sys.argv[i:i + 2]
for l in open(__file__.replace('rename.py', nf)):
    if l.strip() and not l.startswith('#'):
        a, n = l.split()[:2]
        names[f'FUN_{int(a, 16):08x}'] = n
src = open(sys.argv[1]).read()
open(sys.argv[2], 'w').write(re.sub(r'FUN_[0-9a-f]{8,}', lambda m: names.get(m.group(0), m.group(0)), src))
