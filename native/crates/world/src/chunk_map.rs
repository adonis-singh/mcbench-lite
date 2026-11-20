//! A world's chunks by key in insertion order, as today's `Map`: setting a
//! key it holds keeps its place, a removed key that comes back goes last.
//! Removal leaves a hole that iteration skips; holes are swept when they
//! outnumber the chunks.

use std::collections::HashMap;
use std::sync::Arc;

use crate::chunk::{Chunk, IntBuild};

/// The chunks (see the module's doc).
#[derive(Debug, Default)]
pub struct ChunkMap {
    slots: Vec<Option<(u32, Arc<Chunk>)>>,
    at: HashMap<u32, usize, IntBuild>,
}

impl Clone for ChunkMap {
    fn clone(&self) -> ChunkMap {
        let mut out = ChunkMap::with_capacity(self.len());
        for (key, chunk) in self.iter() {
            out.insert(key, chunk.clone());
        }
        out
    }
}

impl ChunkMap {
    /// An empty map with room for `capacity` chunks.
    pub fn with_capacity(capacity: usize) -> ChunkMap {
        ChunkMap {
            slots: Vec::with_capacity(capacity),
            at: HashMap::with_capacity_and_hasher(capacity, IntBuild::default()),
        }
    }

    /// How many chunks it holds.
    pub fn len(&self) -> usize {
        self.at.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.at.is_empty()
    }

    /// Whether it holds `key`.
    pub fn contains(&self, key: u32) -> bool {
        self.at.contains_key(&key)
    }

    /// The chunk at `key`.
    #[inline]
    pub fn get(&self, key: u32) -> Option<&Arc<Chunk>> {
        let at = *self.at.get(&key)?;
        self.slots[at].as_ref().map(|(_, chunk)| chunk)
    }

    /// The chunk at `key`, to change.
    pub fn get_mut(&mut self, key: u32) -> Option<&mut Arc<Chunk>> {
        let at = *self.at.get(&key)?;
        self.slots[at].as_mut().map(|(_, chunk)| chunk)
    }

    /// Sets `key`: in its place when held, last when not.
    pub fn insert(&mut self, key: u32, chunk: Arc<Chunk>) {
        match self.at.get(&key) {
            Some(&at) => self.slots[at] = Some((key, chunk)),
            None => {
                self.at.insert(key, self.slots.len());
                self.slots.push(Some((key, chunk)));
            }
        }
    }

    /// Removes `key`.
    pub fn remove(&mut self, key: u32) -> Option<Arc<Chunk>> {
        let at = self.at.remove(&key)?;
        let removed = self.slots[at].take().map(|(_, chunk)| chunk);
        if self.slots.len() > 64 && self.slots.len() > 2 * self.at.len() {
            self.sweep();
        }
        removed
    }

    fn sweep(&mut self) {
        self.slots.retain(Option::is_some);
        for (at, slot) in self.slots.iter().enumerate() {
            let (key, _) = slot.as_ref().expect("swept");
            self.at.insert(*key, at);
        }
    }

    /// Keys and chunks in order.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &Arc<Chunk>)> {
        self.slots
            .iter()
            .filter_map(|slot| slot.as_ref().map(|(key, chunk)| (*key, chunk)))
    }

    /// Keys in order.
    pub fn keys(&self) -> impl Iterator<Item = u32> + '_ {
        self.iter().map(|(key, _)| key)
    }
}
