//! Where the store meets `mcbench-world`: reading a chat's worlds from their
//! manifest and chunk files (`World.fromManifest`), and marking chunks not
//! stored again after a failed save removed their files (`discardChunks`).
//! Writing goes through `World::persist` and `World::persist_chunks` with the
//! store's chunk writer (`chats.rs`, `persist_into`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mcbench_world::{ChunkCache, World, WorldManifest};

use crate::json::{self, Json};

/// `chunkPath(id, hash)`.
pub(crate) fn chunk_path(worlds: &Path, id: &str, hash: &str) -> PathBuf {
    worlds.join(id).join("chunks").join(format!("{hash}.json"))
}

/// `World.fromManifest(parseJson(text, label), chunkReader(id), cache)`:
/// the world stored as `text`, each chunk file read once per `cache` (one
/// load of a chat's branch shares one, as `loadBranch` does).
pub(crate) fn load(
    worlds: &Path,
    id: &str,
    text: &str,
    label: &str,
    cache: &mut ChunkCache,
) -> Result<World, String> {
    let mut read = |hash: &str| {
        let file = chunk_path(worlds, id, hash);
        std::fs::read(&file).map_err(|error| format!("{}: {error}", file.display()))
    };
    World::from_manifest_cached(&WorldManifest(text.to_string()), &mut read, cache)
        .map_err(|error| format!("{label}: {error}"))
}

/// `discardChunks`' last half: chunks of `worlds` named by one of `hashes`
/// are no longer stored, so the next save writes their files again.
pub(crate) fn forget_stored<'a>(
    worlds: impl IntoIterator<Item = &'a World>,
    hashes: &HashSet<String>,
) {
    for world in worlds {
        for (_, chunk) in world.chunks() {
            if chunk
                .persisted()
                .is_some_and(|hash| hashes.contains(&*hash))
            {
                chunk.set_persisted(None);
            }
        }
    }
}

/// The hashes of the chunks a stored manifest names (`manifest.chunks[i][1]`).
pub(crate) fn chunk_hashes(text: &str) -> Result<Vec<String>, String> {
    let value = json::parse(text)?;
    let chunks = value
        .get("chunks")
        .and_then(Json::as_array)
        .ok_or("a world manifest has no chunks")?;
    chunks
        .iter()
        .map(|chunk| {
            chunk
                .get(1)
                .and_then(Json::as_str)
                .map(str::to_string)
                .ok_or_else(|| "a world manifest's chunk has no hash".to_string())
        })
        .collect()
}
