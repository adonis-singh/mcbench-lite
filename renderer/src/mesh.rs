// BlueMap's hires block pass for one tile (HiresModelManager.render,
// BlockRenderPass, BlockStateModelRenderer, ResourceModelRenderer,
// LiquidModelRenderer), doing only the work that depends on position; the
// rest comes baked (see pack.rs). Faces are written and transformed in the
// same windows and order as BlueMap's, so a tile cut short by the face limit
// holds exactly what BlueMap's does. Every block is read through
// Neighborhood, BlueMap's block cache, quirks included.

use crate::pack::{self, Pack, Variant};
use crate::tile::{Full, Tile};
use crate::world::World;
use std::cell::Cell;

const TILE_SIZE: i32 = 32;
const TILE_OFFSET: i32 = 2;
const BLOCK_SCALE: f32 = 1.0 / 16.0;
// Direction vectors in render order: down, up, north, south, west, east.
const DIRECTIONS: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];
const UP: usize = 1;
// The element corners each face is drawn from (ResourceModelRenderer.buildModelElementResource).
const CORNERS: [[usize; 4]; 6] = [[0, 2, 3, 1], [5, 7, 6, 4], [2, 0, 4, 6], [1, 3, 7, 5], [0, 1, 5, 4], [3, 2, 6, 7]];

pub fn cell(position: i32) -> i32 {
    (position - TILE_OFFSET).div_euclid(TILE_SIZE)
}

// The tiles BlueMap draws for a blueprint (as in
// scripts/McbenchBlueMapReference.java): those over each built chunk
// column and one block past it.
pub fn tiles(world: &World) -> Vec<(i32, i32)> {
    let mut tiles = Vec::new();
    for &(cx, cz) in &world.columns {
        for tx in cell((cx << 4) - 1)..=cell((cx << 4) + 16) {
            for tz in cell((cz << 4) - 1)..=cell((cz << 4) + 16) {
                tiles.push((tx, tz));
            }
        }
    }
    tiles.sort();
    tiles.dedup();
    tiles
}

// The tiles of `after` whose bytes can differ from `before`'s tile at the
// same place: every tile when the masks differ, otherwise each tile that is
// new or that reads a position whose block, light or span membership
// differs. A tile reads its own 32 columns and two blocks past them (tint
// blending's 5x3x5; culling, light, ambient occlusion and liquids read one),
// at every height, so those are compared; any other tile draws the same
// bytes from either world.
pub fn changed(before: &World, after: &World) -> Vec<(i32, i32)> {
    let drawn = tiles(after);
    if before.mask != after.mask {
        return drawn;
    }
    let touched = reading(World::differing_columns(before, after));
    let old = tiles(before);
    drawn.into_iter().filter(|tile| touched.contains(tile) || old.binary_search(tile).is_err()).collect()
}

// The columns tile t reads along x (or z), inclusive: its own and two blocks
// past them (see `changed`).
pub fn reads(t: i32) -> (i32, i32) {
    (t * TILE_SIZE + TILE_OFFSET - 2, t * TILE_SIZE + TILE_OFFSET + TILE_SIZE + 1)
}

// The tiles that read any of the columns (x, z): a tile reads its own
// columns and two blocks past them.
pub fn reading(columns: impl IntoIterator<Item = (i32, i32)>) -> std::collections::HashSet<(i32, i32)> {
    let mut touched = std::collections::HashSet::new();
    for (x, z) in columns {
        for tx in cell(x - 2)..=cell(x + 2) {
            for tz in cell(z - 2)..=cell(z + 2) {
                touched.insert((tx, tz));
            }
        }
    }
    touched
}

// A block as BlueMap's ExtendedBlock holds it: where it says it is (`at`:
// what the render mask, neighbour offsets and hashes use) and where its
// Block reads the world (`reads`). The two differ only in Neighborhood's
// quirk below.
#[derive(Clone, Copy)]
struct Spot {
    at: [i32; 3],
    reads: [i32; 3],
}

