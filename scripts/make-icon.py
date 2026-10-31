# Run: python scripts/make-icon.py crates/app/resources/app.ico crates/app/assets/icon.png
"""Draws the All-in-1 Converter icon (Hotel look): a yellow tile with a dark edge and a hard
shadow, an isometric blue package, and a green go badge with a download arrow.
Writes app.ico (16-256) and a 256 px PNG."""
import math
import sys

from PIL import Image, ImageDraw

ico_path, png_path = sys.argv[1], sys.argv[2]
S = 1024  # drawn large, scaled down for smooth edges
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

OUTLINE = (15, 45, 63, 255)
YELLOW = (255, 205, 56, 255)
YELLOW_LIGHT = (255, 224, 122, 255)
SHADOW = (15, 45, 63, 110)
edge = 44

# Tile with a hard shadow under it.
tile = (40, 30, S - 40, S - 70)
d.rounded_rectangle((tile[0], tile[1] + 40, tile[2], tile[3] + 40), radius=190, fill=SHADOW)
d.rounded_rectangle(tile, radius=190, fill=OUTLINE)
inner = (tile[0] + edge, tile[1] + edge, tile[2] - edge, tile[3] - edge)
d.rounded_rectangle(inner, radius=150, fill=YELLOW)
# A light band along the top, like the hotel's buttons.
d.rounded_rectangle((inner[0] + 40, inner[1] + 28, inner[2] - 40, inner[1] + 80), radius=26, fill=YELLOW_LIGHT)

# Isometric package.
cx, base_y = S * 0.47, S * 0.80
w = 300  # half width along each axis
h = 300
rx, ry = math.cos(math.radians(30)), math.sin(math.radians(30))
front = (cx, base_y)
right = (cx + w * rx, base_y - w * ry)
left = (cx - w * rx, base_y - w * ry)
back = (cx, base_y - 2 * w * ry)
up = lambda p: (p[0], p[1] - h)
TOP, LEFT, RIGHT = (142, 195, 234, 255), (14, 134, 196, 255), (10, 100, 150, 255)
line = 30
d.polygon([front, left, up(left), up(front)], fill=LEFT, outline=OUTLINE, width=line)
d.polygon([front, right, up(right), up(front)], fill=RIGHT, outline=OUTLINE, width=line)
d.polygon([up(front), up(right), up(back), up(left)], fill=TOP, outline=OUTLINE, width=line)
# Tape across the top and down the left face.
mid = lambda a, b, t: (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t)
tape_w = 0.16
t0, t1 = 0.5 - tape_w / 2, 0.5 + tape_w / 2
TAPE = (255, 236, 170, 255)
d.polygon([mid(up(left), up(back), t0), mid(up(left), up(back), t1), mid(up(front), up(right), t1), mid(up(front), up(right), t0)], fill=TAPE, outline=OUTLINE, width=16)
# Re-draw the outer edges so the tape ends stay crisp.
d.line([up(front), up(right), up(back), up(left), up(front)], fill=OUTLINE, width=line, joint="curve")
d.line([up(front), front], fill=OUTLINE, width=line)

# Green go badge with a white download arrow.
bx, by, br = S * 0.73, S * 0.70, 175
d.ellipse((bx - br + 6, by - br + 36, bx + br + 6, by + br + 36), fill=SHADOW)
d.ellipse((bx - br, by - br, bx + br, by + br), fill=OUTLINE)
d.ellipse((bx - br + 32, by - br + 32, bx + br - 32, by + br - 32), fill=(35, 102, 28, 255))
d.ellipse((bx - br + 32, by - br + 32, bx + br - 32, by + br - 44), fill=(63, 165, 53, 255))
arrow = [
    (bx - 38, by - 92), (bx + 38, by - 92), (bx + 38, by - 6), (bx + 88, by - 6),
    (bx, by + 86), (bx - 88, by - 6), (bx - 38, by - 6),
]
d.polygon(arrow, fill=(255, 255, 255, 255))

big = img.resize((256, 256), Image.LANCZOS)
big.save(png_path)
big.save(ico_path, sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
for size in (16, 32, 48):
    img.resize((size, size), Image.LANCZOS).resize((size * 4, size * 4), Image.NEAREST).save(png_path.replace(".png", f"-preview{size}.png"))
print("ok")
