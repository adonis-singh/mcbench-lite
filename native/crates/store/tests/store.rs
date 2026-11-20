//! `test/store.test.js`, ported: each test here has the name and the cases
//! of today's, and the same expected values and messages. Worlds are
//! `mcbench_world::World`s (`common::TestWorld` lists a world's blocks). Not
//! ported: the four migration tests (versions 10, 11, 12 and 13), because
//! this store opens version 14 only (`version_13_is_refused` below stands
//! in), and the sandbox halves of the two attachment
//! tests (`sandbox.syncAttachments` is `mcbench-sandbox`'s).

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{DetHost, TestWorld, block_at};
use mcbench_store::json::Json;
use mcbench_store::{
    ARCHIVE_TTL_MS, BenchMember, Content, Error, ListedBench, LiveConversation, Message, NewBench,
    NewConversation, NewMessage, Part, PromptLink, Reply, SearchScope, Store, ToolCall, object,
};

const T0: i64 = 1794873600000;

struct Folder {
    root: PathBuf,
    host: Arc<DetHost>,
}

impl Folder {
    fn new(name: &str) -> Folder {
        Folder {
            root: common::scratch(name),
            host: DetHost::new(T0),
        }
    }
    fn open(&self) -> Store {
        common::open(
            &self.root,
            self.host.clone(),
            common::limits(4 * 1024 * 1024 * 1024),
        )
        .unwrap()
    }
    fn sql(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.root.join("mcbench.sqlite3")).unwrap()
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn create(store: &Store) -> LiveConversation {
    store
        .create_conversation("openrouter", "test/model", NewConversation::default())
        .unwrap()
}

fn text(content: &str) -> Content {
    Content::Text(content.to_string())
}

/// `appendMessage(conversation, { role: 'user', content }, world.reference())`.
fn prompt(store: &Store, live: &LiveConversation, content: &str, checkpoint: bool) -> String {
    let mut c = live.lock().unwrap();
    let world = checkpoint.then(|| c.world.clone());
    store
        .append_message(&mut c, NewMessage::prompt(&text(content), &[]), world)
        .unwrap()
        .id
        .0
}

fn reply(store: &Store, live: &LiveConversation, content: &str) -> String {
    let mut c = live.lock().unwrap();
    store
        .append_message(
            &mut c,
            NewMessage::reply(&Reply {
                content: content.to_string(),
                ..Reply::default()
            }),
            None,
        )
        .unwrap()
        .id
        .0
}

fn save(store: &Store, live: &LiveConversation) {
    store.save(&mut live.lock().unwrap()).unwrap();
}

fn id(live: &LiveConversation) -> String {
    live.lock().unwrap().id.0.clone()
}

fn content_of(message: &Message) -> String {
    match message {
        Message::User(user) => user.content.text(),
        Message::Assistant(a) => a.content.clone(),
        Message::Tool(t) => t.content.clone(),
        Message::Error(e) => e.content.clone(),
    }
}

fn role(message: &Message) -> &'static str {
    match message {
        Message::User(_) => "user",
        Message::Assistant(_) => "assistant",
        Message::Tool(_) => "tool",
        Message::Error(_) => "error",
    }
}

fn data_file(name: &str, bytes: &[u8], mime: &str) -> Json {
    use base64::Engine;
    object! { "name" => name, "data" => format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)) }
}

fn set_world(live: &LiveConversation, world: TestWorld) {
    live.lock().unwrap().world = world.into_chat();
}

fn count(db: &rusqlite::Connection, sql: &str, id: &str) -> i64 {
    db.query_row(sql, [id], |row| row.get(0)).unwrap()
}

