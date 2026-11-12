// The skulls and heads, beds, shelves, copper chests and copper golem
// statues, with their block entities (the shelf's is a Container, in
// blocks/entities.rs).

use super::Behaviour;
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub mod bed;
pub mod copper_chest;
pub mod copper_golem_statue;
pub mod shelf;
pub mod skull;

pub const CLASSES: &[(&str, Behaviour)] = &[
    ("SkullBlock", skull::BEHAVIOUR),
    ("WallSkullBlock", skull::BEHAVIOUR),
    ("PlayerHeadBlock", skull::BEHAVIOUR),
    ("PlayerWallHeadBlock", skull::BEHAVIOUR),
    ("WitherSkullBlock", skull::BEHAVIOUR),
    ("WitherWallSkullBlock", skull::BEHAVIOUR),
    ("PiglinWallSkullBlock", skull::BEHAVIOUR),
    ("BedBlock", bed::BEHAVIOUR),
    ("ShelfBlock", shelf::BEHAVIOUR),
    ("CopperChestBlock", copper_chest::BEHAVIOUR),
    ("WeatheringCopperChestBlock", copper_chest::BEHAVIOUR),
    ("CopperGolemStatueBlock", copper_golem_statue::BEHAVIOUR),
    ("WeatheringCopperGolemStatueBlock", copper_golem_statue::BEHAVIOUR),
];

// The block entities of this group but the shelf's. Blocks are placed
// without NBT, so each holds its defaults: no skull note block sound or
// custom name, nothing of a statue's golem. Saved data can give a skull a
// note block sound and a name; nothing else here changes after it is made.
#[derive(Clone, Debug, PartialEq)]
pub enum Plain {
    // SkullBlockEntity: its animation only ticks on the client. Its note block
    // sound (no sound is played here) and custom name, as saved data gave them;
    // its profile, which Minecraft resolves on its own, is not modeled.
    Skull { note_block_sound: Option<String>, custom_name: Option<crate::nbt::Tag> },
    // BedBlockEntity: its color is the block's.
    Bed,
    // CopperGolemStatueBlockEntity.
    CopperGolemStatue,
}

const SKULL_CLASSES: [&str; 7] = ["SkullBlock", "WallSkullBlock", "PlayerHeadBlock", "PlayerWallHeadBlock", "WitherSkullBlock", "WitherWallSkullBlock", "PiglinWallSkullBlock"];

impl Plain {
    // The classes of the blocks the entity's type is valid for
    // (BlockEntityType.isValid).
    fn block_classes(&self) -> &'static [&'static str] {
        match self {
            Plain::Skull { .. } => &SKULL_CLASSES,
            Plain::Bed => &["BedBlock"],
            Plain::CopperGolemStatue => &["CopperGolemStatueBlock", "WeatheringCopperGolemStatueBlock"],
        }
    }
}

// BlockEntityType.isValid.
pub fn valid(l: &Level, entity: &Plain, state: State) -> bool {
    entity.block_classes().contains(&l.reg.blocks[l.block(state) as usize].class.as_str())
}

// The entity's type key and saved data (see BlockEntity::saved). A skull
// stores its note block sound and custom name only when set.
pub fn saved(entity: &Plain) -> (&'static str, String) {
    match entity {
        Plain::Skull { note_block_sound, custom_name } => {
            let mut out = crate::nbt::Compound::new();
            if let Some(name) = custom_name {
                out.put("custom_name", name.clone());
            }
            if let Some(sound) = note_block_sound {
                out.put("note_block_sound", crate::nbt::Tag::string(sound));
            }
            out.put("components", crate::nbt::Tag::Compound(crate::nbt::Compound::new()));
            ("minecraft:skull", out.to_string())
        }
        Plain::Bed => ("minecraft:bed", "{components:{}}".into()),
        Plain::CopperGolemStatue => ("minecraft:copper_golem_statue", "{components:{}}".into()),
    }
}

// BlockEntity.preRemoveSideEffects: none of these is a Container.
pub fn pre_remove_side_effects(_l: &mut Level, _entity: &Plain) {}

// BaseEntityBlock.triggerEvent: Block's gives false, then the block entity
// (Level.getBlockEntity makes it if missing) answers, which for these is
// BlockEntity's false.
pub fn trigger_event(l: &mut Level, _state: State, pos: Pos, _id: i32, _param: i32) -> bool {
    l.block_entity(pos);
    false
}
