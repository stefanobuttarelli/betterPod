"""Writes a small test library: Artist/Album/NN Title.wav (untagged, so the
names come from the folders), a cover for one album and an .m4a that the
Miyoo build must skip.

    python3 music.py DIR
"""
import math
import os
import struct
import sys
import zlib

RATE = 44100


def wav(path, freq, seconds):
    # one second (a whole number of periods), repeated
    second = bytearray()
    for i in range(RATE):
        v = int(9000 * math.sin(2 * math.pi * freq * i / RATE))
        second += struct.pack("<hh", v, v)
    frames = bytes(second) * int(seconds)
    with open(path, "wb") as f:
        f.write(b"RIFF" + struct.pack("<I", 36 + len(frames)) + b"WAVE")
        f.write(b"fmt " + struct.pack("<IHHIIHH", 16, 1, 2, RATE, RATE * 4, 4, 16))
        f.write(b"data" + struct.pack("<I", len(frames)) + frames)


def png(path, w, h, pixel):
    rows = b"".join(b"\x00" + b"".join(bytes(pixel(x, y)) for x in range(w)) for y in range(h))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def main(root):
    library = {
        ("Miyoo Ensemble", "Onion Sessions"): ["Boot Chime", "Click Wheel", "Cover Flow"],
        ("Miyoo Ensemble", "Plus Edition"): ["Volume Up", "Hold Switch"],
        ("Stefano Trio", "Knulli Nights"): ["Split Screen", "Ken Burns", "Brick Breaker", "Solitaire"],
    }
    freq = 330
    for (artist, album), titles in library.items():
        d = os.path.join(root, artist, album)
        os.makedirs(d, exist_ok=True)
        for i, t in enumerate(titles, 1):
            wav(os.path.join(d, "%02d %s.wav" % (i, t)), freq, 60)  # (longer than the walk: no End of the Queue)
            freq += 40
    png(os.path.join(root, "Miyoo Ensemble", "Onion Sessions", "cover.png"), 240, 240,
        lambda x, y: (154 + x // 8, 162 - y // 4, 230 - (x + y) // 6))
    with open(os.path.join(root, "Stefano Trio", "Knulli Nights", "05 Itunes Song.m4a"), "wb") as f:
        f.write(b"\x00\x00\x00\x18ftypM4A ")


if __name__ == "__main__":
    main(sys.argv[1])
