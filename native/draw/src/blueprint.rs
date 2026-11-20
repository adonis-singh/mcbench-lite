// The engine's blueprint (McbenchEngine.write; renderer/src/world.rs reads
// it): a palette (count, then each entry's id and properties as Java's
// writeUTF strings), the bounds (min x, y, z, max x, y, z), the count of
// sections, and each section: its section x, y, z and 4,096 palette
// indices (y << 8 | z << 4 | x), all big-endian. Here: the bounds of its
// blocks, and the part of it inside a clip box, as a blueprint of the same
// palette (server/shot-world.js clipWorld, at Minecraft's heights).

const SECTION: usize = 12 + 8192;

struct Parts<'a> {
    // The palette's bytes, with its count.
    palette: &'a [u8],
    sections: &'a [u8],
}

fn int(b: &[u8], at: usize) -> i32 {
    i32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn parts(bytes: &[u8]) -> Result<Parts<'_>, String> {
    let short = || "The blueprint ends early".to_string();
    let mut at = 0usize;
    let take = |at: &mut usize, n: usize| -> Result<usize, String> {
        let start = *at;
        *at = start
            .checked_add(n)
            .filter(|&end| end <= bytes.len())
            .ok_or_else(short)?;
        Ok(start)
    };
    let string = |at: &mut usize| -> Result<(), String> {
        let s = take(at, 2)?;
        take(at, u16::from_be_bytes([bytes[s], bytes[s + 1]]) as usize).map(|_| ())
    };
    let count = int(bytes, take(&mut at, 4)?);
    for _ in 0..count {
        string(&mut at)?;
        let n = int(bytes, take(&mut at, 4)?);
        for _ in 0..n.max(0) {
            string(&mut at)?;
            string(&mut at)?;
        }
    }
    let palette_end = at;
    take(&mut at, 24)?;
    let sections = int(bytes, take(&mut at, 4)?).max(0) as usize;
    let rest = &bytes[at..];
    if rest.len() != sections * SECTION {
        return Err(format!(
            "The blueprint holds {} bytes of sections, not {sections} sections",
            rest.len()
        ));
    }
    Ok(Parts {
        palette: &bytes[..palette_end],
        sections: rest,
    })
}

pub struct Clipped {
    pub bytes: Vec<u8>,
    // The blocks' bounds (min, max) at Minecraft's heights; None for none.
    pub bounds: Option<([i32; 3], [i32; 3])>,
    pub blocks: u64,
}

// The blocks of `bytes` inside the box (inclusive, Minecraft's heights), or
// all of them.
pub fn clip(bytes: &[u8], clip: Option<([i32; 3], [i32; 3])>) -> Result<Clipped, String> {
    let p = parts(bytes)?;
    let mut out_sections = Vec::with_capacity(p.sections.len());
    let (mut min, mut max) = ([i32::MAX; 3], [i32::MIN; 3]);
    let mut blocks = 0u64;
    let mut kept = 0i32;
    for section in p.sections.as_chunks::<SECTION>().0 {
        let origin = [
            int(section, 0) << 4,
            int(section, 4) << 4,
            int(section, 8) << 4,
        ];
        if let Some((lo, hi)) = clip {
            if (0..3).any(|i| origin[i] + 15 < lo[i] || origin[i] > hi[i]) {
                continue;
            }
        }
        let start = out_sections.len();
        out_sections.extend_from_slice(section);
        let cells = &mut out_sections[start + 12..];
        let mut any = false;
        for i in 0..4096usize {
            let value = u16::from_be_bytes([cells[i * 2], cells[i * 2 + 1]]);
            if value == 0 {
                continue;
            }
            let at = [
                origin[0] + (i & 15) as i32,
                origin[1] + (i >> 8) as i32,
                origin[2] + ((i >> 4) & 15) as i32,
            ];
            if let Some((lo, hi)) = clip {
                if (0..3).any(|k| at[k] < lo[k] || at[k] > hi[k]) {
                    cells[i * 2] = 0;
                    cells[i * 2 + 1] = 0;
                    continue;
                }
            }
            any = true;
            blocks += 1;
            for k in 0..3 {
                min[k] = min[k].min(at[k]);
                max[k] = max[k].max(at[k]);
            }
        }
        if any {
            kept += 1;
        } else {
            out_sections.truncate(start);
        }
    }
    let bounds = (blocks > 0).then_some((min, max));
    let mut bytes = Vec::with_capacity(p.palette.len() + 28 + out_sections.len());
    bytes.extend_from_slice(p.palette);
    let (lo, hi) = bounds.unwrap_or(([0; 3], [0; 3]));
    for v in lo.iter().chain(hi.iter()) {
        bytes.extend_from_slice(&v.to_be_bytes());
    }
    bytes.extend_from_slice(&kept.to_be_bytes());
    bytes.extend_from_slice(&out_sections);
    Ok(Clipped {
        bytes,
        bounds,
        blocks,
    })
}

