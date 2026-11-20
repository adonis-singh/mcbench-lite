//! The sidebar's chat list as the server gives it (`server/index.js`
//! `listPayload`, `conversationList`): the store's rows with what the
//! lifecycle adds (a bench chat's progress, the running turn's start),
//! typed, each with its JSON in today's key order. The interface takes
//! [`ConversationList`]; the core makes it with [`list`].

use mcbench_store::json::{Json, Object};
use mcbench_store::{ConversationSummary, ListedBench, Store};

use crate::lifecycle::{Refusal, Turns};

/// A chat of the list, with what the server adds to the store's row.
#[derive(Clone, Debug, PartialEq)]
pub struct ListedChat {
    /// The store's row.
    pub summary: ConversationSummary,
    /// A bench model's rounds sent and what holds its queue
    /// (`memberProgress`), for a bench's chat only.
    pub progress: Option<(i64, Option<&'static str>)>,
    /// When its running turn started (`runningSince`), epoch ms.
    pub running_since: Option<i64>,
}

/// The sidebar's list (`listPayload`).
#[derive(Clone, Debug, PartialEq)]
pub struct ConversationList {
    /// The chats, newest first.
    pub conversations: Vec<ListedChat>,
    /// `store.listBenches()`.
    pub benches: Vec<ListedBench>,
    /// `store.promptTitles()`.
    pub prompt_titles: Object,
    /// How many chats and benches are archived.
    pub archived: (usize, usize),
}

fn summary_json(row: &ConversationSummary) -> Object {
    let Json::Object(object) = (mcbench_store::object! {
        "id" => row.id.0.as_str(), "title" => row.title.as_str(), "provider" => row.provider.as_str(),
        "model" => row.model.as_str(), "effort" => row.effort.clone(), "resolution" => row.resolution,
        "createdAt" => row.created_at.as_str(), "benchId" => row.bench_id.clone(),
        "benchSlot" => row.bench_slot, "unread" => row.unread, "messageCount" => row.message_count,
        "prompts" => row.prompts.clone(),
    }) else {
        unreachable!("an object")
    };
    object
}

impl ListedChat {
    /// The row as `conversationList` gives it: the store's fields, a
    /// bench chat's `round` and `halted`, then `runningSince`.
    pub fn to_json(&self) -> Json {
        let mut object = summary_json(&self.summary);
        if let Some((round, halted)) = self.progress {
            object.insert("round".into(), round.into());
            object.insert("halted".into(), halted.map_or(Json::Null, Json::from));
        }
        object.insert(
            "runningSince".into(),
            self.running_since.map_or(Json::Null, Json::from),
        );
        Json::Object(object)
    }
}

impl ConversationList {
    /// `listPayload()`'s object.
    pub fn to_json(&self) -> Json {
        mcbench_store::object! {
            "conversations" => Json::Array(self.conversations.iter().map(ListedChat::to_json).collect()),
            "benches" => Json::Array(self.benches.iter().map(ListedBench::to_json).collect()),
            "promptTitles" => Json::Object(self.prompt_titles.clone()),
            "archived" => mcbench_store::object! { "conversations" => self.archived.0, "benches" => self.archived.1 },
        }
    }
}

/// `conversationList()`: blocking (the store).
pub fn conversation_list(store: &Store, turns: &Turns) -> Result<Vec<ListedChat>, Refusal> {
    store
        .list_conversations()?
        .into_iter()
        .map(|summary| {
            let id = summary.id.0.clone();
            Ok(ListedChat {
                progress: match summary.bench_id {
                    Some(_) => Some(turns.member_progress(&id)?),
                    None => None,
                },
                running_since: turns.turn(&id).map(|turn| turn.started_at()),
                summary,
            })
        })
        .collect()
}

/// `listPayload()`: blocking (the store).
pub fn list(store: &Store, turns: &Turns) -> Result<ConversationList, Refusal> {
    let archive = store.list_archive()?;
    Ok(ConversationList {
        conversations: conversation_list(store, turns)?,
        benches: store.list_benches()?,
        prompt_titles: store.prompt_titles()?,
        archived: (archive.conversations.len(), archive.benches.len()),
    })
}
