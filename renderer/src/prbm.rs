// BlueMap's PRBMWriter: the tile as PRWM-style attribute arrays, little
// endian, unindexed, then the material groups.

use crate::tile::Tile;

struct Out<'a> {
    bytes: &'a mut Vec<u8>,
}

impl Out<'_> {
    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }
    fn u24(&mut self, value: usize) {
        assert!(value <= 0xffffff, "Value too high: {value}");
        self.bytes.extend_from_slice(&(value as u32).to_le_bytes()[..3]);
    }
    fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }
    fn padding(&mut self) {
        while self.bytes.len() % 4 != 0 {
            self.bytes.push(0);
        }
    }
    fn attribute(&mut self, name: &str, flags: u8) {
        self.bytes.extend_from_slice(name.as_bytes());
        self.bytes.push(0);
        self.bytes.push(flags);
        self.padding();
    }
    // The material groups of the faces in `order` (material, first vertex,
    // vertex count each), then the end mark.
    fn groups(&mut self, tile: &Tile) {
        let faces = &tile.order;
        let size = tile.size;
        let material = |face: u32| tile.material[face as usize];
        if size > 0 {
            let mut last = material(faces[0]);
            let mut group_start = 0usize;
            self.i32(last);
            self.i32(0);
            for (i, &face) in faces.iter().enumerate().skip(1) {
                let current = material(face);
                if current != last {
                    self.i32(((i - group_start) * 3) as i32);
                    group_start = i;
                    self.i32(current);
                    self.i32((group_start * 3) as i32);
                }
                last = current;
            }
            self.i32(((size - group_start) * 3) as i32);
        }
        self.i32(-1);
    }
    // Float.floatToIntBits of each value: every NaN is written as the
    // canonical one.
    fn f32s<const B: usize>(&mut self, values: &[f32]) {
        let mut bytes = [0u8; B];
        for (slot, &value) in bytes.chunks_exact_mut(4).zip(values) {
            slot.copy_from_slice(&float_bits(value).to_le_bytes());
        }
        self.bytes.extend_from_slice(&bytes);
    }
}

fn float_bits(value: f32) -> u32 {
    if value.is_nan() {
        0x7fc00000
    } else {
        value.to_bits()
    }
}

// (byte) (value * 0x80 - 0.5): a float product, then a double difference
// cast through int.
fn normalized_signed(value: f32) -> u8 {
    ((value * 128.0) as f64 - 0.5) as i32 as u8
}

fn normalized_unsigned(value: f32) -> u8 {
    ((value * 255.0) as i32 & 0xff) as u8
}

const FLOAT: u8 = 0;
const NORMALIZED: u8 = 1 << 6;
const SCALAR: u8 = 0;
const VEC2: u8 = 1 << 4;
const VEC3: u8 = 2 << 4;
const F32: u8 = 1;
const I8: u8 = 3;
const U8: u8 = 7;