// A blueprint's palette entries (their bytes) and its sections.
struct Parsed<'a> {
    entries: Vec<&'a [u8]>,
    sections: &'a [u8],
}

fn parse(bytes: &[u8]) -> Result<Parsed<'_>, String> {
    let p = parts(bytes)?;
    let mut entries = Vec::new();
    let mut at = 4usize;
    let utf = |at: &mut usize| {
        let n = u16::from_be_bytes([bytes[*at], bytes[*at + 1]]) as usize;
        *at += 2 + n;
    };
    for _ in 0..int(bytes, 0) {
        let start = at;
        utf(&mut at);
        let n = int(bytes, at);
        at += 4;
        for _ in 0..n.max(0) {
            utf(&mut at);
            utf(&mut at);
        }
        entries.push(&bytes[start..at]);
    }
    Ok(Parsed {
        entries,
        sections: p.sections,
    })
}

// The state the pack gives an entry's bytes (id, then its properties).
fn entry_state(pack: &mcbench_renderer::pack::Pack, entry: &[u8]) -> Result<u32, String> {
    let mut at = 0usize;
    let utf = |at: &mut usize| -> Result<String, String> {
        let n = u16::from_be_bytes([entry[*at], entry[*at + 1]]) as usize;
        let s = std::str::from_utf8(&entry[*at + 2..*at + 2 + n])
            .map_err(|_| "A blueprint name is not UTF-8".to_string())?
            .to_string();
        *at += 2 + n;
        Ok(s)
    };
    let id = utf(&mut at)?;
    let n = int(entry, at);
    at += 4;
    let mut properties = Vec::new();
    for _ in 0..n.max(0) {
        properties.push((utf(&mut at)?, utf(&mut at)?));
    }
    pack.state(&id, &properties)
}

// By palette index, the state each entry is (entry 0 is air: empty space).
pub fn palette_states(
    pack: &mcbench_renderer::pack::Pack,
    bytes: &[u8],
) -> Result<Vec<u32>, String> {
    let p = parse(bytes)?;
    p.entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            if i == 0 {
                Ok(pack.air)
            } else {
                entry_state(pack, e)
            }
        })
        .collect()
}

// An edit as the mesher's World::patch reads it (renderer/src/world.rs:
// the palette entries it adds, the bounds and built column count after
// it, each changed cell and its palette index), from the world's palette
// (`states`, by world index), the base blueprint (its indices to the
// world's: `base_map`) and the new blueprint.
pub struct Edit {
    pub patch: Vec<u8>,
    // The new blueprint's palette index to the world's.
    pub map: Vec<u16>,
    pub added_states: Vec<u32>,
    pub bounds: Option<([i32; 3], [i32; 3])>,
    pub blocks: u64,
    pub cells: usize,
}

