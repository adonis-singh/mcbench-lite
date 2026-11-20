//! The archive and its 7-day purge, deletes for good, and search
//! (`server/store.js`).

use rusqlite::{OptionalExtension, params};

use crate::chats::{empty_chat, remove_all};
use crate::{ARCHIVE_TTL_MS, Error, Store, iso};

/// An archived Build chat (`listArchive().conversations`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivedConversation {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `provider`.
    pub provider: String,
    /// `model`.
    pub model: String,
    /// `archivedAt`, epoch ms.
    pub archived_at: i64,
    /// `expiresAt`: when it is deleted.
    pub expires_at: i64,
}

/// An archived bench (`listArchive().benches`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivedBench {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `archivedAt`.
    pub archived_at: i64,
    /// `expiresAt`.
    pub expires_at: i64,
    /// `models`: how many models it ran.
    pub models: i64,
}

/// What is archived, newest first (`listArchive()`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Archive {
    /// Build chats (a bench's chats are part of it).
    pub conversations: Vec<ArchivedConversation>,
    /// Benches.
    pub benches: Vec<ArchivedBench>,
}

/// Which list a search is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchScope {
    /// `build`: chats.
    Build,
    /// `bench`: benches, by their title, rounds and what their models said.
    Bench,
}

/// A search result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    /// The chat's or bench's id.
    pub id: String,
    /// A snippet of the first matching message or round, the match wrapped
    /// in `\u{1}` and `\u{2}`.
    pub snippet: Option<String>,
}

/// A search takes at most this many different words (`MAX_SEARCH_TERMS`).
pub const MAX_SEARCH_TERMS: usize = 16;

impl Store {
    /// `archiveConversation(id)`.
    pub fn archive_conversation(&self, id: &str) -> Result<(), Error> {
        let now = self.now();
        let changes = self.with(|inner| {
            inner
                .db
                .prepare_cached(
                    "
    UPDATE conversations SET archived_at = ? WHERE id = ? AND archived_at IS NULL
  ",
                )?
                .execute(params![now, id])
        })?;
        if changes != 1 {
            return Err(Error::plain(format!("Conversation {id} not found")));
        }
        self.with(|inner| inner.live.remove(id));
        Ok(())
    }

    /// `removeChats(ids, extra)`: deletes chats for good: rows and search
    /// entries in one transaction, then folders, harnesses and storage
    /// measurements.
    fn remove_chats(
        &self,
        ids: &[String],
        extra: impl FnOnce() -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.transaction(|| {
            for id in ids {
                self.with(|inner| {
                    inner
                        .db
                        .prepare_cached("DELETE FROM search_docs WHERE owner = ?")?
                        .execute([id])?;
                    inner
                        .db
                        .prepare_cached("DELETE FROM conversations WHERE id = ?")?
                        .execute([id])?;
                    Ok::<_, Error>(())
                })?;
            }
            extra()
        })?;
        for id in ids {
            self.with(|inner| inner.live.remove(id));
            for folder in self.storage.chat_folders(id) {
                remove_all(&folder)?;
            }
            self.chat_removed(id);
            self.storage.forget(id);
        }
        Ok(())
    }

