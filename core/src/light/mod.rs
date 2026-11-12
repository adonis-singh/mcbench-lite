// The light engine as McbenchLevel runs it: Paper's own Starlight (moonrise)
// behind a light engine that gathers each chunk's changes (changed positions
// and sections that became empty or not) and lights them before light is
// next read, before chunks are saved and after live ticks (McbenchLevel.Light).
// engine.rs is Starlight; this is the data it works on and the level's side.

mod engine;
mod nibble;

pub mod dims {
    pub use super::engine::{MAX_LIGHT, MIN_LIGHT, MIN_SECTION, SECTIONS};
}

pub use nibble::Nibble;

use crate::dir::{chunk_key, Pos};
use crate::jmap::JavaPosSet;
use crate::fast::FastMap;
use crate::registry::Registry;
use crate::world::World;
use engine::{Run, LIGHT_SECTIONS, MAX_LIGHT, MAX_SECTION, MIN_LIGHT, MIN_SECTION, SECTIONS};

// What a chunk holds of Starlight's data (StarlightChunk): both layers'
// nibbles per light section (one below the world and one above it), their
// emptiness maps, and whether the chunk's light is correct.
pub struct ChunkLight {
    sky: Vec<Nibble>,
    block: Vec<Nibble>,
    sky_empty: Option<Vec<bool>>,
    block_empty: Option<Vec<bool>>,
    pub light_correct: bool,
}

impl ChunkLight {
    // A chunk before it is lit.
    pub fn new() -> ChunkLight {
        ChunkLight { sky: filled_empty_light(), block: filled_empty_light(), sky_empty: None, block_empty: None, light_correct: false }
    }

    pub fn layer(&self, sky: bool) -> &Vec<Nibble> {
        if sky {
            &self.sky
        } else {
            &self.block
        }
    }

    pub fn layer_mut(&mut self, sky: bool) -> &mut Vec<Nibble> {
        if sky {
            &mut self.sky
        } else {
            &mut self.block
        }
    }

    pub fn empty(&self, sky: bool) -> &Option<Vec<bool>> {
        if sky {
            &self.sky_empty
        } else {
            &self.block_empty
        }
    }

    pub fn empty_mut(&mut self, sky: bool) -> &mut Option<Vec<bool>> {
        if sky {
            &mut self.sky_empty
        } else {
            &mut self.block_empty
        }
    }

    // StarLightEngine.getFilledEmptyLight, set as the chunk's nibbles.
    pub fn reset_nibbles(&mut self, sky: bool) {
        *self.layer_mut(sky) = filled_empty_light();
    }
}

fn filled_empty_light() -> Vec<Nibble> {
    (0..LIGHT_SECTIONS).map(|_| Nibble::new(true)).collect()
}

// A chunk's unlit changes (McbenchLevel.Light.Pending): its changed positions,
// in the order a java.util.HashSet of them iterates (the light they make is
// the same in any order: JavaPosSet::order_free), and its sections that
// became empty (true) or not (false).
pub struct Pending {
    pub positions: JavaPosSet,
    pub sections: Vec<Option<bool>>,
}

// The lighting of a level: each lit chunk's light, and the changes gathered
// since the last flush, per chunk in the order the chunks first changed.
pub struct Lighting {
    lights: FastMap<i64, ChunkLight>,
    pending: Vec<(i64, Pending)>,
    // Where each chunk is in `pending`.
    pending_at: FastMap<i64, usize>,
}

impl Lighting {
    pub fn new() -> Lighting {
        Lighting { lights: FastMap::default(), pending: Vec::new(), pending_at: FastMap::default() }
    }

    fn pending(&mut self, x: i32, z: i32) -> &mut Pending {
        let key = chunk_key(x, z);
        let at = match self.pending_at.get(&key) {
            Some(&at) => at,
            None => {
                self.pending.push((key, Pending { positions: JavaPosSet::order_free(), sections: vec![None; SECTIONS] }));
                self.pending_at.insert(key, self.pending.len() - 1);
                self.pending.len() - 1
            }
        };
        &mut self.pending[at].1
    }

    // Light.checkBlock: a block whose light properties changed.
    pub fn check_block(&mut self, pos: Pos) {
        self.pending(pos.x() >> 4, pos.z() >> 4).positions.insert(pos);
    }

