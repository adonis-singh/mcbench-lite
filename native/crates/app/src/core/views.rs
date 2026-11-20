//! What the core reads out of the store for the window: the chat list
//! (`mcbench_agent::list`, re-exported here) and a chat's transcript
//! (`GET /api/conversations/:id`, the branch route's answer and
//! `turn_snapshot`), typed, each with its JSON in today's key order (the
//! JSON is what the differential compares with Node's route bodies).

use mcbench_agent::lifecycle::{Refusal, Turns};
pub use mcbench_agent::list::{ConversationList, ListedChat, conversation_list, list};
use mcbench_store::json::Json;
use mcbench_store::{ConversationHeader, MessageTree, Store, StoredMessage};

/// A chat's stored transcript: its header (with the running turn's
/// start), the messages of its active branch, and its whole branch tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Transcript {
    /// The chat.
    pub chat: String,
    /// GET `/api/conversations/:id`'s other fields.
    pub header: ConversationHeader,
    /// The active branch, oldest first.
    pub messages: Vec<StoredMessage>,
    /// Every message of every branch.
    pub tree: MessageTree,
}

/// The ONE function that makes a chat's transcript: blocking (the store).
pub fn transcript(store: &Store, turns: &Turns, id: &str) -> Result<Transcript, Refusal> {
    let live = store.conversation(id)?.ok_or_else(|| Refusal {
        status: 404,
        message: "Conversation not found".into(),
    })?;
    let conversation = live.lock().expect("a chat's lock");
    let mut header = conversation.header();
    header.running_since = turns.turn(id).map(|turn| turn.started_at());
    Ok(Transcript {
        chat: id.to_string(),
        tree: store.message_tree(&conversation)?,
        messages: conversation.messages.clone(),
        header,
    })
}

/// `messageTree(conversation)`'s object.
pub fn tree_json(tree: &MessageTree) -> Json {
    mcbench_store::object! {
        "headId" => tree.head_id.clone(),
        "messages" => Json::Array(tree.messages.iter().map(|m| mcbench_store::object! {
            "id" => m.id.as_str(), "parentId" => m.parent_id.clone(), "position" => m.position,
            "role" => m.role.clone(), "branchable" => m.branchable,
        }).collect()),
    }
}

/// `publicMessages(conversation)`.
pub fn public_messages(messages: &[StoredMessage]) -> Result<Json, String> {
    messages
        .iter()
        .map(|m| mcbench_json::parse(&m.public_text()))
        .collect::<Result<Vec<_>, _>>()
        .map(Json::Array)
}

impl Transcript {
    /// GET `/api/conversations/:id`'s body.
    pub fn to_json(&self) -> Result<Json, String> {
        let header = &self.header;
        Ok(mcbench_store::object! {
            "id" => header.id.as_str(), "title" => header.title.as_str(),
            "provider" => header.provider.as_str(), "model" => header.model.as_str(),
            "effort" => header.effort.clone(),
            "messages" => public_messages(&self.messages)?,
            "messageTree" => tree_json(&self.tree),
            "runningSince" => header.running_since,
        })
    }

    /// The branch route's body (`POST .../branch`).
    pub fn branch_json(&self) -> Result<Json, String> {
        Ok(mcbench_store::object! {
            "messages" => public_messages(&self.messages)?,
            "messageTree" => tree_json(&self.tree),
        })
    }

    /// The window's `Update::Transcript`: this struct's four fields.
    pub fn into_update(self) -> mcbench_ui::Update {
        mcbench_ui::Update::Transcript {
            chat: self.chat,
            header: self.header,
            messages: self.messages,
            tree: self.tree,
        }
    }
}

/// The window's `Update::Conversations`: the whole list.
pub fn list_update(list: ConversationList) -> mcbench_ui::Update {
    mcbench_ui::Update::Conversations(list)
}
