# RE notes — StarCraft: Remastered 1.23.10.13515 (x64)

On-disk `.text` is encrypted (entropy 8.0); a TLS callback decrypts at startup.
We analyze a runtime dump:

1. Launch SC:R (Battle.net is fine).
2. `sudo sysctl kernel.yama.ptrace_scope=0` (restore to 1 afterward).
3. `python3 -I re/dump.py <x86_64/StarCraft.exe> sc_x64.exe <pid> 0x140000000`

The dump is a PE with raw layout == virtual layout and ImageBase 0x140000000.
`.pdata` gives exact function bounds (38,378 functions). The dump is never committed.

Tools: `re/xref.py` (RIP-relative xrefs, numpy), `re/datlayout.py` (dat layouts).

## Addresses

| Address | What |
|---|---|
| 0x140305d10 | `load_dat(const char* name, DatDesc* table)`; DatDesc = {u64 dest; i32 size; i32 count; u8 flags(bit0: ptr widened 4->8)}, 24 bytes, ends at dest==0 |
| 0x1401c01e0 | init units/orders dat, clears unit array (stride 0x1e8 = x64 CUnit size) |
| 0x140190440 | references `scripts\iscript.bin` |
| 0x140188db0 | loads flingy.dat |
| 0x14035c630 | loads upgrades/weapons/techdata/portdata |
| 0x140da9820 | units.dat descriptor table (field dest ptrs = runtime column arrays) |
| 0x140da9000 | flingy.dat table |
| 0x140da9100 | images.dat table |
| 0x140da95d0 | sprites.dat table |
| 0x140da92a0 | orders.dat table |
| 0x140daa0a0 | weapons.dat table |
| 0x140da9f00 | upgrades.dat table |
| 0x140da9700 | techdata.dat table |
