"""Converts the frames dumped by MIYOOPOD_DUMP_FRAME (raw RGBA 640x480) to PNG.

    python3 raw2png.py FILE.raw...
"""
import struct
import sys
import zlib

W, H = 640, 480


def convert(path):
    data = open(path, "rb").read()
    if len(data) != W * H * 4:
        print("skip %s (%d bytes)" % (path, len(data)))
        return
    rows = b"".join(b"\x00" + data[y * W * 4:(y + 1) * W * 4] for y in range(H))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))

    out = path[:-4] + ".png"
    with open(out, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(rows, 6)) + chunk(b"IEND", b""))


for p in sys.argv[1:]:
    convert(p)
