// The terrain blocks: grass, podzol, mycelium, snow layers, farmland, dirt
// paths, hay bales and the egg and ghast blocks, a module per Java class.
// MudBlock overrides only shapes (nothing the engine reaches), and
// SpreadingSnowyDirtBlock (grass, mycelium) adds only random ticks, which
// never run.

use super::Behaviour;

pub mod dirt_path;
pub mod dried_ghast;
pub mod farmland;
pub mod hay;
pub mod snow_layer;
pub mod sniffer_egg;
pub mod snowy_dirt;
pub mod turtle_egg;

pub const CLASSES: &[(&str, Behaviour)] = &[
    ("SnowyDirtBlock", snowy_dirt::BEHAVIOUR),
    ("GrassBlock", snowy_dirt::BEHAVIOUR),
    ("MyceliumBlock", snowy_dirt::BEHAVIOUR),
    ("SnowLayerBlock", snow_layer::BEHAVIOUR),
    ("FarmBlock", farmland::BEHAVIOUR),
    ("DirtPathBlock", dirt_path::BEHAVIOUR),
    ("HayBlock", hay::BEHAVIOUR),
    ("TurtleEggBlock", turtle_egg::BEHAVIOUR),
    ("SnifferEggBlock", sniffer_egg::BEHAVIOUR),
    ("DriedGhastBlock", dried_ghast::BEHAVIOUR),
];
