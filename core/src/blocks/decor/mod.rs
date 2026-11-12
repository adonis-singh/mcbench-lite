// Decor: the flower pot, candles, carpets, lanterns, chains, lightning rods,
// amethyst clusters, grates, cobwebs, torches and huge mushroom blocks, a
// module per Java class (the weathering variants add only random ticks and
// share their parent's behaviour).

use super::Behaviour;
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::Support;

pub mod amethyst_cluster;
pub mod candle;
pub mod carpet;
pub mod chain;
pub mod flower_pot;
pub mod huge_mushroom;
pub mod lantern;
pub mod lightning_rod;
pub mod torch;
pub mod wall_torch;
pub mod waterlogged_transparent;
pub mod web;

pub const CLASSES: &[(&str, Behaviour)] = &[
    ("FlowerPotBlock", flower_pot::BEHAVIOUR),
    ("CandleBlock", candle::BEHAVIOUR),
    ("CarpetBlock", carpet::BEHAVIOUR),
    ("WoolCarpetBlock", carpet::BEHAVIOUR),
    ("LanternBlock", lantern::BEHAVIOUR),
    ("WeatheringLanternBlock", lantern::BEHAVIOUR),
    ("ChainBlock", chain::BEHAVIOUR),
    ("WeatheringCopperChainBlock", chain::BEHAVIOUR),
    ("LightningRodBlock", lightning_rod::BEHAVIOUR),
    ("WeatheringLightningRodBlock", lightning_rod::BEHAVIOUR),
    ("AmethystClusterBlock", amethyst_cluster::BEHAVIOUR),
    ("WaterloggedTransparentBlock", waterlogged_transparent::BEHAVIOUR),
    ("WeatheringCopperGrateBlock", waterlogged_transparent::BEHAVIOUR),
    ("WebBlock", web::BEHAVIOUR),
    ("TorchBlock", torch::BEHAVIOUR),
    ("WallTorchBlock", wall_torch::BEHAVIOUR),
    ("HugeMushroomBlock", huge_mushroom::BEHAVIOUR),
];

// Block.canSupportCenter(level, pos, direction): the block at `pos` has a
// sturdy centre on its face toward `direction`, which the bottom of a block
// in the unstable-bottom-center tag never has.
pub fn can_support_center(l: &mut Level, pos: Pos, direction: Dir) -> bool {
    let state = l.get(pos);
    if direction == Dir::Down && crate::in_tag!(l, state, "minecraft:unstable_bottom_center") {
        false
    } else {
        crate::shape_ops::is_face_sturdy(l, state, pos, direction, Support::Center)
    }
}
