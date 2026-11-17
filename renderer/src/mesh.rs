// BlueMap's hires block pass for one tile (HiresModelManager.render,
// BlockRenderPass, BlockStateModelRenderer, ResourceModelRenderer,
// LiquidModelRenderer), doing only the work that depends on position; the
// rest comes baked (see pack.rs). Faces are written and transformed in the
// same windows and order as BlueMap's, so a tile cut short by the face limit
// holds exactly what BlueMap's does. Every block is read through
// Neighborhood, BlueMap's block cache, quirks included. Where BlueMap asks
// its own block flags, the mesher asks what Minecraft says (the block table,
// through the pack): a face is culled as Block.shouldRenderFace and
// LiquidBlockRenderer cull it, the blocks that darken a corner are those
// Minecraft shades with, and a block that emits light is not shaded, as
// Minecraft's ModelBlockRenderer leaves it
// (scripts/McbenchBlueMapReference.java runs BlueMap's renderer with the
// same answers).

use crate::pack::{self, Pack, Variant};
use crate::tile::{self, Full, Tile};
use crate::world::World;
use std::cell::Cell;

const TILE_SIZE: i32 = 32;
const TILE_OFFSET: i32 = 2;
// Direction vectors in render order: down, up, north, south, west, east.
const DIRECTIONS: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];
// By neighbour offset ((dx+1)*9 + (dy+1)*3 + dz+1), the direction toward it,
// for the six offsets of a face.
const DIRECTION_OF: [usize; 27] = {
    let mut table = [usize::MAX; 27];
    let mut direction = 0;
    while direction < 6 {
        let [x, y, z] = DIRECTIONS[direction];
        table[((x + 1) * 9 + (y + 1) * 3 + z + 1) as usize] = direction;
        direction += 1;
    }
    table
};
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

// A block: its position, where it reads the world (Minecraft reads every
// block where it is: BlueMap's BlockNeighborhood made a fresh cache entry
// that read the block it was copied from for position 0, 0, 0, which the
// mesher copied until it was held to Minecraft's renderer).
#[derive(Clone, Copy)]
struct Spot {
    at: [i32; 3],
}

// BlueMap's BlockNeighborhood, a cache of 8x8x8 blocks by position mod 8,
// made afresh for each tile, whose main block's move is skipped when it
// lands on its current index (which the column spans rule out).
struct Neighborhood {
    main: Cell<Spot>,
    main_index: Cell<i32>,
}

fn cache_index([x, y, z]: [i32; 3]) -> i32 {
    (x & 7) * 64 + (y & 7) * 8 + (z & 7)
}

impl Neighborhood {
    fn new() -> Neighborhood {
        Neighborhood { main: Cell::new(Spot { at: [0; 3] }), main_index: Cell::new(0) }
    }

    // BlockNeighborhood.set
    fn set(&self, position: [i32; 3]) {
        let index = cache_index(position);
        if index == self.main_index.get() {
            return;
        }
        self.main.set(Spot { at: position });
        self.main_index.set(index);
    }

