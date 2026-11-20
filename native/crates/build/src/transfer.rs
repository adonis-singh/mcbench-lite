//! The world across the child boundary: `mcbench_world::transfer` with
//! `build-sandbox.js`'s arguments (`encodeWorld` of the clone, then
//! `applyChanges(clone, changes, paletteStart, createBlockNormalizer
//! (registry))`).

use async_trait::async_trait;
use mcbench_world::transfer;

use crate::normalize::Normalizer;
use crate::{BuildHost, EncodedHost, World};

/// `encodeWorld(world)`.
pub(crate) fn encode_world(world: &World) -> Vec<u8> {
    transfer::encode_world(world)
}

/// `applyChanges` onto `clone`, whose palette had `palette_start` entries
/// when it was encoded, each palette entry normalised against `registry`
/// with the label `palette entry`. The error is the reason of a malformed
/// result; on an error the clone is as it was.
pub(crate) fn apply_changes(
    clone: &mut World,
    changes: &[u8],
    palette_start: usize,
    registry: &serde_json::Value,
) -> Result<(), String> {
    let normalizer = Normalizer::new(registry);
    let mut normalize = |id: &serde_json::Value, states: &serde_json::Value| {
        let mut spec = serde_json::Map::new();
        spec.insert("id".into(), id.clone());
        spec.insert("states".into(), states.clone());
        normalizer.normalize(&serde_json::Value::Object(spec), "palette entry")
    };
    transfer::apply_changes(
        clone,
        changes,
        palette_start,
        &mut normalize,
        mcbench_world::dimension::y_offset(),
    )
}

/// A [`BuildHost`] as the child's encoded host.
pub(crate) struct Host<'a>(pub(crate) &'a mut dyn BuildHost);

#[async_trait]
impl EncodedHost for Host<'_> {
    async fn read_file(&mut self, path: &str) -> Result<String, String> {
        self.0.read_file(path).await
    }

    async fn settled_world(&mut self) -> Result<Vec<u8>, String> {
        self.0
            .settled_world()
            .await
            .map(|world| encode_world(&world))
    }
}

#[cfg(test)]
mod tests {
    //! build-sandbox.test.js's two `applyChanges` forgery tests, with the
    //! parent's normalizer of the test's registry.
    use super::*;
    use mcbench_world::{BlockState, key_of};
    use serde_json::json;

    fn registry() -> serde_json::Value {
        json!({
            "oak_planks": [{}, {}],
            "stone": [{}, {}],
            "oak_stairs": [{ "facing": ["north", "south", "east", "west"], "half": ["top", "bottom"] }, { "facing": "north", "half": "bottom" }],
            "torch": [{}, {}],
            "wall_torch": [{ "facing": ["north", "south", "east", "west"] }, { "facing": "north" }],
        })
    }

    fn stone() -> BlockState {
        BlockState::new("minecraft:stone", &[])
    }