// BlueMap's BlockNeighborhood, a cache of 8x8x8 ExtendedBlocks by position
// mod 8, made afresh for each tile. A new cache entry is a copy of the
// current block that says it is at 0, 0, 0 while reading where the current
// block is, and set() skips a move to where an entry says it is. So the
// first time a tile's cache meets 0, 0, 0 through the entry for index 0, that
// entry keeps reading the block it was copied from, until it is moved
// elsewhere. Every other entry always reads where it says it is; only
// index 0 needs keeping. (The main block's own move is likewise skipped when
// it lands on its current index, which the column spans rule out.)
struct Neighborhood {
    main: Cell<Spot>,
    main_index: Cell<i32>,
    zero: Cell<Option<Spot>>,
}

fn cache_index([x, y, z]: [i32; 3]) -> i32 {
    (x & 7) * 64 + (y & 7) * 8 + (z & 7)
}

impl Neighborhood {
    fn new() -> Neighborhood {
        let origin = Spot { at: [0; 3], reads: [0; 3] };
        Neighborhood { main: Cell::new(origin), main_index: Cell::new(0), zero: Cell::new(None) }
    }

    // neighborhood[i], made if missing, then set to `position`.
    fn entry(&self, position: [i32; 3]) -> Spot {
        if cache_index(position) != 0 {
            return Spot { at: position, reads: position };
        }
        let mut entry = self.zero.get().unwrap_or(Spot { at: [0; 3], reads: self.main.get().reads });
        if entry.at != position {
            entry = Spot { at: position, reads: position };
        }
        self.zero.set(Some(entry));
        entry
    }

    // BlockNeighborhood.set
    fn set(&self, position: [i32; 3]) {
        let index = cache_index(position);
        if index == self.main_index.get() {
            return;
        }
        self.main.set(self.entry(position));
        self.main_index.set(index);
    }

    // BlockNeighborhood.getNeighborBlock
    fn neighbor(&self, [dx, dy, dz]: [i32; 3]) -> Spot {
        let [x, y, z] = self.main.get().at;
        let position = [x + dx, y + dy, z + dz];
        if cache_index(position) == self.main_index.get() {
            return self.main.get();
        }
        self.entry(position)
    }
}

// What the cache says of the block at an offset from the main block: its
// state and (sky, block) light, as ExtendedBlock.getBlockState and
// getLightData give them.
#[derive(Clone, Copy)]
struct Look {
    state: u32,
    sky: i32,
    block: i32,
}

const UNSEEN: Look = Look { state: pack::NONE, sky: 0, block: 0 };

struct Draw<'a> {
    pack: &'a Pack,
    world: &'a World,
    cache: Neighborhood,
    // What `look` found around the main block, for each offset in -1..=1.
    looks: [Cell<Look>; 27],
}

// Draws tile tx, tz into `tile`, whose memory it reuses.
pub fn tile(pack: &Pack, world: &World, tx: i32, tz: i32, tile: &mut Tile) {
    tile.clear();
    let draw = Draw { pack, world, cache: Neighborhood::new(), looks: [const { Cell::new(UNSEEN) }; 27] };
    let _ = draw.blocks(tile, tx * TILE_SIZE + TILE_OFFSET, tz * TILE_SIZE + TILE_OFFSET);
    tile.sort();
}

// The liquid LiquidModelRenderer draws: its blockState field. Water is
// told by its id, any other liquid by being the same block.
struct Liquid {
    water: bool,
    level: i32,
    block: u16,
    tint: u32,
    blends: bool,
}

