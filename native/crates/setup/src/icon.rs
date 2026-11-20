//! A block's icon (`server/block-icon.js`): the texture Minecraft breaks it
//! into particles with, cut from the bake's atlas as a PNG, multiplied by
//! the colour Minecraft's client multiplies those particles by in the
//! world the app builds in (its default state's, [`BlockTints`] particle).
//! Stairs show their planks and panes their glass; an animation shows its
//! first frame. The particle texture of each block is found at release
//! time ([`particle_texture`], over the client jar's block states and
//! models) and shipped in the client-jar manifest.

use std::collections::HashMap;
use std::sync::Mutex;

use mcbench_world::block_states::BlockStates;
use serde_json::Value;

use crate::bake::{Image, decode_png, encode_png};
use crate::tints::BlockTints;

/// The model's particle texture of block `name` (`oak_stairs`), following
/// its first variant's (or first multipart case's) model's parents and
/// `#` references, from Minecraft's block state definitions and models by
/// name (`oak_stairs`; `block/oak_stairs`), as `particleTexture` does.
pub fn particle_texture(
    name: &str,
    definitions: &dyn Fn(&str) -> Option<Value>,
    models: &dyn Fn(&str) -> Option<Value>,
) -> Result<String, String> {
    let definition = definitions(name)
        .ok_or_else(|| format!("Minecraft has no block state definition of {name}"))?;
    let first = match definition.get("variants") {
        Some(variants) => variants
            .as_object()
            .and_then(|v| v.values().next())
            .cloned(),
        None => definition["multipart"]
            .get(0)
            .map(|case| case["apply"].clone()),
    }
    .ok_or_else(|| format!("The block state definition of {name} has no model"))?;
    let first = if first.is_array() {
        first[0].clone()
    } else {
        first
    };
    let mut textures: serde_json::Map<String, Value> = serde_json::Map::new();
    // builtin/ parents have no model file.
    let mut model = first
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string);
    while let Some(entry) = model
        .as_deref()
        .and_then(|m| models(m.strip_prefix("minecraft:").unwrap_or(m)))
    {
        if let Some(own) = entry.get("textures").and_then(Value::as_object) {
            for (key, value) in own {
                if !textures.contains_key(key) {
                    textures.insert(key.clone(), value.clone());
                }
            }
        }
        model = entry
            .get("parent")
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    let text = |key: &str| {
        textures
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("The model of {name} has no texture {key}"))
    };
    let mut texture = text("particle")?;
    while let Some(reference) = texture.strip_prefix('#') {
        texture = text(reference)?;
    }
    Ok(texture
        .strip_prefix("minecraft:")
        .unwrap_or(&texture)
        .to_string())
}

/// The icon of a texture box of `atlas`: its `size`-square at `x`, `y`,
/// each colour channel multiplied by `color` (0xRRGGBB) and rounded.
pub fn icon(atlas: &Image, [x, y, size]: [u32; 3], color: u32) -> Result<Vec<u8>, String> {
    let tint = [(color >> 16) & 255, (color >> 8) & 255, color & 255];
    let mut out = vec![0u8; (size * size * 4) as usize];
    for row in 0..size {
        let from = (((y + row) * atlas.width + x) * 4) as usize;
        out[(row * size * 4) as usize..((row + 1) * size * 4) as usize]
            .copy_from_slice(&atlas.rgba[from..from + (size * 4) as usize]);
    }
    for pixel in out.as_chunks_mut::<4>().0 {
        for channel in 0..3 {
            pixel[channel] = (pixel[channel] as f64 * tint[channel] as f64 / 255.0).round() as u8;
        }
    }
    encode_png(size, size, &out)
}

/// The icons of every block, made once each.
pub struct Icons {
    atlas: Image,
    boxes: HashMap<String, [u32; 3]>,
    particles: HashMap<String, String>,
    states: BlockStates,
    tints: BlockTints,
    made: Mutex<HashMap<String, Vec<u8>>>,
}

