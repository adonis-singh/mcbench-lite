// The baked pack (server/McbenchBake.java documents the format): what BlueMap
// computes once from the resource pack, read back as plain tables.

pub const CULLING: u8 = 1;
pub const OCCLUDING: u8 = 2;
pub const CULLING_IDENTICAL: u8 = 4;
pub const ALWAYS_WATERLOGGED: u8 = 8;
pub const RANDOM_OFFSET: u8 = 16;
pub const WATERLOGGED: u8 = 32;
pub const WATER: u8 = 64;
pub const AIR: u8 = 128;
pub const NONE: u32 = u32::MAX;

pub struct Property {
    pub name: String,
    pub default: u8,
    pub values: Vec<String>,
}

pub struct Block {
    pub id: String,
    pub first: u32,
    pub properties: Vec<Property>,
}

#[derive(Clone, Copy)]
pub struct State {
    pub flags: u8,
    pub level: u8,
    pub emission: u8,
    pub light_block: u8,
    // 1 skylightDown, 2 shapeOcclusion, 4 tint blends (reads the 5x3x5
    // blocks around it)
    pub light_flags: u8,
    pub full_faces: u8,
    pub block: u16,
    pub tint: u32,
    pub selection: u32,
}

impl State {
    pub fn has(&self, flag: u8) -> bool {
        self.flags & flag != 0
    }
    pub fn blends(&self) -> bool {
        self.light_flags & 4 != 0
    }
}

pub struct Face {
    // 0 down, 1 up, 2 north, 3 south, 4 west, 5 east
    pub direction: u8,
    pub uv: [f32; 8],
    pub texture: i32,
    pub tinted: bool,
    pub cullface: u8,
}

pub struct Element {
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub matrix: u32,
    pub emission: i32,
    pub faces: Vec<Face>,
}

pub struct Mesh {
    pub ambient_occlusion: bool,
    pub elements: Vec<u32>,
}

pub enum Variant {
    Empty,
    Model { mesh: u32, transformed: bool, matrix: u32, directions: u32 },
    Liquid { still: i32, flow: i32 },
}

pub struct VariantSet {
    pub total_weight: f64,
    pub variants: Vec<(f64, u32)>,
}

pub struct Pack {
    pub blocks: Vec<Block>,
    // block indices sorted by id, for lookups
    pub by_id: Vec<u32>,
    pub states: Vec<State>,
    pub air: u32,
    pub water_selection: u32,
    pub water_tint: u32,
    pub water_blends: bool,
    pub tints: Vec<[f32; 3]>,
    pub matrices: Vec<[f32; 12]>,
    pub directions: Vec<[[i8; 3]; 27]>,
    pub elements: Vec<Element>,
    pub meshes: Vec<Mesh>,
    pub variants: Vec<Variant>,
    pub sets: Vec<VariantSet>,
    pub selections: Vec<Vec<u32>>,
    pub liquid_up: Vec<[f32; 8]>,
    pub liquid_side: [f32; 8],
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.at + n > self.bytes.len() {
            return Err("The baked pack ends early".into());
        }
        let slice = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn i8(&mut self) -> Result<i8, String> {
        Ok(self.take(1)?[0] as i8)
    }
    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn f64(&mut self) -> Result<f64, String> {
        let b = self.take(8)?;
        Ok(f64::from_bits(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])))
    }
    fn str(&mut self) -> Result<String, String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "The baked pack has a malformed name".into())
    }
    fn f32s<const N: usize>(&mut self) -> Result<[f32; N], String> {
        let mut values = [0f32; N];
        for value in values.iter_mut() {
            *value = self.f32()?;
        }
        Ok(values)
    }
    fn list<T>(&mut self, mut item: impl FnMut(&mut Self) -> Result<T, String>) -> Result<Vec<T>, String> {
        let n = self.u32()? as usize;
        let mut items = Vec::with_capacity(n);
        for _ in 0..n {
            items.push(item(self)?);
        }
        Ok(items)
    }
}

