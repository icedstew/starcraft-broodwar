"""Launch StarCraft.exe under Wine, wait for .text to decrypt, dump the image as a PE.

usage: python3 -I dump.py <exe> <out.exe> [pid]   (pid = attach to already-running game)
"""
import collections, math, os, re, struct, subprocess, sys, time

exe, out = sys.argv[1], sys.argv[2]
d = open(exe, 'rb').read()
pe = struct.unpack_from('<I', d, 0x3c)[0]
nsec = struct.unpack_from('<H', d, pe + 6)[0]
opt = struct.unpack_from('<H', d, pe + 20)[0]
is64 = struct.unpack_from('<H', d, pe + 24)[0] == 0x20b
size_of_image = struct.unpack_from('<I', d, pe + 24 + 56)[0]
secs = []
for i in range(nsec):
    o = pe + 24 + opt + i * 40
    vs, va = struct.unpack_from('<II', d, o + 8)
    secs.append((o, d[o:o + 8].rstrip(b'\0'), va, vs))
text_va, text_vs = secs[0][2], secs[0][3]


def entropy(b):
    c = collections.Counter(b); t = len(b)
    return -sum(v / t * math.log2(v / t) for v in c.values())


attach = int(sys.argv[3]) if len(sys.argv) > 3 else None


def find_base(pid):
    """Find the region holding our PE headers (Wine/protector may map the image anonymously)."""
    hdr = d[:0x200]
    with open(f'/proc/{pid}/mem', 'rb') as m:
        for line in open(f'/proc/{pid}/maps'):
            lo = int(line.split('-')[0], 16)
            if 'r' not in line.split()[1] or lo & 0xFFFF:
                continue
            try:
                m.seek(lo); b = m.read(0x200)
            except OSError:
                continue
            if b[:2] == b'MZ' and b[pe:pe + 0x18] == hdr[pe:pe + 0x18]:
                return lo


if attach:
    pid, base = attach, (int(sys.argv[4], 16) if len(sys.argv) > 4 else find_base(attach))
    print('attached', pid, 'base', hex(base))
    with open(f'/proc/{pid}/mem', 'rb') as m:
        img = bytearray()
        for off in range(0, size_of_image, 0x1000):
            try:
                m.seek(base + off); img += m.read(0x1000)
            except OSError:
                img += b'\0' * 0x1000
    best = img
    p = None

env = dict(os.environ, WINEPREFIX=os.path.expanduser('~/Games/battlenet'), WINEDEBUG='-all')
p = None if attach else subprocess.Popen(['wine', exe], env=env, cwd=os.path.dirname(exe),
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
name = os.path.basename(exe).lower()
if not attach:
    base = pid = None
deadline = time.time() + 120
best = best if attach else None
while not attach and time.time() < deadline:
    time.sleep(0.2)
    # Find the process that mapped StarCraft.exe (wine may fork/exec helpers).
    for cand in os.listdir('/proc'):
        if not cand.isdigit():
            continue
        try:
            maps = open(f'/proc/{cand}/maps').read()
        except OSError:
            continue
        for line in maps.splitlines():
            if line.lower().endswith(name) and line.split()[2] == '00000000':
                pid, base = int(cand), int(line.split('-')[0], 16)
                break
        if pid:
            break
    if not pid:
        continue
    try:
        with open(f'/proc/{pid}/mem', 'rb') as m:
            m.seek(base + text_va + 0x100000)
            sample = m.read(0x40000)
    except OSError as e:
        print('read fail', e); continue
    e = entropy(sample)
    print(f'pid {pid} base {base:#x} text entropy {e:.2f}', flush=True)
    if e < 7.2:
        time.sleep(3)  # let remaining pages finish decrypting
        with open(f'/proc/{pid}/mem', 'rb') as m:
            img = bytearray()
            for off in range(0, size_of_image, 0x1000):
                try:
                    m.seek(base + off); img += m.read(0x1000)
                except OSError:
                    img += b'\0' * 0x1000
        best = img
        break

if p:
    p.kill()
    subprocess.run(['wineserver', '-k'], env=env)
if best is None:
    sys.exit('never saw decrypted .text')

# Rebuild: headers from the file, raw data == virtual layout, ImageBase = loaded base.
img = best
img[:0x1000] = d[:0x1000]
if is64:
    struct.pack_into('<Q', img, pe + 24 + 24, base)
else:
    struct.pack_into('<I', img, pe + 24 + 28, base)
for o, n, va, vs in secs:
    struct.pack_into('<II', img, o + 16, (vs + 0xFFF) & ~0xFFF, va)  # SizeOfRawData, PointerToRawData
struct.pack_into('<I', img, pe + 24 + 36, 0x1000)  # FileAlignment = SectionAlignment
open(out, 'wb').write(img)
print('wrote', out, hex(len(img)), 'entropy now', '%.2f' % entropy(img[text_va:text_va + text_vs]))
