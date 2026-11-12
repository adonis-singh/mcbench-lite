// JigsawBlock and JigsawBlockEntity: a block entity holding the block's data
// (a new one holds the defaults); generating from it needs a player's
// command.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::codec::{self, Input, R};
use crate::dir::Pos;
use crate::level::Level;
use crate::nbt::{Compound, Tag};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { new_block_entity: Some(new_block_entity), ..NONE };

// JigsawBlockEntity: the name, target and pool (resource locations), the
// block it turns into (final_state, as the text it was given), the joint and the
// two priorities, and whether the block faces horizontally (what a joint the
// saved data lacks is chosen by).
#[derive(Clone, Debug)]
pub struct Jigsaw {
    name: String,
    target: String,
    pool: String,
    final_state: String,
    joint: &'static str,
    placement_priority: i32,
    selection_priority: i32,
    front_horizontal: bool,
}

impl Jigsaw {
    pub fn saved(&self) -> (&'static str, String) {
        let mut out = Compound::new();
        out.put_string("name", &self.name);
        out.put_string("target", &self.target);
        out.put_string("pool", &self.pool);
        out.put_string("final_state", &self.final_state);
        out.put_string("joint", self.joint);
        out.put_int("placement_priority", self.placement_priority);
        out.put_int("selection_priority", self.selection_priority);
        out.put("components", Tag::Compound(Compound::new()));
        ("minecraft:jigsaw", out.to_string())
    }

    // JigsawBlockEntity.loadAdditional.
    pub fn load(&mut self, input: &Input) -> R<()> {
        let location = |key: &str| -> R<String> { Ok(input.read(key, codec::location)?.unwrap_or_else(|| "minecraft:empty".to_string())) };
        self.name = location("name")?;
        self.target = location("target")?;
        self.pool = location("pool")?;
        self.final_state = input.string("final_state")?.unwrap_or("minecraft:air").to_string();
        self.joint = match input.read("joint", |tag| codec::string(tag).map(str::to_string))? {
            Some(name) => match name.as_str() {
                "rollable" => "rollable",
                "aligned" => "aligned",
                other => return codec::fail(format!("Unknown element name:{other}")),
            },
            None => {
                if self.front_horizontal {
                    "aligned"
                } else {
                    "rollable"
                }
            }
        };
        self.placement_priority = input.int_or("placement_priority", 0)?;
        self.selection_priority = input.int_or("selection_priority", 0)?;
        Ok(())
    }
}

// JigsawBlockEntity's constructor (a rollable joint); the block's orientation
// decides the joint of saved data that gives none.
fn new_block_entity(l: &mut Level, state: State, _pos: Pos) -> BlockEntity {
    let orientation = l.reg.property_of(state, "orientation");
    let front = l.reg.value_name(state, orientation).split('_').next().unwrap_or("").to_string();
    BlockEntity::Special(Special::Jigsaw(Jigsaw {
        name: "minecraft:empty".into(),
        target: "minecraft:empty".into(),
        pool: "minecraft:empty".into(),
        final_state: "minecraft:air".into(),
        joint: "rollable",
        placement_priority: 0,
        selection_priority: 0,
        front_horizontal: !matches!(front.as_str(), "up" | "down"),
    }))
}