pub fn edit(
    pack: &mcbench_renderer::pack::Pack,
    states: &[u32],
    base_map: &[u16],
    new: &[u8],
    base: &[u8],
) -> Result<Edit, String> {
    let n = parse(new)?;
    let b = parse(base)?;
    let mut index: std::collections::HashMap<u32, u16> = std::collections::HashMap::new();
    for (i, &s) in states.iter().enumerate().rev() {
        index.insert(s, i as u16);
    }
    index.insert(pack.air, 0);
    let mut added: Vec<&[u8]> = Vec::new();
    let mut added_states = Vec::new();
    let mut map = Vec::with_capacity(n.entries.len());
    for (i, e) in n.entries.iter().enumerate() {
        let state = if i == 0 {
            pack.air
        } else {
            entry_state(pack, e)?
        };
        let world = match index.get(&state) {
            Some(&w) => w,
            None => {
                let w = u16::try_from(states.len() + added.len())
                    .map_err(|_| "The world's palette is full".to_string())?;
                added.push(e);
                added_states.push(state);
                index.insert(state, w);
                w
            }
        };
        map.push(world);
    }
    let all_states: Vec<u32> = states.iter().chain(added_states.iter()).copied().collect();
    // Both blueprints' sections by place, as world palette indices.
    type Pair<'a> = (Option<&'a [u8]>, Option<&'a [u8]>);
    let mut cells: std::collections::BTreeMap<[i32; 3], Pair> = Default::default();
    for s in b.sections.as_chunks::<SECTION>().0 {
        cells
            .entry([int(s, 0), int(s, 4), int(s, 8)])
            .or_default()
            .0 = Some(&s[12..]);
    }
    for s in n.sections.as_chunks::<SECTION>().0 {
        cells
            .entry([int(s, 0), int(s, 4), int(s, 8)])
            .or_default()
            .1 = Some(&s[12..]);
    }
    let value = |c: Option<&[u8]>, i: usize, m: &[u16]| -> u16 {
        c.map_or(0, |c| {
            m[u16::from_be_bytes([c[i * 2], c[i * 2 + 1]]) as usize]
        })
    };
    let mut edits: Vec<u8> = Vec::new();
    let mut count = 0usize;
    let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
    let mut blocks = 0u64;
    let mut columns: std::collections::BTreeSet<(i32, i32)> = Default::default();
    // Sections whose bytes are the same under the same palette indices
    // differ in no cell: only their blocks' bounds are read.
    let same_palette = base_map.len() <= map.len()
        && base_map.iter().zip(&map).all(|(a, b)| a == b)
        || map.len() <= base_map.len() && map.iter().zip(base_map).all(|(a, b)| a == b);
    for (s, (old, new)) in &cells {
        let origin = s.map(|v| v << 4);
        if same_palette && old.is_some() && old == new {
            let c = new.unwrap();
            let mut any = false;
            for i in 0..4096usize {
                if c[i * 2] != 0 || c[i * 2 + 1] != 0 {
                    any = true;
                    let at = [
                        origin[0] + (i & 15) as i32,
                        origin[1] + (i >> 8) as i32,
                        origin[2] + ((i >> 4) & 15) as i32,
                    ];
                    blocks += 1;
                    for k in 0..3 {
                        lo[k] = lo[k].min(at[k]);
                        hi[k] = hi[k].max(at[k]);
                    }
                }
            }
            if any {
                columns.insert((s[0], s[2]));
            }
            continue;
        }
        for i in 0..4096usize {
            let (was, now) = (value(*old, i, base_map), value(*new, i, &map));
            let at = [
                origin[0] + (i & 15) as i32,
                origin[1] + (i >> 8) as i32,
                origin[2] + ((i >> 4) & 15) as i32,
            ];
            if now != 0 {
                blocks += 1;
                columns.insert((s[0], s[2]));
                for k in 0..3 {
                    lo[k] = lo[k].min(at[k]);
                    hi[k] = hi[k].max(at[k]);
                }
            }
            if all_states[was as usize] != all_states[now as usize] {
                for v in at {
                    edits.extend_from_slice(&v.to_be_bytes());
                }
                edits.extend_from_slice(&now.to_be_bytes());
                count += 1;
            }
        }
    }
    let bounds = (blocks > 0).then_some((lo, hi));
    let mut patch = Vec::new();
    patch.extend_from_slice(&(added.len() as i32).to_be_bytes());
    for e in &added {
        patch.extend_from_slice(e);
    }
    let (l, h) = bounds.unwrap_or(([0; 3], [0; 3]));
    for v in l.iter().chain(h.iter()) {
        patch.extend_from_slice(&v.to_be_bytes());
    }
    patch.extend_from_slice(&(columns.len() as i32).to_be_bytes());
    patch.extend_from_slice(&(count as i32).to_be_bytes());
    patch.extend_from_slice(&edits);
    Ok(Edit {
        patch,
        map,
        added_states,
        bounds,
        blocks,
        cells: count,
    })
}

