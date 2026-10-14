import sys,struct,re
from capstone import Cs,CS_ARCH_X86,CS_MODE_32
import os
f=open(os.environ.get('STORM', os.path.expanduser('~/Downloads/StarCraft-BroodWar_1.16.1/storm.dll')),'rb').read()
pe=struct.unpack_from('<I',f,0x3c)[0]; n=struct.unpack_from('<H',f,pe+6)[0]; so=pe+24+struct.unpack_from('<H',f,pe+20)[0]
secs=[struct.unpack_from('<8sIIII',f,so+i*40) for i in range(n)]
def off(a):
    for nm,vs,v,rs,ra in secs:
        if v+0x15000000<=a<v+0x15000000+rs: return a-v-0x15000000+ra
if sys.argv[1]=='refs':
    for t in sys.argv[2:]:
        t=int(t,16)
        for m in re.finditer(re.escape(struct.pack('<I',t)),f):
            o=m.start()
            for nm,vs,v,rs,ra in secs:
                if ra<=o<ra+rs: print(hex(t),'ref', hex(o-ra+v+0x15000000))
else:
    a=int(sys.argv[1],16); k=int(sys.argv[2])
    md=Cs(CS_ARCH_X86,CS_MODE_32)
    for i,ins in enumerate(md.disasm(f[off(a):off(a)+k*8],a)):
        if i>=k: break
        print(f"{ins.address:x}: {ins.mnemonic} {ins.op_str}")
