//! One 16³ chunk of palette indices (`server/world.js` `Chunk`): sparse (an
//! insertion-ordered map, as today's `Map`, whose order the build transfer
//! writes) until it holds more than [`DENSE_THRESHOLD`] blocks, dense (4096
//! `u16`) from then on. Its version is a process-unique stamp: equal (key,
//! version), equal content.

use std::hash::{BuildHasherDefault, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use indexmap::IndexMap;

/// A chunk's side in blocks.
pub const CHUNK_SIZE: i32 = 16;
/// The cells of a chunk (`CHUNK_VOLUME`).
pub const CHUNK_VOLUME: usize = 4096;
/// Past this many blocks a sparse chunk becomes dense (`DENSE_THRESHOLD`).
pub const DENSE_THRESHOLD: usize = 512;

static STAMP: AtomicU64 = AtomicU64::new(0);

fn stamp() -> u64 {
    STAMP.fetch_add(1, Ordering::Relaxed) + 1
}

/// A hasher for small integer keys (chunk keys and cell indices).
#[derive(Default, Clone, Copy)]
pub struct IntHasher(u64);

impl Hasher for IntHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0.rotate_left(8) ^ byte as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }

    fn write_u16(&mut self, value: u16) {
        self.0 = (value as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }

    fn write_u32(&mut self, value: u32) {
        self.0 = (value as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

/// The hasher of this crate's integer maps.
pub type IntBuild = BuildHasherDefault<IntHasher>;

/// A sparse chunk's cells: index to palette index, in insertion order.
pub type Sparse = IndexMap<u16, u16, IntBuild>;

/// How a chunk keeps its cells.
#[derive(Clone, Debug)]
pub enum Cells {
    /// Index to palette index, in the order the cells were set.
    Sparse(Sparse),
    /// Every cell, `(y << 8) | (z << 4) | x`.
    Dense(Box<[u16; CHUNK_VOLUME]>),
}

/// A chunk (see the module's doc).
#[derive(Debug)]
pub struct Chunk {
    pub(crate) cells: Cells,
    pub(crate) count: usize,
    pub(crate) version: u64,
    pub(crate) persisted: Mutex<Option<Arc<str>>>,
    pub(crate) bbox: OnceLock<[u8; 6]>,
}

impl Clone for Chunk {
    fn clone(&self) -> Chunk {
        Chunk {
            cells: self.cells.clone(),
            count: self.count,
            version: self.version,
            persisted: Mutex::new(self.persisted()),
            bbox: self.bbox.clone(),
        }
    }
}

impl Default for Chunk {
    fn default() -> Chunk {
        Chunk::new()
    }
}

/// A new dense array of air.
pub fn air_cells() -> Box<[u16; CHUNK_VOLUME]> {
    Box::new([0; CHUNK_VOLUME])
}

impl Chunk {
    /// An empty sparse chunk with a new version.
    pub fn new() -> Chunk {
        Chunk {
            cells: Cells::Sparse(Sparse::default()),
            count: 0,
            version: stamp(),
            persisted: Mutex::new(None),
            bbox: OnceLock::new(),
        }
    }

    /// A chunk of `cells` holding `count` blocks.
    pub(crate) fn of(cells: Cells, count: usize) -> Chunk {
        Chunk {
            cells,
            count,
            ..Chunk::new()
        }
    }

    /// Its cells.
    pub fn cells(&self) -> &Cells {
        &self.cells
    }

    /// Whether it is dense.
    pub fn is_dense(&self) -> bool {
        matches!(self.cells, Cells::Dense(_))
    }

    /// Its blocks.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Its version stamp.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The hash its file is stored under, once stored.
    pub fn persisted(&self) -> Option<Arc<str>> {
        self.persisted.lock().expect("chunk lock").clone()
    }

    /// Marks it stored under `hash`, or not stored.
    pub fn set_persisted(&self, hash: Option<Arc<str>>) {
        *self.persisted.lock().expect("chunk lock") = hash;
    }

    /// Its local box once computed: min x, y, z, max x, y, z (`bbox`).
    pub fn cached_bbox(&self) -> Option<[u8; 6]> {
        self.bbox.get().copied()
    }

    pub(crate) fn touch(&mut self) {
        self.version = stamp();
        *self.persisted.get_mut().expect("chunk lock") = None;
        self.bbox = OnceLock::new();
    }

    /// The palette index at cell `index`.
    #[inline]
    pub fn get(&self, index: usize) -> u16 {
        match &self.cells {
            Cells::Dense(dense) => dense[index],
            Cells::Sparse(sparse) => sparse.get(&(index as u16)).copied().unwrap_or(0),
        }
    }

    pub(crate) fn ensure_dense(&mut self) -> &mut [u16; CHUNK_VOLUME] {
        if let Cells::Sparse(sparse) = &self.cells {
            let mut dense = air_cells();
            for (&index, &value) in sparse {
                dense[index as usize] = value;
            }
            self.cells = Cells::Dense(dense);
        }
        match &mut self.cells {
            Cells::Dense(dense) => dense,
            Cells::Sparse(_) => unreachable!("made dense above"),
        }
    }

    /// Every cell (`view`): the dense array, or a copy of a sparse chunk's.
    pub fn view(&self) -> std::borrow::Cow<'_, [u16; CHUNK_VOLUME]> {
        match &self.cells {
            Cells::Dense(dense) => std::borrow::Cow::Borrowed(dense),
            Cells::Sparse(sparse) => {
                let mut dense = air_cells();
                for (&index, &value) in sparse {
                    dense[index as usize] = value;
                }
                std::borrow::Cow::Owned(*dense)
            }
        }
    }

    /// Sets cell `index` and returns what it held (`set`).
    pub(crate) fn set(&mut self, index: usize, value: u16) -> u16 {
        let previous = self.get(index);
        if previous == value {
            return previous;
        }
        match &mut self.cells {
            Cells::Dense(dense) => dense[index] = value,
            Cells::Sparse(sparse) => {
                if value == 0 {
                    sparse.shift_remove(&(index as u16));
                } else {
                    sparse.insert(index as u16, value);
                    if sparse.len() > DENSE_THRESHOLD {
                        self.ensure_dense();
                    }
                }
            }
        }
        if previous == 0 {
            self.count += 1;
        } else if value == 0 {
            self.count -= 1;
        }
        self.touch();
        previous
    }

    /// `f(index, palette index)` for every block, in today's order: index
    /// order when dense, insertion order when sparse (`forEach`).
    #[inline]
    pub fn for_each(&self, mut f: impl FnMut(usize, u16)) {
        match &self.cells {
            Cells::Dense(dense) => {
                for (index, &value) in dense.iter().enumerate() {
                    if value != 0 {
                        f(index, value);
                    }
                }
            }
            Cells::Sparse(sparse) => {
                for (&index, &value) in sparse {
                    f(index as usize, value);
                }
            }
        }
    }

    /// `[index, palette index]` pairs in index order (`pairs`).
    pub fn pairs(&self) -> Vec<(u16, u16)> {
        let mut pairs = Vec::with_capacity(self.count);
        match &self.cells {
            Cells::Dense(_) => self.for_each(|index, value| pairs.push((index as u16, value))),
            Cells::Sparse(sparse) => {
                pairs.extend(sparse.iter().map(|(&index, &value)| (index, value)));
                pairs.sort_unstable_by_key(|&(index, _)| index);
            }
        }
        pairs
    }

    /// Its local box, computed once per version (`bounds`); `None` when it
    /// holds nothing.
    pub fn bounds(&self) -> Option<[u8; 6]> {
        if let Some(bbox) = self.bbox.get() {
            return Some(*bbox);
        }
        if self.count == 0 {
            return None;
        }
        let mut b = [16u8, 16, 16, 0, 0, 0];
        let mut any = false;
        self.for_each(|index, _| {
            let (x, z, y) = (
                (index & 15) as u8,
                ((index >> 4) & 15) as u8,
                (index >> 8) as u8,
            );
            if !any {
                b = [x, y, z, x, y, z];
                any = true;
            }
            b[0] = b[0].min(x);
            b[1] = b[1].min(y);
            b[2] = b[2].min(z);
            b[3] = b[3].max(x);
            b[4] = b[4].max(y);
            b[5] = b[5].max(z);
        });
        Some(*self.bbox.get_or_init(|| b))
    }
}
