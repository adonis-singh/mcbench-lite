//! Chats: loading, appending, saving, branches, turn ends, forks
//! (`server/store.js`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rusqlite::{OptionalExtension, params};

use crate::json::Truthy;
use crate::json::{self, Json};
use crate::payload::{self, Content, Message, NewMessage, Part};
use crate::sql::LINK_PROMPT;
use crate::world::{self, chunk_path};
use crate::{
    BenchId, Conversation, ConversationId, ConversationSummary, DEFAULT_TITLE, Error, INTERRUPTED,
    LiveConversation, MessageId, Store, StoredMessage, iso,
};
use mcbench_world::{ChunkCache, World, WriteChunk};

/// Everything GET `/api/conversations/:id` gives about a chat besides its
/// messages and tree, with what the interface needs beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationHeader {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `provider`.
    pub provider: String,
    /// `model`.
    pub model: String,
    /// `effort` (`conversation.effort || null`).
    pub effort: Option<String>,
    /// `resolution`.
    pub resolution: u32,
    /// The bench and slot when the chat is a bench model's.
    pub bench: Option<(String, u32)>,
    /// `runningSince`: when the running turn started, epoch ms (a number,
    /// as Node gives it). The store does not
    /// know it (`reservations.turn(id)?.startedAt` in `server/index.js`): the
    /// core fills it; the store gives `None`.
    pub running_since: Option<i64>,
    /// Whether the chat is shared, and where. The store does not hold it
    /// (`server/share.js` keeps it in the share site's repository): the core
    /// fills it; the store gives `None`.
    pub share: Option<String>,
}

/// What a new chat starts with (`newConversation`'s options).
#[derive(Clone, Debug)]
pub struct NewConversation {
    /// `title` (`New build`).
    pub title: String,
    /// `effort`.
    pub effort: Option<String>,
    /// `resolution` (1).
    pub resolution: u32,
    /// `benchId` and `benchSlot`.
    pub bench: Option<(BenchId, u32)>,
}

impl Default for NewConversation {
    fn default() -> NewConversation {
        NewConversation {
            title: DEFAULT_TITLE.to_string(),
            effort: None,
            resolution: 1,
            bench: None,
        }
    }
}

/// A chat's branch tree (`messageTree`).
#[derive(Clone, Debug, PartialEq)]
pub struct MessageTree {
    /// `headId`.
    pub head_id: Option<String>,
    /// Every message on every branch, by position.
    pub messages: Vec<TreeMessage>,
}

/// A message of the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeMessage {
    /// `id`.
    pub id: String,
    /// `parentId`.
    pub parent_id: Option<String>,
    /// `position`.
    pub position: i64,
    /// `role`.
    pub role: Option<String>,
    /// A prompt the user sent, which an edit branches from.
    pub branchable: bool,
}

/// A row of `messages`.
pub(crate) struct MessageRow {
    pub(crate) id: String,
    pub(crate) parent_id: Option<String>,
    pub(crate) position: u32,
    pub(crate) payload_json: String,
    pub(crate) checkpoint_manifest_json: Option<String>,
    pub(crate) world_manifest_json: Option<String>,
}

/// A loaded branch (`loadBranch`).
pub(crate) struct Branch {
    messages: Vec<StoredMessage>,
    current_message_id: Option<MessageId>,
    next_message_position: u32,
    world: World,
    checkpoints: HashMap<MessageId, World>,
}

/// JavaScript's `text.trim()` is not empty.
fn has_text(text: &str) -> bool {
    !text.trim_matches(crate::storage::js_space).is_empty()
}

/// A row's value as JavaScript's truthiness reads it.
fn sql_truthy(value: &rusqlite::types::Value) -> bool {
    use rusqlite::types::Value;
    match value {
        Value::Null => false,
        Value::Integer(n) => *n != 0,
        Value::Real(n) => *n != 0.0 && !n.is_nan(),
        Value::Text(text) => !text.is_empty(),
        Value::Blob(_) => true,
    }
}

impl Conversation {
    /// Whether `message` has a world checkpoint.
    pub fn checkpoint(&self, message: &MessageId) -> Option<&World> {
        self.checkpoints.get(message)
    }

    /// The prompts the user sent on this branch (`prompts(conversation)`).
    pub fn prompts(&self) -> Vec<&StoredMessage> {
        self.messages
            .iter()
            .filter(|message| message.message.is_prompt())
            .collect()
    }

    /// The chat's header (GET `/api/conversations/:id`'s fields besides
    /// `messages` and `messageTree`).
    pub fn header(&self) -> ConversationHeader {
        ConversationHeader {
            id: self.id.0.clone(),
            title: self.title.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            effort: self.effort.clone().filter(|effort| !effort.is_empty()),
            resolution: self.resolution,
            bench: self
                .bench
                .as_ref()
                .map(|(bench, slot)| (bench.0.clone(), *slot)),
            running_since: None,
            share: None,
        }
    }
}

impl StoredMessage {
    /// A message decoded from its row's text; the error names the chat and
    /// the message.
    pub(crate) fn decode(
        chat: &str,
        id: &str,
        parent: Option<&str>,
        position: u32,
        text: String,
    ) -> Result<StoredMessage, Error> {
        let message = payload::decode(&text, &format!("Conversation {chat} message {id}"))?;
        Ok(StoredMessage {
            id: MessageId(id.to_string()),
            parent: parent.map(|parent| MessageId(parent.to_string())),
            position,
            message,
            text,
        })
    }
}