// Writes the tile, faces in `order`, over `bytes` (whose memory it keeps).
pub fn write(tile: &Tile, bytes: &mut Vec<u8>) {
    let size = tile.size;
    let faces = &tile.order;
    assert_eq!(faces.len(), size, "The tile is not sorted");
    let material = |face: u32| tile.material[face as usize];
    let changes = faces.windows(2).filter(|pair| material(pair[0]) != material(pair[1])).count();
    bytes.clear();
    // 87 bytes a face, the 16 bytes and 12 a change of the groups, and the header, names and padding.
    bytes.reserve_exact(size * 87 + 16 + changes * 12 + 256);
    let mut out = Out { bytes };
    out.u8(1);
    out.u8(0b0000_0111);
    out.u24(size * 3);
    out.u24(0);

    out.attribute("position", FLOAT | VEC3 | F32);
    for &face in faces {
        let at = face as usize * 9;
        out.f32s::<36>(&tile.position[at..at + 9]);
    }

    out.attribute("normal", FLOAT | NORMALIZED | VEC3 | I8);
    for &face in faces {
        let at = face as usize * 9;
        let [x, y, z] = surface_normal(&tile.position[at..at + 9]).map(normalized_signed);
        out.bytes.extend_from_slice(&[x, y, z, x, y, z, x, y, z]);
    }

    out.attribute("color", FLOAT | NORMALIZED | VEC3 | U8);
    for &face in faces {
        let at = face as usize * 3;
        let [r, g, b] = [0, 1, 2].map(|i| normalized_unsigned(tile.color[at + i]));
        out.bytes.extend_from_slice(&[r, g, b, r, g, b, r, g, b]);
    }

    out.attribute("uv", FLOAT | VEC2 | F32);
    for &face in faces {
        let at = face as usize * 6;
        out.f32s::<24>(&tile.uv[at..at + 6]);
    }

    out.attribute("ao", FLOAT | NORMALIZED | SCALAR | U8);
    for &face in faces {
        let at = face as usize * 3;
        out.bytes.extend_from_slice(&[0, 1, 2].map(|i| normalized_unsigned(tile.ao[at + i])));
    }

    for (name, values) in [("blocklight", &tile.blocklight), ("sunlight", &tile.sunlight)] {
        out.attribute(name, FLOAT | SCALAR | I8);
        for &face in faces {
            let value = values[face as usize];
            out.bytes.extend_from_slice(&[value; 3]);
        }
    }

    out.padding();
    out.groups(tile);
}

// What `write` stores for one corner of a face: every attribute's bytes.
#[derive(PartialEq)]
struct Corner {
    position: [u32; 3],
    normal: [u8; 3],
    color: [u8; 3],
    uv: [u32; 2],
    ao: u8,
    blocklight: u8,
    sunlight: u8,
}