#[test]
fn a_fresh_database_is_created_at_the_current_schema_version() {
    let folder = Folder::new("fresh");
    let store = folder.open();
    let db = folder.sql();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        14
    );
    assert_eq!(
        db.query_row("PRAGMA auto_vacuum", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert!(store.list_conversations().unwrap().is_empty());
}

#[test]
fn generated_title_survives_a_new_chat_and_reload_before_the_running_turn_is_saved() {
    let folder = Folder::new("title");
    let store = folder.open();
    let chat = create(&store);
    prompt(&store, &chat, "Build a clock", false);
    save(&store, &chat);
    let pending = reply(&store, &chat, "Working...");
    store
        .set_title(&mut chat.lock().unwrap(), "Redstone Clock")
        .unwrap();
    create(&store);
    let rows = store.list_conversations().unwrap();
    assert_eq!(
        rows.iter().find(|row| row.id.0 == id(&chat)).unwrap().title,
        "Redstone Clock"
    );
    assert_eq!(chat.lock().unwrap().title, "Redstone Clock");
    assert!(
        chat.lock()
            .unwrap()
            .unsaved
            .contains(&mcbench_store::MessageId(pending))
    );
    drop(store);
    let store = folder.open();
    let restored = store.conversation(&id(&chat)).unwrap().unwrap();
    let restored = restored.lock().unwrap();
    assert_eq!(restored.title, "Redstone Clock");
    assert_eq!(
        restored.messages.len(),
        1,
        "updating the title must not save unfinished turn messages"
    );
}

#[test]
fn a_chat_is_unread_from_the_end_of_a_turn_until_it_is_seen() {
    let folder = Folder::new("unread");
    let store = folder.open();
    let chat = create(&store);
    chat.lock().unwrap().effort = Some("high".to_string());
    prompt(&store, &chat, "Build a clock", false);
    save(&store, &chat);
    let row = || {
        store
            .list_conversations()
            .unwrap()
            .into_iter()
            .find(|row| row.id.0 == id(&chat))
            .unwrap()
    };
    assert!(!row().unread);
    assert_eq!(row().effort.as_deref(), Some("high"));
    folder.host.set_now(T0 + 1);
    store.mark_finished(&id(&chat)).unwrap();
    assert!(row().unread);
    folder.host.set_now(T0 + 2);
    store.mark_seen(&id(&chat)).unwrap();
    assert!(!row().unread);
    save(&store, &chat);
    assert!(!row().unread, "saving a conversation keeps its seen state");
}

#[test]
fn title_updates_do_not_restore_archived_conversations() {
    let folder = Folder::new("archived-title");
    let store = folder.open();
    let chat = create(&store);
    store.archive_conversation(&id(&chat)).unwrap();
    let error = store
        .set_title(&mut chat.lock().unwrap(), "Late title")
        .unwrap_err();
    assert!(error.message.contains("not found or archived"));
    assert_eq!(chat.lock().unwrap().title, "New build");
    assert!(
        !store
            .list_conversations()
            .unwrap()
            .iter()
            .any(|row| row.id.0 == id(&chat))
    );
}

#[test]
fn editing_the_first_prompt_keeps_the_title_and_only_an_unchanged_resend_repeats_it() {
    let folder = Folder::new("repeat");
    let store = folder.open();
    let chat = create(&store);
    let chat_id = id(&chat);
    assert!(
        !store
            .repeats_latest_root_prompt(&chat.lock().unwrap(), &text("Build a clock"))
            .unwrap()
    );
    let image = |bytes: &[u8]| Part::Attachment {
        attachment: store
            .save_attachment(&chat_id, &data_file("ref.png", bytes, "image/png"))
            .unwrap(),
    };
    let with = |words: &str, part: Part| {
        Content::Parts(vec![
            Part::Text {
                text: words.to_string(),
            },
            part,
        ])
    };
    let first = {
        let mut c = chat.lock().unwrap();
        let world = c.world.reference();
        store
            .append_message(
                &mut c,
                NewMessage::prompt(&with("Build a clock", image(b"png-a")), &[]),
                Some(world),
            )
            .unwrap()
            .id
            .0
    };
    save(&store, &chat);
    store
        .set_title(&mut chat.lock().unwrap(), "Redstone Clock")
        .unwrap();
    store.revert(&mut chat.lock().unwrap(), &first).unwrap();
    assert_eq!(chat.lock().unwrap().title, "Redstone Clock");
    let repeats = |content: &Content| {
        store
            .repeats_latest_root_prompt(&chat.lock().unwrap(), content)
            .unwrap()
    };
    assert!(
        repeats(&with("Build a clock", image(b"png-a"))),
        "the same text and attachment bytes under a newly stored name are a resend"
    );
    assert!(!repeats(&with("Build a clock tower", image(b"png-a"))));
    assert!(
        !repeats(&text("Build a clock")),
        "dropping the attachment is an edit"
    );
    assert!(
        !repeats(&with("Build a clock", image(b"png-b"))),
        "an attachment with the same name but different bytes is an edit"
    );
    prompt(&store, &chat, "Build a clock tower", true);
    save(&store, &chat);
    assert!(
        repeats(&text("Build a clock tower")),
        "the newest root prompt is the comparison"
    );
    assert!(!repeats(&with("Build a clock", image(b"png-a"))));
}

#[test]
fn persists_stable_message_branches_and_world_checkpoints_across_restarts() {
    let folder = Folder::new("branches");
    let store = folder.open();
    let chat = create(&store);
    let chat_id = id(&chat);
    let user1 = prompt(&store, &chat, "first", true);
    let stone = TestWorld::default().with(0, 0, 0, "minecraft:stone");
    set_world(&chat, stone.clone());
    let assistant1 = reply(&store, &chat, "stone");
    let user2 = prompt(&store, &chat, "second", true);
    set_world(&chat, stone.with(0, 0, 0, "minecraft:oak_planks"));
    {
        let mut c = chat.lock().unwrap();
        let details = vec![serde_json::json!({ "type": "signed", "data": "opaque" })];
        store
            .append_message(
                &mut c,
                NewMessage::reply(&Reply {
                    content: "oak".to_string(),
                    reasoning_details: Some(details),
                    ..Reply::default()
                }),
                None,
            )
            .unwrap();
    }
    let user3 = prompt(&store, &chat, "third", true);
    set_world(
        &chat,
        stone
            .with(0, 0, 0, "minecraft:oak_planks")
            .with(0, 0, 0, "minecraft:gold_block"),
    );
    let old_head = reply(&store, &chat, "gold");
    save(&store, &chat);

    let db = folder.sql();
    let manifest_hash = |sql: &str, id: &str| -> String {
        let text: String = db.query_row(sql, [id], |r| r.get(0)).unwrap();
        mcbench_store::json::parse(&text)
            .unwrap()
            .get("chunks")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .as_array()
            .unwrap()[1]
            .as_str()
            .unwrap()
            .to_string()
    };
    let inactive = manifest_hash(
        "SELECT checkpoint_manifest_json FROM messages WHERE id = ?",
        &user3,
    );
    let old_branch = manifest_hash(
        "SELECT world_manifest_json FROM messages WHERE id = ?",
        &old_head,
    );
    let original = count(
        &db,
        "SELECT COUNT(*) FROM messages WHERE conversation_id = ?",
        &chat_id,
    );
    let max_position = count(
        &db,
        "SELECT MAX(position) FROM messages WHERE conversation_id = ?",
        &chat_id,
    );
    drop(db);

    drop(store);
    let store = folder.open();
    let restored = store.conversation(&chat_id).unwrap().unwrap();
    {
        let r = restored.lock().unwrap();
        assert_eq!(r.messages[0].id.0, user1);
        assert_eq!(r.messages[2].id.0, user2);
        assert_eq!(
            block_at(&folder.root, &chat_id, &r.world, 0, 0, 0).as_deref(),
            Some("minecraft:gold_block")
        );
        let checkpoint = r
            .checkpoints
            .get(&mcbench_store::MessageId(user2.clone()))
            .unwrap();
        assert_eq!(
            block_at(&folder.root, &chat_id, checkpoint, 0, 0, 0).as_deref(),
            Some("minecraft:stone")
        );
        let Message::Assistant(oak) = &r.messages[3].message else {
            panic!()
        };
        assert_eq!(oak.reasoning_details.as_ref().unwrap()[0]["data"], "opaque");
    }
    let removed = store.revert(&mut restored.lock().unwrap(), &user2).unwrap();
    assert_eq!(content_of(&removed.message), "second");
    {
        let r = restored.lock().unwrap();
        assert_eq!(
            r.messages
                .iter()
                .map(|m| m.id.0.clone())
                .collect::<Vec<_>>(),
            [user1.clone(), assistant1.clone()]
        );
        assert_eq!(r.current_message_id.as_ref().unwrap().0, assistant1);
        assert_eq!(
            block_at(&folder.root, &chat_id, &r.world, 0, 0, 0).as_deref(),
            Some("minecraft:stone")
        );
    }
    let chunks = folder.root.join("worlds").join(&chat_id).join("chunks");
    assert!(chunks.join(format!("{inactive}.json")).exists());
    assert!(chunks.join(format!("{old_branch}.json")).exists());

    let edited = prompt(&store, &restored, "edited second", true);
    save(&store, &restored);
    let public = restored
        .lock()
        .unwrap()
        .messages
        .last()
        .unwrap()
        .public_text();
    let public = mcbench_store::json::parse(&public).unwrap();
    assert_eq!(public.get("id").unwrap().as_str(), Some(edited.as_str()));
    assert!(public.get("parentId").is_none());
    assert!(public.get("position").is_none());
    let db = folder.sql();
    let (parent, position): (String, i64) = db
        .query_row(
            "SELECT parent_id, position FROM messages WHERE id = ?",
            [&edited],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(parent, assistant1);
    assert!(position > max_position);
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM messages WHERE conversation_id = ?",
            &chat_id
        ),
        original + 1
    );

    let tree = store.message_tree(&restored.lock().unwrap()).unwrap();
    assert_eq!(tree.head_id.as_deref(), Some(edited.as_str()));
    let fork: Vec<String> = tree
        .messages
        .iter()
        .filter(|m| m.parent_id.as_deref() == Some(assistant1.as_str()) && m.branchable)
        .map(|m| m.id.clone())
        .collect();
    assert_eq!(fork, [user2.clone(), edited.clone()]);

    store
        .activate_branch(&mut restored.lock().unwrap(), &user2)
        .unwrap();
    {
        let r = restored.lock().unwrap();
        assert_eq!(r.current_message_id.as_ref().unwrap().0, old_head);
        assert_eq!(content_of(&r.messages.last().unwrap().message), "gold");
        assert_eq!(
            block_at(&folder.root, &chat_id, &r.world, 0, 0, 0).as_deref(),
            Some("minecraft:gold_block")
        );
    }

    drop(store);
    let store = folder.open();
    let switched = store.conversation(&chat_id).unwrap().unwrap();
    assert_eq!(
        switched
            .lock()
            .unwrap()
            .current_message_id
            .as_ref()
            .unwrap()
            .0,
        old_head
    );
    store
        .activate_branch(&mut switched.lock().unwrap(), &edited)
        .unwrap();
    let s = switched.lock().unwrap();
    assert_eq!(s.current_message_id.as_ref().unwrap().0, edited);
    assert_eq!(
        content_of(&s.messages.last().unwrap().message),
        "edited second"
    );
    assert_eq!(
        block_at(&folder.root, &chat_id, &s.world, 0, 0, 0).as_deref(),
        Some("minecraft:stone")
    );
    assert_eq!(
        store.message_tree(&s).unwrap().head_id.as_deref(),
        Some(edited.as_str())
    );
}

#[test]
fn a_failed_save_cleans_its_new_chunks_and_drops_the_chat_so_memory_is_never_ahead_of_the_database()
{
    let folder = Folder::new("failed-save");
    let store = folder.open();
    let stale = store
        .create_conversation("openrouter", "test/rollback", NewConversation::default())
        .unwrap();
    let chat_id = id(&stale);
    prompt(&store, &stale, "retry", true);
    let world = TestWorld::default().with(0, 0, 0, "minecraft:diamond_block");
    set_world(&stale, world.clone());
    stale.lock().unwrap().current_message_id =
        Some(mcbench_store::MessageId("invalid-head".to_string()));
    let error = store.save(&mut stale.lock().unwrap()).unwrap_err();
    assert!(error.message.contains("invalid active head"));
    let chunks = folder.root.join("worlds").join(&chat_id).join("chunks");
    let listing = || -> Vec<String> {
        if !chunks.exists() {
            return Vec::new();
        }
        std::fs::read_dir(&chunks)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    };
    assert!(listing().is_empty());
    let conversation = store.conversation(&chat_id).unwrap().unwrap();
    assert!(!Arc::ptr_eq(&conversation, &stale));
    assert!(conversation.lock().unwrap().messages.is_empty());
    assert_eq!(conversation.lock().unwrap().world.size(), 0);
    let error = store.save(&mut stale.lock().unwrap()).unwrap_err();
    assert!(error.message.contains("no longer loaded"));

    let message = prompt(&store, &conversation, "retry", true);
    set_world(&conversation, world);
    assert_eq!(
        conversation
            .lock()
            .unwrap()
            .current_message_id
            .as_ref()
            .unwrap()
            .0,
        message
    );
    save(&store, &conversation);
    assert_eq!(
        listing()
            .iter()
            .filter(|name| name.ends_with(".json"))
            .count(),
        1
    );
    let manifest: String = folder
        .sql()
        .query_row(
            "SELECT world_manifest_json FROM conversations WHERE id = ?",
            [&chat_id],
            |r| r.get(0),
        )
        .unwrap();
    let hash = mcbench_store::json::parse(&manifest)
        .unwrap()
        .get("chunks")
        .unwrap()
        .as_array()
        .unwrap()[0]
        .as_array()
        .unwrap()[1]
        .as_str()
        .unwrap()
        .to_string();
    assert!(chunks.join(format!("{hash}.json")).exists());
}