/// `path.basename(name, '.json')`.
fn strip_json(name: &str) -> &str {
    match name.strip_suffix(".json") {
        Some(stem) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// `fs.rmSync(path, { recursive: true, force: true })`.
pub(crate) fn remove_all(path: &Path) -> Result<(), Error> {
    let result = match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(error) => Err(error),
    };
    match result {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// `fs.cpSync(from, to, { recursive: true })`.
fn copy_tree(from: &Path, to: &Path) -> Result<(), Error> {
    let metadata = std::fs::symlink_metadata(from)?;
    if metadata.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(from, to)?;
    }
    Ok(())
}

impl Store {
    pub(crate) fn message_rows(&self, id: &str) -> Result<Vec<MessageRow>, Error> {
        self.with(|inner| {
            let mut statement = inner.db.prepare_cached(
                "
    SELECT * FROM messages WHERE conversation_id = ? ORDER BY position
  ",
            )?;
            let rows = statement
                .query_map([id], |row| {
                    Ok(MessageRow {
                        id: row.get("id")?,
                        parent_id: row.get("parent_id")?,
                        position: row.get("position")?,
                        payload_json: row.get("payload_json")?,
                        checkpoint_manifest_json: row.get("checkpoint_manifest_json")?,
                        world_manifest_json: row.get("world_manifest_json")?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// `activeMessageRows`: the rows from the first message to `head`.
    fn active_rows<'a>(
        id: &str,
        rows: &'a [MessageRow],
        head: Option<&str>,
    ) -> Result<Vec<&'a MessageRow>, Error> {
        let by_id: HashMap<&str, &MessageRow> =
            rows.iter().map(|row| (row.id.as_str(), row)).collect();
        let mut active = Vec::new();
        let mut seen = HashSet::new();
        let mut current = head;
        while let Some(at) = current.filter(|at| !at.is_empty()) {
            if !seen.insert(at) {
                return Err(Error::plain(format!(
                    "Conversation {id} contains a message cycle"
                )));
            }
            let row = by_id.get(at).ok_or_else(|| {
                Error::plain(format!("Conversation {id} references missing message {at}"))
            })?;
            active.push(*row);
            current = row.parent_id.as_deref();
        }
        active.reverse();
        Ok(active)
    }

    /// `loadBranch(id, messageRows, headId, worldManifestJson)`.
    pub(crate) fn load_branch(
        worlds: &Path,
        id: &str,
        rows: &[MessageRow],
        head: Option<&str>,
        world_json: &str,
    ) -> Result<Branch, Error> {
        let active = Self::active_rows(id, rows, head)?;
        let mut cache = ChunkCache::new();
        let world = world::load(
            worlds,
            id,
            world_json,
            &format!("Conversation {id} world"),
            &mut cache,
        )?;
        let mut checkpoints = HashMap::new();
        let mut messages = Vec::with_capacity(active.len());
        for row in active {
            let message = StoredMessage::decode(
                id,
                &row.id,
                row.parent_id.as_deref(),
                row.position,
                row.payload_json.clone(),
            )?;
            if let Some(manifest) = &row.checkpoint_manifest_json {
                let checkpoint = world::load(
                    worlds,
                    id,
                    manifest,
                    &format!("Conversation {id} checkpoint {}", row.id),
                    &mut cache,
                )?;
                checkpoints.insert(MessageId(row.id.clone()), checkpoint);
            }
            messages.push(message);
        }
        Ok(Branch {
            messages,
            current_message_id: head
                .filter(|head| !head.is_empty())
                .map(|head| MessageId(head.to_string())),
            next_message_position: rows.last().map_or(0, |row| row.position + 1),
            world,
            checkpoints,
        })
    }

    fn apply_branch(conversation: &mut Conversation, branch: Branch) {
        conversation.messages = branch.messages;
        conversation.current_message_id = branch.current_message_id;
        conversation.next_message_position = branch.next_message_position;
        conversation.world = branch.world;
        conversation.checkpoints = branch.checkpoints;
        conversation.unsaved = HashSet::new();
    }

    /// `newConversation(provider, model, options)`: a chat not yet stored.
    pub(crate) fn new_conversation(
        &self,
        provider: &str,
        model: &str,
        options: NewConversation,
    ) -> Result<Conversation, Error> {
        if provider.is_empty() || model.is_empty() {
            return Err(Error::plain("Conversation provider and model are required"));
        }
        let id = self.host.uuid();
        Ok(Conversation {
            id: ConversationId(id),
            title: options.title,
            provider: provider.to_string(),
            model: model.to_string(),
            effort: options.effort,
            resolution: options.resolution,
            bench: options.bench,
            created_at: iso(self.now()),
            messages: Vec::new(),
            current_message_id: None,
            next_message_position: 0,
            world: World::new(),
            checkpoints: HashMap::new(),
            unsaved: HashSet::new(),
            token: 0,
        })
    }

    /// Puts `conversation` in `live` under a new token (`live.set`).
    pub(crate) fn make_live(&self, mut conversation: Conversation) -> LiveConversation {
        conversation.token = self.new_token();
        let token = conversation.token;
        let id = conversation.id.0.clone();
        let live = Arc::new(Mutex::new(conversation));
        self.with(|inner| inner.live.insert(id, (token, Arc::clone(&live))));
        live
    }

    /// `createConversation(provider, model, options)`: a new chat, stored.
    pub fn create_conversation(
        &self,
        provider: &str,
        model: &str,
        options: NewConversation,
    ) -> Result<LiveConversation, Error> {
        let conversation = self.new_conversation(provider, model, options)?;
        let live = self.make_live(conversation);
        self.save(&mut live.lock().expect("a new chat's lock"))?;
        Ok(live)
    }

    /// `getConversation(id)`: the chat as loaded, or `None` when it is not
    /// stored or is archived.
    pub fn conversation(&self, id: &str) -> Result<Option<LiveConversation>, Error> {
        self.locked(|| {
            if let Some((_, live)) = self.with(|inner| inner.live.get(id).cloned()) {
                return Ok(Some(live));
            }
            let row = self.with(|inner| {
                inner
                    .db
                    .prepare_cached(
                        "
    SELECT * FROM conversations WHERE id = ? AND archived_at IS NULL
  ",
                    )?
                    .query_row([id], |row| {
                        Ok((
                            row.get::<_, String>("id")?,
                            row.get::<_, String>("title")?,
                            row.get::<_, String>("provider")?,
                            row.get::<_, String>("model")?,
                            row.get::<_, Option<String>>("effort")?,
                            row.get::<_, u32>("resolution")?,
                            row.get::<_, Option<String>>("bench_id")?,
                            row.get::<_, Option<u32>>("bench_slot")?,
                            row.get::<_, String>("created_at")?,
                            row.get::<_, Option<String>>("current_message_id")?,
                            row.get::<_, String>("world_manifest_json")?,
                        ))
                    })
                    .optional()
            })?;
            let Some((
                row_id,
                title,
                provider,
                model,
                effort,
                resolution,
                bench_id,
                bench_slot,
                created_at,
                head,
                world_json,
            )) = row
            else {
                return Ok(None);
            };
            let rows = self.message_rows(id)?;
            let branch = Self::load_branch(
                &self.folder.worlds(),
                id,
                &rows,
                head.as_deref(),
                &world_json,
            )?;
            // Measured now, so its first write finds the measurement in.
            drop(self.storage.measure(id));
            let mut conversation = Conversation {
                id: ConversationId(row_id),
                title,
                provider,
                model,
                effort: effort.filter(|effort| !effort.is_empty()),
                resolution,
                bench: bench_id.map(|bench| (BenchId(bench), bench_slot.unwrap_or(0))),
                created_at,
                messages: Vec::new(),
                current_message_id: None,
                next_message_position: 0,
                world: World::new(),
                checkpoints: HashMap::new(),
                unsaved: HashSet::new(),
                token: 0,
            };
            Self::apply_branch(&mut conversation, branch);
            Ok(Some(self.make_live(conversation)))
        })
    }

    /// `appendMessage(conversation, payload, checkpoint)`: adds a message at
    /// the head, not yet saved, and returns it. `createdAt` follows the
    /// payload's keys.
    pub fn append_message(
        &self,
        conversation: &mut Conversation,
        message: NewMessage,
        checkpoint: Option<World>,
    ) -> Result<StoredMessage, Error> {
        let mut object = message.0;
        if !json::truthy(object.get("createdAt")) {
            object.insert("createdAt".into(), Json::String(iso(self.now())));
        }
        let id = self.host.uuid();
        let text = json::stringify(&Json::Object(object));
        let message = StoredMessage::decode(
            &conversation.id.0,
            &id,
            conversation
                .current_message_id
                .as_ref()
                .map(|m| m.0.as_str()),
            conversation.next_message_position,
            text,
        )?;
        let id = message.id.clone();
        conversation.next_message_position += 1;
        conversation.current_message_id = Some(id.clone());
        conversation.messages.push(message.clone());
        conversation.unsaved.insert(id.clone());
        if let Some(checkpoint) = checkpoint {
            conversation.checkpoints.insert(id, checkpoint);
        }
        Ok(message)
    }

    /// `dismissError(conversation, messageId)`: `dismissed: true` follows
    /// the row's keys, as `message.dismissed = true` and `JSON.stringify`
    /// write it.
    pub fn dismiss_error(
        &self,
        conversation: &mut Conversation,
        message_id: &str,
    ) -> Result<(), Error> {
        let refused = || Error::refused("messageId must point at an error message", 400);
        let message = conversation
            .messages
            .iter_mut()
            .find(|message| message.id.0 == message_id)
            .ok_or_else(refused)?;
        let Message::Error(error) = &mut message.message else {
            return Err(refused());
        };
        if !error.dismissed {
            error.dismissed = true;
            message.text = payload::splice(&message.text, &[("dismissed", &Json::Bool(true))]);
        }
        let id = message.id.clone();
        conversation.unsaved.insert(id);
        self.save(conversation)
    }

    /// Writes `world`'s chunks the store does not hold, counting each new
    /// file against chat `id`'s budget (the disk's reserve at the first), and
    /// returns the manifest. `written` holds what was added.
    pub(crate) fn persist_into(
        &self,
        id: &str,
        world: &World,
        written: &mut Vec<(PathBuf, u64)>,
        manifest: bool,
    ) -> Result<String, Error> {
        let worlds = self.folder.worlds();
        let mut failure = None;
        let result = {
            let mut write = |hash: &str, bytes: &[u8]| -> Result<(), String> {
                let file = chunk_path(&worlds, id, hash);
                if file.exists() {
                    return Ok(());
                }
                // Pairs of integers: the JSON is ASCII, a byte a character.
                if let Err(error) = self
                    .storage
                    .reserve(id, bytes.len() as u64, written.is_empty())
                {
                    let message = error.message.clone();
                    failure = Some(error);
                    return Err(message);
                }
                written.push((file.clone(), bytes.len() as u64));
                let io = || -> std::io::Result<()> {
                    std::fs::create_dir_all(file.parent().expect("a chunk's folder"))?;
                    let temporary =
                        PathBuf::from(format!("{}.{}.tmp", file.display(), std::process::id()));
                    std::fs::write(&temporary, bytes)?;
                    std::fs::rename(&temporary, &file)
                };
                io().map_err(|error| error.to_string())
            };
            let write: &mut WriteChunk<'_> = &mut write;
            if manifest {
                world.persist(write).map(|manifest| manifest.0)
            } else {
                world.persist_chunks(write).map(|()| String::new())
            }
        };
        match result {
            Ok(manifest) => Ok(manifest),
            Err(message) => Err(failure.unwrap_or_else(|| Error::plain(message))),
        }
    }

    /// `discardChunks`: removes and refunds the chunk files a failed write
    /// added.
    pub(crate) fn discard_chunks<'a>(
        &self,
        id: &str,
        written: &[(PathBuf, u64)],
        worlds: impl IntoIterator<Item = &'a World>,
    ) {
        let mut hashes = HashSet::new();
        for (file, bytes) in written {
            let name = file
                .file_name()
                .expect("a chunk file")
                .to_string_lossy()
                .into_owned();
            hashes.insert(strip_json(&name).to_string());
            // Gone already is what is wanted.
            let _ = std::fs::remove_file(file);
            self.storage.refund(id, *bytes);
        }
        // The chunks they held are no longer stored.
        world::forget_stored(worlds, &hashes);
    }

    /// `persistWorld(conversationId, world)`: writes the chunk files of a
    /// build about to be applied, so one that does not fit fails as the
    /// build's own result.
    pub fn persist_world(&self, id: &str, world: &World) -> Result<(), Error> {
        let mut written = Vec::new();
        match self.persist_into(id, world, &mut written, false) {
            Ok(_) => Ok(()),
            Err(error) => {
                self.discard_chunks(id, &written, [world]);
                Err(error)
            }
        }
    }

    pub(crate) fn index_doc(&self, owner: &str, source: &str, body: &str) -> Result<(), Error> {
        self.with(|inner| {
            inner
                .db
                .prepare_cached(
                    "
  INSERT INTO search_docs (owner, source, body) VALUES (?, ?, ?)
  ON CONFLICT(source) DO UPDATE SET body = excluded.body WHERE body != excluded.body
",
                )?
                .execute(params![owner, source, body])?;
            Ok(())
        })
    }

    /// `save(conversation)`: writes the chat's row, the head's world, and
    /// the messages appended or changed since the last save. A save that
    /// fails drops the chat from `live`, so the next load reads what is
    /// stored, and this object can never be saved again.
    pub fn save(&self, conversation: &mut Conversation) -> Result<(), Error> {
        self.locked(|| {
            let id = conversation.id.0.clone();
            let loaded = self.with(|inner| inner.live.get(&id).map(|(token, _)| *token));
            if loaded != Some(conversation.token) {
                return Err(Error::plain(format!(
                    "Conversation {id} is no longer loaded (archived, or dropped by a failed save)"
                )));
            }
            let mut written = Vec::new();
            match self.save_rows(conversation, &mut written) {
                Ok(()) => {
                    conversation.unsaved.clear();
                    Ok(())
                }
                Err(error) => {
                    self.discard_chunks(
                        &id,
                        &written,
                        std::iter::once(&conversation.world)
                            .chain(conversation.checkpoints.values()),
                    );
                    self.with(|inner| inner.live.remove(&id));
                    Err(error)
                }
            }
        })
    }

    fn save_rows(
        &self,
        conversation: &Conversation,
        written: &mut Vec<(PathBuf, u64)>,
    ) -> Result<(), Error> {
        let id = conversation.id.0.as_str();
        let world_json = self.persist_into(id, &conversation.world, written, true)?;
        let head = conversation
            .messages
            .last()
            .map(|message| message.id.0.clone());
        if conversation
            .current_message_id
            .as_ref()
            .map(|m| m.0.clone())
            != head
        {
            return Err(Error::plain(format!(
                "Conversation {id} has an invalid active head"
            )));
        }
        struct Row<'a> {
            message: &'a StoredMessage,
            checkpoint: Option<String>,
        }
        let mut rows = Vec::new();
        for message in &conversation.messages {
            if !conversation.unsaved.contains(&message.id) {
                continue;
            }
            let checkpoint = match conversation.checkpoints.get(&message.id) {
                Some(world) => Some(self.persist_into(id, world, written, true)?),
                None => None,
            };
            rows.push(Row {
                message,
                checkpoint,
            });
        }
        self.transaction(|| {
            let changes = self.with(|inner| {
                inner
                    .db
                    .prepare_cached(
                        "
  INSERT INTO conversations (
    id, title, provider, model, effort, resolution, created_at,
    current_message_id, world_manifest_json, archived_at, bench_id, bench_slot
  ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)
  ON CONFLICT(id) DO UPDATE SET
    title = excluded.title,
    provider = excluded.provider,
    model = excluded.model,
    effort = excluded.effort,
    resolution = excluded.resolution,
    current_message_id = excluded.current_message_id,
    world_manifest_json = excluded.world_manifest_json
  WHERE conversations.archived_at IS NULL
",
                    )?
                    .execute(params![
                        id,
                        conversation.title,
                        conversation.provider,
                        conversation.model,
                        conversation
                            .effort
                            .as_deref()
                            .filter(|effort| !effort.is_empty()),
                        conversation.resolution,
                        conversation.created_at,
                        conversation
                            .current_message_id
                            .as_ref()
                            .map(|m| m.0.as_str()),
                        world_json,
                        conversation
                            .bench
                            .as_ref()
                            .map(|(bench, _)| bench.0.as_str()),
                        conversation.bench.as_ref().map(|(_, slot)| *slot),
                    ])
            })?;
            if changes != 1 {
                return Err(Error::plain(format!("Conversation {id} is archived")));
            }
            for row in &rows {
                let message = row.message;
                let changes = self.with(|inner| {
                    inner
                        .db
                        .prepare_cached(
                            "
  INSERT INTO messages (
    id, conversation_id, parent_id, position, payload_json, checkpoint_manifest_json
  ) VALUES (?, ?, ?, ?, ?, ?)
  ON CONFLICT(id) DO UPDATE SET
    payload_json = excluded.payload_json,
    checkpoint_manifest_json = excluded.checkpoint_manifest_json
  WHERE messages.conversation_id = excluded.conversation_id
",
                        )?
                        .execute(params![
                            message.id.0,
                            id,
                            message.parent.as_ref().map(|p| p.0.as_str()),
                            message.position,
                            message.text,
                            row.checkpoint,
                        ])
                })?;
                if changes != 1 {
                    return Err(Error::plain(format!(
                        "Message {} belongs to another conversation",
                        message.id.0
                    )));
                }
                let text = message.message.search_text();
                if has_text(&text) {
                    self.index_doc(id, &format!("message:{}", message.id.0), &text)?;
                }
                for link in message.message.prompts() {
                    self.with(|inner| {
                        inner.db.prepare_cached(LINK_PROMPT)?.execute(params![
                            message.id.0,
                            link.id,
                            link.title
                        ])?;
                        Ok::<_, Error>(())
                    })?;
                }
            }
            self.index_doc(id, &format!("title:{id}"), &conversation.title)?;
            if let Some(head) = &head {
                self.with(|inner| {
                    inner
                        .db
                        .prepare_cached(
                            "
  UPDATE messages SET world_manifest_json = ? WHERE id = ? AND conversation_id = ?
",
                        )?
                        .execute(params![world_json, head, id])?;
                    Ok::<_, Error>(())
                })?;
            }
            Ok(())
        })
    }

    /// `revert(conversation, messageId)`: the chat goes back to before
    /// prompt `messageId`, with the world checkpointed then, and is saved.
    /// Returns the prompt.
    pub fn revert(
        &self,
        conversation: &mut Conversation,
        message_id: &str,
    ) -> Result<StoredMessage, Error> {
        let index = conversation
            .messages
            .iter()
            .position(|message| message.id.0 == message_id);
        let target = match index {
            Some(index) => conversation.messages[index].clone(),
            None => {
                return Err(Error::refused(
                    "messageId must point at a user message",
                    400,
                ));
            }
        };
        if !target.message.is_prompt() {
            return Err(Error::refused(
                "messageId must point at a user message",
                400,
            ));
        }
        let checkpoint = conversation
            .checkpoints
            .get(&target.id)
            .cloned()
            .ok_or_else(|| Error::refused("No world checkpoint for this message", 400))?;
        let index = index.expect("found above");
        conversation.messages.truncate(index);
        conversation.current_message_id = target.parent.clone();
        conversation.world = checkpoint.reference();
        let kept: HashSet<MessageId> = conversation
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect();
        conversation.checkpoints.retain(|id, _| kept.contains(id));
        conversation.unsaved.retain(|id| kept.contains(id));
        self.save(conversation)?;
        Ok(target)
    }

    /// `editPrompt(conversation, messageId, append)`: the chat goes back to
    /// before prompt `messageId` and `append` adds the new version, stored
    /// whole or not at all. One that fails drops the chat from `live`.
    pub fn edit_prompt(
        &self,
        conversation: &mut Conversation,
        message_id: &str,
        append: impl FnOnce(&mut Conversation) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let result = self.transaction(|| {
            self.revert(conversation, message_id)?;
            append(conversation)
        });
        if result.is_err() {
            self.with(|inner| inner.live.remove(&conversation.id.0));
        }
        result
    }

    /// `messageTree(conversation)`.
    pub fn message_tree(&self, conversation: &Conversation) -> Result<MessageTree, Error> {
        let messages = self.with(|inner| {
            let mut statement = inner.db.prepare_cached(
                "
    SELECT id, parent_id, position,
      json_extract(payload_json, '$.role') AS role,
      json_extract(payload_json, '$.seeBuild') AS see_build
    FROM messages WHERE conversation_id = ? ORDER BY position
  ",
            )?;
            let rows = statement
                .query_map([&conversation.id.0], |row| {
                    let role: rusqlite::types::Value = row.get("role")?;
                    let see_build: rusqlite::types::Value = row.get("see_build")?;
                    let branchable = matches!(&role, rusqlite::types::Value::Text(text) if text == "user") && !sql_truthy(&see_build);
                    Ok(TreeMessage {
                        id: row.get("id")?,
                        parent_id: row.get("parent_id")?,
                        position: row.get("position")?,
                        role: match role {
                            rusqlite::types::Value::Text(text) => Some(text),
                            _ => None,
                        },
                        branchable,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, Error>(rows)
        })?;
        Ok(MessageTree {
            head_id: conversation
                .current_message_id
                .as_ref()
                .map(|m| m.0.clone()),
            messages,
        })
    }

    /// `activateBranch(conversation, messageId)`: the latest branch through
    /// message `messageId` becomes the active one.
    pub fn activate_branch(
        &self,
        conversation: &mut Conversation,
        message_id: &str,
    ) -> Result<(), Error> {
        let rows = self.message_rows(&conversation.id.0)?;
        let mut head = rows
            .iter()
            .find(|row| row.id == message_id)
            .ok_or_else(|| Error::refused("Unknown message branch", 400))?;
        let mut children: HashMap<Option<&str>, Vec<&MessageRow>> = HashMap::new();
        for row in &rows {
            children
                .entry(row.parent_id.as_deref())
                .or_default()
                .push(row);
        }
        let mut seen = HashSet::new();
        while let Some(next) = children.get(&Some(head.id.as_str())) {
            if !seen.insert(head.id.as_str()) {
                return Err(Error::plain(format!(
                    "Conversation {} contains a message cycle",
                    conversation.id.0
                )));
            }
            head = next.last().expect("a child");
        }
        let head_id = head.id.clone();
        self.move_head(conversation, &rows, &head_id)
    }

    /// `followRounds(conversation, roundIds)`: a bench chat goes on the
    /// branch that answered `round_ids` in order, as far as it got.
    pub(crate) fn follow_rounds(
        &self,
        conversation: &mut Conversation,
        round_ids: &[String],
    ) -> Result<(), Error> {
        let rows = self.message_rows(&conversation.id.0)?;
        let mut payloads = HashMap::new();
        for row in &rows {
            let label = format!("Conversation {} message {}", conversation.id.0, row.id);
            payloads.insert(row.id.as_str(), payload::decode(&row.payload_json, &label)?);
        }
        let is_prompt = |row: &MessageRow| payloads[row.id.as_str()].is_prompt();
        let round_of = |row: &MessageRow| match &payloads[row.id.as_str()] {
            Message::User(user) => user.round.as_deref(),
            _ => None,
        };
        let mut children: HashMap<Option<&str>, Vec<&MessageRow>> = HashMap::new();
        for row in &rows {
            children
                .entry(row.parent_id.as_deref())
                .or_default()
                .push(row);
        }
        let none = Vec::new();
        let mut head: Option<&MessageRow> = None;
        for round in round_ids {
            let siblings = children
                .get(&head.map(|row| row.id.as_str()))
                .unwrap_or(&none);
            let prompt = siblings
                .iter()
                .rfind(|row| is_prompt(row) && round_of(row) == Some(round.as_str()));
            let Some(prompt) = prompt else { break };
            let mut next = Some(*prompt);
            while let Some(at) = next {
                head = Some(at);
                next = children
                    .get(&Some(at.id.as_str()))
                    .unwrap_or(&none)
                    .iter()
                    .rfind(|row| !is_prompt(row))
                    .copied();
            }
        }
        if let Some(head) = head {
            let head_id = head.id.clone();
            return self.move_head(conversation, &rows, &head_id);
        }
        let first = conversation
            .prompts()
            .first()
            .map(|message| message.id.0.clone());
        if let Some(first) = first {
            self.revert(conversation, &first)?;
        }
        Ok(())
    }

    /// `moveHead(conversation, messageRows, head)`.
    fn move_head(
        &self,
        conversation: &mut Conversation,
        rows: &[MessageRow],
        head_id: &str,
    ) -> Result<(), Error> {
        let head = rows
            .iter()
            .find(|row| row.id == head_id)
            .expect("the head is a row");
        let Some(world_json) = head
            .world_manifest_json
            .as_deref()
            .filter(|json| !json.is_empty())
        else {
            return Err(Error::refused(
                "Selected branch has no saved world state",
                400,
            ));
        };
        let branch = Self::load_branch(
            &self.folder.worlds(),
            &conversation.id.0,
            rows,
            Some(head_id),
            world_json,
        )?;
        Self::apply_branch(conversation, branch);
        self.save(conversation)
    }

    /// `setTitle(conversation, title)`: stores just the title, so a turn's
    /// unfinished work is not saved with it.
    pub fn set_title(&self, conversation: &mut Conversation, title: &str) -> Result<(), Error> {
        let id = conversation.id.0.clone();
        let changes = self.with(|inner| {
            inner
                .db
                .prepare_cached(
                    "
    UPDATE conversations SET title = ? WHERE id = ? AND archived_at IS NULL
  ",
                )?
                .execute(params![title, id])
        })?;
        if changes != 1 {
            return Err(Error::plain(format!(
                "Conversation {id} not found or archived"
            )));
        }
        self.index_doc(&id, &format!("title:{id}"), title)?;
        conversation.title = title.to_string();
        Ok(())
    }

    /// `repeatsLatestRootPrompt(conversation, content)`: a first prompt
    /// resent unchanged (the same text and attachment bytes).
    pub fn repeats_latest_root_prompt(
        &self,
        conversation: &Conversation,
        content: &Content,
    ) -> Result<bool, Error> {
        let row: Option<String> = self.with(|inner| {
            inner
                .db
                .prepare_cached(
                    "
    SELECT payload_json FROM messages
    WHERE conversation_id = ? AND parent_id IS NULL AND json_extract(payload_json, '$.role') = 'user'
    ORDER BY position DESC LIMIT 1
  ",
                )?
                .query_row([&conversation.id.0], |row| row.get(0))
                .optional()
        })?;
        let Some(stored) = row else { return Ok(false) };
        let signature = |content: &Content| -> Result<String, Error> {
            let parts = match content {
                Content::Text(text) => vec![Part::Text { text: text.clone() }],
                Content::Parts(parts) => parts.clone(),
            };
            let mut signed = Vec::new();
            for part in parts {
                match part {
                    Part::Attachment { attachment } => {
                        let bytes = self.read_attachment(&attachment.url)?;
                        signed.push(crate::object! {
                            "name" => attachment.name.as_str(),
                            "type" => attachment.mime.as_str(),
                            "sha256" => crate::files::sha256_hex(&bytes),
                        });
                    }
                    part => signed.push(payload::to_json(&part)),
                }
            }
            Ok(json::stringify(&Json::Array(signed)))
        };
        let stored = match payload::decode(&stored, "message payload")? {
            Message::User(user) => user.content,
            _ => {
                return Err(Error::plain(
                    "message payload: the latest root prompt is not a user message",
                ));
            }
        };
        Ok(signature(&stored)? == signature(content)?)
    }

    /// `markFinished(id)`: a turn finished after the user last looked.
    pub fn mark_finished(&self, id: &str) -> Result<(), Error> {
        let now = self.now();
        self.with(|inner| {
            inner
                .db
                .prepare_cached("UPDATE conversations SET finished_at = ? WHERE id = ?")?
                .execute(params![now, id])?;
            Ok(())
        })
    }

    /// `markSeen(id)`.
    pub fn mark_seen(&self, id: &str) -> Result<(), Error> {
        let now = self.now();
        self.with(|inner| {
            inner
                .db
                .prepare_cached("UPDATE conversations SET seen_at = ? WHERE id = ?")?
                .execute(params![now, id])?;
            Ok(())
        })
    }

    /// `listConversationIds()`: every chat stored, with whether it is
    /// archived.
    pub fn list_conversation_ids(&self) -> Result<Vec<(String, bool)>, Error> {
        self.ids("SELECT id, archived_at IS NOT NULL AS archived FROM conversations")
    }

    /// `listBenchIds()`.
    pub fn list_bench_ids(&self) -> Result<Vec<(String, bool)>, Error> {
        self.ids("SELECT id, archived_at IS NOT NULL AS archived FROM benches")
    }

    fn ids(&self, sql: &str) -> Result<Vec<(String, bool)>, Error> {
        self.with(|inner| {
            let mut statement = inner.db.prepare_cached(sql)?;
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// `listConversations()`: every chat not archived that has a message,
    /// newest first.
    pub fn list_conversations(&self) -> Result<Vec<ConversationSummary>, Error> {
        self.with(|inner| {
            let mut prompts: HashMap<String, Vec<String>> = HashMap::new();
            let mut statement = inner.db.prepare_cached(
                "
  SELECT DISTINCT m.conversation_id, p.prompt_id FROM message_prompts p JOIN messages m ON m.id = p.message_id
",
            )?;
            for pair in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))? {
                let (chat, prompt) = pair?;
                prompts.entry(chat).or_default().push(prompt);
            }
            let mut statement = inner.db.prepare_cached(&format!(
                "
    SELECT id, title, provider, model, effort, resolution, created_at AS createdAt,
      bench_id AS benchId, bench_slot AS benchSlot,
      COALESCE(finished_at > COALESCE(seen_at, 0), 0) AS unread,
      (SELECT count(*) FROM messages WHERE messages.conversation_id = conversations.id) AS messageCount
    FROM conversations
    WHERE archived_at IS NULL AND NOT ({})
    ORDER BY created_at DESC
  ",
                empty_chat("conversations")
            ))?;
            let rows = statement
                .query_map([], |row| {
                    let id: String = row.get("id")?;
                    Ok(ConversationSummary {
                        prompts: prompts.get(&id).cloned().unwrap_or_default(),
                        id: ConversationId(id),
                        title: row.get("title")?,
                        provider: row.get("provider")?,
                        model: row.get("model")?,
                        effort: row.get::<_, Option<String>>("effort")?.filter(|effort| !effort.is_empty()),
                        resolution: row.get("resolution")?,
                        created_at: row.get("createdAt")?,
                        bench_id: row.get("benchId")?,
                        bench_slot: row.get("benchSlot")?,
                        unread: row.get::<_, i64>("unread")? != 0,
                        message_count: row.get("messageCount")?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// `promptTitles()`: the title each prompt a listed chat or bench came
    /// from was last sent under, in the order JavaScript's object keeps.
    pub fn prompt_titles(&self) -> Result<json::Object, Error> {
        self.with(|inner| {
            let mut titles = json::Object::new();
            let mut statement = inner.db.prepare_cached(
                "
    SELECT p.prompt_id, p.title FROM message_prompts p
    JOIN messages m ON m.id = p.message_id
    JOIN conversations c ON c.id = m.conversation_id
    WHERE c.archived_at IS NULL
    ORDER BY p.rowid
  ",
            )?;
            // `new Map(rows)` keeps a key where it was first set, then
            // `Object.fromEntries` orders the keys as an object does.
            let mut map: Vec<(String, Json)> = Vec::new();
            let set = |key: String, value: Json, map: &mut Vec<(String, Json)>| match map
                .iter_mut()
                .find(|(k, _)| *k == key)
            {
                Some(slot) => slot.1 = value,
                None => map.push((key, value)),
            };
            for pair in statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })? {
                let (id, title) = pair?;
                set(id, Json::String(title), &mut map);
            }
            let mut statement = inner.db.prepare_cached(
                "SELECT rounds_json FROM benches WHERE archived_at IS NULL ORDER BY created_at",
            )?;
            for nodes in statement.query_map([], |row| row.get::<_, String>(0))? {
                let nodes = json::parse(&nodes?)?;
                for node in nodes.as_array().ok_or("rounds_json is not an array")? {
                    if let Some(links) = node.get("prompts").filter(|links| links.truthy()) {
                        for link in links
                            .as_array()
                            .ok_or("a round's prompts are not an array")?
                        {
                            let id = link
                                .get("id")
                                .and_then(Json::as_str)
                                .ok_or("a prompt link has no id")?;
                            set(
                                id.to_string(),
                                link.get("title").cloned().unwrap_or(Json::Null),
                                &mut map,
                            );
                        }
                    }
                }
            }
            for (key, value) in map {
                titles.insert(key, value);
            }
            Ok(titles)
        })
    }

    /// `endTurn(conversation, final)`: ends the turn at the head with
    /// `final` (an error, or an assistant message flagged `stopped`), each
    /// tool call left without a result getting one first, and saves it.
    pub fn end_turn(
        &self,
        conversation: &mut Conversation,
        last: NewMessage,
    ) -> Result<StoredMessage, Error> {
        let messages = &conversation.messages;
        let mut at = messages.len();
        loop {
            if at == 0 {
                return Err(Error::plain(
                    "Cannot read properties of undefined (reading 'role')",
                ));
            }
            at -= 1;
            match &messages[at].message {
                Message::Tool(_) => continue,
                Message::User(user) if user.see_build => continue,
                _ => break,
            }
        }
        let answered: HashSet<&str> = messages[at + 1..]
            .iter()
            .filter_map(|message| match &message.message {
                Message::Tool(tool) => Some(tool.tool_call_id.as_str()),
                _ => None,
            })
            .collect();
        let calls: Vec<String> = match &messages[at].message {
            Message::Assistant(assistant) => assistant
                .tool_calls
                .iter()
                .flatten()
                .map(|call| call.id.clone())
                .filter(|id| !answered.contains(id.as_str()))
                .collect(),
            _ => Vec::new(),
        };
        let (stopped, reason) = (last.is_stopped(), last.content_text());
        for call in calls {
            self.append_message(
                conversation,
                NewMessage::unanswered(&call, stopped, &reason),
                None,
            )?;
        }
        let message = self.append_message(conversation, last, None)?;
        self.save(conversation)?;
        Ok(message)
    }

    /// `endUnfinishedTurn(conversation, reason)`: ends the head's turn as
    /// failed with `reason` if it is still under way; returns whether it did.
    pub fn end_unfinished_turn(
        &self,
        conversation: &mut Conversation,
        reason: &str,
    ) -> Result<bool, Error> {
        let Some(head) = conversation.messages.last() else {
            return Ok(false);
        };
        if head.message.turn_end().is_some() {
            return Ok(false);
        }
        self.end_turn(conversation, NewMessage::error(reason, None))?;
        Ok(true)
    }

    /// `interruptUnfinished()`: ends every stored turn the server stopped in
    /// the middle of; returns the chats it ended. A chat that cannot be read
    /// or loaded is logged and left as stored.
    pub fn interrupt_unfinished(&self) -> Result<Vec<String>, Error> {
        let heads: Vec<(String, String)> = self.with(|inner| {
            let mut statement = inner.db.prepare_cached(
                "
    SELECT c.id, m.payload_json FROM conversations c JOIN messages m ON m.id = c.current_message_id
    WHERE c.archived_at IS NULL
  ",
            )?;
            let rows = statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, Error>(rows)
        })?;
        let mut ended = Vec::new();
        for (id, head) in heads {
            let result = (|| -> Result<bool, Error> {
                let head = payload::decode(&head, &format!("Conversation {id} head"))?;
                if head.turn_end().is_some() {
                    return Ok(false);
                }
                let live = self
                    .conversation(&id)?
                    .ok_or_else(|| Error::plain(format!("Conversation {id} not found")))?;
                let mut conversation = live.lock().expect("a chat's lock");
                self.end_unfinished_turn(&mut conversation, INTERRUPTED)?;
                Ok(true)
            })();
            match result {
                Ok(true) => ended.push(id),
                Ok(false) => {}
                Err(error) => eprintln!(
                    "[store] the turn the server stopped in {id} could not be ended: {error}"
                ),
            }
        }
        Ok(ended)
    }

    /// `forkConversation(source, messageCount, world, id)`: a new chat
    /// continuing `source` from `message_count` messages in, with `world`.
    /// Files the copied messages name move with it; a fork that fails leaves
    /// none.
    pub fn fork_conversation(
        &self,
        source: &Conversation,
        message_count: usize,
        world: &World,
        id: Option<String>,
    ) -> Result<LiveConversation, Error> {
        let id = id.unwrap_or_else(|| self.host.uuid());
        let result = (|| {
            for dir in [
                self.folder.worlds(),
                self.folder.images(),
                self.folder.attachments(),
            ] {
                let from = dir.join(&source.id.0);
                if from.exists() {
                    copy_tree(&from, &dir.join(&id))?;
                }
            }
            self.save_fork(source, message_count, world, &id)
        })();
        if result.is_err() {
            for folder in self.storage.chat_folders(&id) {
                remove_all(&folder)?;
            }
            self.storage.forget(&id);
        }
        result
    }

    fn save_fork(
        &self,
        source: &Conversation,
        message_count: usize,
        world: &World,
        id: &str,
    ) -> Result<LiveConversation, Error> {
        // The URLs are replaced in the row's text, as `JSON.stringify` wrote
        // it (`rewrite` in `saveFork`).
        let rewrite = |text: &str| {
            text.replace(
                &format!("/api/images/{}/", source.id.0),
                &format!("/api/images/{id}/"),
            )
            .replace(
                &format!("/api/attachments/{}/", source.id.0),
                &format!("/api/attachments/{id}/"),
            )
        };
        let mut ids = HashMap::new();
        let mut messages: Vec<StoredMessage> = Vec::new();
        for message in source.messages.iter().take(message_count) {
            let copy = self.host.uuid();
            let parent = messages.last().map(|m| m.id.0.clone());
            let stored = StoredMessage::decode(
                id,
                &copy,
                parent.as_deref(),
                messages.len() as u32,
                rewrite(&message.text),
            )?;
            ids.insert(message.id.clone(), stored.id.clone());
            messages.push(stored);
        }
        let checkpoints = source
            .checkpoints
            .iter()
            .filter_map(|(key, value)| ids.get(key).map(|copy| (copy.clone(), value.clone())))
            .collect();
        let conversation = Conversation {
            id: ConversationId(id.to_string()),
            title: source.title.clone(),
            provider: source.provider.clone(),
            model: source.model.clone(),
            effort: source.effort.clone(),
            resolution: source.resolution,
            bench: None,
            created_at: iso(self.now()),
            current_message_id: messages.last().map(|m| m.id.clone()),
            next_message_position: messages.len() as u32,
            unsaved: messages.iter().map(|m| m.id.clone()).collect(),
            messages,
            world: world.reference(),
            checkpoints,
            token: 0,
        };
        let live = self.make_live(conversation);
        self.save(&mut live.lock().expect("a new chat's lock"))?;
        Ok(live)
    }

    /// `collectChunks()`: removes chunk files no stored world names (left by
    /// edits, branches and reverts) and the world folders of deleted chats.
    pub fn collect_chunks(&self) -> Result<(), Error> {
        let (manifests, conversations) = self.with(|inner| {
            let mut statement = inner.db.prepare_cached(
                "
    SELECT id AS owner, world_manifest_json AS manifest FROM conversations
    UNION SELECT conversation_id, world_manifest_json FROM messages WHERE world_manifest_json IS NOT NULL
    UNION SELECT conversation_id, checkpoint_manifest_json FROM messages WHERE checkpoint_manifest_json IS NOT NULL
  ",
            )?;
            let manifests = statement
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            let mut statement = inner.db.prepare_cached("SELECT id FROM conversations")?;
            let conversations = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<HashSet<_>, _>>()?;
            Ok::<_, Error>((manifests, conversations))
        })?;
        let mut referenced = HashSet::new();
        for (owner, manifest) in &manifests {
            for hash in world::chunk_hashes(manifest)? {
                referenced.insert(format!("{owner}/{hash}"));
            }
        }
        let worlds = self.folder.worlds();
        if !worlds.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(&worlds)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !conversations.contains(&id) {
                remove_all(&entry.path())?;
                continue;
            }
            let directory = worlds.join(&id).join("chunks");
            if !directory.exists() {
                continue;
            }
            let names: Vec<String> = std::fs::read_dir(&directory)?
                .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
                .collect::<Result<_, _>>()?;
            let named: HashSet<String> = names
                .iter()
                .filter(|name| referenced.contains(&format!("{id}/{}", strip_json(name))))
                .cloned()
                .collect();
            for name in crate::storage::expendable(&names, &named, 0, |_| 0.0) {
                remove_all(&directory.join(name))?;
            }
        }
        Ok(())
    }

    /// `reportReferences(id)`: the test_build reports chat `id`'s stored
    /// messages name, which are never pruned.
    pub fn report_references(&self, id: &str) -> Result<Vec<String>, Error> {
        let payloads: Vec<String> = self.with(|inner| {
            let mut statement = inner.db.prepare_cached(
                "
    SELECT payload_json FROM messages WHERE conversation_id = ? AND instr(payload_json, '/work/verification/') > 0
  ",
            )?;
            let rows = statement.query_map([id], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?;
            Ok::<_, Error>(rows)
        })?;
        let pattern = crate::regex!(r"/work/verification/([0-9a-f-]{36}\.json)");
        let mut names: Vec<String> = Vec::new();
        for payload in &payloads {
            for found in pattern.captures_iter(payload) {
                let name = found[1].to_string();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        Ok(names)
    }
}

/// `emptyChat(chat)`: a chat with no message.
pub(crate) fn empty_chat(chat: &str) -> String {
    format!("NOT EXISTS (SELECT 1 FROM messages WHERE messages.conversation_id = {chat}.id)")
}

/// `unansweredResult(stopped, reason)`: the result of a tool call its turn
/// ended without answering.
pub fn unanswered_result(stopped: bool, reason: &str) -> Json {
    if stopped {
        crate::object! { "ok" => false, "cancelled" => true, "error" => "Cancelled by user" }
    } else {
        crate::object! {
            "ok" => false,
            "interrupted" => true,
            "error" => format!("This tool call did not finish ({reason}). Its side effects are unknown; inspect the build or workspace before retrying a change."),
        }
    }
}