// The tile of `write` indexed for the GPU, with 32-bit indices: the two
// faces of a quad, (a, b, c) and (a, c, d) as the tile holds them, store
// the corner they repeat once when every attribute's bytes agree; any other
// corner is stored as it is. The triangles, their order, the groups and
// every corner's bytes are those of `write`'s tile. Faces pair up from the
// start of each material group.
pub fn write_indexed(tile: &Tile, bytes: &mut Vec<u8>) {
    let size = tile.size;
    let faces = &tile.order;
    assert_eq!(faces.len(), size, "The tile is not sorted");
    let vertices = size * 3;
    let normals: Vec<[u8; 3]> = faces.iter().map(|&face| {
        let at = face as usize * 9;
        surface_normal(&tile.position[at..at + 9]).map(normalized_signed)
    }).collect();
    let colors: Vec<[u8; 3]> = faces.iter().map(|&face| {
        let at = face as usize * 3;
        [0, 1, 2].map(|i| normalized_unsigned(tile.color[at + i]))
    }).collect();
    // Corner `v` of the faces in order: face v / 3, its corner v % 3.
    let corner = |v: usize| {
        let (at, c) = (v / 3, v % 3);
        let face = faces[at] as usize;
        let p = face * 9 + c * 3;
        let u = face * 6 + c * 2;
        Corner {
            position: [0, 1, 2].map(|i| float_bits(tile.position[p + i])),
            normal: normals[at],
            color: colors[at],
            uv: [0, 1].map(|i| float_bits(tile.uv[u + i])),
            ao: normalized_unsigned(tile.ao[face * 3 + c]),
            blocklight: tile.blocklight[face],
            sunlight: tile.sunlight[face],
        }
    };

    // The corners kept (by v), and for each corner drawn the kept one it is.
    let mut kept: Vec<u32> = Vec::with_capacity(vertices);
    let mut index: Vec<u32> = Vec::with_capacity(vertices);
    let material = |at: usize| tile.material[faces[at] as usize];
    let mut group_end = 0;
    let mut at = 0;
    while at < size {
        if at == group_end {
            group_end = (at..size).find(|&i| material(i) != material(at)).unwrap_or(size);
        }
        let v = at * 3;
        if at + 2 <= group_end && corner(v + 3) == corner(v) && corner(v + 4) == corner(v + 2) {
            let first = kept.len() as u32;
            kept.extend_from_slice(&[v as u32, v as u32 + 1, v as u32 + 2, v as u32 + 5]);
            index.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
            at += 2;
        } else {
            let first = kept.len() as u32;
            let (face, drawn) = (at * 3, if at + 2 <= group_end { 6 } else { 3 });
            kept.extend((face..face + drawn).map(|v| v as u32));
            index.extend(first..first + drawn as u32);
            at += drawn / 3;
        }
    }
    let unique = kept.len();

    let changes = faces.windows(2).filter(|pair| tile.material[pair[0] as usize] != tile.material[pair[1] as usize]).count();
    bytes.clear();
    // 29 bytes a kept corner, 4 an index, the 16 bytes and 12 a change of the groups, and the header, names and padding.
    bytes.reserve_exact(unique * 29 + vertices * 4 + 16 + changes * 12 + 256);
    let mut out = Out { bytes };
    out.u8(1);
    out.u8(0b1100_0111);
    out.u24(unique);
    out.u24(vertices);

    // The face (as stored) and corner of each kept corner, and its place in `order`.
    let stored = |v: &u32| (*v as usize / 3, faces[*v as usize / 3] as usize, *v as usize % 3);

    out.attribute("position", FLOAT | VEC3 | F32);
    for v in &kept {
        let (_, face, c) = stored(v);
        out.f32s::<12>(&tile.position[face * 9 + c * 3..face * 9 + c * 3 + 3]);
    }

    out.attribute("normal", FLOAT | NORMALIZED | VEC3 | I8);
    for v in &kept {
        out.bytes.extend_from_slice(&normals[stored(v).0]);
    }

    out.attribute("color", FLOAT | NORMALIZED | VEC3 | U8);
    for v in &kept {
        out.bytes.extend_from_slice(&colors[stored(v).0]);
    }

    out.attribute("uv", FLOAT | VEC2 | F32);
    for v in &kept {
        let (_, face, c) = stored(v);
        out.f32s::<8>(&tile.uv[face * 6 + c * 2..face * 6 + c * 2 + 2]);
    }

    out.attribute("ao", FLOAT | NORMALIZED | SCALAR | U8);
    for v in &kept {
        let (_, face, c) = stored(v);
        out.bytes.push(normalized_unsigned(tile.ao[face * 3 + c]));
    }

    for (name, values) in [("blocklight", &tile.blocklight), ("sunlight", &tile.sunlight)] {
        out.attribute(name, FLOAT | SCALAR | I8);
        for v in &kept {
            out.bytes.push(values[stored(v).1]);
        }
    }

    out.padding();
    for &i in &index {
        out.bytes.extend_from_slice(&i.to_le_bytes());
    }
    out.groups(tile);
}

