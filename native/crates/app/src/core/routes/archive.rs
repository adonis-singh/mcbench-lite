//! The archive, search and the leaderboard (`server/index.js`:
//! `GET /api/archive`, `POST .../restore` and `DELETE` of a chat or bench,
//! `GET /api/search`, `GET /api/leaderboard`, `DELETE /api/votes`; the
//! 7-day purge is `Core`'s own task).

use mcbench_agent::lifecycle::Refusal;
use mcbench_store::json::Json;
use mcbench_store::{Archive, SearchHit, SearchScope};

use crate::core::{Core, refusal};

impl Core {
    /// `GET /api/archive`, typed.
    pub async fn archive_list(&self) -> Result<Archive, Refusal> {
        self.blocking(|core| core.0.store.list_archive().map_err(Refusal::from))
            .await
    }

    /// `GET /api/archive`: what is archived, newest first, with when each
    /// is deleted.
    pub async fn archive(&self) -> Result<Json, Refusal> {
        let archive = self.archive_list().await?;
        Ok(mcbench_store::object! {
            "conversations" => Json::Array(archive.conversations.iter().map(|row| mcbench_store::object! {
                "id" => row.id.as_str(), "title" => row.title.as_str(), "provider" => row.provider.as_str(),
                "model" => row.model.as_str(), "archivedAt" => row.archived_at, "expiresAt" => row.expires_at,
            }).collect()),
            "benches" => Json::Array(archive.benches.iter().map(|row| mcbench_store::object! {
                "id" => row.id.as_str(), "title" => row.title.as_str(), "archivedAt" => row.archived_at,
                "expiresAt" => row.expires_at, "models" => row.models,
            }).collect()),
        })
    }

    /// `POST /api/conversations/:id/restore`.
    pub async fn restore_conversation(&self, id: &str) -> Result<(), Refusal> {
        let id = id.to_string();
        self.blocking(move |core| {
            core.0
                .store
                .restore_conversation(&id)
                .map_err(Refusal::from)
        })
        .await?;
        self.list_changed();
        Ok(())
    }

    /// `POST /api/benches/:id/restore`: the bench carries on with the
    /// rounds it had queued.
    pub async fn restore_bench(&self, id: &str) -> Result<(), Refusal> {
        let bench = id.to_string();
        self.blocking(move |core| core.0.store.restore_bench(&bench).map_err(Refusal::from))
            .await?;
        self.0.turns.advance_bench(id).await;
        Ok(())
    }

    /// `DELETE /api/conversations/:id`: an archived chat deleted for good.
    pub async fn delete_conversation(&self, id: &str) -> Result<(), Refusal> {
        let id = id.to_string();
        self.blocking(move |core| core.0.store.delete_conversation(&id).map_err(Refusal::from))
            .await?;
        self.list_changed();
        Ok(())
    }

    /// `DELETE /api/benches/:id`: an archived bench deleted for good.
    pub async fn delete_bench(&self, id: &str) -> Result<(), Refusal> {
        let id = id.to_string();
        self.blocking(move |core| core.0.store.delete_bench(&id).map_err(Refusal::from))
            .await?;
        self.list_changed();
        Ok(())
    }

    /// `GET /api/search?scope=&q=`: `results`, each `{ id, snippet }`.
    pub async fn search(&self, scope: &str, query: &str) -> Result<Json, Refusal> {
        let scope = match scope {
            "build" => SearchScope::Build,
            "bench" => SearchScope::Bench,
            _ => return Err(refusal(400, "scope must be build or bench")),
        };
        let hits = self.search_hits(scope, query).await?;
        Ok(mcbench_store::object! { "results" => Json::Array(
            hits.iter()
                .map(|hit| mcbench_store::object! { "id" => hit.id.as_str(), "snippet" => hit.snippet.clone() })
                .collect(),
        ) })
    }

    /// `GET /api/search`'s `results`, typed.
    pub async fn search_hits(
        &self,
        scope: SearchScope,
        query: &str,
    ) -> Result<Vec<SearchHit>, Refusal> {
        let query = query.to_string();
        self.blocking(move |core| core.0.store.search(scope, &query).map_err(Refusal::from))
            .await
    }

    /// `GET /api/leaderboard`: `{ entries, votes }`.
    pub async fn leaderboard(&self) -> Result<Json, Refusal> {
        self.blocking(|core| {
            let entries = core.0.store.leaderboard()?;
            Ok(mcbench_store::object! {
                "entries" => Json::Array(entries.iter().map(|entry| mcbench_store::object! {
                    "provider" => entry.provider.as_str(), "model" => entry.model.as_str(),
                    "effort" => entry.effort.clone(), "wins" => entry.wins, "votes" => entry.votes,
                    "rating" => mcbench_store::json::number(entry.rating), "benches" => entry.benches,
                }).collect()),
                "votes" => core.0.store.vote_count()?,
            })
        })
        .await
    }

    /// `GET /api/leaderboard`, typed: its `votes` and its `entries` in the
    /// server's order.
    pub async fn leaderboard_board(&self) -> Result<mcbench_ui::Leaderboard, Refusal> {
        self.blocking(|core| {
            let votes = core.0.store.vote_count()?;
            Ok(mcbench_ui::Leaderboard {
                votes: u64::try_from(votes).expect("a count of votes is not negative"),
                entries: core.0.store.leaderboard()?,
            })
        })
        .await
    }

    /// `DELETE /api/votes`: every vote, blind or not; benches stay.
    pub async fn clear_votes(&self) -> Result<(), Refusal> {
        self.blocking(|core| core.0.store.clear_all_votes().map_err(Refusal::from))
            .await
    }
}
