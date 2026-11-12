// StructureBlock and StructureBlockEntity. A structure block saves, loads or
// unloads the structure it is named for when it is powered. Blocks are
// placed without NBT, so no structure block is named: saveStructure,
// placeStructure and unloadStructure all find no structure name and do
// nothing, and the powered flag is all that changes.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::codec::{self, Input, R};
use crate::nbt::{Compound, Tag};
use crate::dir::Pos;
use crate::level::Level;
use crate::property;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    neighbor_changed: Some(neighbor_changed),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// StructureBlockEntity: what saved data gave it, or its defaults: the structure
// name (a resource location, none for text that is no location), the author and
// metadata, the position and size (clamped), the rotation, mirror and mode,
// the flags, the integrity and the seed.
#[derive(Clone, Debug)]
pub struct StructureBlock {
    mode: &'static str,
    powered: bool,
    name: String,
    author: String,
    metadata: String,
    pos: [i32; 3],
    size: [i32; 3],
    rotation: &'static str,
    mirror: &'static str,
    ignore_entities: bool,
    strict: bool,
    show_air: bool,
    show_bounding_box: bool,
    integrity: f32,
    seed: i64,
}

impl StructureBlock {
    pub fn saved(&self) -> (&'static str, String) {
        let mut out = Compound::new();
        out.put_string("name", &self.name);
        out.put_string("author", &self.author);
        out.put_string("metadata", &self.metadata);
        out.put_int("posX", self.pos[0]);
        out.put_int("posY", self.pos[1]);
        out.put_int("posZ", self.pos[2]);
        out.put_int("sizeX", self.size[0]);
        out.put_int("sizeY", self.size[1]);
        out.put_int("sizeZ", self.size[2]);
        out.put_string("rotation", self.rotation);
        out.put_string("mirror", self.mirror);
        out.put_string("mode", self.mode);
        out.put_boolean("ignoreEntities", self.ignore_entities);
        out.put_boolean("strict", self.strict);
        out.put_boolean("powered", self.powered);
        out.put_boolean("showair", self.show_air);
        out.put_boolean("showboundingbox", self.show_bounding_box);
        out.put("integrity", Tag::float(self.integrity));
        out.put("seed", Tag::Long(self.seed));
        out.put("components", Tag::Compound(Compound::new()));
        ("minecraft:structure_block", out.to_string())
    }

    // StructureBlockEntity.loadAdditional (the block state follows the mode: see after_load).
    pub fn load(&mut self, input: &Input) -> R<()> {
        // setStructureName: text that is no resource location is none.
        let name = input.string("name")?.unwrap_or("");
        self.name = if name.is_empty() { String::new() } else { codec::resource_location(name).unwrap_or_default() };
        self.author = input.string("author")?.unwrap_or("").to_string();
        self.metadata = input.string("metadata")?.unwrap_or("").to_string();
        let clamp = |value: i32, min: i32, max: i32| value.max(min).min(max);
        self.pos = [clamp(input.int_or("posX", 0)?, -48, 48), clamp(input.int_or("posY", 1)?, -48, 48), clamp(input.int_or("posZ", 0)?, -48, 48)];
        self.size = [clamp(input.int_or("sizeX", 0)?, 0, 48), clamp(input.int_or("sizeY", 0)?, 0, 48), clamp(input.int_or("sizeZ", 0)?, 0, 48)];
        let choice = |key: &str, names: &[&'static str], default: &'static str| -> R<&'static str> {
            match input.read(key, |tag| codec::string(tag).map(str::to_string))? {
                None => Ok(default),
                Some(name) => match names.iter().find(|n| **n == name) {
                    Some(&known) => Ok(known),
                    None => codec::fail(format!("No value with id: {name}")),
                },
            }
        };
        self.rotation = choice("rotation", &["NONE", "CLOCKWISE_90", "CLOCKWISE_180", "COUNTERCLOCKWISE_90"], "NONE")?;
        self.mirror = choice("mirror", &["NONE", "LEFT_RIGHT", "FRONT_BACK"], "NONE")?;
        self.mode = choice("mode", &["SAVE", "LOAD", "CORNER", "DATA"], "DATA")?;
        self.ignore_entities = input.bool_or("ignoreEntities", true)?;
        self.strict = input.bool_or("strict", false)?;
        self.powered = input.bool_or("powered", false)?;
        self.show_air = input.bool_or("showair", false)?;
        self.show_bounding_box = input.bool_or("showboundingbox", true)?;
        self.integrity = input.float_or("integrity", 1.0)?;
        self.seed = input.long_or("seed", 0)?;
        Ok(())
    }
}

fn mode_property(l: &Level) -> u16 {
    property!(l, "mode", ["save", "load", "corner", "data"])
}

// StructureBlockEntity's constructor: the mode from the block state.
fn new_block_entity(l: &mut Level, state: State, _pos: Pos) -> BlockEntity {
    let mode = match l.reg.value_name(state, mode_property(l)) {
        "save" => "SAVE",
        "load" => "LOAD",
        "corner" => "CORNER",
        "data" => "DATA",
        other => panic!("no structure mode {other}"),
    };
    BlockEntity::Special(Special::StructureBlock(StructureBlock {
        mode,
        powered: false,
        name: String::new(),
        author: String::new(),
        metadata: String::new(),
        pos: [0, 1, 0],
        size: [0, 0, 0],
        rotation: "NONE",
        mirror: "NONE",
        ignore_entities: true,
        strict: false,
        show_air: false,
        show_bounding_box: true,
        integrity: 1.0,
        seed: 0,
    }))
}

// StructureBlockEntity.loadAdditional's updateBlockState: the block's mode
// property follows the entity's (set with flag 2).
pub fn after_load(l: &mut Level, pos: Pos) {
    let Some(BlockEntity::Special(Special::StructureBlock(structure))) = l.block_entities.get(&pos) else { return };
    let mode = structure.mode.to_ascii_lowercase();
    let state = l.get(pos);
    if l.reg.blocks[l.block(state) as usize].key == "minecraft:structure_block" {
        l.set(pos, l.reg.with_name(state, mode_property(l), &mode), 2);
    }
}

// StructureBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, _state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    if !matches!(l.block_entity(pos), Some(BlockEntity::Special(Special::StructureBlock(_)))) {
        return;
    }
    let has_neighbor_signal = l.has_neighbor_signal(pos);
    let Some(BlockEntity::Special(Special::StructureBlock(structure))) = l.block_entities.get_mut(&pos) else { unreachable!() };
    // setPowered(true) then trigger, which acts on the structure it is named for in
    // the modes that save, load or unload one.
    if has_neighbor_signal && !structure.powered {
        structure.powered = true;
        if !structure.name.is_empty() && structure.mode != "DATA" {
            crate::level::refuse(format!("the structure block at {pos} is triggered with the structure name {}", structure.name));
        }
    } else if !has_neighbor_signal && structure.powered {
        structure.powered = false;
    }
}
