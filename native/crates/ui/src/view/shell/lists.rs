//! The list the core publishes (`Update::Conversations`: the chats with
//! their running turns, the benches, the prompts' titles, the archive's
//! counts), held once for every view: the sidebar's sections, the chat pane
//! and the window's title read it here. A view that shows a list observes
//! this entity. The rows come newest first and nothing reorders them.

use gpui_kit::*;

use crate::state::place::Titles;
use crate::{ConversationList, ConversationSummary, ListedBench, ListedChat};

/// The list.
pub struct Lists {
    published: ConversationList,
    /// The first list has arrived.
    pub arrived: bool,
}

impl Default for Lists {
    fn default() -> Self {
        Lists {
            published: ConversationList {
                conversations: Vec::new(),
                benches: Vec::new(),
                prompt_titles: Default::default(),
                archived: (0, 0),
            },
            arrived: false,
        }
    }
}

impl Lists {
    /// Every listed chat with its running turn, bench models' included.
    pub fn all(&self) -> &[ListedChat] {
        &self.published.conversations
    }

    /// The Build chats (not a bench model's), newest first.
    pub fn chats(&self) -> Vec<&ConversationSummary> {
        self.published
            .conversations
            .iter()
            .map(|c| &c.summary)
            .filter(|c| c.bench_id.is_none())
            .collect()
    }

    /// A listed chat's row.
    pub fn chat(&self, id: &str) -> Option<&ConversationSummary> {
        self.published
            .conversations
            .iter()
            .map(|c| &c.summary)
            .find(|c| c.id.0 == id)
    }

    /// The listed benches.
    pub fn benches(&self) -> &[ListedBench] {
        &self.published.benches
    }

    /// The whole list as published.
    pub fn published(&self) -> &ConversationList {
        &self.published
    }

    /// A new list.
    pub fn set(&mut self, list: ConversationList, cx: &mut Context<Self>) {
        self.published = list;
        self.arrived = true;
        cx.notify();
    }
}

impl Titles for Lists {
    fn chat(&self, id: &str) -> Option<String> {
        Lists::chat(self, id).map(|c| c.title.clone())
    }
    fn bench(&self, id: &str) -> Option<String> {
        self.benches()
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.title.clone())
    }
    /// A member's model, or its Build letter while the bench hides names
    /// (`memberName`, `benchLetter`: `A` plus its index in the bench's
    /// order).
    fn member(&self, bench: &str, member: &str) -> Option<String> {
        let row = Lists::chat(self, member)?;
        let bench = self.benches().iter().find(|b| b.id == bench)?;
        if bench.blind && !bench.revealed {
            let index = bench.order.iter().position(|m| m == member)?;
            Some(format!("Build {}", (b'A' + index as u8) as char))
        } else {
            Some(row.model.clone())
        }
    }
}
