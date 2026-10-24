"""Draws the installer's header and sidebar images (NSIS wants 24-bit BMPs): the Command Center's
eclipse, a black disc with a cold white rim and corona, on near-black. Run it again after changing it:
    python src-tauri/installer/make_art.py
"""
import math
import os

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
BG = (7, 8, 13)
GLOW = (190, 230, 255)


def eclipse(size, r):
    """An eclipse centred in a square image of `size`, disc radius `r`."""
    s = size * 4  # draw large, then shrink for smooth edges
    R = r * 4
    c = s // 2
    img = Image.new("RGB", (s, s), BG)
    corona = Image.new("RGB", (s, s), (0, 0, 0))
    d = ImageDraw.Draw(corona)
    # Rays: thin wedges of light around the disc, uneven like the canvas version.
    for i in range(48):
        a = i / 48 * math.tau + (i % 5) * 0.03
        length = R * (1.25 + 0.55 * abs(math.sin(i * 1.7)))
        w = 0.035 + 0.02 * (i % 3)
        pts = [(c + math.cos(a - w) * R, c + math.sin(a - w) * R), (c + math.cos(a) * length, c + math.sin(a) * length), (c + math.cos(a + w) * R, c + math.sin(a + w) * R)]
        k = 0.45 + 0.4 * abs(math.sin(i * 2.3))
        d.polygon(pts, fill=tuple(int(v * k) for v in GLOW))
    d.ellipse((c - R * 1.18, c - R * 1.18, c + R * 1.18, c + R * 1.18), fill=tuple(int(v * 0.55) for v in GLOW))
    corona = corona.filter(ImageFilter.GaussianBlur(R * 0.22))
    img = ImageChops.add(img, corona)
    d = ImageDraw.Draw(img)
    # The white-hot rim, then the black disc.
    d.ellipse((c - R - 6, c - R - 6, c + R + 6, c + R + 6), fill=(240, 250, 255))
    img = ImageChops.lighter(img, img.filter(ImageFilter.GaussianBlur(10)))
    d = ImageDraw.Draw(img)
    d.ellipse((c - R, c - R, c + R, c + R), fill=(2, 3, 6))
    return img.resize((size, size), Image.LANCZOS)


def font(px, bold=True):
    for name in (["seguisb.ttf", "segoeuib.ttf"] if bold else []) + ["segoeui.ttf", "arial.ttf"]:
        try:
            return ImageFont.truetype(name, px)
        except OSError:
            continue
    return ImageFont.load_default()


# Header: 150 × 57, the eclipse on the left.
head = Image.new("RGB", (150, 57), BG)
e = eclipse(57, 15)
head.paste(e, (2, 0))
d = ImageDraw.Draw(head)
d.text((62, 13), "Island", font=font(15), fill=(238, 246, 255))
d.text((62, 30), "Shell", font=font(15), fill=GLOW)
head.save(os.path.join(HERE, "header.bmp"))

# Sidebar: 164 × 314, the eclipse and the name.
side = Image.new("RGB", (164, 314), BG)
e = eclipse(164, 34)
side.paste(e, (0, 50))
d = ImageDraw.Draw(side)
f = font(20)
for text, y, col in (("Island", 228, (238, 246, 255)), ("Shell", 252, GLOW)):
    w = d.textlength(text, font=f)
    d.text(((164 - w) / 2, y), text, font=f, fill=col)
side.save(os.path.join(HERE, "sidebar.bmp"))
print("wrote header.bmp and sidebar.bmp")
