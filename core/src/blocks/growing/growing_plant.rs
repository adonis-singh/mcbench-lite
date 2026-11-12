// GrowingPlantBlock: what the head and body classes share. canSurvive and
// tick are its own; the class's fields (growthDirection, getHeadBlock,
// getBodyBlock) and the cave vines' berries come from `plant`.

use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::{BlockId, State, Support};
use std::sync::OnceLock;

// The fields of a head or body class: GrowingPlantBlock.growthDirection,
// getHeadBlock(), getBodyBlock(), and whether it is a CaveVines (the head and
// body copy BERRIES to each other when one becomes the other).
pub struct Plant {
    pub growth: Dir,
    pub head: BlockId,
    pub body: BlockId,
    pub berries: bool,
}

pub fn plant(l: &Level, state: State) -> &'static Plant {
    static PLANTS: OnceLock<Vec<Option<Plant>>> = OnceLock::new();
    let plants = PLANTS.get_or_init(|| {
        let mut plants: Vec<Option<Plant>> = l.reg.blocks.iter().map(|_| None).collect();
        for (head, body, growth, berries) in [
            ("minecraft:weeping_vines", "minecraft:weeping_vines_plant", Dir::Down, false),
            ("minecraft:twisting_vines", "minecraft:twisting_vines_plant", Dir::Up, false),
            ("minecraft:cave_vines", "minecraft:cave_vines_plant", Dir::Down, true),
        ] {
            let (head, body) = (l.reg.block_key(head), l.reg.block_key(body));
            plants[head as usize] = Some(Plant { growth, head, body, berries });
            plants[body as usize] = Some(Plant { growth, head, body, berries });
        }
        plants
    });
    plants[l.block(state) as usize].as_ref().unwrap_or_else(|| panic!("{} is no growing plant", l.reg.describe(state)))
}

// CaveVines.BERRIES.
pub fn berries(l: &Level) -> u16 {
    property!(l, "berries", ["true", "false"])
}

// GrowingPlantHeadBlock.AGE (AGE_25).
pub fn age(l: &Level) -> u16 {
    property!(
        l,
        "age",
        ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23", "24", "25"]
    )
}

// GrowingPlantBlock.canSurvive: canAttachTo is true for every class here;
// the block behind (opposite the growth) is a part of the plant or has a
// sturdy face toward it.
pub fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let plant = plant(l, state);
    let behind_pos = pos.relative(plant.growth.opposite());
    let behind = l.get(behind_pos);
    let block = l.block(behind);
    block == plant.head || block == plant.body || crate::shape_ops::is_face_sturdy(l, behind, behind_pos, plant.growth, Support::Full)
}

// GrowingPlantBlock.tick: a plant that cannot survive breaks and drops.
pub fn tick(l: &mut Level, state: State, pos: Pos) {
    if !crate::blocks::can_survive(l, state, pos) {
        l.destroy_block(pos, true, 512);
    }
}
