// The viewer's indexing as it was at 5c9c031 (web/prbm.mjs, done in the worker in JS before the module did it), for
// test/mesher-identity.test.js and test/prbm.test.js: the module's indexed tile must be these bytes.
// BlueMap hires tiles are PRWM-style binary meshes ("PRBM"): an 8-byte
// header, each vertex attribute as a named typed array, optional indices,
// then material groups. BlueMap writes them unindexed, with every quad as two
// triangles (a, b, c) then (a, c, d) and each corner stored again.
//
// shareQuadCorners rewrites a tile indexed: a quad's repeated corners become
// one vertex, but only when every attribute byte is identical. The triangles,
// their order, the groups and every vertex value are unchanged, so the
// renderer draws the same pixels while shading a third fewer vertices.
export function shareQuadCorners(bytes) {
  const flags = bytes[1];
  if (bytes[0] !== 1) throw new Error(`Unsupported PRBM version ${bytes[0]}`);
  if (flags >> 7 & 1) throw new Error('The PRBM tile is indexed already');
  if (flags >> 5 & 1) throw new Error('Big-endian PRBM tiles are not supported');
  const attributeCount = flags & 0x1f;
  const vertices = bytes[2] | bytes[3] << 8 | bytes[4] << 16;

  let pos = 8;
  const attributes = [];
  for (let a = 0; a < attributeCount; a++) {
    const nameStart = pos;
    while (pos < bytes.length && bytes[pos] !== 0) pos++;
    if (pos >= bytes.length) throw new Error('Malformed PRBM tile: unterminated attribute name');
    const name = bytes.subarray(nameStart, pos);
    pos++;
    const attributeFlags = bytes[pos];
    pos++;
    pos = Math.ceil(pos / 4) * 4;
    const cardinality = (attributeFlags >> 4 & 3) + 1;
    // BlueMap's encodings (PRBMLoader's table): 1 Float32, 3 Int8, 4 Int16,
    // 6 Int32, 7 Uint8, 8 Uint16, 10 Uint32.
    const elementBytes = [0, 4, 0, 1, 2, 0, 4, 1, 2, 0, 4][attributeFlags & 0x0f];
    if (!elementBytes) throw new Error(`Unknown PRBM attribute encoding ${attributeFlags & 0x0f}`);
    const stride = cardinality * elementBytes;
    attributes.push({ name, attributeFlags, stride, values: bytes.subarray(pos, pos + stride * vertices) });
    pos += stride * vertices;
    if (pos > bytes.length) throw new Error('Malformed PRBM tile: attribute data past the end');
  }
  pos = Math.ceil(pos / 4) * 4;
  const groupBytes = bytes.subarray(pos);
  const view = new DataView(groupBytes.buffer, groupBytes.byteOffset, groupBytes.byteLength);
  const groups = [];
  for (let at = 0; at + 4 <= groupBytes.length; at += 12) {
    if (view.getInt32(at, true) === -1) break;
    groups.push({ start: view.getInt32(at + 4, true), count: view.getInt32(at + 8, true) });
  }

  const columns = attributes.map(({ values }) => values);
  const strides = attributes.map(({ stride }) => stride);
  const same = (i, j) => {
    for (let a = 0; a < columns.length; a++) {
      const values = columns[a]; const stride = strides[a];
      for (let k = 0, x = i * stride, y = j * stride; k < stride; k++) if (values[x + k] !== values[y + k]) return false;
    }
    return true;
  };
  const index = new Uint32Array(vertices);
  const kept = new Uint32Array(vertices);
  let unique = 0;
  for (const group of groups.length ? groups : [{ start: 0, count: vertices }]) {
    const end = group.start + group.count;
    let v = group.start;
    for (; v + 6 <= end; v += 6) {
      if (same(v + 3, v) && same(v + 4, v + 2)) {
        const a = unique;
        kept[unique++] = v; kept[unique++] = v + 1; kept[unique++] = v + 2; kept[unique++] = v + 5;
        index[v] = a; index[v + 1] = a + 1; index[v + 2] = a + 2;
        index[v + 3] = a; index[v + 4] = a + 2; index[v + 5] = a + 3;
      } else {
        for (let k = 0; k < 6; k++) { index[v + k] = unique; kept[unique++] = v + k; }
      }
    }
    for (; v < end; v++) { index[v] = unique; kept[unique++] = v; }
  }

  // Header, attributes over the kept vertices, Uint32 indices, the same groups.
  const pad = (n) => Math.ceil(n / 4) * 4;
  let size = 8;
  for (const { name, stride } of attributes) size = pad(size + name.length + 2) + stride * unique;
  size = pad(size) + vertices * 4;
  size = pad(size) + groupBytes.length;
  const out = new Uint8Array(size);
  out[0] = 1;
  out[1] = 0x80 | 0x40 | attributeCount;
  out[2] = unique & 0xff; out[3] = unique >> 8 & 0xff; out[4] = unique >> 16 & 0xff;
  out[5] = vertices & 0xff; out[6] = vertices >> 8 & 0xff; out[7] = vertices >> 16 & 0xff;
  let at = 8;
  for (const { name, attributeFlags, stride, values } of attributes) {
    out.set(name, at); at += name.length;
    out[at++] = 0;
    out[at++] = attributeFlags;
    at = pad(at);
    for (let i = 0; i < unique; i++) {
      const from = kept[i] * stride; const to = at + i * stride;
      for (let k = 0; k < stride; k++) out[to + k] = values[from + k];
    }
    at += stride * unique;
  }
  at = pad(at);
  out.set(new Uint8Array(index.buffer), at);
  at = pad(at + vertices * 4);
  out.set(groupBytes, at);
  return out;
}
