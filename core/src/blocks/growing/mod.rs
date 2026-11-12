// The growing plants: the vines (GrowingPlantBlock and its head and body
// classes), sugar cane, cactus, bamboo and the chorus plant and flower.

use super::Behaviour;

pub mod bamboo_sapling;
pub mod bamboo_stalk;
pub mod cactus;
pub mod chorus_flower;
pub mod chorus_plant;
pub mod growing_plant;
pub mod growing_plant_body;
pub mod growing_plant_head;
pub mod sugar_cane;

pub const CLASSES: &[(&str, Behaviour)] = &[
    ("WeepingVinesBlock", growing_plant_head::BEHAVIOUR),
    ("TwistingVinesBlock", growing_plant_head::BEHAVIOUR),
    ("CaveVinesBlock", growing_plant_head::BEHAVIOUR),
    ("WeepingVinesPlantBlock", growing_plant_body::BEHAVIOUR),
    ("TwistingVinesPlantBlock", growing_plant_body::BEHAVIOUR),
    ("CaveVinesPlantBlock", growing_plant_body::BEHAVIOUR),
    ("SugarCaneBlock", sugar_cane::BEHAVIOUR),
    ("CactusBlock", cactus::BEHAVIOUR),
    ("BambooSaplingBlock", bamboo_sapling::BEHAVIOUR),
    ("BambooStalkBlock", bamboo_stalk::BEHAVIOUR),
    ("ChorusPlantBlock", chorus_plant::BEHAVIOUR),
    ("ChorusFlowerBlock", chorus_flower::BEHAVIOUR),
];
