# Converts Xvfb's frame buffer file (-fbdir: XWD, ZPixmap, 32 bits a pixel)
# to a PNG, with the standard library only:
#
#   python3 -I xwd2png.py <Xvfb_screen0> <out.png>
import struct
import sys
import zlib

data = open(sys.argv[1], 'rb').read()
fields = struct.unpack('>25I', data[:100])
header_size, _, pixmap_format, depth, width, height = fields[:6]
byte_order, bits_per_pixel, bytes_per_line = fields[7], fields[11], fields[12]
red_mask, green_mask, blue_mask = fields[14:17]
ncolors = fields[19]
if pixmap_format != 2 or bits_per_pixel != 32:
    sys.exit(f'{sys.argv[1]}: format {pixmap_format} at {bits_per_pixel} bits a pixel, not ZPixmap at 32')
start = header_size + ncolors * 12
order = '>' if byte_order else '<'
shift = lambda mask: (mask & -mask).bit_length() - 1
rs, gs, bs = shift(red_mask), shift(green_mask), shift(blue_mask)
rows = []
for y in range(height):
    line = data[start + y * bytes_per_line:start + y * bytes_per_line + width * 4]
    pixels = struct.unpack(f'{order}{width}I', line)
    row = bytearray(b'\0')
    for p in pixels:
        row += bytes(((p >> rs) & 255, (p >> gs) & 255, (p >> bs) & 255))
    rows.append(bytes(row))
chunk = lambda kind, body: struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
png += chunk(b'IDAT', zlib.compress(b''.join(rows), 6)) + chunk(b'IEND', b'')
open(sys.argv[2], 'wb').write(png)
