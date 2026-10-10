"""Print function bounds (from .pdata) and disassemble: python3 -I fn.py dump.exe 0xADDR [n_lines]"""
import subprocess, sys
dump, addr = sys.argv[1], int(sys.argv[2], 16)
n = int(sys.argv[3]) if len(sys.argv) > 3 else 100000
sys.argv = sys.argv[:2]
exec(open(__file__.replace('fn.py', 'xref.py')).read().split("if __name__")[0])
f = func_of(addr - base)
i = np.searchsorted(funcs[:, 0], f)
# Merge chained chunks: following pdata entries until a gap
lo, hi = base + int(funcs[i, 0]), base + int(funcs[i, 1])
print(f'func {lo:#x}-{hi:#x} ({hi - lo} bytes)')
out = subprocess.run(['objdump', '-d', '-M', 'intel', '--no-show-raw-insn', f'--start-address={lo}', f'--stop-address={hi}', dump],
                     capture_output=True, text=True).stdout.splitlines()[7:]
print('\n'.join(out[:n]))
