import sys,struct
from capstone import Cs,CS_ARCH_X86,CS_MODE_32
exe=open(sys.argv[1],'rb').read()
pe=struct.unpack_from('<I',exe,0x3c)[0]
nsec=struct.unpack_from('<H',exe,pe+6)[0]; opt=struct.unpack_from('<H',exe,pe+20)[0]
base=struct.unpack_from('<I',exe,pe+24+28)[0]
secs=[]
o=pe+24+opt
for i in range(nsec):
    va,sz,raw=struct.unpack_from('<III',exe,o+12)[0],struct.unpack_from('<I',exe,o+16)[0],struct.unpack_from('<I',exe,o+20)[0]
    secs.append((base+va,sz,raw)); o+=40
def off(a):
    for va,sz,raw in secs:
        if va<=a<va+sz: return raw+a-va
a=int(sys.argv[2],16); n=int(sys.argv[3]) if len(sys.argv)>3 else 80
md=Cs(CS_ARCH_X86,CS_MODE_32)
c=0
for i in md.disasm(exe[off(a):off(a)+2000],a):
    print(f"{i.address:x}: {i.mnemonic} {i.op_str}"); c+=1
    if c>=n or (i.mnemonic=='ret' and len(sys.argv)<=4): break
