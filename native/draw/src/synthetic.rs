// The synthetic pack: a few blocks of the project's own (textures drawn
// here, block models written here), in the formats of today's bake, so the
// crate's tests draw pictures without any of Minecraft's data. Written by
// `mcbench-draw synthetic <folder>` into tests/synthetic (committed; a test
// holds the committed files to what this writes).
//
// Blocks: test:stone (a full cube of two random variants), test:glass
// (blended, see-through, skipping its own faces), test:lamp (emits 15),
// test:leaves (tinted green, cut out), test:slab (type=bottom|top),
// test:torch (a small post that emits 14), test:sign (drawn by a block
// entity renderer's quad), test:water (level=0..7, a fluid drawn as
// Minecraft's LiquidBlockRenderer draws it), test:lava and test:wet_slab
// (a bottom slab holding water).

use std::path::Path;

pub const TEXTURES: [&str; 14] = [
    "test:block/stone",
    "test:block/stone_mossy",
    "test:block/glass",
    "test:block/lamp",
    "test:block/leaves",
    "test:block/slab",
    "test:block/torch",
    "test:block/sign",
    "test:block/slab_top",
    "test:block/water_still",
    "test:block/water_flow",
    "test:block/lava_still",
    "test:block/lava_flow",
    "test:block/water_overlay",
];

struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f32(&mut self, v: f32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn f64(&mut self, v: f64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn str(&mut self, s: &str) -> &mut Self {
        self.u16(s.len() as u16);
        self.0.extend_from_slice(s.as_bytes());
        self
    }
}

const NONE: u32 = u32::MAX;
// State flags (renderer/src/pack.rs).
const SHADES: u8 = 1;
const WATERLOGGED: u8 = 8;
const WATER: u8 = 16;
const AIR: u8 = 32;

struct State {
    name: &'static str,
    flags: u8,
    emission: u8,
    light_block: u8,
    faces: u8,
    cull: u16,
    block: u16,
    tint: u32,
    selection: u32,
    // Block table: six full faces; render layers: the model's and the
    // fluid's layer.
    full: bool,
    layer: u8,
    fluid_layer: u8,
    // Its render class (render_classes()).
    render: u16,
}

// The states, by state index (block by block, a block's states in order).
fn states() -> Vec<State> {
    let s = |name,
             flags,
             emission,
             light_block,
             faces,
             cull,
             block,
             tint,
             selection,
             full,
             layer,
             fluid_layer,
             render| State {
        name,
        flags,
        emission,
        light_block,
        faces,
        cull,
        block,
        tint,
        selection,
        full,
        layer,
        fluid_layer,
        render,
    };
    let mut out = vec![
        s("minecraft:air", AIR, 0, 0, 0, 0, 0, 0, NONE, false, 0, 0, 0),
        s("test:stone", SHADES, 0, 15, 1, 1, 1, 0, 0, true, 0, 0, 1),
        s("test:glass", SHADES, 0, 0, 0, 2, 2, 0, 1, false, 2, 0, 2),
        s("test:lamp", SHADES, 15, 15, 1, 1, 3, 0, 2, true, 0, 0, 1),
        s("test:leaves", SHADES, 0, 1, 0, 0, 4, 1, 3, false, 1, 0, 3),
        s(
            "test:slab[type=bottom]",
            0,
            0,
            0,
            0,
            3,
            5,
            0,
            4,
            false,
            0,
            0,
            4,
        ),
        s(
            "test:slab[type=top]",
            0,
            0,
            0,
            0,
            4,
            5,
            0,
            5,
            false,
            0,
            0,
            5,
        ),
        s("test:torch", 0, 14, 0, 0, 0, 6, 0, 6, false, 1, 0, 6),
        s("test:sign", 0, 0, 0, 0, 0, 7, 0, 7, false, 0, 0, 6),
    ];
    const LEVELS: [&str; 8] = [
        "test:water[level=0]",
        "test:water[level=1]",
        "test:water[level=2]",
        "test:water[level=3]",
        "test:water[level=4]",
        "test:water[level=5]",
        "test:water[level=6]",
        "test:water[level=7]",
    ];
    for (level, name) in LEVELS.into_iter().enumerate() {
        out.push(s(
            name,
            WATER,
            0,
            1,
            0,
            0,
            8,
            0,
            NONE,
            false,
            0,
            2,
            7 + level as u16,
        ));
    }
    out.push(s("test:lava", 0, 15, 1, 0, 0, 9, 0, NONE, false, 0, 0, 15));
    out.push(s(
        "test:wet_slab",
        WATERLOGGED,
        0,
        1,
        0,
        3,
        10,
        0,
        4,
        false,
        0,
        2,
        16,
    ));
    out
}

// The render classes: (fluid group, amount, self-hidden faces, the
// greatest side height a neighbour hides, flags, sturdy faces).
fn render_classes() -> Vec<(u8, u8, u8, f32, u8, u8)> {
    // Flags (renderer/src/pack.rs): 1 hides a fluid's bottom above, 2 a
    // fluid's full top below, 4 solid, 8 blocks motion, 16 solid render,
    // 64 water beside it drawn with the overlay.
    let mut out = vec![
        (0, 0, 0, -1.0, 0, 0),     // air
        (0, 0, 0, 1.0, 31, 0x3f),  // full cubes
        (0, 0, 0, -1.0, 76, 0x3f), // glass
        (0, 0, 0, -1.0, 76, 0x3f), // leaves
        (0, 0, 0, 0.5, 12, 1),     // bottom slab
        (0, 0, 0, -1.0, 13, 2),    // top slab
        (0, 0, 0, -1.0, 0, 0),     // torch, sign
    ];
    for level in 0..8u8 {
        out.push((1, 8 - level, 0, -1.0, 0, 0)); // water, its amount 8 less the level
    }
    out.push((2, 8, 0, -1.0, 0, 0)); // lava
    out.push((1, 8, 1, 0.5, 12, 1)); // a bottom slab holding water: its own bottom hides the water's
    out
}

// A cube's six faces from `lo` to `hi` (sixteenths), each culled toward its
// own direction when `cull`, all with `texture`.
fn element(
    out: &mut Out,
    lo: [f32; 3],
    hi: [f32; 3],
    emission: u32,
    texture: u32,
    tinted: bool,
    cull: [bool; 6],
) {
    for v in lo.iter().chain(hi.iter()) {
        out.f32(*v);
    }
    // Matrix 0: the 1/16 scale.
    out.u32(0).u32(emission).u8(6);
    for direction in 0..6u8 {
        // The face's texture rectangle: the element's extent across it.
        let (a, b) = match direction {
            0 | 1 => (0, 2),
            2 | 3 => (0, 1),
            _ => (2, 1),
        };
        let (u0, u1, v0, v1) = (
            lo[a] / 16.0,
            hi[a] / 16.0,
            1.0 - hi[b] / 16.0,
            1.0 - lo[b] / 16.0,
        );
        let (v0, v1) = if direction < 2 {
            (lo[b] / 16.0, hi[b] / 16.0)
        } else {
            (v0, v1)
        };
        out.u8(direction);
        for v in [u0, v0, u0, v1, u1, v1, u1, v0] {
            out.f32(v);
        }
        out.u32(texture)
            .u8(tinted as u8)
            .u8(if cull[direction as usize] {
                direction
            } else {
                255
            });
    }
}

pub fn pack() -> Vec<u8> {
    let mut o = Out(Vec::new());
    o.0.extend_from_slice(b"MCBP");
    o.u32(7);
    // Blocks.
    let blocks: [(&str, u32, &str, &[&str]); 11] = [
        ("minecraft:air", 0, "", &[]),
        ("test:stone", 1, "", &[]),
        ("test:glass", 2, "", &[]),
        ("test:lamp", 3, "", &[]),
        ("test:leaves", 4, "", &[]),
        ("test:slab", 5, "type", &["bottom", "top"]),
        ("test:torch", 7, "", &[]),
        ("test:sign", 8, "", &[]),
        (
            "test:water",
            9,
            "level",
            &["0", "1", "2", "3", "4", "5", "6", "7"],
        ),
        ("test:lava", 17, "", &[]),
        ("test:wet_slab", 18, "", &[]),
    ];
    o.u32(blocks.len() as u32);
    for (id, first, property, values) in blocks {
        o.str(id).u32(first);
        if values.is_empty() {
            o.u8(0);
        } else {
            o.u8(1).str(property).u8(0).u8(values.len() as u8);
            for v in values {
                o.str(v);
            }
        }
    }
    let states = states();
    o.u32(states.len() as u32);
    for s in &states {
        o.u8(s.flags)
            .u8(0)
            .u8(s.emission)
            .u8(s.light_block)
            .u8(0)
            .u8(s.faces)
            .u16(s.cull)
            .u16(s.block)
            .u32(s.tint)
            .u32(s.selection)
            .u16(s.render);
    }
    // Face sets: none, all full. Face shapes 0 empty, 1 full: light stops
    // between two faces when either is full.
    o.u32(2);
    o.0.extend_from_slice(&[0; 6]);
    o.0.extend_from_slice(&[1; 6]);
    o.u16(2).u8(0b1110);
    // Cull classes: 0 none, 1 full, 2 glass (skips glass), 3 bottom slab, 4
    // top slab. Cull shape 1 is the full face; a face beside a full face is
    // hidden.
    o.u32(5);
    for (shapes, row, column) in [
        ([0u8; 6], 0u16, 0u16),
        ([1; 6], 0, 0),
        ([0; 6], 1, 1),
        ([1, 0, 0, 0, 0, 0], 0, 0),
        ([0, 1, 0, 0, 0, 0], 0, 0),
    ] {
        o.0.extend_from_slice(&shapes);
        o.u16(row).u16(column).u16(0);
    }
    o.u16(2).u8(0b1010);
    // skipRendering: row 0 skips nothing, row 1 (glass) skips column 1
    // (glass) in every direction.
    o.u16(2).u16(0).u16(0).u16(1).u16(1).u8(0x3f);
    // Air; water's selection none, its tint 2.
    o.u32(0).u32(NONE).u32(2).u8(0);
    // Render classes: the seed of the block's own position and no offset
    // for every one, then each one's fluid facts.
    let classes = render_classes();
    o.u32(classes.len() as u32);
    for (fluid, amount, self_hidden, hides, flags, sturdy) in classes {
        o.u8(0).u8(0).u8(0).u8(0).f32(0.0).f32(0.0);
        o.u8(fluid).u8(amount).u8(0).u8(self_hidden);
        for _ in 0..4 {
            o.f32(hides);
        }
        o.u8(flags).u8(sturdy);
    }
    // Fluid groups: water, lava (their still and flowing textures); the
    // water overlay texture.
    o.u32(2).u8(0).u32(9).u32(10).u8(1).u32(11).u32(12).u32(13);
    // Tints: white, leaf green, water blue.
    o.u32(3);
    for t in [[1.0f32, 1.0, 1.0], [0.42, 0.68, 0.25], [0.25, 0.46, 0.89]] {
        o.f32(t[0]).f32(t[1]).f32(t[2]);
    }
    // Matrices: the 1/16 scale.
    o.u32(1);
    let s = 1.0 / 16.0;
    for v in [s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, s, 0.0] {
        o.f32(v);
    }
    // One direction table: no turn.
    o.u32(1);
    for i in 0..27i32 {
        o.u8((i / 9 - 1) as i8 as u8)
            .u8((i / 3 % 3 - 1) as i8 as u8)
            .u8((i % 3 - 1) as i8 as u8);
    }
    // Elements.
    let full = ([0.0, 0.0, 0.0], [16.0, 16.0, 16.0]);
    o.u32(8);
    element(&mut o, full.0, full.1, 0, 0, false, [true; 6]);
    element(&mut o, full.0, full.1, 0, 1, false, [true; 6]);
    element(&mut o, full.0, full.1, 0, 2, false, [true; 6]);
    element(&mut o, full.0, full.1, 15, 3, false, [true; 6]);
    element(&mut o, full.0, full.1, 0, 4, true, [true; 6]);
    element(
        &mut o,
        [0.0; 3],
        [16.0, 8.0, 16.0],
        0,
        5,
        false,
        [true, false, true, true, true, true],
    );
    element(
        &mut o,
        [0.0, 8.0, 0.0],
        [16.0; 3],
        0,
        8,
        false,
        [false, true, true, true, true, true],
    );
    element(
        &mut o,
        [7.0, 0.0, 7.0],
        [9.0, 10.0, 9.0],
        14,
        6,
        false,
        [true, false, false, false, false, false],
    );
    // Meshes: one element each, ambient occlusion but for the torch.
    o.u32(8);
    for e in 0..8u32 {
        o.u8((e != 7) as u8).u16(1).u32(e);
    }
    // Variants: the eight models, then the sign's block entity quad.
    o.u32(9);
    for mesh in 0..8u32 {
        o.u8(1).u32(mesh).u8(0).u32(0).u32(0);
    }
    o.u8(3).u32(0).u8(0).u8(0).u8(0);
    // Variant sets: stone picks between its two models by position; every
    // other state has one.
    let sets: [&[(f64, u32)]; 8] = [
        &[(1.0, 0), (1.0, 1)],
        &[(1.0, 2)],
        &[(1.0, 3)],
        &[(1.0, 4)],
        &[(1.0, 5)],
        &[(1.0, 6)],
        &[(1.0, 7)],
        &[(1.0, 8)],
    ];
    o.u32(sets.len() as u32);
    for set in sets {
        o.f64(set.iter().map(|(w, _)| w).sum())
            .u16(set.len() as u16);
        for (w, v) in set {
            o.f64(*w).u32(*v);
        }
    }
    // Selections: one set each, not multipart.
    o.u32(8);
    for set in 0..8u32 {
        o.u8(0).u16(1).u32(set);
    }
    // The sign's quad: a board across the block, facing south, its corners
    // in blocks.
    o.u32(1);
    for c in [
        [0.1f32, 0.3, 0.55],
        [0.9, 0.3, 0.55],
        [0.9, 0.9, 0.55],
        [0.1, 0.9, 0.55],
    ] {
        o.f32(c[0]).f32(c[1]).f32(c[2]);
    }
    for v in [0.0f32, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0] {
        o.f32(v);
    }
    o.u32(1).u32(0).u32(7).u32(0);
    o.u32(1).u32(1).u32(0);
    // Liquid uvs: no liquid is drawn.
    for _ in 0..(361 + 1) * 8 {
        o.f32(0.0);
    }
    o.0
}

// A texture's pixel at x, y: patterns of the project's own.
fn texel(texture: usize, x: u32, y: u32) -> [u8; 4] {
    let n = (x * 7 + y * 13 + texture as u32 * 31).wrapping_mul(2654435761u32) >> 27;
    let n = n as u8;
    match texture {
        0 => [118 + n, 118 + n, 122 + n, 255],
        1 => [96 + n, 118 + n * 2, 88 + n, 255],
        // Glass: an opaque rim, a half-transparent pane, clear spots.
        2 => {
            if x == 0 || y == 0 || x == 15 || y == 15 {
                [200, 230, 240, 255]
            } else if (x + y).is_multiple_of(5) {
                [0, 0, 0, 0]
            } else {
                [170, 210, 235, 96]
            }
        }
        3 => [250, 210 - n * 2, 90 + n, 255],
        // Leaves: grey (tinted green), with holes.
        4 => {
            if (x * 3 + y * 5).is_multiple_of(7) {
                [0, 0, 0, 0]
            } else {
                [150 + n * 2, 150 + n * 2, 150 + n * 2, 255]
            }
        }
        5 => {
            if y.is_multiple_of(4) || (x + (y / 4) * 8).is_multiple_of(16) {
                [190, 180, 170, 255]
            } else {
                [160, 70 + n, 60, 255]
            }
        }
        6 => [130 + n, 90, 40, 255],
        7 => [150 + n, 110 + n, 60, 255],
        // Water (grey, tinted blue), half transparent; its flow in stripes.
        9 => [170 + n, 170 + n, 170 + n, 170],
        10 => [
            if (x + y) % 4 < 2 { 200 } else { 150 } + n,
            170 + n,
            170 + n,
            170,
        ],
        11 => [230, 100 + n * 2, 20, 255],
        12 => [if (x + y) % 4 < 2 { 250 } else { 200 }, 80 + n * 2, 20, 255],
        // The water overlay: grey, mostly opaque, with a light rim.
        13 => {
            if x == 0 || y == 0 || x == 15 || y == 15 {
                [230, 230, 230, 220]
            } else {
                [150 + n, 150 + n, 150 + n, 200]
            }
        }
        _ => [90 + n, 90 + n, 140 + n, 255],
    }
}

// The atlas: the textures in a row of four, 16 texels each, and its map.
pub fn atlas() -> (Vec<u8>, String) {
    let (w, h) = (64u32, 64u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut map = serde_json::Map::new();
    for (i, name) in TEXTURES.iter().enumerate() {
        let (ox, oy) = ((i as u32 % 4) * 16, (i as u32 / 4) * 16);
        for y in 0..16 {
            for x in 0..16 {
                let at = (((oy + y) * w + ox + x) * 4) as usize;
                rgba[at..at + 4].copy_from_slice(&texel(i, x, y));
            }
        }
        map.insert(name.to_string(), serde_json::json!([ox, oy, 16, 16]));
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image_compat(&rgba, w, h)
        .expect("a PNG");
    (png, serde_json::to_string(&map).unwrap())
}

trait Compat {
    fn write_image_compat(self, rgba: &[u8], w: u32, h: u32) -> image::ImageResult<()>;
}

impl<W: std::io::Write> Compat for image::codecs::png::PngEncoder<W> {
    fn write_image_compat(self, rgba: &[u8], w: u32, h: u32) -> image::ImageResult<()> {
        use image::ImageEncoder;
        self.write_image(rgba, w, h, image::ExtendedColorType::Rgba8)
    }
}

// The block table and render layers for the states (assets::facts).
fn table_and_layers() -> (String, String) {
    let mut table = String::new();
    let mut layers = String::from("layer\tsolid\t0\nlayer\tcutout\t0\nlayer\ttranslucent\t1\n");
    for s in states() {
        table.push_str(&format!(
            "{}\t0\t0\t{}\n",
            s.name,
            if s.full { 63 } else { 0 }
        ));
        layers.push_str(&format!("{}\t{}\t{}\n", s.name, s.layer, s.fluid_layer));
    }
    // The table's three pair tables (not read by the facts).
    table.push_str("pairs\nfaceoccludes\nfacehides\n");
    (table, layers)
}

// A blueprint of (Minecraft position, state index) blocks over the
// palette of the pack's states, every state in it.
pub fn blueprint(blocks: &[([i32; 3], u16)]) -> Vec<u8> {
    let all: Vec<u16> = (0..states().len() as u16).collect();
    blueprint_over(blocks, &all)
}

// The same over the palette of air and the states used, in the order first
// used (as the engine writes a palette: what one world holds).
pub fn blueprint_compact(blocks: &[([i32; 3], u16)]) -> Vec<u8> {
    let mut palette = vec![0u16];
    for (_, s) in blocks {
        if !palette.contains(s) {
            palette.push(*s);
        }
    }
    blueprint_over(blocks, &palette)
}

fn blueprint_over(blocks: &[([i32; 3], u16)], used: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    let every = states();
    let palette: Vec<&State> = used.iter().map(|&i| &every[i as usize]).collect();
    let index = |state: u16| used.iter().position(|&u| u == state).unwrap() as u16;
    out.extend_from_slice(&(palette.len() as i32).to_be_bytes());
    for s in palette {
        let (id, props) = match s.name.split_once('[') {
            Some((id, rest)) => (
                id,
                rest.trim_end_matches(']')
                    .split(',')
                    .map(|p| p.split_once('=').unwrap())
                    .collect::<Vec<_>>(),
            ),
            None => (s.name, Vec::new()),
        };
        out.extend_from_slice(&(id.len() as u16).to_be_bytes());
        out.extend_from_slice(id.as_bytes());
        out.extend_from_slice(&(props.len() as i32).to_be_bytes());
        for (k, v) in props {
            for t in [k, v] {
                out.extend_from_slice(&(t.len() as u16).to_be_bytes());
                out.extend_from_slice(t.as_bytes());
            }
        }
    }
    let mut sections: std::collections::BTreeMap<[i32; 3], Vec<u8>> = Default::default();
    for (b, v) in blocks {
        let cells = sections
            .entry(b.map(|c| c >> 4))
            .or_insert_with(|| vec![0; 8192]);
        let i = (((b[1] & 15) << 8) | ((b[2] & 15) << 4) | (b[0] & 15)) as usize;
        cells[i * 2..i * 2 + 2].copy_from_slice(&index(*v).to_be_bytes());
    }
    let lo = [0, 1, 2].map(|k| blocks.iter().map(|(b, _)| b[k]).min().unwrap());
    let hi = [0, 1, 2].map(|k| blocks.iter().map(|(b, _)| b[k]).max().unwrap());
    for v in lo.iter().chain(hi.iter()) {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&(sections.len() as i32).to_be_bytes());
    for (s, cells) in sections {
        for v in s {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&cells);
    }
    out
}

// The test world, at Minecraft heights (y from 64): a floor of stone, a
// glass box holding a lamp, a closed stone room with a lamp inside (light
// that reaches out only through its open door), a leaf bush, slabs both
// ways, torches and a sign.
pub fn world() -> Vec<([i32; 3], u16)> {
    let mut b = Vec::new();
    let y0 = 64;
    for x in -10..=10 {
        for z in -10..=10 {
            b.push(([x, y0, z], 1));
        }
    }
    for x in 2..=4 {
        for y in 1..=3 {
            for z in 2..=4 {
                b.push(([x, y0 + y, z], if (x, y, z) == (3, 2, 3) { 3 } else { 2 }));
            }
        }
    }
    for x in -8..=-4 {
        for y in 1..=4 {
            for z in -8..=-4 {
                let wall = x == -8 || x == -4 || z == -8 || z == -4 || y == 4;
                let door = x == -4 && z == -6 && y <= 2;
                if wall && !door {
                    b.push(([x, y0 + y, z], 1));
                }
            }
        }
    }
    b.push(([-6, y0 + 1, -6], 3));
    for (x, y, z) in [
        (6, 1, -6),
        (7, 1, -6),
        (6, 2, -6),
        (6, 1, -7),
        (7, 2, -7),
        (5, 1, -5),
    ] {
        b.push(([x, y0 + y, z], 4));
    }
    for x in -3..=3 {
        b.push(([x, y0 + 1, 8], if x % 2 == 0 { 5 } else { 6 }));
    }
    b.push(([0, y0 + 1, 0], 7));
    b.push(([-2, y0 + 1, 5], 7));
    b.push(([1, y0 + 1, -2], 8));
    // Fluids (state 9 + level is test:water at that level): still water
    // against the glass box (drawn with the overlay), water flowing down a
    // step, a pool of lava, and a slab holding water beside a source.
    for z in 2..=4 {
        b.push(([1, y0 + 1, z], 9));
    }
    b.push(([6, y0 + 1, 2], 1));
    b.push(([6, y0 + 1, 3], 1));
    b.push(([6, y0 + 2, 2], 9));
    b.push(([6, y0 + 2, 3], 10));
    b.push(([7, y0 + 1, 2], 11));
    b.push(([7, y0 + 1, 3], 12));
    b.push(([8, y0 + 1, 2], 13));
    b.push(([-8, y0 + 1, 6], 17));
    b.push(([-7, y0 + 1, 6], 17));
    b.push(([0, y0 + 1, 6], 18));
    b.push(([1, y0 + 1, 6], 9));
    b
}

// Resolved shots of the world (the page's heights: Minecraft's less 64),
// as server/shot.js resolveShot gives them: an iso view, a top view with
// its grid, a perspective shot clipped to the glass box, a view from low
// down, and a view of the fluids, each at 480 x 360.
pub fn shots() -> serde_json::Value {
    use serde_json::json;
    let d = std::f64::consts::PI / 180.0;
    let grid = json!({
        "columns": { "edges": (-11..=11).map(|b| json!({ "at": (b as f64 + 11.0) * 480.0 / 22.0, "major": b % 5 == 0 })).collect::<Vec<_>>(),
                     "labels": (-10..=10).filter(|b| b % 5 == 0).map(|b| json!({ "at": (b as f64 + 11.5) * 480.0 / 22.0, "text": b.to_string() })).collect::<Vec<_>>() },
        "rows": { "edges": (-8..=8).map(|b| json!({ "at": (b as f64 + 8.25) * 360.0 / 16.5, "major": b % 5 == 0 })).collect::<Vec<_>>(),
                  "labels": (-5..=5).filter(|b| b % 5 == 0).map(|b| json!({ "at": (b as f64 + 8.75) * 360.0 / 16.5, "text": b.to_string() })).collect::<Vec<_>>() },
        "legend": "+x →   +z ↓   grid: 1 block, labels every 5",
        "step": 5,
    });
    json!([
        { "width": 480, "height": 360, "clip": null, "grid": null, "camera": { "yaw": -45.0 * d, "pitch": 30.0 * d, "focus": [0.5, 2.0, 0.5], "focusDist": 21.0, "fov": 70 } },
        { "width": 480, "height": 360, "clip": null, "grid": grid, "camera": { "yaw": 0.0, "pitch": 90.0 * d, "focus": [0.5, 5.0, 0.5], "focusDist": 8.0, "ortho": { "halfWidth": 11.0, "halfHeight": 8.25, "near": 0.05, "far": 20.0 } } },
        { "width": 480, "height": 360, "clip": { "min": [2, 1, 2], "max": [4, 3, 4] }, "grid": null, "camera": { "yaw": -135.0 * d, "pitch": 20.0 * d, "focus": [3.5, 2.5, 3.5], "focusDist": 7.0, "fov": 60 } },
        { "width": 480, "height": 360, "clip": null, "grid": null, "camera": { "yaw": -70.0 * d, "pitch": 12.0 * d, "focus": [-4.0, 2.0, -6.0], "focusDist": 8.0, "fov": 80 } },
        { "width": 480, "height": 360, "clip": null, "grid": null, "camera": { "yaw": -25.0 * d, "pitch": 28.0 * d, "focus": [3.5, 1.5, 4.0], "focusDist": 10.0, "fov": 70 } },
    ])
}

// An entity capture with no kinds: the synthetic pack draws no entities.
const CAPTURE: &str = r#"{"atlas":{"width":16,"height":16,"rects":{}},"version":1,"id":0,"textures":[],"shapes":"","quads":"","poses":"","meshes":"AAAAAA==","kinds":{},"items":{},"specials":{}}"#;

// A client jar holding only the dimension type the helper reads (its
// height: the overworld's, -64 and 384 blocks).
fn client_jar() -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default());
    zip.start_file("data/minecraft/dimension_type/overworld.json", options)
        .unwrap();
    zip.write_all(br#"{"min_y":-64,"height":384}"#).unwrap();
    zip.finish().unwrap().into_inner()
}

// Every file, by name.
pub fn files() -> Vec<(&'static str, Vec<u8>)> {
    let (png, map) = atlas();
    let (table, layers) = table_and_layers();
    let textures: Vec<serde_json::Value> = TEXTURES
        .iter()
        .map(|t| serde_json::json!({ "resourcePath": t }))
        .collect();
    vec![
        ("pack.bin", pack()),
        ("textures.json", serde_json::to_vec(&textures).unwrap()),
        ("atlas.png", png),
        ("atlas_map.json", map.into_bytes()),
        ("block-table.tsv", table.into_bytes()),
        ("render-layers.tsv", layers.into_bytes()),
        ("entities.json", CAPTURE.as_bytes().to_vec()),
        ("client.jar", client_jar()),
        ("blueprint.bin", blueprint(&world())),
        ("shots.json", serde_json::to_vec_pretty(&shots()).unwrap()),
    ]
}

pub fn paths(folder: &Path) -> crate::assets::Paths {
    crate::assets::Paths {
        pack: folder.join("pack.bin"),
        textures: folder.join("textures.json"),
        atlas: folder.join("atlas.png"),
        atlas_map: folder.join("atlas_map.json"),
        block_table: folder.join("block-table.tsv"),
        render_layers: folder.join("render-layers.tsv"),
        entities: folder.join("entities.json"),
        client_jar: folder.join("client.jar"),
        // The synthetic pack draws see_build's day alone.
        night: None,
        software_vulkan: None,
    }
}

pub const Y_OFFSET: i32 = 64;

pub fn write(folder: &Path) -> Result<(), String> {
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    for (name, bytes) in files() {
        std::fs::write(folder.join(name), bytes).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(())
}
