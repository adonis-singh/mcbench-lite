//! A world's palette (`server/world.js` `PaletteStore`): block states by
//! their index, zero for air, interned by their signature. A palette is
//! shared by a world and the worlds cloned from it, as today's object is: a
//! state a clone interns is in the original's palette too, and so in its
//! manifest. Entries are only ever added, so readers take `&BlockState` with
//! no lock; interning takes a lock.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::BlockState;
use crate::js::{code_unit_cmp, object_order};

/// The most entries past air a palette holds: dense chunks are `u16`
/// (`MAX_PALETTE`).
pub const MAX_PALETTE: usize = 65535;

const SEGMENT: usize = 256;

static PALETTES: AtomicU64 = AtomicU64::new(1);

/// `stateSignature(id, states)`: the id and the properties sorted by name
/// (UTF-16 order, as `sort()`), `id|a=1,b=2`.
pub fn signature(id: &str, properties: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = properties.iter().collect();
    sorted.sort_by(|a, b| code_unit_cmp(&a.0, &b.0));
    let mut out = String::with_capacity(id.len() + 1 + properties.len() * 12);
    out.push_str(id);
    out.push('|');
    for (at, (key, value)) in sorted.into_iter().enumerate() {
        if at > 0 {
            out.push(',');
        }
        out.push_str(key);
        out.push('=');
        out.push_str(value);
    }
    out
}

/// One segment of entries, each written once.
type Segment = Box<[OnceLock<BlockState>]>;

/// A palette (see the module's doc).
pub struct Palette {
    uid: u64,
    len: AtomicUsize,
    segments: Box<[OnceLock<Segment>]>,
    by_signature: Mutex<HashMap<String, u16>>,
}

impl std::fmt::Debug for Palette {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Palette")
            .field("uid", &self.uid)
            .field("len", &self.len())
            .finish()
    }
}

impl Default for Palette {
    fn default() -> Palette {
        Palette::new()
    }
}

impl Palette {
    /// A palette holding air alone.
    pub fn new() -> Palette {
        let segments = (0..(MAX_PALETTE + 1).div_ceil(SEGMENT))
            .map(|_| OnceLock::new())
            .collect();
        Palette {
            uid: PALETTES.fetch_add(1, Ordering::Relaxed),
            len: AtomicUsize::new(1),
            segments,
            by_signature: Mutex::new(HashMap::new()),
        }
    }

    /// A number naming this palette for the life of the process (today's
    /// object identity).
    pub fn uid(&self) -> u64 {
        self.uid
    }

    /// Its entries, air included (`entries.length`).
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    /// Whether it holds air alone.
    pub fn is_empty(&self) -> bool {
        self.len() == 1
    }

    /// The entry at `index`, `None` for air and past the end (`get`).
    pub fn get(&self, index: usize) -> Option<&BlockState> {
        if index == 0 || index >= self.len() {
            return None;
        }
        self.segments[index / SEGMENT].get()?[index % SEGMENT].get()
    }

    /// The entry at `index`, which must be one (`entries[index]`).
    pub fn entry(&self, index: usize) -> &BlockState {
        self.get(index)
            .unwrap_or_else(|| panic!("palette index {index} has no entry"))
    }

    /// Every entry past air, in order (`entries.slice(1)`).
    pub fn entries(&self) -> impl Iterator<Item = &BlockState> {
        (1..self.len()).map(|index| self.entry(index))
    }

    /// The index of `id` with `properties`, added when new (`intern`).
    /// `properties` are kept as an object made from them keeps its keys.
    pub fn intern(&self, id: &str, properties: &[(String, String)]) -> Result<u16, String> {
        let properties = object_order(properties.to_vec());
        let signature = signature(id, &properties);
        let mut index = self.by_signature.lock().expect("palette lock");
        if let Some(&found) = index.get(&signature) {
            return Ok(found);
        }
        let at = self.len();
        if at > MAX_PALETTE {
            return Err(format!(
                "Build uses more than {MAX_PALETTE} distinct block states"
            ));
        }
        let state = BlockState {
            id: id.to_string(),
            properties,
        };
        let segment = self.segments[at / SEGMENT]
            .get_or_init(|| (0..SEGMENT).map(|_| OnceLock::new()).collect());
        segment[at % SEGMENT]
            .set(state)
            .expect("an entry is written once");
        index.insert(signature, at as u16);
        self.len.store(at + 1, Ordering::Release);
        Ok(at as u16)
    }

    /// The index of a state already in the palette, without adding it.
    pub fn index_of(&self, id: &str, properties: &[(String, String)]) -> Option<u16> {
        let properties = object_order(properties.to_vec());
        self.by_signature
            .lock()
            .expect("palette lock")
            .get(&signature(id, &properties))
            .copied()
    }

    /// The indices interning `blocks` in order would give, without changing
    /// the palette (`plan`).
    pub fn plan(&self, blocks: &[BlockState]) -> Result<Vec<u16>, String> {
        let index = self.by_signature.lock().expect("palette lock");
        let len = self.len();
        let mut added: HashMap<String, usize> = HashMap::new();
        let mut out = Vec::with_capacity(blocks.len());
        for block in blocks {
            let signature = signature(&block.id, &object_order(block.properties.clone()));
            if let Some(&found) = index.get(&signature) {
                out.push(found);
                continue;
            }
            if let Some(&found) = added.get(&signature) {
                out.push(found as u16);
                continue;
            }
            let at = len + added.len();
            if at > MAX_PALETTE {
                return Err(format!(
                    "Build uses more than {MAX_PALETTE} distinct block states"
                ));
            }
            added.insert(signature, at);
            out.push(at as u16);
        }
        Ok(out)
    }
}
