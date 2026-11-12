// Plants: the flowers, crops, saplings, mushrooms and the other classes of
// VegetationBlock, a module per Java class (the classes that add nothing to
// VegetationBlock share its behaviour).

use super::vegetation::PLAIN;
use super::Behaviour;

pub mod attached_stem;
pub mod azalea;
pub mod cactus_flower;
pub mod cocoa;
pub mod crop;
pub mod double_plant;
pub mod dry_vegetation;
pub mod fungus;
pub mod mushroom;
pub mod nether_wart;
pub mod pitcher_crop;
pub mod roots;
pub mod sweet_berry_bush;
pub mod stem;
pub mod waterlily;
pub mod wither_rose;

pub const CLASSES: &[(&str, Behaviour)] = &[
    ("BushBlock", PLAIN),
    ("FlowerBlock", PLAIN),
    ("TallGrassBlock", PLAIN),
    ("SaplingBlock", PLAIN),
    ("FlowerBedBlock", PLAIN),
    ("EyeblossomBlock", PLAIN),
    ("SweetBerryBushBlock", sweet_berry_bush::BEHAVIOUR),
    ("RootsBlock", roots::BEHAVIOUR),
    ("AzaleaBlock", azalea::BEHAVIOUR),
    ("FungusBlock", fungus::BEHAVIOUR),
    ("MushroomBlock", mushroom::BEHAVIOUR),
    ("DryVegetationBlock", dry_vegetation::BEHAVIOUR),
    ("ShortDryGrassBlock", dry_vegetation::BEHAVIOUR),
    ("TallDryGrassBlock", dry_vegetation::BEHAVIOUR),
    ("WitherRoseBlock", wither_rose::BEHAVIOUR),
    ("DoublePlantBlock", double_plant::BEHAVIOUR),
    ("TallFlowerBlock", double_plant::BEHAVIOUR),
    ("StemBlock", stem::BEHAVIOUR),
    ("AttachedStemBlock", attached_stem::BEHAVIOUR),
    ("NetherWartBlock", nether_wart::BEHAVIOUR),
    ("CocoaBlock", cocoa::BEHAVIOUR),
    ("WaterlilyBlock", waterlily::BEHAVIOUR),
    ("CactusFlowerBlock", cactus_flower::BEHAVIOUR),
    ("CropBlock", crop::BEHAVIOUR),
    ("CarrotBlock", crop::BEHAVIOUR),
    ("PotatoBlock", crop::BEHAVIOUR),
    ("BeetrootBlock", crop::BEHAVIOUR),
    ("TorchflowerCropBlock", crop::BEHAVIOUR),
    ("PitcherCropBlock", pitcher_crop::BEHAVIOUR),
];
