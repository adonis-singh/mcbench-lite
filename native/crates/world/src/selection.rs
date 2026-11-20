//! Boxes the user picks in the viewer (`server/selection.js`): a box named
//! by Minecraft's name of the block it mostly holds and where it sits in
//! the build ("Dark Oak Stairs, top"), and the selection part a message
//! carries.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::world::coords_of;
use crate::{BlockBox, MAX_COORD, World};

/// A selection part of a user message, as the composer sends it and
/// [`read_selection`] keeps it (`readSelection`'s result, its keys in this
/// order).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    /// One capital letter.
    pub letter: String,
    /// Its label, at most 120 UTF-16 units.
    pub label: String,
    /// The lowest corner.
    pub min: [i64; 3],
    /// The highest corner.
    pub max: [i64; 3],
    /// How many blocks it holds.
    pub blocks: i64,
    /// Its main block, `None` for empty space.
    pub block: Option<String>,
}

/// What [`describe_selection`] says of a box.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionDescription {
    /// The blocks in the box.
    pub blocks: u64,
    /// The block it holds most of, `None` when empty.
    pub block: Option<String>,
    /// "Dark Oak Stairs, top", or "Empty space".
    pub label: String,
}

/// An error the HTTP layer answers 400 (`statusCode: 400`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn placement(bounds: BlockBox, min: [i32; 3], max: [i32; 3]) -> String {
    let center = |a: usize| (min[a] as f64 + max[a] as f64) / 2.0;
    let share = |a: usize| {
        let extent = (bounds.max[a] - bounds.min[a] + 1) as f64;
        if extent < 3.0 {
            0.5
        } else {
            (center(a) - bounds.min[a] as f64 + 0.5) / extent
        }
    };
    let (y, x, z) = (share(1), share(0), share(2));
    let vertical = if y > 2.0 / 3.0 {
        "top"
    } else if y < 1.0 / 3.0 {
        "base"
    } else {
        ""
    };
    let north_south = if z < 1.0 / 3.0 {
        "north"
    } else if z > 2.0 / 3.0 {
        "south"
    } else {
        ""
    };
    let west_east = if x < 1.0 / 3.0 {
        "west"
    } else if x > 2.0 / 3.0 {
        "east"
    } else {
        ""
    };
    let side = [north_south, west_east]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("-");
    let side = if side.is_empty() {
        String::new()
    } else {
        format!("{side} side")
    };
    let parts: Vec<&str> = [vertical, side.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        "middle".to_string()
    } else {
        parts.join(", ")
    }
}

/// `isBox`: three integers.
fn integers(value: &Value) -> Option<Vec<f64>> {
    let list = value.as_array()?;
    if list.len() != 3 {
        return None;
    }
    list.iter()
        .map(|v| v.as_f64().filter(|n| n.is_finite() && n.fract() == 0.0))
        .collect()
}

/// Names the box `min`..`max` (as the request sends them) by Minecraft's
/// name of its most common block (`names`: block id to name, the block
/// table's) and where it sits (`describeSelection`).
pub fn describe_selection(
    world: &World,
    min: &Value,
    max: &Value,
    names: &IndexMap<String, String>,
) -> Result<SelectionDescription, Refused> {
    let refused = || {
        Refused(format!(
            "A selection needs integer corners within +/-{MAX_COORD} with min at most max"
        ))
    };
    let (Some(low), Some(high)) = (integers(min), integers(max)) else {
        return Err(refused());
    };
    if (0..3).any(|a| low[a] > high[a])
        || low.iter().chain(&high).any(|v| v.abs() > MAX_COORD as f64)
    {
        return Err(refused());
    }
    let min = [low[0] as i32, low[1] as i32, low[2] as i32];
    let max = [high[0] as i32, high[1] as i32, high[2] as i32];
    let (low, high) = (min.map(|v| v >> 4), max.map(|v| v >> 4));
    let mut keys: Vec<(u32, [i32; 3])> = world
        .chunks()
        .map(|(key, _)| (key, coords_of(key)))
        .filter(|(_, at)| (0..3).all(|a| at[a] >= low[a] && at[a] <= high[a]))
        .collect();
    keys.sort_by_key(|&(_, at)| at);
    let mut counts: IndexMap<&str, u64> = IndexMap::new();
    let mut blocks = 0;
    for (key, _) in keys {
        world.for_each_in_chunk(key, |p, entry| {
            if (0..3).any(|a| p[a] < min[a] || p[a] > max[a]) {
                return;
            }
            blocks += 1;
            *counts.entry(entry.id.as_str()).or_insert(0) += 1;
        });
    }
    if blocks == 0 {
        return Ok(SelectionDescription {
            blocks,
            block: None,
            label: "Empty space".into(),
        });
    }
    let mut best: (&str, u64) = counts
        .first()
        .map(|(k, v)| (*k, *v))
        .expect("a block was counted");
    for (&id, &count) in counts.iter().skip(1) {
        if count > best.1 {
            best = (id, count);
        }
    }
    let name = names.get(best.0).map_or("undefined", String::as_str);
    let bounds = world.bounds().expect("a world with blocks has bounds");
    Ok(SelectionDescription {
        blocks,
        block: Some(best.0.to_string()),
        label: format!("{name}, {}", placement(bounds, min, max)),
    })
}

fn is_main_block(text: &str) -> bool {
    text.strip_prefix("minecraft:").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    })
}

/// A selection part of a message as the composer sends it (`readSelection`),
/// its label cut to 120 UTF-16 units.
pub fn read_selection(value: &Value) -> Result<Selection, Refused> {
    let refused = || {
        Refused("Each selection needs a letter, a label, integer corners, a block count and its main block".into())
    };
    let field = |name: &str| value.get(name).unwrap_or(&Value::Null);
    let letter = field("letter")
        .as_str()
        .filter(|l| l.len() == 1 && l.as_bytes()[0].is_ascii_uppercase())
        .ok_or_else(refused)?;
    let label = field("label")
        .as_str()
        .filter(|l| !l.is_empty())
        .ok_or_else(refused)?;
    let corner = |name: &str| -> Result<[i64; 3], Refused> {
        let list = integers(field(name)).ok_or_else(refused)?;
        let fit = |v: f64| {
            if v.abs() < 9.007_199_254_740_992e15 {
                Ok(v as i64)
            } else {
                Err(refused())
            }
        };
        Ok([fit(list[0])?, fit(list[1])?, fit(list[2])?])
    };
    let (min, max) = (corner("min")?, corner("max")?);
    let blocks = field("blocks")
        .as_f64()
        .filter(|n| n.is_finite() && n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15)
        .ok_or_else(refused)? as i64;
    let block = match field("block") {
        Value::Null if value.get("block").is_some() => None,
        Value::String(text) if is_main_block(text) => Some(text.clone()),
        _ => return Err(refused()),
    };
    Ok(Selection {
        letter: letter.to_string(),
        label: cut_utf16(label, 120),
        min,
        max,
        blocks,
        block,
    })
}

/// The first `units` UTF-16 units of `text` (`slice(0, units)`); a pair cut
/// in two loses its first half, which a Rust string cannot hold alone.
fn cut_utf16(text: &str, units: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in text.chars() {
        used += c.len_utf16();
        if used > units {
            break;
        }
        out.push(c);
    }
    out
}
