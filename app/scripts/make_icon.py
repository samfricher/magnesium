"""Draw the app icon: three proof dots, two filled and one still to earn."""
import struct, sys, zlib

SIZE = 1024
BG = (21, 21, 22)
FG = (233, 231, 226)
DIM = (142, 140, 134)

def inside_tile(x, y):
    m, r = 100, 200
    if x < m or y < m or x >= SIZE - m or y >= SIZE - m:
        return False
    cx = min(max(x, m + r), SIZE - m - r - 1)
    cy = min(max(y, m + r), SIZE - m - r - 1)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r

rows = []
centres = [(322, 512), (512, 512), (702, 512)]
radius, ring = 62, 16
for y in range(SIZE):
    row = bytearray([0])
    for x in range(SIZE):
        if not inside_tile(x, y):
            row += bytes((0, 0, 0, 0))
            continue
        colour = BG
        for index, (cx, cy) in enumerate(centres):
            d2 = (x - cx) ** 2 + (y - cy) ** 2
            if d2 <= radius * radius:
                if index < 2:
                    colour = FG
                elif d2 >= (radius - ring) ** 2:
                    colour = DIM
        row += bytes(colour + (255,))
    rows.append(bytes(row))

def chunk(kind, data):
    body = kind + data
    return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(b"".join(rows), 9)) + chunk(b"IEND", b"")
open(sys.argv[1], "wb").write(png)
