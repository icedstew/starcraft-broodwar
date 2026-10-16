# Save / load (1.16.1), not ported yet

Mapped 2026-10-17 from the decompile (triage, not yet re-verified line by line).

## Container

- 0x4c3450 writer (data pointer in EAX) / 0x4c3280 reader: u32 CRC32 (table 0x50d2b0, seed
  0xffffffff, over the uncompressed bytes), u32 nblocks = ceil(size / 0x2000), per block
  u32 csize + data. csize == raw size: stored; else PKWARE DCL implode (binary, dict
  0x400): read 0x4c3190 -> explode 0x410470, write 0x4c3200 -> implode 0x410b30.
- src/replay.rs `section()` reads the same layout (no CRC check). Writing needs an
  implode encoder, or stored blocks (readable, not byte-identical).
- Entry: writer 0x4c2910 (command 0x06 via 0x4865d0; 0x4d02d0 is the same stream);
  loader 0x4cfef0 (from 0x4eed10). Header: u32 0x00690003 (version 3 in the low word;
  the loader branches on version < 3), chunk 181 B (0x4cf160), chunk 0x1b0, chunk 96000,
  u32 0x512688. Pre-save mutators: 0x4935f0, 0x4989a0, 0x499a60, 0x494030.

## Sections in file order (writer -> loader: size, pool, Rust)

| # | Writer -> loader | Contents | Rust |
|---|---|---|---|
| 1 | 0x4d64c0 -> 0x4d6220 | images: u16 n + n*0x42; pool 0x52f568 5000*0x40; fixups 0x4d6330 / 0x4d5600 | images |
| 2 | 0x498740 -> 0x498570 | sprites: u16 n + n*0x26; pool 0x629d98 2500*0x24; 0x497380 / 0x497310; two 0x400 list-head tables | sprites, sprite_rows |
| 3 | 0x487ec0 x2 -> 0x488100 | thingies: 2 pools (0x6509d8, 0x652928) 500*0x10, u16 n + n*0x12 | thingies |
| 4 | 0x495b50 -> 0x495a50 | flingy record 0x63fee0 (0x4e) when 0x63feec != 0 | partial |
| 5 | 0x4eaaf0 -> 0x49e910 | units: 0x6a4 in-use bytes, u32 n, n*(u32 idx + 0x150); 0x4e3410 / 0x4e2e20 detach / reattach (0x46a300, 0x469ec0, 0x4533d0) | units + lists |
| 5b | 0x4915c0, 0x4ea970 | unit-ref list heads (8 + up to 12 u32) | partial |
| 6 | 0x48aeb0 -> 0x48ae40 | bullets: u16 n + n*0x72 (0x6e before v3); pool 0x64b2e8 100*0x70; 0x48a9a0 / 0x48a8d0 | bullets |
| 7 | 0x48c910 -> 0x48c870 | paths: 40000 raw (0x6416a0, 2000*0x14) + heads 0x64b2e0 / e4 | paths |
| 8 | inline | tiles: u32 w*h*2 + chunk, u32 w*h + chunk | terrain, partial |
| 9 | 0x47da80 -> 0x47d9d0 | AI tables 0x5000 / 0x1000 / 0x28 / 0x14 (pool 0x659b10) | AI, missing |
| 10 | inline | 0x20000 (0x5993c4), 0x20000 (0x628494), 0x40000 (tile flags 0x6d1260) | partial |
| 11 | 0x4899d0 -> 0x4897e0 | victory / scores: u16 n + n*0x960+0x18 + 0x500 | missing |
| 12 | inline | u32 size + blob 0x5993d4 (unknown) | missing |
| 13 | 0x4cee00 / 0x4ceda0 | selections 0x6284e8 (96 dwords) | selections |
| 14 | 0x46ec40 -> 0x46ea70, 0x4a1180 | SAI / region blob (0xb7a28 + tail) | sai, partial |
| 15 | 0x4bf390 -> 0x4bf020 | 18 setup chunks (players etc.) | partial |
| 16 | inline | screen position 0x62848c / 0x6284a8 | view |

## Pointer <-> index (1-based)

- Units: (p - 0x59cca8) / 0x150 + 1 | (byte +0xa5 << 11); back 0x59cb58 + (idx & 0x7ff) * 0x150.
- Sprites: (p - 0x629d98) / 0x24 + 1; back 0x629d74 + idx * 0x24.
- Images: ((p - 0x52f568) >> 6) + 1; back 0x52f528 + idx * 0x40.
- Thingies: ((p - 0x6509d8) >> 4) + 1; back 0x6509c8 + idx * 0x10 (pool B 0x652918).
- Paths: (p - 0x6416a0) / 0x14 + 1; back 0x64168c + idx * 0x14.
- AI: (p - 0x659b10) / 0x14 + 1; back 0x659afc + idx * 0x14.

Not traced yet: 0x4ce950, 0x4cf2a0, 0x4897e0, the 0x4ea970 reader.

Open decision (user): native-compatible .snx (every pool in 1.16.1 layout, AI data
included) or an scbw-only format.
