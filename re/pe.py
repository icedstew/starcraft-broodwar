import struct, sys
EXE="/home/stubob/Games/battlenet/drive_c/Program Files (x86)/StarCraft/x86/StarCraft.exe"
d=open(EXE,'rb').read()
pe=struct.unpack_from('<I',d,0x3c)[0]
nsec=struct.unpack_from('<H',d,pe+6)[0]; opt=struct.unpack_from('<H',d,pe+20)[0]
base=struct.unpack_from('<I',d,pe+24+28)[0]
secs=[]
for i in range(nsec):
    o=pe+24+opt+i*40
    name=d[o:o+8].rstrip(b'\0').decode(); vs,va,rs,ro=struct.unpack_from('<IIII',d,o+8)
    secs.append((name,base+va,vs,ro,rs))
def f2v(off):
    for n,va,vs,ro,rs in secs:
        if ro<=off<ro+rs: return va+off-ro
def v2f(v):
    for n,va,vs,ro,rs in secs:
        if va<=v<va+rs: return ro+v-va
def u32(v): return struct.unpack_from('<I',d,v2f(v))[0]
def find_ptr(v):
    p=struct.pack('<I',v); i=d.find(p); r=[]
    while i>=0: r.append(f2v(i)); i=d.find(p,i+1)
    return r
if __name__=='__main__':
    for a in sys.argv[1:]: print(a,[hex(x) for x in find_ptr(int(a,16)) if x])
