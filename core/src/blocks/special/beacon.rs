// BeaconBlock and BeaconBlockEntity: the beam is checked up the column ten
// blocks a tick, and every 80 game ticks the pyramid below it sets the
// level. The effects reach players within range, and none exist; the
// sounds reach no one.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::codec::{self, Input, R};
use crate::dir::Pos;
use crate::level::Level;
use crate::nbt::{Compound, Tag};
use crate::registry::{Registry, State};
use crate::world::{HEIGHT, MIN_Y};

pub const BEHAVIOUR: Behaviour = Behaviour {
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    ..NONE
};

// BeaconBeamOwner.Section: a color and a height.
pub type Section = (i32, i32);

// BeaconBlockEntity: the level, how far up the column the check has got
// (set when the entity gets its level) and the beam sections found and being
// found, and what saved data gave it: the effects chosen (keys), the custom
// name, the lock (as the tags their codecs write) and Paper's effect range.
// The effects reach players within range, and none exist.
#[derive(Clone, Debug)]
pub struct Beacon {
    pub levels: i32,
    pub last_check_y: i32,
    pub beam_sections: Vec<Section>,
    pub checking_beam_sections: Vec<Section>,
    pub primary: Option<String>,
    pub secondary: Option<String>,
    pub name: Option<Tag>,
    pub lock: Option<Tag>,
    pub range: f64,
}

impl Beacon {
    pub fn saved(&self) -> (&'static str, String) {
        let mut out = Compound::new();
        if let Some(effect) = &self.primary {
            out.put_string("primary_effect", effect);
        }
        if let Some(effect) = &self.secondary {
            out.put_string("secondary_effect", effect);
        }
        out.put_int("Levels", self.levels);
        if let Some(name) = &self.name {
            out.put("CustomName", name.clone());
        }
        if let Some(lock) = &self.lock {
            out.put("lock", lock.clone());
        }
        out.put("Paper.Range", Tag::double(self.range));
        out.put("components", Tag::Compound(Compound::new()));
        ("minecraft:beacon", out.to_string())
    }

    // BeaconBlockEntity.loadAdditional.
    pub fn load(&mut self, reg: &Registry, input: &Input) -> R<()> {
        // loadEffect: the effect by name (any effect is kept, not only those a
        // beacon offers).
        let effect = |key: &str| -> R<Option<String>> {
            input.read(key, |tag| {
                let effect = codec::location(tag)?;
                if !reg.mob_effects.contains(&effect) {
                    return codec::fail(format!("Unknown registry key in ResourceKey[minecraft:root / minecraft:mob_effect]: {effect}"));
                }
                Ok(effect)
            })
        };
        self.primary = effect("primary_effect")?;
        self.secondary = effect("secondary_effect")?;
        self.levels = input.int_or("Levels", 0)?;
        self.name = input.read("CustomName", |tag| crate::text::component(reg, tag))?;
        self.lock = input.read("lock", |tag| crate::components::item_predicate(reg, tag))?;
        self.range = input.double_or("Paper.Range", -1.0)?;
        Ok(())
    }

    // The entity's hidden state as McbenchEngine.state writes it.
    pub fn state_text(&self, pos: Pos) -> String {
        let list = |sections: &[Section]| {
            if sections.is_empty() {
                "-".to_string()
            } else {
                sections.iter().map(|(color, height)| format!("{color}:{height}")).collect::<Vec<_>>().join(",")
            }
        };
        format!("beacon {} {} {} {} {} {}", pos.x(), pos.y(), pos.z(), self.last_check_y, list(&self.beam_sections), list(&self.checking_beam_sections))
    }
}

// BeaconBlockEntity's constructor, then setLevel: lastCheckY is one below the
// world.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::Beacon(Beacon {
        levels: 0,
        last_check_y: MIN_Y - 1,
        beam_sections: Vec::new(),
        checking_beam_sections: Vec::new(),
        primary: None,
        secondary: None,
        name: None,
        lock: None,
        range: -1.0,
    }))
}

fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(tick)
}

fn beacon(l: &mut Level, pos: Pos) -> &mut Beacon {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Special(Special::Beacon(beacon))) => beacon,
        _ => panic!("no beacon at {pos}"),
    }
}