// Edited copies of a blueprint, for the edit tests: `steps` blueprints,
// each the one before with a few cells near its blocks set to entries of
// its own palette or cleared (a seeded sequence; the palette is kept).
pub fn edits_for_tests(bytes: &[u8], seed: u64, steps: usize) -> Vec<Vec<u8>> {
    let p = parse(bytes).expect("a blueprint");
    let palette_end = parts(bytes).expect("a blueprint").palette.len();
    let entries = p.entries.len() as u64;
    let mut sections: std::collections::BTreeMap<[i32; 3], Vec<u16>> = p
        .sections
        .as_chunks::<SECTION>()
        .0
        .iter()
        .map(|s| {
            (
                [int(s, 0), int(s, 4), int(s, 8)],
                (0..4096)
                    .map(|i| u16::from_be_bytes([s[12 + i * 2], s[13 + i * 2]]))
                    .collect(),
            )
        })
        .collect();
    let mut state = seed | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let encode = |sections: &std::collections::BTreeMap<[i32; 3], Vec<u16>>| {
        let mut out = bytes[..palette_end].to_vec();
        let (mut lo, mut hi) = ([i32::MAX; 3], [i32::MIN; 3]);
        let mut body = Vec::new();
        let mut count = 0i32;
        for (s, cells) in sections {
            if cells.iter().all(|&c| c == 0) {
                continue;
            }
            count += 1;
            for v in s {
                body.extend_from_slice(&v.to_be_bytes());
            }
            for (i, &c) in cells.iter().enumerate() {
                body.extend_from_slice(&c.to_be_bytes());
                if c != 0 {
                    let at = [
                        (s[0] << 4) + (i & 15) as i32,
                        (s[1] << 4) + (i >> 8) as i32,
                        (s[2] << 4) + ((i >> 4) & 15) as i32,
                    ];
                    for k in 0..3 {
                        lo[k] = lo[k].min(at[k]);
                        hi[k] = hi[k].max(at[k]);
                    }
                }
            }
        }
        for v in lo.iter().chain(hi.iter()) {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&body);
        out
    };
    let mut out = vec![encode(&sections)];
    for _ in 0..steps {
        let keys: Vec<[i32; 3]> = sections.keys().copied().collect();
        for _ in 0..1 + next() % 5 {
            let s = keys[(next() % keys.len() as u64) as usize];
            let filled: Vec<usize> = (0..4096).filter(|&i| sections[&s][i] != 0).collect();
            let Some(&i) = filled.get((next() % filled.len().max(1) as u64) as usize) else {
                continue;
            };
            let at = [
                (s[0] << 4) + (i & 15) as i32 + (next() % 5) as i32 - 2,
                (s[1] << 4) + (i >> 8) as i32 + (next() % 5) as i32 - 2,
                (s[2] << 4) + ((i >> 4) & 15) as i32 + (next() % 5) as i32 - 2,
            ];
            let value = if next() % 3 == 0 {
                0
            } else {
                1 + (next() % (entries - 1)) as u16
            };
            let cells = sections
                .entry(at.map(|v| v >> 4))
                .or_insert_with(|| vec![0; 4096]);
            cells[(((at[1] & 15) << 8) | ((at[2] & 15) << 4) | (at[0] & 15)) as usize] = value;
        }
        out.push(encode(&sections));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // A blueprint of one palette entry beside air and the given blocks.
    pub fn blueprint(blocks: &[[i32; 3]]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&2i32.to_be_bytes());
        for id in ["minecraft:air", "minecraft:stone"] {
            out.extend_from_slice(&(id.len() as u16).to_be_bytes());
            out.extend_from_slice(id.as_bytes());
            out.extend_from_slice(&0i32.to_be_bytes());
        }
        let mut sections: std::collections::BTreeMap<[i32; 3], Vec<u8>> = Default::default();
        for b in blocks {
            let s = b.map(|v| v >> 4);
            let cells = sections.entry(s).or_insert_with(|| vec![0; 8192]);
            let i = (((b[1] & 15) << 8) | ((b[2] & 15) << 4) | (b[0] & 15)) as usize;
            cells[i * 2 + 1] = 1;
        }
        let lo = [0, 1, 2].map(|k| blocks.iter().map(|b| b[k]).min().unwrap());
        let hi = [0, 1, 2].map(|k| blocks.iter().map(|b| b[k]).max().unwrap());
        for v in lo.iter().chain(hi.iter()) {
            out.extend_from_slice(&v.to_be_bytes());
        }
        out.extend_from_slice(&(sections.len() as i32).to_be_bytes());
        for (s, cells) in sections {
            for v in s {
                out.extend_from_slice(&v.to_be_bytes());
            }
            out.extend_from_slice(&cells);
        }
        out
    }

    #[test]
    fn whole_blueprint_keeps_every_block_and_its_bounds() {
        let blocks = [[0, -64, 0], [17, 3, -5], [-20, 100, 40]];
        let bytes = blueprint(&blocks);
        let c = clip(&bytes, None).unwrap();
        assert_eq!(c.blocks, 3);
        assert_eq!(c.bounds, Some(([-20, -64, -5], [17, 100, 40])));
        assert_eq!(c.bytes, bytes);
    }

    #[test]
    fn clip_keeps_only_blocks_inside_and_drops_empty_sections() {
        let blocks = [[0, 0, 0], [1, 0, 0], [5, 0, 0], [40, 0, 0]];
        let c = clip(&blueprint(&blocks), Some(([0, 0, 0], [4, 0, 0]))).unwrap();
        assert_eq!(c.blocks, 2);
        assert_eq!(c.bounds, Some(([0, 0, 0], [1, 0, 0])));
        assert_eq!(c.bytes, blueprint(&[[0, 0, 0], [1, 0, 0]]));
    }

    #[test]
    fn a_truncated_blueprint_is_an_error() {
        let bytes = blueprint(&[[0, 0, 0]]);
        assert!(clip(&bytes[..bytes.len() - 1], None).is_err());
    }
}