    fn header(entries: serde_json::Value) -> Vec<u8> {
        let text = entries.to_string();
        let mut out = (text.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(text.as_bytes());
        out
    }

    fn u32le(n: u32) -> Vec<u8> {
        n.to_le_bytes().to_vec()
    }

    fn i32le(n: u32) -> Vec<u8> {
        (n as i32).to_le_bytes().to_vec()
    }

    fn sparse(key: u32, pairs: &[(u16, u16)]) -> Vec<u8> {
        let mut out = i32le(key);
        out.push(0);
        out.extend_from_slice(&(pairs.len() as u16).to_le_bytes());
        for (index, value) in pairs {
            out.extend_from_slice(&index.to_le_bytes());
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }

    fn order(keys: &[u32]) -> Vec<u8> {
        let mut out = u32le(keys.len() as u32);
        for &key in keys {
            out.extend(i32le(key));
        }
        out
    }

    fn base() -> World {
        let mut base = World::new();
        base.set([0, 0, 0], stone()).unwrap();
        base.finalize();
        base
    }

    #[test]
    fn a_forged_result_from_a_compromised_build_process_is_rejected_before_touching_the_world() {
        let base = base();
        let palette_length = base.palette().len();
        let mut legit = base.clone_world().unwrap();
        legit
            .set([1, 0, 0], BlockState::new("minecraft:oak_planks", &[]))
            .unwrap();
        let good = transfer::encode_changes(&legit, palette_length);
        let mut accepted = base.clone_world().unwrap();
        apply_changes(&mut accepted, &good, palette_length, &registry()).unwrap();
        assert_eq!(accepted.get([1, 0, 0]).unwrap().id, "minecraft:oak_planks");

        let key = key_of(0, 0, 0);
        let p = palette_length as u16;
        let cat = |parts: &[Vec<u8>]| parts.concat();
        let forged: Vec<(&str, Vec<u8>, &str)> = vec![
            (
                "command injection in a block id",
                cat(&[
                    header(json!([["minecraft:stone run op @a", {}]])),
                    u32le(1),
                    sparse(key, &[(1, p)]),
                    order(&[key]),
                ]),
                "palette entry: unknown block \"stone run op @a\" (closest matches: stone)",
            ),
            (
                "unknown block state",
                cat(&[
                    header(json!([["minecraft:oak_stairs", { "facing": "up" }]])),
                    u32le(1),
                    sparse(key, &[(1, p)]),
                    order(&[key]),
                ]),
                "palette entry: \"oak_stairs\" state facing=up is invalid (valid: north, south, east, west)",
            ),
            (
                "palette index past the palette",
                cat(&[
                    header(json!([])),
                    u32le(1),
                    sparse(key, &[(1, 999)]),
                    order(&[key]),
                ]),
                "invalid block 1=999",
            ),
            (
                "cell index past the chunk",
                cat(&[
                    header(json!([])),
                    u32le(1),
                    sparse(key, &[(5000, 1)]),
                    order(&[key]),
                ]),
                "invalid block 5000=1",
            ),
            (
                "chunk outside the coordinate range",
                cat(&[
                    header(json!([])),
                    u32le(1),
                    sparse(key_of(300, 0, 0), &[(0, 1)]),
                    order(&[key, key_of(300, 0, 0)]),
                ]),
                "chunk key 851968512 out of range",
            ),
            (
                "chunk dropped from the order",
                cat(&[header(json!([])), u32le(0), order(&[])]),
                "chunk order is incomplete",
            ),
            (
                "truncated buffer",
                good[..good.len() - 3].to_vec(),
                "truncated",
            ),
            (
                "trailing bytes",
                cat(&[good.clone(), vec![0]]),
                "trailing bytes",
            ),
        ];
        for (name, buffer, message) in forged {
            let mut target = base.clone_world().unwrap();
            let error =
                apply_changes(&mut target, &buffer, palette_length, &registry()).unwrap_err();
            assert_eq!(error, message, "{name}");
            assert_eq!(
                target.get([0, 0, 0]).unwrap().id,
                "minecraft:stone",
                "{name}"
            );
            assert_eq!(target.size(), 1, "{name}");
        }
    }

    #[test]
    fn a_forged_result_past_the_height_limit_or_naming_new_block_states_leaves_the_shared_palette_and_chunks_as_they_were()
     {
        let base = base();
        let palette_length = base.palette().len();
        let high = key_of(0, 200, 0);
        let mut cells = 1u16.to_le_bytes().to_vec();
        cells.extend_from_slice(&0u16.to_le_bytes());
        cells.extend_from_slice(&(palette_length as u16).to_le_bytes());
        let mut buffer = header(json!([["minecraft:oak_planks", {}]]));
        buffer.extend(u32le(1));
        buffer.extend(i32le(high));
        buffer.push(0);
        buffer.extend(cells);
        buffer.extend(order(&[key_of(0, 0, 0), high]));
        let mut target = base.clone_world().unwrap();
        let before: Vec<*const _> = target
            .chunks()
            .map(|(_, chunk)| std::sync::Arc::as_ptr(chunk))
            .collect();
        let error = apply_changes(&mut target, &buffer, palette_length, &registry()).unwrap_err();
        assert!(error.contains("reaches Minecraft y="), "{error}");
        assert_eq!(
            base.palette().len(),
            palette_length,
            "nothing was interned into the shared palette"
        );
        let after: Vec<*const _> = target
            .chunks()
            .map(|(_, chunk)| std::sync::Arc::as_ptr(chunk))
            .collect();
        assert_eq!(before, after);
        assert_eq!(target.size(), 1);
    }
}