impl Pack {
    pub fn read(bytes: &[u8]) -> Result<Pack, String> {
        let mut r = Reader { bytes, at: 0 };
        if r.take(4)? != b"MCBP" || r.u32()? != 1 {
            return Err("Not a version 1 baked pack".into());
        }
        let blocks = r.list(|r| {
            let id = r.str()?;
            let first = r.u32()?;
            let count = r.u8()?;
            let mut properties = Vec::new();
            for _ in 0..count {
                let name = r.str()?;
                let default = r.u8()?;
                let n = r.u8()?;
                let mut values = Vec::new();
                for _ in 0..n {
                    values.push(r.str()?);
                }
                properties.push(Property { name, default, values });
            }
            Ok(Block { id, first, properties })
        })?;
        let states = r.list(|r| {
            Ok(State {
                flags: r.u8()?,
                level: r.u8()?,
                emission: r.u8()?,
                light_block: r.u8()?,
                light_flags: r.u8()?,
                full_faces: r.u8()?,
                block: r.u16()?,
                tint: r.u32()?,
                selection: r.u32()?,
            })
        })?;
        let air = r.u32()?;
        let water_selection = r.u32()?;
        let water_tint = r.u32()?;
        let water_blends = r.u8()? & 4 != 0;
        let tints = r.list(|r| r.f32s::<3>())?;
        let matrices = r.list(|r| r.f32s::<12>())?;
        let directions = r.list(|r| {
            let mut table = [[0i8; 3]; 27];
            for entry in table.iter_mut() {
                *entry = [r.i8()?, r.i8()?, r.i8()?];
            }
            Ok(table)
        })?;
        let elements = r.list(|r| {
            let from = r.f32s::<3>()?;
            let to = r.f32s::<3>()?;
            let matrix = r.u32()?;
            let emission = r.u32()? as i32;
            let count = r.u8()?;
            let mut faces = Vec::new();
            for _ in 0..count {
                faces.push(Face {
                    direction: r.u8()?,
                    uv: r.f32s::<8>()?,
                    texture: r.u32()? as i32,
                    tinted: r.u8()? != 0,
                    cullface: r.u8()?,
                });
            }
            Ok(Element { from, to, matrix, emission, faces })
        })?;
        let meshes = r.list(|r| {
            let ambient_occlusion = r.u8()? != 0;
            let count = r.u16()?;
            let mut elements = Vec::new();
            for _ in 0..count {
                elements.push(r.u32()?);
            }
            Ok(Mesh { ambient_occlusion, elements })
        })?;
        let variants = r.list(|r| match r.u8()? {
            0 => Ok(Variant::Empty),
            1 => Ok(Variant::Model { mesh: r.u32()?, transformed: r.u8()? != 0, matrix: r.u32()?, directions: r.u32()? }),
            2 => Ok(Variant::Liquid { still: r.u32()? as i32, flow: r.u32()? as i32 }),
            other => Err(format!("Unknown baked renderer {other}")),
        })?;
        let sets = r.list(|r| {
            let total_weight = r.f64()?;
            let count = r.u16()?;
            let mut variants = Vec::new();
            for _ in 0..count {
                variants.push((r.f64()?, r.u32()?));
            }
            Ok(VariantSet { total_weight, variants })
        })?;
        let selections = r.list(|r| {
            let count = r.u16()?;
            let mut list = Vec::new();
            for _ in 0..count {
                list.push(r.u32()?);
            }
            Ok(list)
        })?;
        let mut liquid_up = Vec::new();
        for _ in -180..=180 {
            liquid_up.push(r.f32s::<8>()?);
        }
        let liquid_side = r.f32s::<8>()?;
        if r.at != bytes.len() {
            return Err("The baked pack has trailing bytes".into());
        }
        let mut by_id: Vec<u32> = (0..blocks.len() as u32).collect();
        by_id.sort_by(|&a, &b| blocks[a as usize].id.cmp(&blocks[b as usize].id));
        Ok(Pack {
            blocks,
            by_id,
            states,
            air,
            water_selection,
            water_tint,
            water_blends,
            tints,
            matrices,
            directions,
            elements,
            meshes,
            variants,
            sets,
            selections,
            liquid_up,
            liquid_side,
        })
    }

    // The state a blueprint palette entry names, every property it leaves
    // out at the block's default (McbenchBlueMapReference.BlueprintWorld.read).
    pub fn state(&self, id: &str, properties: &[(String, String)]) -> Result<u32, String> {
        let found = self.by_id.binary_search_by(|&b| self.blocks[b as usize].id.as_str().cmp(id));
        let block = &self.blocks[*self.by_id.get(found.map_err(|_| format!("Unknown block {id}"))?).unwrap() as usize];
        let mut chosen: Vec<usize> = block.properties.iter().map(|p| p.default as usize).collect();
        for (name, value) in properties {
            let unknown = || format!("Unknown block state {id}[{name}={value}]");
            let p = block.properties.iter().position(|p| &p.name == name).ok_or_else(unknown)?;
            chosen[p] = block.properties[p].values.iter().position(|v| v == value).ok_or_else(unknown)?;
        }
        let mut index = 0usize;
        for (p, property) in block.properties.iter().enumerate() {
            index = index * property.values.len() + chosen[p];
        }
        Ok(block.first + index as u32)
    }
}