impl Draw<'_> {
    fn blocks(&self, tile: &mut Tile, min_x: i32, min_z: i32) -> Result<(), Full> {
        for x in min_x..min_x + TILE_SIZE {
            for z in min_z..min_z + TILE_SIZE {
                if !self.world.mask.contains(x, z) {
                    continue;
                }
                let (max_y, min_y) = self.world.span(x, z);
                let mut y = max_y;
                while y >= min_y {
                    self.cache.set([x, y, z]);
                    let [ax, _, az] = self.cache.main.get().at;
                    if self.world.mask.contains(ax, az) {
                        let start = tile.size;
                        self.block(tile)?;
                        tile.translate(start, (x - min_x) as f32, y as f32, (z - min_z) as f32);
                    }
                    y -= 1;
                }
            }
        }
        Ok(())
    }

    // ExtendedBlock.getBlockState: air outside the render mask.
    fn state(&self, spot: Spot) -> u32 {
        if self.world.mask.contains(spot.at[0], spot.at[2]) {
            self.world.block(spot.reads)
        } else {
            self.pack.air
        }
    }

    // ExtendedBlock.getLightData: full sky outside the render mask.
    fn light(&self, spot: Spot) -> (i32, i32) {
        let (sky, block) = self.world.light(spot.reads);
        if self.world.mask.contains(spot.at[0], spot.at[2]) {
            (sky, block)
        } else {
            (15, block)
        }
    }

    // Both of the above at once.
    #[inline(always)]
    fn see(&self, spot: Spot) -> Look {
        let (state, sky, block) = self.world.look(spot.reads);
        if self.world.mask.contains(spot.at[0], spot.at[2]) {
            Look { state, sky, block }
        } else {
            Look { state: self.pack.air, sky: 15, block }
        }
    }

    // The block at `offset` (each of x, y, z in -1..=1) from the main block.
    // The cache answers the same offset with the same block for as long as
    // the main block stays, and no two positions around it share a cache
    // index 0, so asking it once changes it as much as asking again.
    #[inline(always)]
    fn look(&self, offset: [i32; 3]) -> Look {
        let slot = &self.looks[((offset[0] + 1) * 9 + (offset[1] + 1) * 3 + offset[2] + 1) as usize];
        let look = slot.get();
        if look.state != pack::NONE {
            return look;
        }
        let look = self.see(self.cache.neighbor(offset));
        slot.set(look);
        look
    }

    fn props(&self, state: u32) -> &pack::State {
        &self.pack.states[state as usize]
    }

    // BlendedBlockColorCalculator's reads: the 5x3x5 blocks around.
    fn blend(&self) {
        for dy in -1..=1 {
            for dx in -2..=2 {
                for dz in -2..=2 {
                    self.cache.neighbor([dx, dy, dz]);
                }
            }
        }
    }

    // BlockStateModelRenderer.render
    fn block(&self, tile: &mut Tile) -> Result<(), Full> {
        let main = self.cache.main.get();
        let state = self.state(main);
        let state_props = self.props(state);
        if state_props.has(pack::AIR) {
            return Ok(());
        }
        for look in &self.looks {
            look.set(UNSEEN);
        }
        let (sky, block) = self.light(main);
        self.looks[13].set(Look { state, sky, block });
        self.model(tile, state_props.selection)?;
        if state_props.has(pack::WATERLOGGED) || state_props.has(pack::ALWAYS_WATERLOGGED) {
            self.model(tile, self.pack.water_selection)?;
        }
        Ok(())
    }

    // BlockStateModelRenderer.renderModel: one variant from each set the
    // state applies, chosen by position.
    fn model(&self, tile: &mut Tile, selection: u32) -> Result<(), Full> {
        if selection == pack::NONE {
            return Ok(());
        }
        let [x, y, z] = self.cache.main.get().at;
        let hash = variant_hash(x, y, z) as f64;
        for &set in &self.pack.selections[selection as usize] {
            let set = &self.pack.sets[set as usize];
            let mut left = hash * set.total_weight;
            let Some(&(_, variant)) = set.variants.iter().find(|&&(weight, _)| {
                left -= weight;
                left <= 0.0
            }) else {
                continue;
            };
            match self.pack.variants[variant as usize] {
                Variant::Empty => {}
                Variant::Model { mesh, transformed, matrix, directions } => {
                    self.resource(tile, mesh, transformed.then_some(matrix), directions)?
                }
                Variant::Liquid { still, flow } => self.liquid(tile, still, flow)?,
            }
        }
        Ok(())
    }

    // ResourceModelRenderer.render
    fn resource(&self, tile: &mut Tile, mesh: u32, transform: Option<u32>, directions: u32) -> Result<(), Full> {
        let main = self.cache.main.get();
        let own = self.look([0, 0, 0]);
        let own_props = self.props(own.state);
        let mesh = &self.pack.meshes[mesh as usize];
        let table = &self.pack.directions[directions as usize];
        let start = tile.size;
        let mut tinted = false;
        for &element in &mesh.elements {
            let element = &self.pack.elements[element as usize];
            let element_start = tile.size;
            let (lo, hi) = (element.from, element.to);
            let corners = [
                [lo[0], lo[1], lo[2]],
                [lo[0], lo[1], hi[2]],
                [hi[0], lo[1], lo[2]],
                [hi[0], lo[1], hi[2]],
                [lo[0], hi[1], lo[2]],
                [lo[0], hi[1], hi[2]],
                [hi[0], hi[1], lo[2]],
                [hi[0], hi[1], hi[2]],
            ];
            for face in &element.faces {
                let direction = face.direction as usize;
                let faced = self.look(relative(table, DIRECTIONS[direction]));
                let sun = own.sky.max(faced.sky);
                let block_light = own.block.max(faced.block);
                if face.cullface != 255 {
                    let other = self.look(relative(table, DIRECTIONS[face.cullface as usize])).state;
                    let props = self.props(other);
                    if props.has(pack::CULLING) || (props.has(pack::CULLING_IDENTICAL) && other == own.state) {
                        continue;
                    }
                }
                tile.room(2)?;
                let c = CORNERS[direction].map(|corner| corners[corner]);
                let color = if face.tinted {
                    // The tint is taken once per variant, at its first tinted face.
                    if !tinted && own_props.blends() {
                        self.blend();
                    }
                    tinted = true;
                    self.pack.tints[own_props.tint as usize]
                } else {
                    [1.0, 1.0, 1.0]
                };
                let ao = if mesh.ambient_occlusion {
                    c.map(|corner| self.ao(table, corner, DIRECTIONS[direction]))
                } else {
                    [1.0; 4]
                };
                tile.quad(c, face.uv, color, ao, sun as u8, block_light.max(element.emission) as u8, face.texture);
            }
            tile.transform(element_start, &self.pack.matrices[element.matrix as usize]);
        }
        if let Some(matrix) = transform {
            tile.transform(start, &self.pack.matrices[matrix as usize]);
        }
        if own_props.has(pack::RANDOM_OFFSET) {
            let [x, _, z] = main.at;
            let dx = (offset_hash(x, z, 123984) - 0.5) * 0.75;
            let dz = (offset_hash(x, z, 345542) - 0.5) * 0.75;
            tile.translate(start, dx, 0.0, dz);
        }
        Ok(())
    }

    // ResourceModelRenderer.testAo: the corner (in element space) is
    // darkened by each occluding block around it on the face's side.
    fn ao(&self, table: &[[i8; 3]; 27], vertex: [f32; 3], direction: [i32; 3]) -> f32 {
        let side = |value: f32| if value == 16.0 { 1 } else if value == 0.0 { -1 } else { 0 };
        let (x, y, z) = (side(vertex[0]), side(vertex[1]), side(vertex[2]));
        let occluding = |offset: [i32; 3]| self.props(self.look(relative(table, offset)).state).has(pack::OCCLUDING);
        let mut count = 0;
        if x * direction[0] + y * direction[1] > 0 && occluding([x, y, 0]) {
            count += 1;
        }
        if x * direction[0] + z * direction[2] > 0 && occluding([x, 0, z]) {
            count += 1;
        }
        if y * direction[1] + z * direction[2] > 0 && occluding([0, y, z]) {
            count += 1;
        }
        if x * direction[0] + y * direction[1] + z * direction[2] > 0 && occluding([x, y, z]) {
            count += 1;
        }
        if count > 3 {
            count = 3;
        }
        java_max(0.0, java_min(1.0 - count as f32 * 0.25, 1.0))
    }

    // LiquidModelRenderer.render and build
    fn liquid(&self, tile: &mut Tile, still: i32, flow: i32) -> Result<(), Full> {
        let own = self.look([0, 0, 0]);
        let real = self.props(own.state);
        // A waterlogged block draws as BlockState.WATER: water at level 0.
        let liquid = if real.has(pack::WATERLOGGED) || real.has(pack::ALWAYS_WATERLOGGED) {
            Liquid { water: true, level: 0, block: real.block, tint: self.pack.water_tint, blends: self.pack.water_blends }
        } else {
            Liquid { water: real.has(pack::WATER), level: real.level as i32, block: real.block, tint: real.tint, blends: real.blends() }
        };
        let mut corners: [[f32; 3]; 8] = [
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 16.0],
            [16.0, 0.0, 0.0],
            [16.0, 0.0, 16.0],
            [0.0, 16.0, 0.0],
            [0.0, 16.0, 16.0],
            [16.0, 16.0, 0.0],
            [16.0, 16.0, 16.0],
        ];
        if liquid.level < 8 && !(liquid.level == 0 && self.same_liquid(&liquid, [0, 1, 0])) {
            corners[4][1] = self.corner_height(&liquid, -1, -1);
            corners[5][1] = self.corner_height(&liquid, -1, 0);
            corners[6][1] = self.corner_height(&liquid, 0, -1);
            corners[7][1] = self.corner_height(&liquid, 0, 0);
        }
        if liquid.blends {
            self.blend();
        }
        let tint = self.pack.tints[liquid.tint as usize];
        let start = tile.size;
        for (direction, &step) in DIRECTIONS.iter().enumerate() {
            let neighbor = self.look(step);
            if self.same_liquid(&liquid, step) || (direction != UP && self.props(neighbor.state).has(pack::CULLING)) {
                continue;
            }
            tile.room(2)?;
            let c = CORNERS[direction].map(|corner| corners[corner]);
            let mut uvs: [f32; 8] = [0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0];
            let mut flowing = false;
            if direction == UP {
                let angle = self.flowing_angle(&liquid);
                if angle != -1 {
                    flowing = true;
                    uvs = self.pack.liquid_up[(angle + 180) as usize];
                }
            } else if direction != 0 {
                flowing = true;
                uvs = self.pack.liquid_side;
            }
            let (sky, glow) = if direction == UP { (own.sky, own.block) } else { (neighbor.sky, neighbor.block) };
            tile.quad(c, uvs, tint, [1.0; 4], sky as u8, glow as u8, if flowing { flow } else { still });
        }
        tile.scale(start, BLOCK_SCALE);
        Ok(())
    }

    fn same_liquid(&self, liquid: &Liquid, offset: [i32; 3]) -> bool {
        let other = self.props(self.look(offset).state);
        if liquid.water {
            return other.has(pack::WATER) || other.has(pack::WATERLOGGED) || other.has(pack::ALWAYS_WATERLOGGED);
        }
        other.block == liquid.block
    }

    fn corner_height(&self, liquid: &Liquid, x: i32, z: i32) -> f32 {
        for ix in x..=x + 1 {
            for iz in z..=z + 1 {
                if self.same_liquid(liquid, [ix, 1, iz]) {
                    return 16.0;
                }
            }
        }
        let mut sum_height = 0f32;
        let mut count = 0i32;
        for ix in x..=x + 1 {
            for iz in z..=z + 1 {
                let other = self.props(self.look([ix, 0, iz]).state);
                if self.same_liquid(liquid, [ix, 0, iz]) {
                    if other.level == 0 {
                        return 14.0;
                    }
                    sum_height += base_height(other.level as i32);
                    count += 1;
                } else if other.has(pack::AIR) {
                    count += 1;
                }
            }
        }
        if sum_height == 0.0 || count == 0 {
            return 3.0;
        }
        sum_height / count as f32
    }

    // LiquidModelRenderer.getFlowingAngle
    fn flowing_angle(&self, liquid: &Liquid) -> i32 {
        let own = base_height(liquid.level) * BLOCK_SCALE;
        if own as f64 > 0.8 {
            return -1;
        }
        let compare = |dx: i32, dz: i32| {
            let other = self.props(self.look([dx, 0, dz]).state);
            if other.has(pack::AIR) || !self.same_liquid(liquid, [dx, 0, dz]) {
                return 0.0;
            }
            base_height(other.level as i32) * BLOCK_SCALE - own
        };
        flow_angle(own, [compare(-1, 0), compare(1, 0), compare(0, -1), compare(0, 1)])
    }
}