    // Light.updateSectionStatus: a section that became empty or not.
    pub fn update_section_status(&mut self, x: i32, y: i32, z: i32, empty: bool) {
        self.pending(x, z).sections[(y - MIN_SECTION) as usize] = Some(empty);
    }

    // Light.flush: every chunk's gathered changes are lit.
    // One sky and one block engine serve every chunk, as Starlight's are
    // reused: a run leaves nothing behind (Run::assert_idle).
    pub fn flush(&mut self, reg: &Registry, world: &World) {
        self.pending_at.clear();
        let (sky, block) = (&mut Run::new(reg, world, true), &mut Run::new(reg, world, false));
        for (key, mut pending) in std::mem::take(&mut self.pending) {
            let (x, z) = (key as i32, (key >> 32) as i32);
            sky.blocks_changed_in_chunk(&mut self.lights, x, z, &mut pending);
            sky.assert_idle();
            block.blocks_changed_in_chunk(&mut self.lights, x, z, &mut pending);
            block.assert_idle();
        }
    }

    // StarLightInterface.lightChunk with getEmptySectionsForChunk, for a chunk
    // just made (McbenchLevel.chunk), which then has correct light.
    pub fn light_chunk(&mut self, reg: &Registry, world: &World, x: i32, z: i32) {
        let chunk = world.chunk(chunk_key(x, z));
        let empty: Vec<Option<bool>> = chunk.sections.iter().map(|section| Some(section.as_ref().map_or(true, |s| s.non_air == 0))).collect();
        Run::new(reg, world, true).light(&mut self.lights, x, z, &mut empty.clone());
        Run::new(reg, world, false).light(&mut self.lights, x, z, &mut empty.clone());
        self.lights.get_mut(&chunk_key(x, z)).expect("the light of a chunk just lit").light_correct = true;
    }

    // StarLightInterface.relightChunks: the columns lit again from their
    // blocks, sky light then block light.
    pub fn relight(&mut self, reg: &Registry, world: &World, columns: &[(i32, i32)]) -> usize {
        Run::new(reg, world, true).relight_chunks(&mut self.lights, columns);
        Run::new(reg, world, false).relight_chunks(&mut self.lights, columns)
    }

    // --- Reading light (StarLightInterface) ---

    // getSkyLightValue.
    pub fn sky_light_value(&self, pos: Pos) -> i32 {
        let Some(chunk) = self.lights.get(&pos.chunk()).filter(|light| light.light_correct) else { return 15 };
        let mut section_y = pos.y() >> 4;
        let mut y = pos.y();
        if section_y > MAX_LIGHT {
            return 15;
        }
        if section_y < MIN_LIGHT {
            section_y = MIN_LIGHT;
            y = MIN_LIGHT << 4;
        }
        let nibbles = &chunk.sky;
        let immediate = &nibbles[(section_y - MIN_LIGHT) as usize];
        if !immediate.is_null_visible() {
            return immediate.get_visible(index(pos.x(), y, pos.z()));
        }
        let Some(empty) = &chunk.sky_empty else { return 15 };
        let mut lowest_y = MIN_LIGHT - 1;
        for curr_y in (MIN_SECTION..=MAX_SECTION).rev() {
            if !empty[(curr_y - MIN_SECTION) as usize] {
                lowest_y = curr_y;
                break;
            }
        }
        if section_y > lowest_y {
            return 15;
        }
        for curr_y in section_y + 1..=MAX_LIGHT {
            let nibble = &nibbles[(curr_y - MIN_LIGHT) as usize];
            if !nibble.is_null_visible() {
                return nibble.get_visible(index(pos.x(), 0, pos.z()));
            }
        }
        15
    }

    // getBlockLightValue.
    #[allow(dead_code)]
    pub fn block_light_value(&self, pos: Pos) -> i32 {
        let cy = pos.y() >> 4;
        if cy < MIN_LIGHT || cy > MAX_LIGHT {
            return 0;
        }
        match self.lights.get(&pos.chunk()) {
            None => 0,
            Some(chunk) => chunk.block[(cy - MIN_LIGHT) as usize].get_visible(index(pos.x(), pos.y(), pos.z())),
        }
    }

    // getRawBrightness.
    #[allow(dead_code)]
    pub fn raw_brightness(&self, pos: Pos, ambient_darkness: i32) -> i32 {
        let sky = self.sky_light_value(pos) - ambient_darkness;
        if sky == 15 {
            return 15;
        }
        sky.max(self.block_light_value(pos))
    }

