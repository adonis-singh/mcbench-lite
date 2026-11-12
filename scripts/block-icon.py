"""Renders web/icons/grass-block.png, the app's logo, from Minecraft's own
grass block textures in the cached 1.21.10 atlas (data/assets). The cube uses
the inventory view: 30 degree pitch, 45 degree yaw, top face at full light and
the two side faces at 80% and 60%, as Minecraft shades block items. Each
texel is drawn as its own parallelogram, so the pixel art stays sharp.

Run once after `npm start` has cached the atlas:
    python3 scripts/block-icon.py
"""
import json
import math
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / 'data' / 'assets' / '1.21.10'
OUT = ROOT / 'web' / 'icons' / 'grass-block.png'
PLAINS_GRASS = (0x91, 0xBD, 0x59)  # the default biome tint for grass
SIZE = 256  # output pixels
SUPERSAMPLE = 4

atlas = Image.open(ASSETS / 'atlas.png').convert('RGBA')
spots = json.loads((ASSETS / 'atlas_map.json').read_text())


def texture(name):
    x, y, w, h = spots[f'block/{name}']
    return atlas.crop((x, y, x + w, y + h))


def tint(image, color):
    pixels = image.load()
    for y in range(image.height):
        for x in range(image.width):
            r, g, b, a = pixels[x, y]
            pixels[x, y] = (r * color[0] // 255, g * color[1] // 255, b * color[2] // 255, a)
    return image


top = tint(texture('grass_block_top'), PLAINS_GRASS)
side = texture('grass_block_side')
side.alpha_composite(tint(texture('grass_block_side_overlay'), PLAINS_GRASS))

# Screen vectors for one block edge along east (x), south (z) and up (y). The
# cube is taller (top diamond plus a vertical edge) than it is wide, so its
# height sets the scale.
c45, s30, c30 = math.cos(math.radians(45)), math.sin(math.radians(30)), math.cos(math.radians(30))
scale = SIZE * SUPERSAMPLE / (2 * c45 * s30 + c30)
ex = (scale * c45, scale * c45 * s30)
ez = (-ex[0], ex[1])
up = (0, -scale * c30)
height = ex[1] + ez[1] - up[1]

canvas = Image.new('RGBA', (SIZE * SUPERSAMPLE, SIZE * SUPERSAMPLE), (0, 0, 0, 0))
draw = ImageDraw.Draw(canvas)


def face(image, corner, u, v, light):
    """Paints `image` on the parallelogram corner + s*u + t*v, s and t in 0..1."""
    pixels = image.load()
    n = image.width
    for j in range(n):
        for i in range(n):
            r, g, b, a = pixels[i, j]
            if a == 0:
                continue
            quad = [(corner[0] + (i + di) / n * u[0] + (j + dj) / n * v[0],
                     corner[1] + (i + di) / n * u[1] + (j + dj) / n * v[1]) for di, dj in ((0, 0), (1, 0), (1, 1), (0, 1))]
            draw.polygon(quad, fill=(int(r * light), int(g * light), int(b * light), a))


add = lambda p, q: (p[0] + q[0], p[1] + q[1])
neg = lambda p: (-p[0], -p[1])
# The top face's far (north-west) vertex, centred, with the cube centred vertically.
far = (SIZE * SUPERSAMPLE / 2, (SIZE * SUPERSAMPLE - height) / 2)
face(top, far, ex, ez, 1.0)                              # top: u east, v south
face(side, add(far, ez), ex, neg(up), 0.8)               # left face (south side)
face(side, add(add(far, ez), ex), neg(ez), neg(up), 0.6) # right face (east side)

canvas.resize((SIZE, SIZE), Image.LANCZOS).save(OUT)
print(f'wrote {OUT.relative_to(ROOT)}')
