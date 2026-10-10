"""Dump BW button sets from the x64 dump: 250 x {u64 count, Button* ptr, u64 linked}
Button (32 bytes): u16 pos, u16 icon, pad, cond*, act*, u16 cond_var, u16 act_var, u16 enabled_str, u16 disabled_str.
usage: python3 -I buttons.py dump.exe [--summary]"""
import collections, struct, sys
d = open(sys.argv[1], 'rb').read(); B = 0x140000000
TABLE = 0x140dd9e80
sets = []
for i in range(250):
    n, p, link = struct.unpack_from('<QQQ', d, TABLE - B + i * 24)
    btns = [struct.unpack_from('<HH4xQQHHHH', d, p - B + k * 32) for k in range(n)] if p else []
    sets.append((link, btns))
if '--summary' in sys.argv:
    conds = collections.Counter(b[2] for _, bs in sets for b in bs)
    acts = collections.Counter(b[3] for _, bs in sets for b in bs)
    print(len(conds), 'conditions,', len(acts), 'actions')
    print('conds', [(hex(a), c) for a, c in conds.most_common()])
    print('acts', [(hex(a), c) for a, c in acts.most_common()])
else:
    for i, (link, bs) in enumerate(sets):
        print(i, hex(link), [(b[0], b[1], hex(b[2]), hex(b[3]), b[4], b[5], b[6], b[7]) for b in bs])