    // --- What the level holds of it, for comparing engines and saving ---

    pub fn chunk_light(&self, key: i64) -> &ChunkLight {
        &self.lights[&key]
    }

    // The gathered changes, as McbenchEngine.state writes them: per chunk in
    // the order they were first gathered, its positions sorted, and its
    // sections (n unchanged, t became empty, f became not empty).
    pub fn pending_text(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        for (key, pending) in &self.pending {
            let mut positions = pending.positions.to_vec();
            positions.sort();
            let positions: Vec<String> = positions.iter().map(|p| format!("{},{},{}", p.x(), p.y(), p.z())).collect();
            let sections: String = pending.sections.iter().map(|s| match s { None => 'n', Some(true) => 't', Some(false) => 'f' }).collect();
            writeln!(out, "pending {} {} {} {sections}", *key as i32, (*key >> 32) as i32, if positions.is_empty() { "-".to_string() } else { positions.join(";") }).unwrap();
        }
        out
    }

    // Every lit chunk's light, chunk by chunk in (x, z) order: whether it is
    // correct, each layer's emptiness map (1 for an empty section) and each
    // light section's nibble as a token: its visible and updating states, d
    // if it holds changes to make visible, then the FNV-1a hash of its
    // visible bytes after a colon and of its working bytes, where it has
    // others, after a slash.
    pub fn light_text(&self) -> String {
        use std::fmt::Write;
        let mut keys: Vec<i64> = self.lights.keys().copied().collect();
        keys.sort_by_key(|key| (*key as i32, (*key >> 32) as i32));
        let mut out = String::new();
        for key in keys {
            let light = &self.lights[&key];
            writeln!(out, "light {} {} {}", key as i32, (key >> 32) as i32, light.light_correct as u8).unwrap();
            for (name, sky) in [("sky", true), ("block", false)] {
                let empty = match light.empty(sky) {
                    None => "-".to_string(),
                    Some(map) => map.iter().map(|&e| if e { '1' } else { '0' }).collect(),
                };
                let tokens: Vec<String> = light.layer(sky).iter().map(|n| n.token()).collect();
                writeln!(out, "{name} {empty} {}", tokens.join(" ")).unwrap();
            }
        }
        out
    }
}

#[inline]
fn index(x: i32, y: i32, z: i32) -> usize {
    (x & 15 | (z & 15) << 4 | (y & 15) << 8) as usize
}

// --- The sky (Level.updateSkyBrightness and what reads the sun) ---

// The world's day time: McbenchLevel sets 6000 (noon) and turns the daylight
// cycle off, so it never changes.
pub const DAY_TIME: i64 = 6000;

// Mth.frac(double).
fn frac(number: f64) -> f64 {
    number - number.floor()
}

// DimensionType.timeOfDay of the overworld (which has no fixed time).
pub fn time_of_day(day_time: i64) -> f32 {
    assert!(day_time == DAY_TIME, "the day time is fixed at noon");
    let fraction = frac(day_time as f64 / 24000.0 - 0.25);
    let d = 0.5 - (fraction * std::f64::consts::PI).cos() / 2.0;
    (fraction * 2.0 + d) as f32 / 3.0
}

// Mth.cos(float): the sine table entry a quarter turn on.
pub fn mth_cos(value: f32) -> f32 {
    let index = ((value * 10430.378f32 + 16384.0f32) as i32 & 65535) as f64;
    (index * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32
}

// Level.updateSkyBrightness with no rain and no thunder.
pub fn sky_darken(day_time: i64) -> i32 {
    let d = 1.0 - (0.0f32 * 5.0f32) as f64 / 16.0;
    let d1 = 1.0 - (0.0f32 * 5.0f32) as f64 / 16.0;
    let cos = mth_cos(time_of_day(day_time) * (std::f64::consts::PI * 2.0) as f32) as f64;
    let d2 = 0.5 + 2.0 * cos.clamp(-0.25, 0.25);
    ((1.0 - d2 * d * d1) * 11.0) as i32
}

// Level.getSunAngle(1.0F).
pub fn sun_angle(day_time: i64) -> f32 {
    time_of_day(day_time) * (std::f64::consts::PI * 2.0) as f32
}
