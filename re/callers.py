"""Functions in a C dump that call `name` (FUN_xxx or named). usage: python3 -I callers.py dump.c FUN_004ec170"""
import re, sys
src = open(sys.argv[1]).read()
target = sys.argv[2]
for m in re.finditer(r'^// ==== ([0-9a-f]+) (\S+)\n', src, re.M):
    end = src.find('\n// ==== ', m.end())
    body = src[m.end():end if end > 0 else None]
    if re.search(rf'\b{re.escape(target)}\(', body) and not body.lstrip().startswith(('void ' + target, 'int ' + target)):
        print(m.group(1), m.group(2), body.count('\n'))
