//! `test/helpers/fake-facts.js`'s `lastPass`: the block ids of
//! `FakeFacts.supported` are of the import's last pass.

#![allow(dead_code)]

use mcbench_world::Palette;
use mcbench_world::facts::LastPass;

pub const SUPPORTED: [&str; 7] = [
    "minecraft:torch",
    "minecraft:wall_torch",
    "minecraft:redstone_wire",
    "minecraft:sand",
    "minecraft:water",
    "minecraft:lever",
    "minecraft:rail",
];

pub struct FakeFacts;

impl LastPass for FakeFacts {
    fn last_pass(&self, palette: &Palette) -> Result<Vec<u8>, String> {
        Ok((0..palette.len())
            .map(|i| {
                u8::from(
                    palette
                        .get(i)
                        .is_some_and(|e| SUPPORTED.contains(&e.id.as_str())),
                )
            })
            .collect())
    }
}