// ARGB.average.
fn average(a: i32, b: i32) -> i32 {
    let (a, b) = (a as u32, b as u32);
    let channel = |shift: u32| (((a >> shift & 255) + (b >> shift & 255)) / 2) << shift;
    (channel(24) | channel(16) | channel(8) | channel(0)) as i32
}

// Level.getHeight(Heightmap.Types.WORLD_SURFACE, x, z): one above the
// highest block that is not air, or the bottom of the world.
fn world_surface_height(l: &mut Level, x: i32, z: i32) -> i32 {
    if !l.world.has_chunk(x >> 4, z >> 4) {
        return MIN_Y;
    }
    for y in (MIN_Y..MIN_Y + HEIGHT).rev() {
        if !l.is_air(l.world.get(Pos::new(x, y, z))) {
            return y + 1;
        }
    }
    MIN_Y
}

// BeaconBlockEntity.tick.
fn tick(l: &mut Level, pos: Pos, _state: State) {
    let (x, y, z) = (pos.x(), pos.y(), pos.z());
    let mut block_y;
    if beacon(l, pos).last_check_y < y {
        block_y = y;
        let beacon = beacon(l, pos);
        beacon.checking_beam_sections = Vec::new();
        beacon.last_check_y = y - 1;
    } else {
        block_y = beacon(l, pos).last_check_y + 1;
    }
    let mut section: Option<usize> = if beacon(l, pos).checking_beam_sections.is_empty() { None } else { Some(beacon(l, pos).checking_beam_sections.len() - 1) };
    let height = world_surface_height(l, x, z);
    let mut i = 0;
    while i < 10 && block_y <= height {
        let state = l.get(Pos::new(x, block_y, z));
        if let Some(&color) = l.reg.beam_colors.get(&l.block(state)) {
            let beacon = beacon(l, pos);
            if beacon.checking_beam_sections.len() <= 1 {
                beacon.checking_beam_sections.push((color, 1));
                section = Some(beacon.checking_beam_sections.len() - 1);
            } else if let Some(at) = section {
                if color == beacon.checking_beam_sections[at].0 {
                    beacon.checking_beam_sections[at].1 += 1;
                } else {
                    let mixed = average(beacon.checking_beam_sections[at].0, color);
                    beacon.checking_beam_sections.push((mixed, 1));
                    section = Some(beacon.checking_beam_sections.len() - 1);
                }
            }
        } else {
            let opaque = l.reg.states[state as usize].light_block >= 15 && l.reg.blocks[l.block(state) as usize].key != "minecraft:bedrock";
            if section.is_none() || opaque {
                let beacon = beacon(l, pos);
                beacon.checking_beam_sections.clear();
                beacon.last_check_y = height;
                break;
            }
            let at = section.unwrap();
            beacon(l, pos).checking_beam_sections[at].1 += 1;
        }
        block_y += 1;
        beacon(l, pos).last_check_y += 1;
        i += 1;
    }
    let old_levels = beacon(l, pos).levels;
    if l.game_time % 80 == 0 {
        if !beacon(l, pos).beam_sections.is_empty() {
            let levels = update_base(l, x, y, z);
            beacon(l, pos).levels = levels;
        }
        // applyEffects reaches the players in range, and there are none;
        // the ambient sound reaches no one.
    }
    // (BeaconActivatedEvent and BeaconDeactivatedEvent have no listener.)
    if beacon(l, pos).last_check_y >= height {
        let beacon = beacon(l, pos);
        beacon.last_check_y = MIN_Y - 1;
        beacon.beam_sections = beacon.checking_beam_sections.clone();
        // The activation sound and the construct-beacon trigger reach no one.
        let _ = old_levels;
    }
}

// BeaconBlockEntity.updateBase: the number of full layers of base blocks
// under the beacon, up to four.
fn update_base(l: &mut Level, x: i32, y: i32, z: i32) -> i32 {
    let mut layers = 0;
    let mut i1 = 1;
    while i1 <= 4 {
        let layer_y = y - i1;
        if layer_y < MIN_Y {
            break;
        }
        let mut full = true;
        let mut bx = x - i1;
        while bx <= x + i1 && full {
            for bz in z - i1..=z + i1 {
                let state = l.get(Pos::new(bx, layer_y, bz));
                if !l.reg.in_tag(state, "minecraft:beacon_base_blocks") {
                    full = false;
                    break;
                }
            }
            bx += 1;
        }
        if !full {
            break;
        }
        layers = i1;
        i1 += 1;
    }
    layers
}
