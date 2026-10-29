"""A larger test library for profiling: 12 albums of 10 short tracks, each
album with its own cover (so previews, Ken Burns and Cover Flow have art).

    python3 music.py DIR
"""
import os
import struct
import sys
import zlib

RATE = 44100


def wav(path, seconds):
    frames = b"\x00\x00\x00\x00" * int(RATE * seconds)
    with open(path, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", 36 + len(frames)) + b"WAVE")
        f.write(b"fmt " + struct.pack("<IHHIIHH", 16, 1, 2, RATE, RATE * 4, 4, 16))
        f.write(b"data" + struct.pack("<I", len(frames)) + frames)


def png(path, w, h, seed):
    rows = bytearray()
    for y in range(h):
        rows.append(0)
        for x in range(w):
            rows += bytes(((x * 3 + seed * 40) % 256, (y * 2 + seed * 70) % 256, ((x + y) + seed * 20) % 256))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(bytes(rows))) + chunk(b"IEND", b""))


def main(root):
    for a in range(12):
        d = os.path.join(root, "Artist %02d" % (a // 2), "Album %02d" % a)
        os.makedirs(d, exist_ok=True)
        png(os.path.join(d, "cover.png"), 300, 300, a)
        for t in range(10):
            wav(os.path.join(d, "%02d Track %d.wav" % (t + 1, t + 1)), 20 if t == 0 else 1)


if __name__ == "__main__":
    main(sys.argv[1])