    // BlockNeighborhood.getNeighborBlock
    fn neighbor(&self, [dx, dy, dz]: [i32; 3]) -> Spot {
        let [x, y, z] = self.main.get().at;
        let position = [x + dx, y + dy, z + dz];
        if cache_index(position) == self.main_index.get() {
            return self.main.get();
        }
        Spot { at: position }
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
    // For a block drawn alone (lone), the directions (bits over down, up,
    // north, south, west, east) toward which a face with a cullface is
    // hidden, in place of the neighbours there.
    culled: Option<u8>,
    // Which variants model() draws: its models (MODELS), what block entity
    // renderers draw (ENTITIES), or both.
    drawn: Cell<u8>,
}

const MODELS: u8 = 1;
const ENTITIES: u8 = 2;

// Draws tile tx, tz into `tile`, whose memory it reuses.
pub fn tile(pack: &Pack, world: &World, tx: i32, tz: i32, tile: &mut Tile) {
    tile.clear();
    let draw = Draw { pack, world, cache: Neighborhood::new(), looks: [const { Cell::new(UNSEEN) }; 27], culled: None, drawn: Cell::new(MODELS | ENTITIES) };
    let _ = draw.blocks(tile, tx * TILE_SIZE + TILE_OFFSET, tz * TILE_SIZE + TILE_OFFSET);
    tile.sort();
}

// The faces the tile mesher draws of `state` (not the water it may hold)
// standing alone at 0, 0, 0 in air in open sky, unshaded, a face with a
// cullface hidden toward the directions of `culled` (bits over down, up,
// north, south, west, east): its models' faces into `models` and what
// Minecraft's block entity renderer draws of it into `entities`. The live
// viewer's own geometry of a block, which see_build draws the blocks
// Minecraft draws through a block entity renderer with, culled as it culls
// them (web/viewer.js).
pub fn lone(pack: &Pack, state: u32, culled: u8, models: &mut Tile, entities: &mut Tile) {
    let world = World::open_air(pack);
    let draw = Draw { pack, world: &world, cache: Neighborhood::new(), looks: [const { Cell::new(UNSEEN) }; 27], culled: Some(culled), drawn: Cell::new(MODELS) };
    draw.looks[13].set(Look { state, sky: 15, block: 0 });
    let selection = pack.states[state as usize].selection;
    models.clear();
    models.drawing = tile::Source { state, at: [0; 3], kind: tile::MODEL };
    let _ = draw.model(models, selection);
    draw.drawn.set(ENTITIES);
    entities.clear();
    entities.drawing = models.drawing;
    let _ = draw.model(entities, selection);
}

// The step to the other half of a double block `state` is (Variant::Entity's
// partner), 0, 0, 0 for none.
pub fn partner(pack: &Pack, state: u32) -> [i32; 3] {
    let selection = pack.states[state as usize].selection;
    if selection == pack::NONE {
        return [0; 3];
    }
    for &set in &pack.selections[selection as usize].sets {
        for &(_, variant) in &pack.sets[set as usize].variants {
            if let Variant::Entity { partner, .. } = pack.variants[variant as usize] {
                return partner;
            }
        }
    }
    [0; 3]
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
                        tile.drawing.at = [x, y, z];
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
            self.world.block(spot.at)
        } else {
            self.pack.air
        }
    }

    // ExtendedBlock.getLightData: full sky outside the render mask.
    fn light(&self, spot: Spot) -> (i32, i32) {
        let (sky, block) = self.world.light(spot.at);
        if self.world.mask.contains(spot.at[0], spot.at[2]) {
            (sky, block)
        } else {
            (15, block)
        }
    }

    // Both of the above at once.
    #[inline(always)]
    fn see(&self, spot: Spot) -> Look {
        let (state, sky, block) = self.world.look(spot.at);
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
        tile.drawing.state = state;
        // SectionCompiler.compile: the block's fluid first (any state whose
        // FluidState is not empty), then its model.
        if self.drawn.get() & MODELS != 0 && self.pack.render(state).fluid != 0 {
            tile.drawing.kind = tile::FLUID;
            self.fluid(tile)?;
        }
        self.model(tile, state_props.selection)?;
        Ok(())
    }

    // One variant from each set the state applies, chosen as Minecraft's
    // section renderer chooses it (SectionCompiler.compile): a RandomSource
    // seeded with BlockState.getSeed of the block's position, from which a
    // set of more than one variant takes nextInt of its total weight
    // (WeightedVariants, WeightedList.getRandomOrThrow); a multipart
    // blockstate first takes nextLong and seeds every set's choice with it
    // (MultiPartModel.collectParts).
    fn model(&self, tile: &mut Tile, selection: u32) -> Result<(), Full> {
        if selection == pack::NONE {
            return Ok(());
        }
        let [x, y, z] = self.cache.main.get().at;
        let random_class = self.pack.render(self.look([0, 0, 0]).state);
        let [sx, sy, sz] = random_class.seed_step.map(i32::from);
        let mut random = LegacyRandom::new(block_seed(x + sx, y + sy, z + sz));
        let selection = &self.pack.selections[selection as usize];
        let shared = if selection.multipart { random.next_long() } else { 0 };
        for &set in &selection.sets {
            let set = &self.pack.sets[set as usize];
            if selection.multipart {
                random.set_seed(shared);
            }
            let variant = if set.variants.len() == 1 {
                set.variants[0].1
            } else {
                let mut left = random.next_int(set.total_weight as i32);
                let Some(&(_, variant)) = set.variants.iter().find(|&&(weight, _)| {
                    left -= weight as i32;
                    left < 0
                }) else {
                    unreachable!("a variant set's weights add up to its total weight")
                };
                variant
            };
            let drawn = self.drawn.get();
            match self.pack.variants[variant as usize] {
                Variant::Empty => {}
                Variant::Model { mesh, transformed, matrix, directions } if drawn & MODELS != 0 => {
                    tile.drawing.kind = tile::MODEL;
                    self.resource(tile, mesh, transformed.then_some(matrix), directions)?
                }
                // A liquid block's model is nothing in Minecraft (its render
                // shape is invisible): its fluid is drawn by `fluid`.
                Variant::Liquid { .. } => {}
                Variant::Entity { mesh, partner } if drawn & ENTITIES != 0 => {
                    tile.drawing.kind = tile::ENTITY;
                    self.entity(tile, mesh, partner)?
                }
                _ => {}
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
                    let [x, y, z] = relative(table, DIRECTIONS[face.cullface as usize]);
                    let toward = DIRECTION_OF[((x + 1) * 9 + (y + 1) * 3 + z + 1) as usize];
                    let hidden = match self.culled {
                        Some(bits) => bits >> toward & 1 == 1,
                        None => self.pack.hides(own.state, self.look([x, y, z]).state, toward),
                    };
                    if hidden {
                        continue;
                    }
                }
                tile.room(2)?;
                let c = CORNERS[direction].map(|corner| corners[corner]);
                let color = if face.tinted {
                    // The tint is taken once per variant, at its first tinted face.
                    if !tinted && own_props.blends {
                        self.blend();
                    }
                    tinted = true;
                    self.pack.tints[own_props.tint as usize]
                } else {
                    [1.0, 1.0, 1.0]
                };
                let ao = if mesh.ambient_occlusion && own_props.emission == 0 {
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
        let random = self.pack.render(own.state);
        if random.offset != 0 {
            let [x, _, z] = main.at;
            let [dx, dy, dz] = offset(*random, x, z);
            tile.translate(start, dx, dy, dz);
        }
        Ok(())
    }

    // What Minecraft's block entity renderer draws of the block: its quads,
    // tinted as captured, lit by the light of the block's own cell, the same
    // at every corner, and never culled or shaded, as Minecraft draws a block
    // entity (the light at its position, BlockEntityRenderState's
    // lightCoords); a double block whose other half (`partner`) is there, of
    // the same block, in the brighter sky and block light of the two
    // (BrightnessCombiner).
    fn entity(&self, tile: &mut Tile, mesh: u32, partner: [i32; 3]) -> Result<(), Full> {
        let own = self.look([0, 0, 0]);
        let (mut sky, mut block) = (own.sky, own.block);
        if partner != [0, 0, 0] {
            let other = self.look(partner);
            if self.props(other.state).block == self.props(own.state).block {
                sky = sky.max(other.sky);
                block = block.max(other.block);
            }
        }
        for &at in &self.pack.entity_meshes[mesh as usize] {
            let quad = &self.pack.entity_quads[at as usize];
            let (corners, uv) = self.pack.entity_shapes[quad.shape as usize];
            tile.room(2)?;
            tile.quad(corners, uv, self.pack.tints[quad.tint as usize], [1.0; 4], sky as u8, block as u8, quad.texture);
        }
        Ok(())
    }

    // ResourceModelRenderer.testAo: the corner (in element space) is
    // darkened by each block around it on the face's side that shades.
    fn ao(&self, table: &[[i8; 3]; 27], vertex: [f32; 3], direction: [i32; 3]) -> f32 {
        let side = |value: f32| if value == 16.0 { 1 } else if value == 0.0 { -1 } else { 0 };
        let (x, y, z) = (side(vertex[0]), side(vertex[1]), side(vertex[2]));
        let occluding = |offset: [i32; 3]| self.props(self.look(relative(table, offset)).state).has(pack::SHADES);
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

    // The block at an offset from the main block, any distance off.
    fn at(&self, offset: [i32; 3]) -> Look {
        if offset.iter().all(|v| (-1..=1).contains(v)) {
            self.look(offset)
        } else {
            self.see(self.cache.neighbor(offset))
        }
    }

    fn render_at(&self, offset: [i32; 3]) -> &pack::Render {
        self.pack.render(self.at(offset).state)
    }

    // LiquidBlockRenderer.tesselate: the main block's fluid. Its heights at
    // the four corners (getHeight, calculateAverageHeight), its top face
    // (and that face seen from below where a neighbour above lets one see
    // it, FluidState.shouldRenderBackwardUpFace), its bottom and its sides
    // (each seen from both sides but beside a block drawn with the water
    // overlay), each hidden where the fluid's own block or its neighbour
    // hides it or the neighbour holds the same fluid; the top's texture
    // flowing along FlowingFluid.getFlow; the light of each face the
    // brighter of its cell's and the one above's (the bottom's of the cell
    // below), LiquidBlockRenderer.getLightColor. The textures' coordinates
    // are over each texture: the client's shrink toward a face's middle by
    // its atlas's size (uvShrinkRatio) is the atlas's, not drawn here.
    fn fluid(&self, tile: &mut Tile) -> Result<(), Full> {
        let own = self.look([0, 0, 0]);
        let render = self.pack.render(own.state);
        let group = render.fluid;
        let fluid = self.pack.fluid_groups[group as usize - 1];
        let tint = if fluid.lava {
            [1.0; 3]
        } else {
            if self.pack.water_blends {
                self.blend();
            }
            self.pack.tints[self.pack.water_tint as usize]
        };
        let same = |offset: [i32; 3]| self.render_at(offset).fluid == group;
        let own_height = |r: &pack::Render| r.amount as f32 / 9.0;
        let height = |[x, y, z]: [i32; 3]| {
            let r = self.render_at([x, y, z]);
            if r.fluid == group {
                if same([x, y + 1, z]) {
                    1.0
                } else {
                    own_height(r)
                }
            } else if r.fluid_flags & pack::SOLID == 0 {
                0.0
            } else {
                -1.0
            }
        };
        // shouldRenderFace: the fluid's own block hides it not, nor holds the neighbour the same fluid.
        let shown = |d: usize| render.self_hidden >> d & 1 == 0 && !same(DIRECTIONS[d]);
        let top = !same([0, 1, 0]);
        let bottom = shown(0) && self.render_at([0, -1, 0]).fluid_flags & pack::HIDES_BOTTOM_ABOVE == 0;
        let sides = [shown(2), shown(3), shown(4), shown(5)];
        if !(top || bottom || sides.iter().any(|&side| side)) {
            return Ok(());
        }
        let here = height([0, 0, 0]);
        let (mut ne, mut nw, mut se, mut sw) = if here >= 1.0 {
            (1.0, 1.0, 1.0, 1.0)
        } else {
            let (n, s, e, w) = (height([0, 0, -1]), height([0, 0, 1]), height([1, 0, 0]), height([-1, 0, 0]));
            let average = |a: f32, b: f32, diagonal: [i32; 3]| {
                if a >= 1.0 || b >= 1.0 {
                    return 1.0;
                }
                let mut sum = [0f32; 2];
                let mut add = |h: f32| {
                    if h >= 0.8 {
                        sum[0] += h * 10.0;
                        sum[1] += 10.0;
                    } else if h >= 0.0 {
                        sum[0] += h;
                        sum[1] += 1.0;
                    }
                };
                if b > 0.0 || a > 0.0 {
                    let d = height(diagonal);
                    if d >= 1.0 {
                        return 1.0;
                    }
                    add(d);
                }
                add(here);
                add(b);
                add(a);
                sum[0] / sum[1]
            };
            (average(n, e, [1, 0, -1]), average(n, w, [-1, 0, -1]), average(s, e, [1, 0, 1]), average(s, w, [-1, 0, 1]))
        };
        let bottom_y = if bottom { 0.001 } else { 0.0 };
        let lit = |offset: [i32; 3]| {
            let (sky, block) = self.fluid_light(offset);
            (sky as u8, block as u8)
        };
        let lowest = nw.min(sw).min(se.min(ne));
        if top && !(lowest == 1.0 && self.render_at([0, 1, 0]).fluid_flags & pack::HIDES_TOP_BELOW != 0) {
            nw -= 0.001;
            sw -= 0.001;
            se -= 0.001;
            ne -= 0.001;
            let flow = self.flow(render, group);
            let (uv, texture) = if flow[0] == 0.0 && flow[2] == 0.0 {
                ([0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0], fluid.still)
            } else {
                let angle = crate::trig::mth_atan2(flow[2], flow[0]) as f32 - 1.5707964;
                let (s, c) = (crate::trig::mth_sin(angle) * 0.25, crate::trig::mth_cos(angle) * 0.25);
                ([0.5 + (-c - s), 0.5 + (-c + s), 0.5 + (-c + s), 0.5 + (c + s), 0.5 + (c + s), 0.5 + (c - s), 0.5 + (c - s), 0.5 + (-c - s)], fluid.flow)
            };
            let (sky, block) = lit([0, 0, 0]);
            let corners = [[0.0, nw, 0.0], [0.0, sw, 1.0], [1.0, se, 1.0], [1.0, ne, 0.0]];
            tile.room(2)?;
            tile.quad(corners, uv, tint, [1.0; 4], sky, block, texture);
            // shouldRenderBackwardUpFace: a cell beside or above the one
            // above holds another fluid and does not hide what is behind it.
            let backward = (-1..=1).any(|dx| (-1..=1).any(|dz| {
                let r = self.render_at([dx, 1, dz]);
                r.fluid != group && r.fluid_flags & pack::SOLID_RENDER == 0
            }));
            if backward {
                tile.room(2)?;
                tile.quad([corners[0], corners[3], corners[2], corners[1]], [uv[0], uv[1], uv[6], uv[7], uv[4], uv[5], uv[2], uv[3]], tint, [1.0; 4], sky, block, texture);
            }
        }
        if bottom {
            let (sky, block) = lit([0, -1, 0]);
            tile.room(2)?;
            tile.quad([[0.0, bottom_y, 1.0], [0.0, bottom_y, 0.0], [1.0, bottom_y, 0.0], [1.0, bottom_y, 1.0]], [0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0], tint, [1.0; 4], sky, block, fluid.still);
        }
        let (sky, block) = lit([0, 0, 0]);
        // North, south, west, east: the side's two top heights and its two ends.
        let faces = [
            (nw, ne, [0.0, 0.001], [1.0, 0.001]),
            (se, sw, [1.0, 0.999], [0.0, 0.999]),
            (sw, nw, [0.001, 1.0], [0.001, 0.0]),
            (ne, se, [0.999, 0.0], [0.999, 1.0]),
        ];
        for (side, &(h1, h2, [x1, z1], [x2, z2])) in faces.iter().enumerate() {
            let neighbour = self.render_at(DIRECTIONS[side + 2]);
            if !sides[side] || neighbour.hides_side[side] >= h1.max(h2) {
                continue;
            }
            let overlay = !fluid.lava && neighbour.fluid_flags & pack::WATER_OVERLAY != 0;
            let texture = if overlay { self.pack.water_overlay } else { fluid.flow };
            let (v1, v2) = ((1.0 - h1) * 0.5, (1.0 - h2) * 0.5);
            let corners = [[x1, h1, z1], [x2, h2, z2], [x2, bottom_y, z2], [x1, bottom_y, z1]];
            let uv = [0.0, v1, 0.5, v2, 0.5, 0.5, 0.0, 0.5];
            tile.room(2)?;
            tile.quad(corners, uv, tint, [1.0; 4], sky, block, texture);
            if !overlay {
                tile.room(2)?;
                tile.quad([corners[3], corners[2], corners[1], corners[0]], [uv[6], uv[7], uv[4], uv[5], uv[2], uv[3], uv[0], uv[1]], tint, [1.0; 4], sky, block, texture);
            }
        }
        Ok(())
    }

    // LiquidBlockRenderer.getLightColor: the brighter sky and block light of
    // the cell and the one above (LevelRenderer.getLightColor of each: an
    // emissive state's cell at full light, a state's emission its least
    // block light), in whole levels.
    fn fluid_light(&self, [x, y, z]: [i32; 3]) -> (i32, i32) {
        let cell = |offset: [i32; 3]| {
            let look = self.at(offset);
            if self.pack.render(look.state).fluid_flags & pack::EMISSIVE != 0 {
                (15, 15)
            } else {
                (look.sky, look.block.max(self.props(look.state).emission as i32))
            }
        };
        let (here, above) = (cell([x, y, z]), cell([x, y + 1, z]));
        (here.0.max(above.0), here.1.max(above.1))
    }

    // FlowingFluid.getFlow: the fluid's flow from the heights of the fluid
    // beside it (and below a neighbour that blocks no motion), turned down
    // where a falling fluid meets a solid face, normalized (Vec3.normalize).
    fn flow(&self, render: &pack::Render, group: u8) -> [f64; 3] {
        let own = render.amount as f32 / 9.0;
        let affects = |r: &pack::Render| r.fluid == 0 || r.fluid == group;
        let (mut x, mut z) = (0f64, 0f64);
        // Direction.Plane.HORIZONTAL: north, east, south, west.
        for d in [2, 5, 3, 4] {
            let step = DIRECTIONS[d];
            let neighbour = self.render_at(step);
            if !affects(neighbour) {
                continue;
            }
            let mut height = neighbour.amount as f32 / 9.0;
            let mut difference = 0f32;
            if height == 0.0 {
                if neighbour.fluid_flags & pack::BLOCKS_MOTION == 0 {
                    let below = self.render_at([step[0], -1, step[2]]);
                    if affects(below) {
                        height = below.amount as f32 / 9.0;
                        if height > 0.0 {
                            difference = own - (height - 0.8888889);
                        }
                    }
                }
            } else if height > 0.0 {
                difference = own - height;
            }
            if difference != 0.0 {
                x += (step[0] as f32 * difference) as f64;
                z += (step[2] as f32 * difference) as f64;
            }
        }
        let normalize = |v: [f64; 3]| {
            let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            if length < 1.0e-5f32 as f64 {
                [0.0; 3]
            } else {
                [v[0] / length, v[1] / length, v[2] / length]
            }
        };
        let mut flow = [x, 0.0, z];
        if render.falling {
            // FlowingFluid.isSolidFace of the neighbour and the one above it.
            let solid = |offset: [i32; 3], d: usize| {
                let r = self.render_at(offset);
                r.fluid != group && r.fluid_flags & pack::ICE == 0 && r.sturdy >> d & 1 == 1
            };
            for d in [2, 5, 3, 4] {
                let step = DIRECTIONS[d];
                if solid(step, d) || solid([step[0], 1, step[2]], d) {
                    let n = normalize(flow);
                    flow = [n[0], n[1] - 6.0, n[2]];
                    break;
                }
            }
        }
        normalize(flow)
    }
}

// Where ResourceModelRenderer.getRotationRelativeBlock looks for an offset.
fn relative(table: &[[i8; 3]; 27], offset: [i32; 3]) -> [i32; 3] {
    let entry = table[((offset[0] + 1) * 9 + (offset[1] + 1) * 3 + offset[2] + 1) as usize];
    [entry[0] as i32, entry[1] as i32, entry[2] as i32]
}

// Mth.getSeed(x, y, z) (x * 3129871 overflowing as an int, as Java's does).
pub fn block_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut seed = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781) ^ y as i64;
    seed = seed.wrapping_mul(seed).wrapping_mul(42317861).wrapping_add(seed.wrapping_mul(11));
    seed >> 16
}

