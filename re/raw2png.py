# raw2png.py <in.raw> <out.png>: 640x480 indexed + 256 PALETTEENTRY (r,g,b,flags) -> RGB PNG
import sys, zlib, struct
d = open(sys.argv[1], "rb").read()
px, pal = d[:640 * 480], d[640 * 480:]
rows = b"".join(b"\0" + b"".join(pal[i * 4:i * 4 + 3] for i in px[y * 640:(y + 1) * 640]) for y in range(480))
chunk = lambda t, b: struct.pack(">I", len(b)) + t + b + struct.pack(">I", zlib.crc32(t + b))
open(sys.argv[2], "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 640, 480, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))
