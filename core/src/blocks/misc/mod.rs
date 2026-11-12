// Plants and decorations that are not VegetationBlock's plain classes:
// leaves, the mangrove blocks, the dripleaves, sea pickles, the hanging
// blocks, firefly bushes, frogspawn and leaf litter. A module per Java class
// (the leaves classes share LeavesBlock's).

use super::vegetation::PLAIN;
use super::Behaviour;

pub mod big_dripleaf;
pub mod big_dripleaf_stem;
pub mod frogspawn;
pub mod hanging_moss;
pub mod hanging_roots;
pub mod leaf_litter;
pub mod leaves;
pub mod mangrove_propagule;
pub mod mangrove_roots;
pub mod sea_pickle;
pub mod small_dripleaf;
pub mod spore_blossom;

pub const CLASSES: &[(&str, Behaviour)] = &[
    ("TintedParticleLeavesBlock", leaves::BEHAVIOUR),
    ("UntintedParticleLeavesBlock", leaves::BEHAVIOUR),
    ("MangroveLeavesBlock", leaves::BEHAVIOUR),
    ("MangrovePropaguleBlock", mangrove_propagule::BEHAVIOUR),
    ("MangroveRootsBlock", mangrove_roots::BEHAVIOUR),
    ("BigDripleafBlock", big_dripleaf::BEHAVIOUR),
    ("BigDripleafStemBlock", big_dripleaf_stem::BEHAVIOUR),
    ("SmallDripleafBlock", small_dripleaf::BEHAVIOUR),
    ("SeaPickleBlock", sea_pickle::BEHAVIOUR),
    ("HangingRootsBlock", hanging_roots::BEHAVIOUR),
    ("SporeBlossomBlock", spore_blossom::BEHAVIOUR),
    ("HangingMossBlock", hanging_moss::BEHAVIOUR),
    // FireflyBushBlock adds only animateTick and bonemeal to VegetationBlock.
    ("FireflyBushBlock", PLAIN),
    ("FrogspawnBlock", frogspawn::BEHAVIOUR),
    ("LeafLitterBlock", leaf_litter::BEHAVIOUR),
];