// LegacyRandomSource, the RandomSource the section renderer chooses
// variants with: Java's linear congruential generator.
pub struct LegacyRandom(i64);

impl LegacyRandom {
    const MULTIPLIER: i64 = 0x5DEECE66D;
    const MASK: i64 = (1 << 48) - 1;

    pub fn new(seed: i64) -> LegacyRandom {
        LegacyRandom((seed ^ Self::MULTIPLIER) & Self::MASK)
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.0 = (seed ^ Self::MULTIPLIER) & Self::MASK;
    }

    fn next(&mut self, bits: u32) -> i32 {
        self.0 = self.0.wrapping_mul(Self::MULTIPLIER).wrapping_add(11) & Self::MASK;
        (self.0 >> (48 - bits)) as i32
    }

    // BitRandomSource.nextInt(bound)
    pub fn next_int(&mut self, bound: i32) -> i32 {
        assert!(bound > 0, "Bound must be positive");
        if bound & (bound - 1) == 0 {
            return ((bound as i64 * self.next(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next(31);
            let value = bits % bound;
            if bits.wrapping_sub(value).wrapping_add(bound - 1) >= 0 {
                return value;
            }
        }
    }

    // BitRandomSource.nextLong
    pub fn next_long(&mut self) -> i64 {
        let high = self.next(32) as i64;
        let low = self.next(32) as i64;
        (high << 32).wrapping_add(low)
    }
}

// BlockBehaviour.Properties.offsetType's offset functions (XZ, XYZ), of
// Mth.getSeed(x, 0, z), as the pose translates by them: in floats.
pub fn offset(random: pack::Render, x: i32, z: i32) -> [f32; 3] {
    let seed = block_seed(x, 0, z);
    let h = random.max_horizontal as f64;
    let horizontal = |bits: i64| ((((bits & 15) as f32 / 15.0) as f64 - 0.5) * 0.5).clamp(-h, h);
    let dy = if random.offset == 2 { (((seed >> 4 & 15) as f32 / 15.0) as f64 - 1.0) * random.max_vertical as f64 } else { 0.0 };
    [horizontal(seed) as f32, dy as f32, horizontal(seed >> 8) as f32]
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

#[cfg(test)]
mod tests {
    use super::*;

    // java.util.Random, which LegacyRandomSource is call for call: new
    // Random(42).nextInt() is -1170105035 and nextInt(10) 0, then 3; a
    // nextLong of seed 0 is -4962768465676381896.
    #[test]
    fn legacy_random_is_javas_generator() {
        let mut random = LegacyRandom::new(42);
        assert_eq!(random.next(32), -1170105035);
        let mut random = LegacyRandom::new(42);
        assert_eq!([random.next_int(10), random.next_int(10)], [0, 3]);
        assert_eq!(LegacyRandom::new(0).next_long(), -4962768465676381896);
    }
}
