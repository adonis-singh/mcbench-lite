'use strict';

// A small PRBM writer, and a reader that expands indices back into the
// per-corner vertex stream the renderer sees.
function writePrbm(attributes, vertices, groups) {
  const parts = [];
  const header = Buffer.alloc(8);
  header[0] = 1; header[1] = attributes.length;
  header[2] = vertices & 0xff; header[3] = vertices >> 8 & 0xff; header[4] = vertices >> 16 & 0xff;
  parts.push(header);
  let length = 8;
  for (const { name, flags, values } of attributes) {
    const head = [...Buffer.from(name), 0, flags];
    const padded = Math.ceil((length + head.length) / 4) * 4 - length;
    const block = Buffer.alloc(padded);
    Buffer.from(head).copy(block);
    parts.push(block, Buffer.from(values.buffer));
    length += padded + values.byteLength;
  }
  parts.push(Buffer.alloc(Math.ceil(length / 4) * 4 - length));
  const groupBytes = Buffer.alloc(groups.length * 12 + 4);
  groups.forEach(({ materialIndex, start, count }, i) => {
    groupBytes.writeInt32LE(materialIndex, i * 12); groupBytes.writeInt32LE(start, i * 12 + 4); groupBytes.writeInt32LE(count, i * 12 + 8);
  });
  groupBytes.writeInt32LE(-1, groups.length * 12);
  parts.push(groupBytes);
  return Buffer.concat(parts);
}

// `corners`: every drawn corner's attribute bytes, in draw order; `groups`:
// the material group bytes; `unique`: the vertices stored.
function expand(buffer) {
  const bytes = new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
  const flags = bytes[1];
  const vertices = bytes[2] | bytes[3] << 8 | bytes[4] << 16;
  const indices = bytes[5] | bytes[6] << 8 | bytes[7] << 16;
  let pos = 8;
  const streams = [];
  for (let a = 0; a < (flags & 0x1f); a++) {
    while (bytes[pos] !== 0) pos++;
    pos++;
    const attributeFlags = bytes[pos++];
    pos = Math.ceil(pos / 4) * 4;
    const stride = ((attributeFlags >> 4 & 3) + 1) * [0, 4, 0, 1, 2, 0, 4, 1, 2, 0, 4][attributeFlags & 0x0f];
    streams.push({ stride, values: bytes.subarray(pos, pos + stride * vertices) });
    pos += stride * vertices;
  }
  let order = [...Array(vertices).keys()];
  if (flags >> 7 & 1) {
    pos = Math.ceil(pos / 4) * 4;
    order = [...new Uint32Array(bytes.slice(pos, pos + indices * 4).buffer)];
    pos += indices * 4;
  }
  pos = Math.ceil(pos / 4) * 4;
  return {
    corners: order.map((v) => streams.map(({ stride, values }) => Buffer.from(values.subarray(v * stride, v * stride + stride)).toString('hex')).join('|')),
    groups: Buffer.from(bytes.subarray(pos)).toString('hex'),
    unique: vertices,
  };
}

module.exports = { writePrbm, expand };