#[test]
fn archives_in_sqlite_and_removes_artifacts_only_after_the_retention_period() {
    let folder = Folder::new("archive");
    let store = folder.open();
    let chat = store
        .create_conversation("openrouter", "test/archive", NewConversation::default())
        .unwrap();
    let chat_id = id(&chat);
    prompt(&store, &chat, "archive me", true);
    save(&store, &chat);
    let dirs: Vec<PathBuf> = ["worlds", "images", "attachments"]
        .iter()
        .map(|d| folder.root.join(d).join(&chat_id))
        .collect();
    for dir in &dirs {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("keep"), "x").unwrap();
    }
    store.archive_conversation(&chat_id).unwrap();
    let archived: i64 = folder
        .sql()
        .query_row(
            "SELECT archived_at FROM conversations WHERE id = ?",
            [&chat_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(store.conversation(&chat_id).unwrap().is_none());
    assert!(
        !store
            .list_conversations()
            .unwrap()
            .iter()
            .any(|row| row.id.0 == chat_id)
    );
    store
        .clear_expired_archive(archived + ARCHIVE_TTL_MS - 1)
        .unwrap();
    assert!(dirs[0].exists());
    store
        .clear_expired_archive(archived + ARCHIVE_TTL_MS + 1)
        .unwrap();
    for dir in &dirs {
        assert!(!dir.exists());
    }
    assert_eq!(
        count(
            &folder.sql(),
            "SELECT COUNT(*) FROM conversations WHERE id = ?",
            &chat_id
        ),
        0
    );
}

#[test]
fn attachments_preserve_metadata_and_bytes_for_the_sandbox_bridge() {
    let folder = Folder::new("attachment");
    let store = folder.open();
    let chat = format!("test-{T0}");
    let attachment = store
        .save_attachment(
            &chat,
            &data_file("reference notes.md", b"hello attachment", "text/markdown"),
        )
        .unwrap();
    assert_eq!(attachment.name, "reference notes.md");
    assert_eq!(attachment.mime, "text/markdown");
    assert_eq!(attachment.size, 16);
    let pattern =
        regex::Regex::new(r"^/work/attachments/[a-f0-9]{8}-reference-notes\.md$").unwrap();
    assert!(pattern.is_match(&attachment.path), "{}", attachment.path);
    assert_eq!(
        store.read_attachment(&attachment.url).unwrap(),
        b"hello attachment"
    );
}

#[test]
fn attachments_of_100_mib_survive_storage_and_larger_files_are_rejected() {
    let folder = Folder::new("large");
    let store = folder.open();
    let payload = vec![0x5a; 100 * 1024 * 1024];
    let attachment = store
        .save_attachment(
            "large-upload",
            &data_file("reference.mp4", &payload, "video/mp4"),
        )
        .unwrap();
    assert_eq!(attachment.size, payload.len() as u64);
    assert_eq!(
        common::sha(&store.read_attachment(&attachment.url).unwrap()),
        common::sha(&payload)
    );
    let error = store
        .save_attachment(
            "large-upload",
            &data_file("too-large.mp4", &vec![0; payload.len() + 1], "video/mp4"),
        )
        .unwrap_err();
    assert!(error.message.contains("exceeds 100 MB"));
}

#[test]
fn a_refused_message_writes_none_of_its_attachments() {
    let folder = Folder::new("refused-upload");
    let store = folder.open();
    let chat = "refused-upload";
    let file =
        |name: &str, bytes: usize| data_file(name, &vec![1; bytes], "application/octet-stream");
    let dir = folder.root.join("attachments").join(chat);
    let error = store
        .save_attachments(
            chat,
            &[
                file("a.bin", 4),
                object! { "name" => "b.bin", "data" => "https://example.com/b" },
            ],
        )
        .unwrap_err();
    assert_eq!(error.status, Some(400));
    assert!(error.message.contains("not base64 data"));
    let big = 70 * 1024 * 1024;
    let error = store
        .save_attachments(
            chat,
            &[file("a.bin", big), file("b.bin", big), file("c.bin", big)],
        )
        .unwrap_err();
    assert_eq!(error.status, Some(400));
    assert!(error.message.contains("exceed 200 MB total"));
    assert!(!dir.exists());
    assert_eq!(
        store
            .save_attachments(chat, &[file("a.bin", 4), file("b.bin", 5)])
            .unwrap()
            .len(),
        2
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
}

#[cfg(unix)]
#[test]
fn the_data_folder_is_readable_by_its_owner_alone() {
    use std::os::unix::fs::PermissionsExt;
    let folder = Folder::new("mode");
    let _store = folder.open();
    assert_eq!(
        std::fs::metadata(&folder.root)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn a_chat_joins_the_list_only_once_it_has_a_message() {
    let folder = Folder::new("list");
    let store = folder.open();
    let chat = create(&store);
    assert!(
        !store
            .list_conversations()
            .unwrap()
            .iter()
            .any(|row| row.id.0 == id(&chat))
    );
    prompt(&store, &chat, "Build a well", false);
    save(&store, &chat);
    assert!(
        store
            .list_conversations()
            .unwrap()
            .iter()
            .any(|row| row.id.0 == id(&chat))
    );
}

fn new_bench(
    store: &Store,
    host: &DetHost,
    title: &str,
    content: &str,
    models: &[(&str, Option<&str>)],
    blind: bool,
) -> (mcbench_store::Bench, Vec<LiveConversation>) {
    store
        .create_bench(
            &host_uuid(host),
            NewBench {
                title: title.to_string(),
                blind,
                resolution: None,
                content: text(content),
                prompts: Vec::new(),
                members: models
                    .iter()
                    .map(|(model, effort)| BenchMember {
                        provider: "openrouter".to_string(),
                        model: model.to_string(),
                        effort: effort.map(str::to_string),
                    })
                    .collect(),
            },
        )
        .unwrap()
}

fn host_uuid(host: &DetHost) -> String {
    use mcbench_store::Host;
    host.uuid()
}

fn bench_prompt(store: &Store, member: &LiveConversation, round: &str, content: &str) -> String {
    let mut c = member.lock().unwrap();
    let world = c.world.reference();
    store
        .append_message(
            &mut c,
            NewMessage::bench_prompt(round, None, &text(content)),
            Some(world),
        )
        .unwrap()
        .id
        .0
}

#[test]
fn a_bench_keeps_its_rounds_and_runs_each_model_as_a_chat_outside_the_build_list() {
    let folder = Folder::new("bench");
    let store = folder.open();
    let (mut bench, members) = new_bench(
        &store,
        &folder.host,
        "New bench",
        "A lighthouse",
        &[
            ("a/model", None),
            ("b/model", Some("high")),
            ("c/model", None),
        ],
        false,
    );
    let ids: Vec<String> = members.iter().map(id).collect();
    assert_eq!(bench.order, Json::from(ids.clone()));
    let round = bench.round_id(0).unwrap().to_string();
    for member in &members {
        bench_prompt(&store, member, &round, "A lighthouse");
        save(&store, member);
    }
    reply(&store, &members[0], "Built it");
    save(&store, &members[0]);
    store
        .append_message(
            &mut members[1].lock().unwrap(),
            NewMessage::error("Provider failed", None),
            None,
        )
        .unwrap();
    save(&store, &members[1]);
    store
        .append_message(
            &mut members[2].lock().unwrap(),
            NewMessage::stopped("(stopped by user)", None, None),
            None,
        )
        .unwrap();
    save(&store, &members[2]);
    store
        .set_bench_title(&mut bench, "Stormy lighthouse")
        .unwrap();
    store
        .append_round(&mut bench, &text("Add a keeper"), &[])
        .unwrap();

    drop(members);
    drop(store);
    let store = folder.open();
    let stored = store.bench(&bench.id).unwrap().unwrap();
    let contents: Vec<String> = (0..2)
        .map(|i| stored.round_content(i).unwrap().text())
        .collect();
    assert_eq!(contents, ["A lighthouse", "Add a keeper"]);
    let listed = store
        .list_benches()
        .unwrap()
        .iter()
        .map(ListedBench::to_json)
        .find(|row| row.get("id").unwrap().as_str() == Some(bench.id.as_str()))
        .unwrap();
    assert_eq!(
        listed,
        object! {
            "id" => bench.id.as_str(), "title" => "Stormy lighthouse", "createdAt" => bench.created_at.as_str(), "head" => stored.head.clone(),
            "resolution" => 1i64, "rounds" => 2usize, "prompts" => Vec::<String>::new(), "blind" => false, "revealed" => false, "order" => ids.clone(),
        }
    );
    let mut rows: Vec<_> = store
        .list_conversations()
        .unwrap()
        .into_iter()
        .filter(|row| row.bench_id.as_deref() == Some(bench.id.as_str()))
        .collect();
    rows.sort_by_key(|row| row.bench_slot);
    let rows: Vec<(String, i64, String)> = rows
        .into_iter()
        .map(|row| (row.model, row.bench_slot.unwrap(), row.title))
        .collect();
    assert_eq!(
        rows,
        [
            ("a/model".into(), 0, "Stormy lighthouse".into()),
            ("b/model".into(), 1, "Stormy lighthouse".into()),
            ("c/model".into(), 2, "Stormy lighthouse".into())
        ]
    );
    let progress = |i: usize| store.bench_progress(&ids[i]).unwrap();
    assert_eq!((progress(0).round, progress(0).halted), (1, None));
    assert_eq!((progress(1).round, progress(1).halted), (1, Some("failed")));
    assert_eq!(
        (progress(2).round, progress(2).halted),
        (1, Some("stopped"))
    );
    let members: Vec<(String, Option<String>)> = store
        .bench_members(&bench.id)
        .unwrap()
        .iter()
        .map(|m| {
            let m = m.lock().unwrap();
            (m.model.clone(), m.effort.clone())
        })
        .collect();
    assert_eq!(
        members,
        [
            ("a/model".into(), None),
            ("b/model".into(), Some("high".into())),
            ("c/model".into(), None)
        ]
    );
    let mut archived = store.archive_bench(&bench.id).unwrap();
    archived.sort();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(archived, sorted);
    assert!(store.bench(&bench.id).unwrap().is_none());
    assert!(store.conversation(&ids[0]).unwrap().is_none());
}

#[test]
fn a_forked_bench_chat_continues_as_an_ordinary_chat_with_its_own_copy_of_the_files() {
    let folder = Folder::new("fork");
    let store = folder.open();
    let (_, members) = new_bench(
        &store,
        &folder.host,
        "Tower",
        "Build a tower",
        &[("test/model", None)],
        false,
    );
    let source = &members[0];
    let source_id = id(source);
    let attachment = store
        .save_attachment(&source_id, &data_file("ref.png", b"png", "image/png"))
        .unwrap();
    {
        let mut c = source.lock().unwrap();
        let world = c.world.reference();
        let content = Content::Parts(vec![
            Part::Text {
                text: "Build a tower".into(),
            },
            Part::Attachment {
                attachment: attachment.clone(),
            },
        ]);
        store
            .append_message(&mut c, NewMessage::prompt(&content, &[]), Some(world))
            .unwrap();
    }
    set_world(
        source,
        TestWorld::default().with(0, 0, 0, "minecraft:stone"),
    );
    reply(&store, source, "Built");
    let round_two = prompt(&store, source, "Make it taller", true);
    set_world(
        source,
        TestWorld::default()
            .with(0, 0, 0, "minecraft:stone")
            .with(0, 1, 0, "minecraft:stone"),
    );
    save(&store, source);
    let s = source.lock().unwrap().clone();
    let at = s.messages.iter().position(|m| m.id.0 == round_two).unwrap();
    let checkpoint = s
        .checkpoints
        .get(&mcbench_store::MessageId(round_two))
        .unwrap()
        .clone();
    let fork = id(&store.fork_conversation(&s, at, &checkpoint, None).unwrap());
    drop(members);
    drop(store);
    let store = folder.open();
    let restored = store.conversation(&fork).unwrap().unwrap();
    let r = restored.lock().unwrap();
    assert!(r.bench.is_none());
    assert_eq!(r.title, "Tower");
    assert_eq!(
        r.messages
            .iter()
            .map(|m| role(&m.message))
            .collect::<Vec<_>>(),
        ["user", "assistant"]
    );
    assert_eq!(
        r.world.size(),
        1,
        "the fork has the world as round one left it"
    );
    let Message::User(user) = &r.messages[0].message else {
        panic!()
    };
    let Content::Parts(parts) = &user.content else {
        panic!()
    };
    let Part::Attachment { attachment: copied } = &parts[1] else {
        panic!()
    };
    assert_eq!(copied.url, attachment.url.replace(&source_id, &fork));
    assert_eq!(store.read_attachment(&copied.url).unwrap(), b"png");
    assert!(
        store
            .list_conversations()
            .unwrap()
            .iter()
            .any(|row| row.id.0 == fork && row.bench_id.is_none())
    );
}

#[test]
fn search_finds_chats_in_build_and_benches_in_bench_by_what_was_said() {
    let folder = Folder::new("search");
    let store = folder.open();
    let chat = create(&store);
    prompt(&store, &chat, "Build a lighthouse on a cliff", true);
    reply(&store, &chat, "Added a spiral staircase inside the tower.");
    save(&store, &chat);
    store
        .set_title(&mut chat.lock().unwrap(), "Coastal beacon")
        .unwrap();
    let (bench, members) = new_bench(
        &store,
        &folder.host,
        "Castle duel",
        "A castle with a drawbridge",
        &[("a/model", None)],
        false,
    );
    prompt(&store, &members[0], "A castle with a drawbridge", true);
    reply(&store, &members[0], "The portcullis now has chains.");
    save(&store, &members[0]);
    let ids = |scope, query: &str| {
        store
            .search(scope, query)
            .unwrap()
            .into_iter()
            .map(|hit| hit.id)
            .collect::<Vec<_>>()
    };
    let chat_id = id(&chat);
    assert_eq!(
        ids(SearchScope::Build, "staircase"),
        std::slice::from_ref(&chat_id)
    );
    assert_eq!(
        ids(SearchScope::Build, "beacon"),
        std::slice::from_ref(&chat_id),
        "titles are searchable after a rename"
    );
    assert!(
        ids(SearchScope::Build, "portcullis").is_empty(),
        "bench chats stay out of Build"
    );
    assert_eq!(
        ids(SearchScope::Bench, "portcullis"),
        std::slice::from_ref(&bench.id),
        "a model reply finds its bench"
    );
    assert_eq!(
        ids(SearchScope::Bench, "drawbr"),
        std::slice::from_ref(&bench.id),
        "words match by prefix"
    );
    assert!(ids(SearchScope::Bench, "staircase").is_empty());
    assert_eq!(
        store
            .search(SearchScope::Build, "spiral staircase")
            .unwrap()[0]
            .snippet
            .as_deref(),
        Some("Added a \u{1}spiral\u{2} \u{1}staircase\u{2} inside the tower.")
    );
    assert!(ids(SearchScope::Build, "\"*(").is_empty());
    let started = std::time::Instant::now();
    assert_eq!(
        ids(SearchScope::Build, &"staircase ".repeat(4000)),
        std::slice::from_ref(&chat_id)
    );
    assert!(
        // Today's test bounds it at 500 ms; here it is a bound a starved
        // machine meets.
        started.elapsed().as_secs() < 5,
        "a repeated word took {:?}",
        started.elapsed()
    );
    let error = store
        .search(SearchScope::Build, "a b c d e f g h i j k l m n o p q")
        .unwrap_err();
    assert_eq!(error.status, Some(400));
    assert!(error.message.contains("at most 16 different words"));
    store.archive_conversation(&chat_id).unwrap();
    store.archive_bench(&bench.id).unwrap();
    assert!(ids(SearchScope::Build, "staircase").is_empty());
    assert!(ids(SearchScope::Bench, "portcullis").is_empty());
}

#[test]
fn an_edited_bench_round_branches_and_each_model_follows_the_branch_it_answered() {
    let folder = Folder::new("bench-branches");
    let store = folder.open();
    let (mut bench, members) = new_bench(
        &store,
        &folder.host,
        "Branches",
        "A tower",
        &[("a/model", None)],
        false,
    );
    let member = members[0].clone();
    let member_id = id(&member);
    let turn = |round: &str, content: &str, block: &str| {
        bench_prompt(&store, &member, round, content);
        let size = member.lock().unwrap().world.size() as i32;
        let mut world = TestWorld::default();
        // The world so far, then one block above it.
        for y in 0..size {
            world = world.with(
                0,
                y,
                0,
                &block_at(
                    &folder.root,
                    &member_id,
                    &member.lock().unwrap().world,
                    0,
                    y,
                    0,
                )
                .unwrap_or_else(|| "minecraft:stone".into()),
            );
        }
        set_world(&member, world.with(0, size, 0, block));
        reply(&store, &member, &format!("Built {block}"));
        save(&store, &member);
    };
    let first = bench.round_id(0).unwrap().to_string();
    turn(&first, "A tower", "minecraft:stone");
    store
        .append_round(&mut bench, &text("Add a roof"), &[])
        .unwrap();
    let roof = bench.round_id(1).unwrap().to_string();
    turn(&roof, "Add a roof", "minecraft:oak_planks");
    let reverted = store
        .edit_bench_round(&mut bench, 1, &text("Add a spire"), &[])
        .unwrap();
    assert_eq!(reverted.len(), 1);
    assert!(Arc::ptr_eq(&reverted[0], &member));
    assert_eq!(
        (0..2)
            .map(|i| bench.round_content(i).unwrap().text())
            .collect::<Vec<_>>(),
        ["A tower", "Add a spire"]
    );
    let spire = bench.round_id(1).unwrap().to_string();
    turn(&spire, "Add a spire", "minecraft:cobblestone");
    assert_eq!(
        bench.rounds[1].get("branch").unwrap(),
        &object! { "index" => 1usize, "count" => 2usize, "previousId" => roof.as_str(), "nextId" => Json::Null }
    );
    store.follow_bench_branch(&mut bench, &roof).unwrap();
    assert_eq!(
        (0..2)
            .map(|i| bench.round_content(i).unwrap().text())
            .collect::<Vec<_>>(),
        ["A tower", "Add a roof"]
    );
    assert_eq!(
        block_at(
            &folder.root,
            &member_id,
            &member.lock().unwrap().world,
            0,
            1,
            0
        )
        .as_deref(),
        Some("minecraft:oak_planks")
    );
    assert_eq!(
        content_of(&member.lock().unwrap().messages.last().unwrap().message),
        "Built minecraft:oak_planks"
    );
    store.follow_bench_branch(&mut bench, &spire).unwrap();
    drop(reverted);
    drop(member);
    drop(members);
    drop(store);
    let store = folder.open();
    let reopened = store.conversation(&member_id).unwrap().unwrap();
    let r = reopened.lock().unwrap();
    assert_eq!(
        block_at(&folder.root, &member_id, &r.world, 0, 1, 0).as_deref(),
        Some("minecraft:cobblestone")
    );
    let prompts: Vec<String> = r.prompts().iter().map(|m| content_of(&m.message)).collect();
    assert_eq!(prompts, ["A tower", "Add a spire"]);
    let listed = store
        .list_benches()
        .unwrap()
        .iter()
        .map(ListedBench::to_json)
        .find(|row| row.get("id").unwrap().as_str() == Some(bench.id.as_str()))
        .unwrap();
    assert_eq!(listed.get("rounds").unwrap().as_f64(), Some(2.0));
}

#[test]
fn blind_votes_rank_the_models_and_a_bench_that_shows_its_names_takes_no_votes() {
    let folder = Folder::new("votes");
    let store = folder.open();
    let (mut bench, members) = new_bench(
        &store,
        &folder.host,
        "Votes",
        "A castle",
        &[
            ("a/model", None),
            ("b/model", None),
            ("b/model", Some("high")),
        ],
        true,
    );
    let [a, b, c]: [String; 3] = members
        .iter()
        .map(id)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let first = bench.round_id(0).unwrap().to_string();
    store
        .append_round(&mut bench, &text("Add a moat"), &[])
        .unwrap();
    let second = bench.round_id(1).unwrap().to_string();
    let all = [a.clone(), b.clone(), c.clone()];
    folder.host.set_now(T0 + 1);
    store.cast_vote(&mut bench, &first, Some(&a), &all).unwrap();
    folder.host.set_now(T0 + 2);
    store.cast_vote(&mut bench, &second, None, &all).unwrap();
    let votes = store.bench(&bench.id).unwrap().unwrap().votes;
    assert_eq!(
        votes,
        [
            (
                first.clone(),
                mcbench_store::Vote {
                    pick: Some(a.clone()),
                    blind: true
                }
            ),
            (
                second.clone(),
                mcbench_store::Vote {
                    pick: None,
                    blind: true
                }
            )
        ]
    );
    let mut order: Vec<String> =
        mcbench_store::json::stringify(&store.bench(&bench.id).unwrap().unwrap().order)
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(|s| s.trim_matches('"').to_string())
            .collect();
    order.sort();
    let mut sorted = all.to_vec();
    sorted.sort();
    assert_eq!(order, sorted);
    let board = store.leaderboard().unwrap();
    let rows: Vec<(String, Option<String>, i64, i64, usize)> = board
        .iter()
        .map(|e| {
            (
                e.model.clone(),
                e.effort.clone(),
                e.wins,
                e.votes,
                e.benches,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("a/model".into(), None, 1, 2, 1),
            ("b/model".into(), None, 0, 2, 1),
            ("b/model".into(), Some("high".into()), 0, 2, 1)
        ]
    );
    assert!(board[0].rating > 1000.0 && board[1].rating < 1000.0);
    store.reveal_bench(&mut bench).unwrap();
    let error = store
        .cast_vote(&mut bench, &second, Some(&b), &all)
        .unwrap_err();
    assert!(
        error
            .message
            .contains("shows its models' names: it takes no votes")
    );
    let kept = store.bench(&bench.id).unwrap().unwrap();
    assert_eq!(
        kept.votes.iter().find(|(r, _)| *r == second).unwrap().1,
        mcbench_store::Vote {
            pick: None,
            blind: true
        },
        "the vote cast blind stays"
    );
    assert_eq!(
        store
            .leaderboard()
            .unwrap()
            .iter()
            .map(|e| e.votes)
            .collect::<Vec<_>>(),
        [2, 2, 2]
    );
    let (mut open, open_members) = new_bench(
        &store,
        &folder.host,
        "Open",
        "A hut",
        &[("a/model", None), ("b/model", None)],
        false,
    );
    let round = open.round_id(0).unwrap().to_string();
    let judged: Vec<String> = open_members.iter().map(id).collect();
    assert!(
        store
            .cast_vote(&mut open, &round, Some(&judged[0]), &judged)
            .unwrap_err()
            .message
            .contains("takes no votes")
    );
    store.clear_vote(&mut bench, &first).unwrap();
    assert_eq!(
        store
            .leaderboard()
            .unwrap()
            .iter()
            .map(|e| (e.wins, e.votes))
            .collect::<Vec<_>>(),
        [(0, 1), (0, 1), (0, 1)],
        "the tie is the one vote left"
    );
    assert_eq!(store.vote_count().unwrap(), 1);
    store.clear_all_votes().unwrap();
    assert_eq!(store.vote_count().unwrap(), 0);
    assert!(store.bench(&bench.id).unwrap().unwrap().votes.is_empty());
}

#[test]
fn startup_removes_chunk_files_no_stored_world_names_and_folders_of_deleted_chats() {
    let folder = Folder::new("collect");
    let store = folder.open();
    let chat = create(&store);
    let chat_id = id(&chat);
    set_world(&chat, TestWorld::default().with(0, 0, 0, "minecraft:stone"));
    save(&store, &chat);
    let chunks = folder.root.join("worlds").join(&chat_id).join("chunks");
    let names = || {
        let mut names: Vec<String> = std::fs::read_dir(&chunks)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let kept = names();
    assert_eq!(kept.len(), 1);
    std::fs::write(chunks.join("ffffffffffffffffffffffff.json"), "[]").unwrap();
    std::fs::create_dir_all(
        folder
            .root
            .join("worlds")
            .join("deleted-chat")
            .join("chunks"),
    )
    .unwrap();
    store.collect_chunks().unwrap();
    assert_eq!(names(), kept);
    assert!(!folder.root.join("worlds").join("deleted-chat").exists());
    let live = store.conversation(&chat_id).unwrap().unwrap();
    assert_eq!(
        block_at(&folder.root, &chat_id, &live.lock().unwrap().world, 0, 0, 0).as_deref(),
        Some("minecraft:stone")
    );
}

#[test]
fn a_bench_is_written_whole_or_not_at_all() {
    let folder = Folder::new("bench-whole");
    let store = folder.open();
    let before = store.list_benches().unwrap().len();
    let members = |second: &str| {
        vec![
            BenchMember {
                provider: "openrouter".into(),
                model: "a/model".into(),
                effort: None,
            },
            BenchMember {
                provider: second.into(),
                model: "b/model".into(),
                effort: None,
            },
        ]
    };
    let bench = |title: &str, members| NewBench {
        title: title.into(),
        blind: false,
        resolution: None,
        content: text("A hut"),
        prompts: vec![],
        members,
    };
    let error = store
        .create_bench("broken", bench("Broken", members("")))
        .unwrap_err();
    assert!(error.message.contains("provider and model are required"));
    let db = folder.sql();
    db.execute_batch("CREATE TRIGGER refuse_second BEFORE INSERT ON conversations WHEN NEW.bench_slot = 1 BEGIN SELECT RAISE(ABORT, 'refused'); END").unwrap();
    let error = store
        .create_bench("half", bench("Half", members("openrouter")))
        .unwrap_err();
    db.execute_batch("DROP TRIGGER refuse_second").unwrap();
    assert!(error.message.contains("refused"), "{}", error.message);
    assert!(store.bench("half").unwrap().is_none());
    assert_eq!(store.list_benches().unwrap().len(), before);
    assert!(store.bench_members("half").unwrap().is_empty());
    let (whole, chats) = store
        .create_bench("half", bench("Whole", members("openrouter")))
        .unwrap();
    assert_eq!(whole.round_content(0).unwrap().text(), "A hut");
    assert_eq!(
        store
            .bench_members("half")
            .unwrap()
            .iter()
            .map(id)
            .collect::<Vec<_>>(),
        chats.iter().map(id).collect::<Vec<_>>()
    );
}

fn calls() -> NewMessage {
    let call = |id: &str, name: &str| ToolCall {
        id: id.into(),
        kind: "function".into(),
        function: mcbench_store::FunctionCall {
            name: name.into(),
            arguments: "{}".into(),
        },
        extra_content: None,
    };
    NewMessage::reply(&Reply {
        content: String::new(),
        tool_calls: vec![call("done", "build"), call("lost", "bash")],
        ..Reply::default()
    })
}

fn tool(id: &str) -> NewMessage {
    NewMessage::tool_result(
        id,
        &mcbench_store::ToolTelemetry {
            name: "build".into(),
            started_at: "2026-11-17T00:00:00.000Z".into(),
            wall_ms: 1,
            ok: true,
            cancelled: false,
        },
        "{\"ok\":true}",
    )
}

#[test]
fn startup_ends_every_turn_the_server_stopped_in_the_middle_of_with_a_result_for_each_unanswered_call()
 {
    let folder = Folder::new("interrupt");
    let store = folder.open();
    let chat = |messages: Vec<NewMessage>| {
        let live = create(&store);
        prompt(&store, &live, "Build a hut", true);
        for message in messages {
            store
                .append_message(&mut live.lock().unwrap(), message, None)
                .unwrap();
        }
        save(&store, &live);
        id(&live)
    };
    let shots = NewMessage::screenshots("shots", &[]);
    let ids = [
        ("prompt", chat(vec![])),
        ("calls", chat(vec![calls()])),
        ("partial", chat(vec![calls(), tool("done")])),
        (
            "shots",
            chat(vec![calls(), tool("done"), tool("lost"), shots]),
        ),
        (
            "final",
            chat(vec![NewMessage::reply(&Reply {
                content: "Built".into(),
                ..Reply::default()
            })]),
        ),
        (
            "stopped",
            chat(vec![NewMessage::stopped("(stopped by user)", None, None)]),
        ),
        (
            "failed",
            chat(vec![NewMessage::error("Provider failed", None)]),
        ),
    ];
    drop(store);
    let store = folder.open();
    let ended = store.interrupt_unfinished().unwrap();
    let names: Vec<&str> = ids
        .iter()
        .filter(|(_, id)| ended.contains(id))
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(names, ["prompt", "calls", "partial", "shots"]);
    drop(store);
    let store = folder.open();
    let tail = |id: &str| -> Vec<String> {
        let live = store.conversation(id).unwrap().unwrap();
        let c = live.lock().unwrap();
        c.messages[1..]
            .iter()
            .map(|m| match &m.message {
                Message::Tool(t) => {
                    let interrupted = mcbench_store::json::parse(&t.content)
                        .unwrap()
                        .get("interrupted")
                        .is_some();
                    format!(
                        "tool {}{}",
                        t.tool_call_id,
                        if interrupted { " interrupted" } else { "" }
                    )
                }
                other => role(other).to_string(),
            })
            .collect()
    };
    let id = |name: &str| ids.iter().find(|(n, _)| *n == name).unwrap().1.clone();
    assert_eq!(tail(&id("prompt")), ["error"]);
    assert_eq!(
        tail(&id("calls")),
        [
            "assistant",
            "tool done interrupted",
            "tool lost interrupted",
            "error"
        ]
    );
    assert_eq!(
        tail(&id("partial")),
        ["assistant", "tool done", "tool lost interrupted", "error"]
    );
    assert_eq!(
        tail(&id("shots")),
        ["assistant", "tool done", "tool lost", "user", "error"]
    );
    let live = store.conversation(&id("prompt")).unwrap().unwrap();
    assert_eq!(
        content_of(&live.lock().unwrap().messages.last().unwrap().message),
        mcbench_store::INTERRUPTED
    );
    assert_eq!(tail(&id("final")), ["assistant"]);
    assert!(
        store.interrupt_unfinished().unwrap().is_empty(),
        "every stored head now ends a turn"
    );
}

#[test]
fn a_chat_that_cannot_be_loaded_at_startup_is_logged_and_the_other_chats_turns_are_still_ended() {
    let folder = Folder::new("broken-head");
    let store = folder.open();
    let unfinished = || {
        let live = create(&store);
        prompt(&store, &live, "Build a hut", true);
        save(&store, &live);
        id(&live)
    };
    let (broken, fine) = (unfinished(), unfinished());
    let db = folder.sql();
    let (head, payload): (String, String) = db
        .query_row(
            "SELECT m.id, m.payload_json FROM conversations c JOIN messages m ON m.id = c.current_message_id WHERE c.id = ?",
            [&broken],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    db.execute(
        "UPDATE messages SET payload_json = ? WHERE id = ?",
        [&payload[..10], &head],
    )
    .unwrap();
    drop(store);
    let store = folder.open();
    // The failure is logged to stderr naming its chat (`[store] the turn the
    // server stopped in <id> could not be ended: ...`).
    let ended = store.interrupt_unfinished().unwrap();
    assert!(ended.contains(&fine), "the loadable chat is ended");
    assert!(!ended.contains(&broken));
    db.execute(
        "UPDATE messages SET payload_json = ? WHERE id = ?",
        [&payload, &head],
    )
    .unwrap();
    let live = store.conversation(&fine).unwrap().unwrap();
    assert_eq!(
        content_of(&live.lock().unwrap().messages.last().unwrap().message),
        mcbench_store::INTERRUPTED
    );
}

#[test]
fn a_turn_still_under_way_is_ended_as_failed_with_the_reason_given_and_a_head_that_ends_a_turn_is_left_alone()
 {
    let folder = Folder::new("unfinished");
    let store = folder.open();
    let chat = create(&store);
    assert!(
        !store
            .end_unfinished_turn(&mut chat.lock().unwrap(), mcbench_store::UNSAVED)
            .unwrap(),
        "a chat with no turn"
    );
    prompt(&store, &chat, "Build a hut", true);
    let call = ToolCall {
        id: "lost".into(),
        kind: "function".into(),
        function: mcbench_store::FunctionCall {
            name: "bash".into(),
            arguments: "{}".into(),
        },
        extra_content: None,
    };
    store
        .append_message(
            &mut chat.lock().unwrap(),
            NewMessage::reply(&Reply {
                tool_calls: vec![call],
                ..Reply::default()
            }),
            None,
        )
        .unwrap();
    save(&store, &chat);
    assert!(
        store
            .end_unfinished_turn(&mut chat.lock().unwrap(), mcbench_store::UNSAVED)
            .unwrap()
    );
    let chat_id = id(&chat);
    drop(chat);
    drop(store);
    let store = folder.open();
    let stored = store.conversation(&chat_id).unwrap().unwrap();
    let mut s = stored.lock().unwrap();
    assert_eq!(
        s.messages
            .iter()
            .map(|m| role(&m.message))
            .collect::<Vec<_>>(),
        ["user", "assistant", "tool", "error"]
    );
    let Message::Tool(t) = &s.messages[2].message else {
        panic!()
    };
    assert_eq!(t.tool_call_id, "lost");
    assert_eq!(content_of(&s.messages[3].message), mcbench_store::UNSAVED);
    assert!(
        !store
            .end_unfinished_turn(&mut s, mcbench_store::UNSAVED)
            .unwrap()
    );
    assert_eq!(s.messages.len(), 4);
}

/// One model's chat cannot be saved while `work` runs.
fn failing_save<T>(folder: &Folder, id: &str, work: impl FnOnce() -> T) -> T {
    let db = folder.sql();
    db.execute_batch(&format!(
        "CREATE TRIGGER refuse_chat BEFORE UPDATE ON conversations WHEN NEW.id = '{id}' BEGIN SELECT RAISE(ABORT, 'the disk is full'); END"
    ))
    .unwrap();
    let result = work();
    db.execute_batch("DROP TRIGGER refuse_chat").unwrap();
    result
}

#[test]
fn a_bench_round_edit_branch_switch_or_retry_is_stored_whole_or_not_at_all() {
    let folder = Folder::new("bench-change");
    let store = folder.open();
    let (mut bench, members) = new_bench(
        &store,
        &folder.host,
        "Whole",
        "A tower",
        &[("a/model", None), ("b/model", None)],
        true,
    );
    let turn = |member: &LiveConversation, round: &str, content: &str| {
        bench_prompt(&store, member, round, content);
        reply(&store, member, "Built");
        save(&store, member);
    };
    let first = bench.round_id(0).unwrap().to_string();
    for member in &members {
        turn(member, &first, "A tower");
    }
    store
        .append_round(&mut bench, &text("Add a roof"), &[])
        .unwrap();
    let roof = bench.round_id(1).unwrap().to_string();
    for member in &members {
        turn(member, &roof, "Add a roof");
    }
    let [one, two]: [String; 2] = members
        .iter()
        .map(id)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    store
        .cast_vote(&mut bench, &roof, Some(&one), &[one.clone(), two.clone()])
        .unwrap();
    drop(members);
    let bench_id = bench.id.clone();
    let lines = || -> Vec<Option<String>> {
        let db = folder.sql();
        let mut statement = db.prepare("SELECT current_message_id FROM conversations WHERE bench_id = ? ORDER BY bench_slot").unwrap();
        let stored: Vec<Option<String>> = statement
            .query_map([&bench_id], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let served: Vec<Option<String>> = store
            .bench_members(&bench_id)
            .unwrap()
            .iter()
            .map(|m| {
                m.lock()
                    .unwrap()
                    .current_message_id
                    .as_ref()
                    .map(|m| m.0.clone())
            })
            .collect();
        assert_eq!(served, stored);
        let mut out = vec![store.bench(&bench_id).unwrap().unwrap().head];
        out.extend(stored);
        out
    };
    let before = lines();
    let error = failing_save(&folder, &two, || {
        store.edit_bench_round(&mut bench, 1, &text("Add a spire"), &[])
    })
    .unwrap_err();
    assert!(error.message.contains("the disk is full"));
    assert_eq!(lines(), before, "no model goes back without the bench");
    assert_eq!(
        bench
            .rounds
            .iter()
            .map(|r| r.get("id").unwrap().as_str().unwrap().to_string())
            .collect::<Vec<_>>(),
        [first.clone(), roof.clone()]
    );
    assert_eq!(bench.nodes.len(), 2, "and the new version is not stored");
    let second = store.conversation(&two).unwrap().unwrap();
    let error = failing_save(&folder, &two, || {
        store.retry_bench_round(&mut bench, &mut second.lock().unwrap())
    })
    .unwrap_err();
    assert!(error.message.contains("the disk is full"));
    drop(second);
    let votes = store.bench(&bench.id).unwrap().unwrap().votes;
    assert_eq!(
        votes.iter().find(|(r, _)| *r == roof).unwrap().1,
        mcbench_store::Vote {
            pick: Some(one.clone()),
            blind: true
        },
        "a retry that failed takes no vote back"
    );
    assert_eq!(bench.votes, votes);
    assert_eq!(lines(), before);
    store
        .edit_bench_round(&mut bench, 1, &text("Add a spire"), &[])
        .unwrap();
    let spire = bench.round_id(1).unwrap().to_string();
    for member in store.bench_members(&bench.id).unwrap() {
        turn(&member, &spire, "Add a spire");
    }
    let on_spire = lines();
    let error = failing_save(&folder, &two, || {
        store.follow_bench_branch(&mut bench, &roof)
    })
    .unwrap_err();
    assert!(error.message.contains("the disk is full"));
    assert_eq!(
        lines(),
        on_spire,
        "the bench does not change line without its models"
    );
    assert_eq!(bench.head, on_spire[0]);
    store.follow_bench_branch(&mut bench, &roof).unwrap();
    assert_eq!(lines()[0].as_deref(), Some(roof.as_str()));
}

#[cfg(unix)]
#[test]
fn attachments_are_written_whole_or_not_at_all_and_those_of_a_message_that_was_not_saved_are_removed()
 {
    let folder = Folder::new("attachments-whole");
    let store = folder.open();
    let chat = "whole-chat";
    let file = |name: &str| data_file(name, &[0; 64], "application/octet-stream");
    let used = store.storage().used(chat).unwrap();
    // The disk fails at the second file: a read-only file stands where it
    // goes (the host's ids are known: the two files take ids 1 and 2).
    let dir = folder.root.join("attachments").join(chat);
    std::fs::create_dir_all(&dir).unwrap();
    let blocker = dir.join("00000002-b.bin");
    std::fs::write(&blocker, "").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&blocker, std::fs::Permissions::from_mode(0o444)).unwrap();
    }
    let error = store
        .save_attachments(chat, &[file("a.bin"), file("b.bin")])
        .unwrap_err();
    assert!(
        error.message.contains("ermission denied"),
        "{}",
        error.message
    );
    assert!(!dir.exists(), "the first file does not stay");
    assert_eq!(
        store.storage().used(chat).unwrap(),
        used,
        "and its bytes are given back"
    );
    let kept = store.save_attachments(chat, &[file("kept.bin")]).unwrap();
    let saved = store
        .save_attachments(chat, &[file("a.bin"), file("b.bin")])
        .unwrap();
    store.discard_attachments(chat, &saved).unwrap();
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [std::path::Path::new(&kept[0].url)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()]
    );
    assert_eq!(store.storage().used(chat).unwrap(), used + 64);
}

#[test]
fn a_fork_that_cannot_be_saved_leaves_none_of_its_copied_files() {
    let folder = Folder::new("fork-fails");
    let store = folder.open();
    let (_, members) = new_bench(
        &store,
        &folder.host,
        "Forked tower",
        "Build a tower",
        &[("test/model", None)],
        false,
    );
    let source = &members[0];
    let attachment = store
        .save_attachment(&id(source), &data_file("ref.png", b"png", "image/png"))
        .unwrap();
    {
        let mut c = source.lock().unwrap();
        let world = c.world.reference();
        let content = Content::Parts(vec![
            Part::Text {
                text: "Build a tower".into(),
            },
            Part::Attachment { attachment },
        ]);
        store
            .append_message(&mut c, NewMessage::prompt(&content, &[]), Some(world))
            .unwrap();
    }
    set_world(
        source,
        TestWorld::default().with(0, 0, 0, "minecraft:stone"),
    );
    reply(&store, source, "Built");
    save(&store, source);
    let folders = || {
        ["attachments", "worlds"]
            .iter()
            .map(|d| {
                let mut names: Vec<String> = std::fs::read_dir(folder.root.join(d))
                    .unwrap()
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                names
            })
            .collect::<Vec<_>>()
    };
    let before = folders();
    let chats = store.list_conversations().unwrap().len();
    let db = folder.sql();
    db.execute_batch("CREATE TRIGGER refuse_fork BEFORE INSERT ON conversations WHEN NEW.title = 'Forked tower' AND NEW.bench_id IS NULL BEGIN SELECT RAISE(ABORT, 'the disk is full'); END").unwrap();
    let s = source.lock().unwrap().clone();
    let error = store
        .fork_conversation(&s, s.messages.len(), &s.world, None)
        .unwrap_err();
    db.execute_batch("DROP TRIGGER refuse_fork").unwrap();
    assert!(error.message.contains("the disk is full"));
    assert_eq!(folders(), before);
    assert_eq!(store.list_conversations().unwrap().len(), chats);
}

#[test]
fn an_empty_chat_is_not_found_by_search_and_is_removed_after_the_retention_period_unless_this_run_used_it()
 {
    let folder = Folder::new("empty");
    let store = folder.open();
    let (empty, held, started) = (id(&create(&store)), id(&create(&store)), create(&store));
    prompt(&store, &started, "Build a well", true);
    save(&store, &started);
    let started = id(&started);
    let found: Vec<String> = store
        .search(SearchScope::Build, "New build")
        .unwrap()
        .into_iter()
        .map(|h| h.id)
        .collect();
    assert!(
        found.contains(&started),
        "a chat with a message is found by its title"
    );
    assert!(
        !found.contains(&empty) && !found.contains(&held),
        "an empty chat is not"
    );
    let attachments = folder.root.join("attachments").join(&empty);
    std::fs::create_dir_all(&attachments).unwrap();
    drop(store);
    let store = folder.open();
    assert!(store.conversation(&held).unwrap().is_some());
    let rows = || {
        let db = folder.sql();
        let mut ids: Vec<String> = [&empty, &held, &started]
            .iter()
            .filter(|id| count(&db, "SELECT COUNT(*) FROM conversations WHERE id = ?", id) == 1)
            .map(|id| id.to_string())
            .collect();
        ids.sort();
        ids
    };
    let mut all = vec![empty.clone(), held.clone(), started.clone()];
    all.sort();
    store
        .clear_expired_archive(T0 + ARCHIVE_TTL_MS - 60_000)
        .unwrap();
    assert_eq!(
        rows(),
        all,
        "an empty chat is kept for the retention period"
    );
    store
        .clear_expired_archive(T0 + ARCHIVE_TTL_MS + 60_000)
        .unwrap();
    let mut kept = vec![held.clone(), started.clone()];
    kept.sort();
    assert_eq!(
        rows(),
        kept,
        "then removed, unless this run made it or was asked for it"
    );
    assert!(store.conversation(&empty).unwrap().is_none());
    assert!(!attachments.exists());
    assert_eq!(
        count(
            &folder.sql(),
            "SELECT COUNT(*) FROM search_docs WHERE owner = ?",
            &empty
        ),
        0
    );
    let live = store.conversation(&held).unwrap().unwrap();
    prompt(&store, &live, "Build a hut", false);
    save(&store, &live);
    assert!(
        store
            .list_conversations()
            .unwrap()
            .iter()
            .any(|row| row.id.0 == held)
    );
}

#[test]
fn a_chats_resolution_is_stored_with_it_listed_and_kept_by_its_fork() {
    let folder = Folder::new("resolution");
    let store = folder.open();
    let chat = create(&store);
    assert_eq!(
        chat.lock().unwrap().resolution,
        1,
        "a chat starts at Standard"
    );
    chat.lock().unwrap().resolution = 5;
    prompt(&store, &chat, "A lighthouse", true);
    reply(&store, &chat, "Built.");
    save(&store, &chat);
    let chat_id = id(&chat);
    drop(chat);
    drop(store);
    let store = folder.open();
    let stored = store.conversation(&chat_id).unwrap().unwrap();
    assert_eq!(stored.lock().unwrap().resolution, 5);
    assert_eq!(
        store
            .list_conversations()
            .unwrap()
            .iter()
            .find(|r| r.id.0 == chat_id)
            .unwrap()
            .resolution,
        5
    );
    let s = stored.lock().unwrap().clone();
    let fork = id(&store
        .fork_conversation(&s, s.messages.len(), &s.world, None)
        .unwrap());
    assert_eq!(
        store
            .conversation(&fork)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .resolution,
        5
    );
    stored.lock().unwrap().resolution = 1;
    save(&store, &stored);
    drop(stored);
    drop(store);
    let store = folder.open();
    assert_eq!(
        store
            .conversation(&chat_id)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .resolution,
        1,
        "the resolution can be changed again"
    );
}

#[test]
fn a_benchs_resolution_is_every_models_chats_and_is_listed_with_it() {
    let folder = Folder::new("bench-resolution");
    let store = folder.open();
    let members = || {
        vec![
            BenchMember {
                provider: "openrouter".into(),
                model: "test/one".into(),
                effort: None,
            },
            BenchMember {
                provider: "openrouter".into(),
                model: "test/two".into(),
                effort: None,
            },
        ]
    };
    let make = |id: &str, title: &str, resolution: u32, content: &str| {
        store
            .create_bench(
                id,
                NewBench {
                    title: title.into(),
                    blind: false,
                    resolution: Some(resolution),
                    content: text(content),
                    prompts: vec![],
                    members: members(),
                },
            )
            .unwrap()
    };
    let fine = make("fine", "Fine bench", 10, "A cathedral");
    let plain = make("plain", "Plain bench", 1, "A shed");
    assert_eq!(
        fine.1
            .iter()
            .map(|c| c.lock().unwrap().resolution)
            .collect::<Vec<_>>(),
        [10, 10]
    );
    assert_eq!(
        plain
            .1
            .iter()
            .map(|c| c.lock().unwrap().resolution)
            .collect::<Vec<_>>(),
        [1, 1]
    );
    let listed: Vec<Json> = store
        .list_benches()
        .unwrap()
        .iter()
        .map(ListedBench::to_json)
        .collect();
    let resolution = |id: &str| {
        listed
            .iter()
            .find(|r| r.get("id").unwrap().as_str() == Some(id))
            .unwrap()
            .get("resolution")
            .unwrap()
            .as_f64()
    };
    assert_eq!(resolution("fine"), Some(10.0));
    assert_eq!(resolution("plain"), Some(1.0));
}

const CASTLE: (&str, &str, &str) = (
    "castle",
    "Castle",
    "Build a castle with a moat and four towers.",
);
const TOWER: (&str, &str) = ("tower", "Tower");

fn link(id: &str, title: &str) -> PromptLink {
    PromptLink {
        id: id.into(),
        title: title.into(),
    }
}

#[test]
fn a_message_sent_from_library_prompts_stores_and_returns_its_links_and_its_fork_keeps_them() {
    let folder = Folder::new("links");
    let store = folder.open();
    let chat = create(&store);
    let links = vec![link(CASTLE.0, CASTLE.1), link(TOWER.0, "Tall tower")];
    {
        let mut c = chat.lock().unwrap();
        let world = c.world.reference();
        store
            .append_message(
                &mut c,
                NewMessage::prompt(&text(CASTLE.2), &links),
                Some(world),
            )
            .unwrap();
    }
    reply(&store, &chat, "Built.");
    save(&store, &chat);
    let plain = create(&store);
    prompt(&store, &plain, "A shed", true);
    save(&store, &plain);
    let (chat_id, plain_id) = (id(&chat), id(&plain));
    drop((chat, plain));
    drop(store);
    let store = folder.open();
    let first = |id: &str| {
        store
            .conversation(id)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .messages[0]
            .message
            .prompts()
            .to_vec()
    };
    assert_eq!(first(&chat_id), links);
    assert!(first(&plain_id).is_empty());
    let source = store
        .conversation(&chat_id)
        .unwrap()
        .unwrap()
        .lock()
        .unwrap()
        .clone();
    let fork = id(&store
        .fork_conversation(&source, 2, &source.world, None)
        .unwrap());
    drop(store);
    let store = folder.open();
    assert_eq!(
        store
            .conversation(&fork)
            .unwrap()
            .unwrap()
            .lock()
            .unwrap()
            .messages[0]
            .message
            .prompts()
            .to_vec(),
        links
    );
}

#[test]
fn a_bench_round_stores_the_library_prompts_it_was_sent_from() {
    let folder = Folder::new("round-links");
    let store = folder.open();
    let members = vec![
        BenchMember {
            provider: "openrouter".into(),
            model: "test/one".into(),
            effort: None,
        },
        BenchMember {
            provider: "openrouter".into(),
            model: "test/two".into(),
            effort: None,
        },
    ];
    let (mut bench, _) = store
        .create_bench(
            "linked",
            NewBench {
                title: "Linked bench".into(),
                blind: false,
                resolution: Some(1),
                content: text(CASTLE.2),
                prompts: vec![link(CASTLE.0, CASTLE.1)],
                members,
            },
        )
        .unwrap();
    assert_eq!(
        bench.round_prompts(0).unwrap(),
        Some(vec![link(CASTLE.0, CASTLE.1)])
    );
    store
        .append_round(&mut bench, &text("Add a keep"), &[])
        .unwrap();
    store
        .append_round(
            &mut bench,
            &text("Build a huge tower with a spiral stair inside."),
            &[link(TOWER.0, TOWER.1)],
        )
        .unwrap();
    drop(store);
    let store = folder.open();
    let stored = store.bench("linked").unwrap().unwrap();
    assert_eq!(
        (0..3)
            .map(|i| stored.round_prompts(i).unwrap())
            .collect::<Vec<_>>(),
        [
            Some(vec![link(CASTLE.0, CASTLE.1)]),
            None,
            Some(vec![link(TOWER.0, TOWER.1)])
        ]
    );
}

#[test]
fn the_chat_list_carries_the_prompts_of_every_branch_of_a_chat_and_each_prompts_latest_title() {
    let folder = Folder::new("list-prompts");
    let store = folder.open();
    let (castle, tower) = ("castle-uuid", "tower-uuid");
    let send = |live: &LiveConversation, content: &str, links: &[PromptLink]| {
        let mut c = live.lock().unwrap();
        let world = c.world.reference();
        let id = store
            .append_message(
                &mut c,
                NewMessage::prompt(&text(content), links),
                Some(world),
            )
            .unwrap()
            .id
            .0;
        store.save(&mut c).unwrap();
        id
    };
    let chat = create(&store);
    let first = send(&chat, CASTLE.2, &[link(castle, "Old castle")]);
    store.revert(&mut chat.lock().unwrap(), &first).unwrap();
    send(
        &chat,
        "Build a huge tower with a spiral stair inside.",
        &[link(tower, "Tower")],
    );
    let plain = create(&store);
    send(&plain, "A shed", &[]);
    let row = |id: &str| {
        store
            .list_conversations()
            .unwrap()
            .into_iter()
            .find(|r| r.id.0 == id)
            .unwrap()
    };
    let mut prompts = row(&id(&chat)).prompts;
    prompts.sort();
    assert_eq!(
        prompts,
        [castle, tower],
        "the reverted message still counts"
    );
    assert!(row(&id(&plain)).prompts.is_empty());
    let title = |id: &str| {
        store
            .prompt_titles()
            .unwrap()
            .get(id)
            .and_then(Json::as_str)
            .map(str::to_string)
    };
    assert_eq!(title(castle).as_deref(), Some("Old castle"));
    let later = create(&store);
    send(&later, CASTLE.2, &[link(castle, "Castle")]);
    assert_eq!(
        title(castle).as_deref(),
        Some("Castle"),
        "the title it was last sent under"
    );
    let source = later.lock().unwrap().clone();
    let fork = id(&store
        .fork_conversation(&source, 1, &source.world, None)
        .unwrap());
    assert_eq!(row(&fork).prompts, [castle]);
    for chat in [id(&later), fork] {
        store.archive_conversation(&chat).unwrap();
    }
    assert_eq!(
        title(castle).as_deref(),
        Some("Old castle"),
        "an archived chat is not listed, nor are its titles"
    );
    store.archive_conversation(&id(&chat)).unwrap();
    assert_eq!(title(tower), None);
}

#[test]
fn a_bench_is_listed_with_the_prompts_any_of_its_rounds_came_from() {
    let folder = Folder::new("bench-prompts");
    let store = folder.open();
    let castle = "bench-castle";
    let members = vec![
        BenchMember {
            provider: "openrouter".into(),
            model: "test/one".into(),
            effort: None,
        },
        BenchMember {
            provider: "openrouter".into(),
            model: "test/two".into(),
            effort: None,
        },
    ];
    let (mut bench, _) = store
        .create_bench(
            "listed",
            NewBench {
                title: "Listed bench".into(),
                blind: false,
                resolution: Some(1),
                content: text(CASTLE.2),
                prompts: vec![link(castle, "Bench castle")],
                members,
            },
        )
        .unwrap();
    store
        .append_round(&mut bench, &text("Add a keep"), &[])
        .unwrap();
    store
        .append_round(&mut bench, &text(CASTLE.2), &[link(castle, "Bench castle")])
        .unwrap();
    let listed = store
        .list_benches()
        .unwrap()
        .iter()
        .map(ListedBench::to_json)
        .find(|r| r.get("id").unwrap().as_str() == Some("listed"))
        .unwrap();
    assert_eq!(listed.get("prompts").unwrap(), &Json::from(vec![castle]));
    assert_eq!(
        store
            .prompt_titles()
            .unwrap()
            .get(castle)
            .and_then(Json::as_str),
        Some("Bench castle"),
        "a round not yet sent to any model is named too"
    );
    store.archive_bench("listed").unwrap();
    assert!(store.prompt_titles().unwrap().get(castle).is_none());
}

#[test]
fn version_13_is_refused() {
    let folder = Folder::new("v13");
    drop(folder.open());
    folder
        .sql()
        .execute_batch("PRAGMA user_version = 13")
        .unwrap();
    let error = common::open(&folder.root, folder.host.clone(), common::limits(1 << 30))
        .map(|_| ())
        .unwrap_err();
    assert_eq!(
        error,
        Error::plain("Database schema version 13 is unsupported (expected 14)")
    );
    let _ = mcbench_store::World::new();
}
