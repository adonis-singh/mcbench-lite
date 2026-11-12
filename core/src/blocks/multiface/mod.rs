// The face-attached blocks: MultifaceBlock (glow lichen, sculk vein), vines
// and the pale moss carpet, a module per Java class. GlowLichenBlock and
// SculkVeinBlock add nothing the level reaches to MultifaceBlock (the
// spreader, bonemeal and the sculk charge are driven by a player or a sculk
// catalyst, which this level has none of), so they share its behaviour.

use super::Behaviour;

pub mod mossy_carpet;
pub mod multiface;
pub mod vine;

pub const CLASSES: &[(&str, Behaviour)] = &[
    ("VineBlock", vine::BEHAVIOUR),
    ("GlowLichenBlock", multiface::BEHAVIOUR),
    ("SculkVeinBlock", multiface::BEHAVIOUR),
    ("MossyCarpetBlock", mossy_carpet::BEHAVIOUR),
];