impl Icons {
    /// From the bake's atlas (`atlas.png`, `atlas_map.json`), the shipped
    /// particle textures, the block table's states and the tints.
    pub fn new(
        atlas_png: &[u8],
        atlas_map: &[u8],
        particles: &[(String, String)],
        states: BlockStates,
        tints: BlockTints,
    ) -> Result<Icons, String> {
        let map: serde_json::Map<String, Value> =
            serde_json::from_slice(atlas_map).map_err(|e| format!("atlas_map.json: {e}"))?;
        let boxes = map
            .into_iter()
            .map(|(key, b)| {
                let n = |i: usize| {
                    b.get(i)
                        .and_then(Value::as_u64)
                        .map(|v| v as u32)
                        .ok_or_else(|| format!("atlas_map.json's box of {key} is not numbers"))
                };
                Ok((key.clone(), [n(0)?, n(1)?, n(2)?]))
            })
            .collect::<Result<_, String>>()?;
        Ok(Icons {
            atlas: decode_png(atlas_png).map_err(|e| format!("atlas.png: {e}"))?,
            boxes,
            particles: particles.iter().cloned().collect(),
            states,
            tints,
            made: Mutex::new(HashMap::new()),
        })
    }

    /// The PNG for a block name such as `glass`, or `None` for a name that
    /// is not a block of the block table.
    pub fn block_icon(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        if let Some(png) = self.made.lock().expect("the icons' lock").get(name) {
            return Ok(Some(png.clone()));
        }
        let id = format!("minecraft:{name}");
        if !self.states.defaults.contains_key(&id) {
            return Ok(None);
        }
        let texture = self
            .particles
            .get(&id)
            .ok_or_else(|| format!("The shipped data names no particle texture of {id}"))?;
        let at = *self
            .boxes
            .get(texture)
            .ok_or_else(|| format!("The atlas has no texture {texture}"))?;
        let state = self.states.id_of(&id, &[])? as usize;
        let png = icon(&self.atlas, at, self.tints.particle[state])?;
        self.made
            .lock()
            .expect("the icons' lock")
            .insert(name.to_string(), png.clone());
        Ok(Some(png))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_block_shows_the_texture_its_model_breaks_into() {
        let definitions: serde_json::Map<String, Value> = serde_json::from_str(
            r#"{"oak_stairs":{"variants":{"facing=east":{"model":"minecraft:block/oak_stairs"}}},"oak_fence":{"multipart":[{"apply":{"model":"minecraft:block/oak_fence_post"}}]}}"#,
        )
        .unwrap();
        let models: serde_json::Map<String, Value> = serde_json::from_str(
            r##"{"block/stairs":{"textures":{"particle":"#side"}},"block/oak_stairs":{"parent":"minecraft:block/stairs","textures":{"side":"minecraft:block/oak_planks"}},"block/fence_post":{"textures":{"particle":"#texture"}},"block/oak_fence_post":{"parent":"block/fence_post","textures":{"texture":"block/oak_planks"}}}"##,
        )
        .unwrap();
        let d = |n: &str| definitions.get(n).cloned();
        let m = |n: &str| models.get(n).cloned();
        assert_eq!(
            particle_texture("oak_stairs", &d, &m).unwrap(),
            "block/oak_planks"
        );
        assert_eq!(
            particle_texture("oak_fence", &d, &m).unwrap(),
            "block/oak_planks"
        );
    }

    #[test]
    fn icons_are_square_first_frames_multiplied_by_their_colour() {
        // A 2-wide strip of two 2x2 frames: the icon is the first.
        let rgba: Vec<u8> = (0..2 * 4 * 4).map(|i| (i * 29 % 256) as u8).collect();
        let atlas = Image {
            width: 2,
            height: 4,
            rgba,
        };
        let white = decode_png(&icon(&atlas, [0, 0, 2], 0xffffff).unwrap()).unwrap();
        assert_eq!(white.rgba, atlas.rgba[..16]);
        let tinted = decode_png(&icon(&atlas, [0, 0, 2], 0x3f76e4).unwrap()).unwrap();
        for (w, t) in white
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .zip(tinted.rgba.as_chunks::<4>().0)
        {
            let scale = |v: u8, c: u32| (v as f64 * c as f64 / 255.0).round() as u8;
            assert_eq!(
                *t,
                [
                    scale(w[0], 0x3f),
                    scale(w[1], 0x76),
                    scale(w[2], 0xe4),
                    w[3]
                ]
            );
        }
    }
}