// The angle LiquidModelRenderer.getFlowingAngle gives an own height (in
// blocks) and the height differences toward -x, +x, -z and +z.
pub fn flow_angle(own: f32, compared: [f32; 4]) -> i32 {
    if own as f64 > 0.8 {
        return -1;
    }
    let mut x = 0f32;
    let mut y = 0f32;
    x += compared[0];
    x -= compared[1];
    y -= compared[2];
    y += compared[3];
    if x == 0.0 && y == 0.0 {
        return -1;
    }
    // VectorM2f.angleTo(0, -1)
    let length = ((x * x + y * y) as f64).sqrt() as f32;
    let dot = x * 0.0 + y * -1.0;
    let other = ((0f32 * 0f32 + -1f32 * -1f32) as f64).sqrt();
    let angle = crate::trig::acos(dot as f64 / (length as f64 * other)) as f32;
    let angle = (angle as f64 * crate::trig::RAD_TO_DEG) as i32;
    if x < 0.0 {
        angle
    } else {
        -angle
    }
}

pub fn base_height(level: i32) -> f32 {
    if level >= 8 {
        16.0
    } else {
        14.0 - level as f32 * 1.9
    }
}

// Where ResourceModelRenderer.getRotationRelativeBlock looks for an offset.
fn relative(table: &[[i8; 3]; 27], offset: [i32; 3]) -> [i32; 3] {
    let entry = table[((offset[0] + 1) * 9 + (offset[1] + 1) * 3 + offset[2] + 1) as usize];
    [entry[0] as i32, entry[1] as i32, entry[2] as i32]
}

// VariantSet.hashToFloat
pub fn variant_hash(x: i32, y: i32, z: i32) -> f32 {
    let hash = (x as i64).wrapping_mul(73438747) ^ (y as i64).wrapping_mul(9357269) ^ (z as i64).wrapping_mul(4335792);
    (hash.wrapping_mul(hash.wrapping_add(456149)) & 0x00ffffff) as f32 / 0x01000000 as f32
}

// ResourceModelRenderer.hashToFloat
pub fn offset_hash(x: i32, z: i32, seed: i64) -> f32 {
    let hash = (x as i64).wrapping_mul(73428767) ^ (z as i64).wrapping_mul(4382893) ^ seed.wrapping_mul(457);
    (hash.wrapping_mul(hash.wrapping_add(456149)) & 0x00ffffff) as f32 / 0x01000000 as f32
}

// Java's Math.max/min on floats (NaN wins, -0 is below +0).
fn java_max(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { b } else { a };
    }
    if a >= b {
        a
    } else {
        b
    }
}

fn java_min(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        return f32::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_negative() { a } else { b };
    }
    if a <= b {
        a
    } else {
        b
    }
}