    /// `removeBench(id)`: a bench, its votes and its models' chats, for good.
    fn remove_bench(&self, id: &str) -> Result<(), Error> {
        let members: Vec<String> = self.with(|inner| {
            let mut statement = inner
                .db
                .prepare_cached("SELECT id FROM conversations WHERE bench_id = ?")?;
            let rows = statement
                .query_map([id], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, Error>(rows)
        })?;
        self.remove_chats(&members, || {
            self.with(|inner| {
                inner
                    .db
                    .prepare_cached("DELETE FROM search_docs WHERE owner = ?")?
                    .execute([id])?;
                inner
                    .db
                    .prepare_cached("DELETE FROM bench_votes WHERE bench_id = ?")?
                    .execute([id])?;
                inner
                    .db
                    .prepare_cached("DELETE FROM benches WHERE id = ?")?
                    .execute([id])?;
                Ok(())
            })
        })?;
        remove_all(&self.folder.attachments().join(id))?;
        self.storage.forget(id);
        Ok(())
    }

    /// `listArchive()`.
    pub fn list_archive(&self) -> Result<Archive, Error> {
        self.with(|inner| {
            let mut statement = inner.db.prepare_cached(
                "
      SELECT id, title, provider, model, archived_at AS archivedAt FROM conversations
      WHERE archived_at IS NOT NULL AND bench_id IS NULL ORDER BY archived_at DESC
    ",
            )?;
            let conversations = statement
                .query_map([], |row| {
                    let archived_at: i64 = row.get("archivedAt")?;
                    Ok(ArchivedConversation {
                        id: row.get("id")?,
                        title: row.get("title")?,
                        provider: row.get("provider")?,
                        model: row.get("model")?,
                        archived_at,
                        expires_at: archived_at + ARCHIVE_TTL_MS,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            let mut statement = inner.db.prepare_cached(
                "
      SELECT id, title, archived_at AS archivedAt, (SELECT count(*) FROM conversations WHERE bench_id = benches.id) AS models
      FROM benches WHERE archived_at IS NOT NULL ORDER BY archived_at DESC
    ",
            )?;
            let benches = statement
                .query_map([], |row| {
                    let archived_at: i64 = row.get("archivedAt")?;
                    Ok(ArchivedBench {
                        id: row.get("id")?,
                        title: row.get("title")?,
                        archived_at,
                        expires_at: archived_at + ARCHIVE_TTL_MS,
                        models: row.get("models")?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Archive { conversations, benches })
        })
    }

    /// `nextArchiveExpiry(now)`: when the next archived chat or bench is due
    /// after `now`, or `None`.
    pub fn next_archive_expiry(&self, now: i64) -> Result<Option<i64>, Error> {
        let archived_at: Option<i64> = self.with(|inner| {
            inner
                .db
                .prepare_cached(
                    "
    SELECT min(archived_at) FROM (
      SELECT archived_at FROM conversations WHERE archived_at IS NOT NULL AND bench_id IS NULL
      UNION ALL SELECT archived_at FROM benches WHERE archived_at IS NOT NULL
    ) WHERE archived_at > ?
  ",
                )?
                .query_row([now - ARCHIVE_TTL_MS], |row| row.get(0))
        })?;
        Ok(archived_at.map(|at| at + ARCHIVE_TTL_MS))
    }

    /// `checkArchivedChat(id, unarchived)`.
    fn check_archived_chat(&self, id: &str, unarchived: Error) -> Result<(), Error> {
        let row: Option<(Option<String>, Option<i64>)> = self.with(|inner| {
            inner
                .db
                .prepare_cached("SELECT bench_id, archived_at FROM conversations WHERE id = ?")?
                .query_row([id], |row| Ok((row.get(0)?, row.get(1)?)))
                .optional()
        })?;
        match row {
            None => Err(Error::refused("Conversation not found", 404)),
            Some((Some(bench), _)) if !bench.is_empty() => Err(Error::refused(
                "A bench chat is changed only through its bench",
                409,
            )),
            Some((_, None)) => Err(unarchived),
            Some(_) => Ok(()),
        }
    }

    fn check_archived_bench(&self, id: &str, unarchived: Error) -> Result<(), Error> {
        let row: Option<Option<i64>> = self.with(|inner| {
            inner
                .db
                .prepare_cached("SELECT archived_at FROM benches WHERE id = ?")?
                .query_row([id], |row| row.get(0))
                .optional()
        })?;
        match row {
            None => Err(Error::refused("Bench not found", 404)),
            Some(None) => Err(unarchived),
            Some(_) => Ok(()),
        }
    }

    /// `restoreConversation(id)`.
    pub fn restore_conversation(&self, id: &str) -> Result<(), Error> {
        self.check_archived_chat(id, Error::refused("This chat is not archived", 404))?;
        self.with(|inner| {
            inner
                .db
                .prepare_cached("UPDATE conversations SET archived_at = NULL WHERE id = ?")?
                .execute([id])?;
            Ok(())
        })
    }

    /// `restoreBench(id)`: with its models' chats.
    pub fn restore_bench(&self, id: &str) -> Result<(), Error> {
        self.check_archived_bench(id, Error::refused("This bench is not archived", 404))?;
        self.transaction(|| {
            self.with(|inner| {
                inner
                    .db
                    .prepare_cached("UPDATE benches SET archived_at = NULL WHERE id = ?")?
                    .execute([id])?;
                inner
                    .db
                    .prepare_cached(
                        "UPDATE conversations SET archived_at = NULL WHERE bench_id = ?",
                    )?
                    .execute([id])?;
                Ok(())
            })
        })
    }

    /// `deleteConversation(id)`: an archived chat, for good.
    pub fn delete_conversation(&self, id: &str) -> Result<(), Error> {
        self.check_archived_chat(id, Error::refused("Archive it first", 409))?;
        self.remove_chats(&[id.to_string()], || Ok(()))
    }

    /// `deleteBench(id)`.
    pub fn delete_bench(&self, id: &str) -> Result<(), Error> {
        self.check_archived_bench(id, Error::refused("Archive it first", 409))?;
        self.remove_bench(id)
    }

    /// `clearExpiredArchive(now)`: deletes what was archived 7 days before
    /// `now`, and empty chats as old that this run did not make or load.
    pub fn clear_expired_archive(&self, now: i64) -> Result<(), Error> {
        let cutoff = now - ARCHIVE_TTL_MS;
        let (ids, empty, benches) = self.with(|inner| {
            let ids: Vec<String> = inner
                .db
                .prepare_cached(
                    "
    SELECT id FROM conversations WHERE archived_at IS NOT NULL AND archived_at <= ? AND bench_id IS NULL
  ",
                )?
                .query_map([cutoff], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            let empty: Vec<String> = inner
                .db
                .prepare_cached(&format!(
                    "
    SELECT id FROM conversations WHERE archived_at IS NULL AND bench_id IS NULL AND created_at <= ? AND {}
  ",
                    empty_chat("conversations")
                ))?
                .query_map([iso(cutoff)], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?
                .into_iter()
                .filter(|id| !inner.live.contains_key(id))
                .collect();
            let benches: Vec<String> = inner
                .db
                .prepare_cached("SELECT id FROM benches WHERE archived_at IS NOT NULL AND archived_at <= ?")?
                .query_map([cutoff], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            Ok::<_, Error>((ids, empty, benches))
        })?;
        for id in ids.iter().chain(&empty) {
            if let Err(error) = self.remove_chats(std::slice::from_ref(id), || Ok(())) {
                eprintln!("[store] archive cleanup failed for {id}: {}", error.message);
            }
        }
        for id in &benches {
            if let Err(error) = self.remove_bench(id) {
                eprintln!(
                    "[store] archive cleanup failed for bench {id}: {}",
                    error.message
                );
            }
        }
        Ok(())
    }

    /// `search(scope, query)`: best match first. At most
    /// [`MAX_SEARCH_TERMS`] different words.
    pub fn search(&self, scope: SearchScope, query: &str) -> Result<Vec<SearchHit>, Error> {
        let pattern = crate::regex!(r"[\p{L}\p{N}_]+");
        let mut terms: Vec<&str> = Vec::new();
        for found in pattern.find_iter(query) {
            if !terms.contains(&found.as_str()) {
                terms.push(found.as_str());
            }
        }
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        if terms.len() > MAX_SEARCH_TERMS {
            return Err(Error::refused(
                format!("A search takes at most {MAX_SEARCH_TERMS} different words"),
                400,
            ));
        }
        let matched: String = terms
            .iter()
            .map(|term| format!("\"{term}\"*"))
            .collect::<Vec<_>>()
            .join(" ");
        struct Row {
            source: String,
            snippet: Option<String>,
            chat: Option<String>,
            chat_bench: Option<String>,
            chat_archived: Option<i64>,
            chat_empty: Option<i64>,
            bench: Option<String>,
            bench_archived: Option<i64>,
        }
        let rows: Vec<Row> = self.with(|inner| {
            let mut statement = inner.db.prepare_cached(&format!(
                "
    SELECT d.owner, d.source, snippet(search, 0, char(1), char(2), '…', 12) AS snippet,
      c.id AS chatId, c.bench_id AS benchId, c.archived_at AS chatArchived, {} AS chatEmpty,
      b.id AS bench, b.archived_at AS benchArchived
    FROM search
    JOIN search_docs d ON d.id = search.rowid
    LEFT JOIN conversations c ON c.id = d.owner
    LEFT JOIN benches b ON b.id = COALESCE(c.bench_id, d.owner)
    WHERE search MATCH ?
    ORDER BY bm25(search)
    LIMIT 500
  ",
                empty_chat("c")
            ))?;
            let rows = statement
                .query_map([matched], |row| {
                    Ok(Row {
                        source: row.get("source")?,
                        snippet: row.get("snippet")?,
                        chat: row.get("chatId")?,
                        chat_bench: row.get("benchId")?,
                        chat_archived: row.get("chatArchived")?,
                        chat_empty: row.get("chatEmpty")?,
                        bench: row.get("bench")?,
                        bench_archived: row.get("benchArchived")?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, Error>(rows)
        })?;
        let truthy = |value: &Option<String>| value.as_ref().is_some_and(|text| !text.is_empty());
        let mut results: Vec<SearchHit> = Vec::new();
        for row in rows {
            let id = match scope {
                SearchScope::Build => (truthy(&row.chat)
                    && !truthy(&row.chat_bench)
                    && row.chat_archived.is_none()
                    && row.chat_empty.unwrap_or(0) == 0)
                    .then(|| row.chat.clone())
                    .flatten(),
                SearchScope::Bench => (truthy(&row.bench)
                    && row.bench_archived.is_none()
                    && (!truthy(&row.chat) || row.chat_archived.is_none()))
                .then(|| row.bench.clone())
                .flatten(),
            };
            let Some(id) = id else { continue };
            let at = match results.iter().position(|hit| hit.id == id) {
                Some(at) => at,
                None => {
                    results.push(SearchHit { id, snippet: None });
                    results.len() - 1
                }
            };
            let hit = &mut results[at];
            if hit.snippet.as_deref().is_none_or(str::is_empty) && !row.source.starts_with("title:")
            {
                hit.snippet = row.snippet;
            }
        }
        Ok(results)
    }
}
