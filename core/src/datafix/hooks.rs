// The schemas' Hook functions (Hook.HookFunction), which run on the saved
// data before a type reads it and after it writes it: the item stack's hooks
// that name the block entity and entity of an item's tag by its id (V99,
// V704 and V705 ADD_NAMES).

use super::ops::{Dy, Nbt};
use super::tables;
use super::types::ensure_namespaced;
use crate::level::refuse;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Hook {
    Identity,
    V99AddNames,
    V704AddNames,
    V705AddNames,
    // A hook this port does not run (it names the class).
    Unported(std::rc::Rc<str>),
}

impl Hook {
    pub fn parse(name: &str) -> Hook {
        match name {
            "IDENTITY" => Hook::Identity,
            "net.minecraft.util.datafix.schemas.V99$1" => Hook::V99AddNames,
            "net.minecraft.util.datafix.schemas.V704$1" => Hook::V704AddNames,
            "net.minecraft.util.datafix.schemas.V705$1" => Hook::V705AddNames,
            other => Hook::Unported(other.into()),
        }
    }

    pub fn apply(&self, value: Nbt) -> Nbt {
        match self {
            Hook::Identity => value,
            Hook::V99AddNames => add_names(Dy(value), "V99.ITEM_TO_BLOCKENTITY", "V99.ITEM_TO_ENTITY").0,
            Hook::V704AddNames => add_names(Dy(value), "V704.ITEM_TO_BLOCKENTITY", "V99.ITEM_TO_ENTITY").0,
            Hook::V705AddNames => add_names(Dy(value), "V704.ITEM_TO_BLOCKENTITY", "V705.ITEM_TO_ENTITY").0,
            Hook::Unported(name) => refuse(format!("the data fixer's schema hook {name}")),
        }
    }
}

// V99.addNames: the id of the block entity in an item's BlockEntityTag and
// of the entity in its EntityTag, from the item's id.
fn add_names(tag: Dy, block_entities: &str, entities: &str) -> Dy {
    let item = tag.clone();
    tag.update("tag", |d| {
        d.update("BlockEntityTag", |be| {
            let id = item.get("id").as_string().result().map(|s| ensure_namespaced(&s)).unwrap_or_else(|| "minecraft:air".into());
            if id != "minecraft:air" {
                if let Some(name) = tables::map(block_entities).get(&id) {
                    return be.set("id", item.create_string(name));
                }
            }
            be
        })
        .update("EntityTag", |e| {
            if e.get("id").is_present() {
                return e;
            }
            let id = ensure_namespaced(&item.get("id").as_string_or(""));
            match tables::map(entities).get(&id) {
                Some(name) => e.set("id", item.create_string(name)),
                None => e,
            }
        })
    })
}
