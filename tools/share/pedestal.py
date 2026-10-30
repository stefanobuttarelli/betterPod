"""The Share pedestal and its light (share_ped_*.png, share_beam_*.png), at
any scale: S=1 is the console's (260 × 150 and 260 × 240 px), the web page
and the Mac app use S=3.

    python3 tools/share/pedestal.py OUTDIR [S] [blue|green…]
"""
import sys
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

OUT = sys.argv[1].rstrip("/") + "/"
SCALE = int(sys.argv[2]) if len(sys.argv) > 2 else 1
ONLY = set(sys.argv[3:])
# above the console's scale, drawn twice as big and shrunk: smooth edges
# (PIL's ellipses aren't antialiased); S=1 stays exactly the console's
S = SCALE * 2 if SCALE > 1 else 1


def u(v):
    return int(round(v * S))


def grad(w, h, c0, c1):
    t = np.linspace(0, 1, h)
    a = np.array(c0, float)[None, :] * (1 - t[:, None]) + np.array(c1, float)[None, :] * t[:, None]
    return Image.fromarray(np.repeat(a[:, None, :], w, 1).astype(np.uint8)).convert("RGBA")


CW, CH = u(260), u(150)
AX, AY = u(130), u(24)  # the top's centre in the image


def pedestal(glow):
    im = Image.new("RGBA", (CW, CH), (0, 0, 0, 0))
    w, h = u(176), u(40)
    cx, top = AX, AY
    body = grad(w, h + u(24), (246, 248, 250), (200, 205, 212))
    a = np.abs(np.linspace(-1, 1, w))
    sh = Image.new("RGBA", (w, h + u(24)), (120, 128, 140, 0))
    sh.putalpha(Image.fromarray(np.repeat((a ** 3 * 120)[None, :], h + u(24), 0).astype(np.uint8)))
    body.alpha_composite(sh)
    hl = Image.new("RGBA", (w, h + u(24)), (255, 255, 255, 0))
    hl.putalpha(Image.fromarray(np.repeat((np.exp(-((np.linspace(-1, 1, w) + 0.45) ** 2) / 0.02) * 180)[None, :], h + u(24), 0).astype(np.uint8)))
    body.alpha_composite(hl)
    m = Image.new("L", (w, h + u(24)), 0)
    d = ImageDraw.Draw(m)
    d.rectangle((0, u(12), w - 1, h + u(12)), fill=255)
    d.ellipse((0, h, w - 1, h + u(23)), fill=255)
    body.putalpha(m)
    # the reflection: each column mirrored about its own lowest pixel, so it
    # hugs the curved base all along (a glossy floor)
    B = np.array(body).astype(float)
    H0 = B.shape[0]
    RH = u(44)
    RA = np.zeros((H0 + RH, w, 4))
    for x in range(w):
        col = np.nonzero(B[:, x, 3] > 8)[0]
        if len(col) == 0:
            continue
        b = col[-1]
        for k in range(RH):
            y = b - k
            if y < 0:
                break
            fade = 0.30 * (1 - k / RH) ** 1.6
            RA[b + 1 + k, x, :3] = B[y, x, :3]
            RA[b + 1 + k, x, 3] = B[y, x, 3] * fade
    refl = Image.fromarray(RA.clip(0, 255).astype(np.uint8), "RGBA")
    im.alpha_composite(refl, (cx - w // 2, top - u(12)))
    s = Image.new("RGBA", (CW, CH), (0, 0, 0, 0))
    ImageDraw.Draw(s).ellipse((cx - w // 2 - u(6), top + h + u(2), cx + w // 2 + u(6), top + h + u(22)), fill=(60, 70, 90, 70))
    im.alpha_composite(s.filter(ImageFilter.GaussianBlur(6 * S)))
    im.alpha_composite(body, (cx - w // 2, top - u(12)))
    d = ImageDraw.Draw(im)
    d.ellipse((cx - w // 2, top - u(12), cx + w // 2, top + u(12)), fill=(252, 253, 254), outline=(200, 206, 214), width=S)
    disc = grad(w - u(28), u(18), tuple(min(255, c + 120) for c in glow), glow)
    dm = Image.new("L", disc.size, 0)
    ImageDraw.Draw(dm).ellipse((0, 0, disc.width - 1, disc.height - 1), fill=255)
    disc.putalpha(dm)
    im.alpha_composite(disc, (cx - (w - u(28)) // 2, top - u(9)))
    g = Image.new("RGBA", (CW, CH), (0, 0, 0, 0))
    ImageDraw.Draw(g).ellipse((cx - u(60), top - u(8), cx + u(60), top - u(1)), fill=(255, 255, 255, 150))
    im.alpha_composite(g.filter(ImageFilter.GaussianBlur(2 * S)))
    return im


def beam(glow):
    im = Image.new("RGBA", (CW, u(240)), (0, 0, 0, 0))
    cx, top = AX, u(226)
    ImageDraw.Draw(im).polygon([(cx - u(70), top), (cx + u(70), top), (cx + u(84), top - u(200)), (cx - u(84), top - u(200))], fill=glow + (30,))
    im = im.filter(ImageFilter.GaussianBlur(10 * S))
    # the glow just above the disc
    g = Image.new("RGBA", (CW, u(240)), (0, 0, 0, 0))
    ImageDraw.Draw(g).ellipse((cx - u(80), top - u(14), cx + u(80), top + u(8)), fill=glow + (90,))
    im.alpha_composite(g.filter(ImageFilter.GaussianBlur(8 * S)))
    return im


for name, glow in (("blue", (60, 150, 255)), ("green", (40, 190, 110))):
    if ONLY and name not in ONLY:
        continue
    for img, f in ((pedestal(glow), "share_ped_"), (beam(glow), "share_beam_")):
        if S != SCALE:
            img = img.resize((img.width * SCALE // S, img.height * SCALE // S), Image.LANCZOS)
        img.save(OUT + f + name + ".png", optimize=True)
print("ok")