// PRBMWriter.calculateSurfaceNormal
fn surface_normal(p: &[f32]) -> [f32; 3] {
    let (p1x, p1y, p1z) = (p[0], p[1], p[2]);
    let (p2x, p2y, p2z) = (p[3] - p1x, p[4] - p1y, p[5] - p1z);
    let (p3x, p3y, p3z) = (p[6] - p1x, p[7] - p1y, p[8] - p1z);
    let nx = p2y * p3z - p2z * p3y;
    let ny = p2z * p3x - p2x * p3z;
    let nz = p2x * p3y - p2y * p3x;
    let length = ((nx * nx + ny * ny + nz * nz) as f64).sqrt() as f32;
    [nx / length, ny / length, nz / length]
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every drawn corner's attribute bytes in draw order, the material
    // groups' bytes and the corners stored, of a tile `write` or
    // `write_indexed` made.
    fn expand(bytes: &[u8]) -> (Vec<Vec<u8>>, Vec<u8>, usize) {
        let indexed = bytes[1] >> 7 & 1 == 1;
        assert_eq!(bytes[1] & 0x1f, 7);
        let stored = bytes[2] as usize | (bytes[3] as usize) << 8 | (bytes[4] as usize) << 16;
        let drawn = bytes[5] as usize | (bytes[6] as usize) << 8 | (bytes[7] as usize) << 16;
        let mut at = 8;
        let mut streams = Vec::new();
        for stride in [12, 3, 3, 8, 1, 1, 1] {
            while bytes[at] != 0 {
                at += 1;
            }
            at += 2;
            at = at.next_multiple_of(4);
            streams.push((stride, &bytes[at..at + stride * stored]));
            at += stride * stored;
        }
        at = at.next_multiple_of(4);
        let order: Vec<usize> = if indexed {
            let indices = bytes[at..at + drawn * 4].chunks_exact(4).map(|i| u32::from_le_bytes([i[0], i[1], i[2], i[3]]) as usize).collect();
            at += drawn * 4;
            indices
        } else {
            assert_eq!(drawn, 0);
            (0..stored).collect()
        };
        let corners = order.iter().map(|&i| streams.iter().flat_map(|&(stride, values)| values[i * stride..(i + 1) * stride].to_vec()).collect()).collect();
        (corners, bytes[at.next_multiple_of(4)..].to_vec(), stored)
    }

    #[test]
    fn indexing_shares_a_quads_repeated_corners_only_when_every_attribute_agrees() {
        let mut tile = Tile::default();
        let square = |x: f32| [[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x + 1.0, 1.0, 0.0], [x, 1.0, 0.0]];
        let uv = [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0];
        // Two quads of material 5, a quad of 2 whose second face is lit differently, and a lone face of 9.
        tile.quad(square(0.0), uv, [0.5, 0.25, 1.0], [1.0, 0.5, 0.0, 1.0], 15, 3, 5);
        tile.quad(square(2.0), uv, [0.5, 0.25, 1.0], [0.0, 1.0, 1.0, 0.5], 14, 0, 5);
        tile.quad(square(4.0), uv, [1.0, 1.0, 1.0], [1.0; 4], 15, 0, 2);
        tile.blocklight[5] = 7;
        tile.quad(square(6.0), uv, [1.0, 0.0, 0.0], [1.0; 4], 1, 1, 9);
        tile.size -= 1;
        tile.position.truncate(tile.size * 9);
        tile.uv.truncate(tile.size * 6);
        tile.ao.truncate(tile.size * 3);
        tile.color.truncate(tile.size * 3);
        tile.sunlight.truncate(tile.size);
        tile.blocklight.truncate(tile.size);
        tile.material.truncate(tile.size);
        tile.sort();
        let (mut plain, mut indexed) = (Vec::new(), Vec::new());
        write(&tile, &mut plain);
        write_indexed(&tile, &mut indexed);
        let (before, groups_before, stored_before) = expand(&plain);
        let (after, groups_after, stored_after) = expand(&indexed);
        assert_eq!(stored_before, 21);
        assert_eq!(stored_after, 4 + 4 + 6 + 3, "two quads shared, the quad with a mismatch and the lone face kept");
        assert_eq!(after, before, "every drawn corner keeps its bytes and place");
        assert_eq!(groups_after, groups_before);
        assert_eq!(indexed[1], 0b1100_0111);
        // An empty tile is a header, empty attributes and the end of the groups.
        let (mut empty_plain, mut empty_indexed) = (Vec::new(), Vec::new());
        write(&Tile::default(), &mut empty_plain);
        write_indexed(&Tile::default(), &mut empty_indexed);
        assert_eq!(expand(&empty_indexed).1, expand(&empty_plain).1);
        assert_eq!(expand(&empty_indexed).2, 0);
    }
}
