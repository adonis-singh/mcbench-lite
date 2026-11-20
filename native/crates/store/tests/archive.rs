//! The archive rules of `test/server-archive.test.js` (4 of 5, through the
//! store instead of `handleApi`; the fifth tests the chat list stream's
//! counts, which `server/index.js` publishes) and the attachment rules of
//! `test/attachment-serving.test.js` (2 of 3: stored names and types, and
//! images and attachments resolving to their files; the headers and the
//! request timeouts are the HTTP server's, which the native app does not
//! have).

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{DetHost, TestWorld};
use mcbench_store::json::Json;
use mcbench_store::{
    ARCHIVE_TTL_MS, BenchMember, Content, ImageData, NewBench, NewConversation, NewMessage, Store,
    object,
};

const T0: i64 = 1794873600000;
const FOLDERS: [&str; 5] = ["worlds", "images", "attachments", "sandbox", "harness"];

fn folders(root: &Path, id: &str) -> Vec<&'static str> {
    FOLDERS
        .iter()
        .copied()
        .filter(|f| root.join(f).join(id).exists())
        .collect()
}

/// A chat with something in each of its folders, as the test's `fill`.
fn fill(store: &Store, root: &Path, id: &str) {
    let live = store.conversation(id).unwrap().unwrap();
    let mut c = live.lock().unwrap();
    c.world = TestWorld::default()
        .with(0, 0, 0, "minecraft:stone")
        .into_chat();
    let message = NewMessage::reply(&mcbench_store::Reply {
        content: "Placed a block".into(),
        ..Default::default()
    });
    store.append_message(&mut c, message, None).unwrap();
    store.save(&mut c).unwrap();
    store
        .save_images(id, &[ImageData::Png(b"png".to_vec())], "")
        .unwrap();
    for folder in ["sandbox", "harness"] {
        std::fs::create_dir_all(root.join(folder).join(id)).unwrap();
        std::fs::write(root.join(folder).join(id).join("notes.txt"), "kept").unwrap();
    }
}

fn notes() -> Json {
    object! { "name" => "notes.txt", "data" => "data:text/plain;base64,bm90ZXM=" }
}

fn new_chat(store: &Store, root: &Path, text: &str) -> String {
    let live = store
        .create_conversation("openrouter", "chat/model", NewConversation::default())
        .unwrap();
    let id = live.lock().unwrap().id.0.clone();
    let files = store.save_attachments(&id, &[notes()]).unwrap();
    {
        let mut c = live.lock().unwrap();
        let world = c.world.reference();
        let content = Content::Parts(vec![
            mcbench_store::Part::Text { text: text.into() },
            mcbench_store::Part::Attachment {
                attachment: files[0].clone(),
            },
        ]);
        store
            .append_message(&mut c, NewMessage::prompt(&content, &[]), Some(world))
            .unwrap();
        store.save(&mut c).unwrap();
        store.set_title(&mut c, "A title").unwrap();
    }
    fill(store, root, &id);
    id
}

fn new_bench(store: &Store, root: &Path, host: &DetHost, text: &str) -> (String, Vec<String>) {
    let id = mcbench_store::Host::uuid(host);
    let round_file = store.save_attachments(&id, &[notes()]).unwrap().remove(0);
    let members = ["bench/a", "bench/b"]
        .iter()
        .map(|m| BenchMember {
            provider: "openrouter".into(),
            model: m.to_string(),
            effort: None,
        })
        .collect();
    let (mut bench, chats) = store
        .create_bench(
            &id,
            NewBench {
                title: text.into(),
                blind: true,
                resolution: Some(1),
                content: Content::Text(text.into()),
                prompts: vec![],
                members,
            },
        )
        .unwrap();
    let ids: Vec<String> = chats
        .iter()
        .map(|c| c.lock().unwrap().id.0.clone())
        .collect();
    drop(chats);
    for member in &ids {
        // The round's file, each model's own copy (startRound).
        store.share_attachment(&round_file, member).unwrap();
        fill(store, root, member);
    }
    let round = bench.round_id(0).unwrap().to_string();
    store
        .cast_vote(&mut bench, &round, Some(&ids[0]), &ids)
        .unwrap();
    (id, ids)
}

fn rows(root: &Path, id: &str) -> [i64; 5] {
    let db = rusqlite::Connection::open(root.join("mcbench.sqlite3")).unwrap();
    let count = |sql: &str| db.query_row(sql, [id], |r| r.get::<_, i64>(0)).unwrap();
    [
        db.query_row(
            "SELECT count(*) FROM conversations WHERE id = ?1 OR bench_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap(),
        count("SELECT count(*) FROM benches WHERE id = ?"),
        count("SELECT count(*) FROM messages WHERE conversation_id = ?"),
        count("SELECT count(*) FROM bench_votes WHERE bench_id = ?"),
        count("SELECT count(*) FROM search_docs WHERE owner = ?"),
    ]
}

fn open(name: &str) -> (PathBuf, Arc<DetHost>, Store) {
    let root = common::scratch(name);
    let host = DetHost::new(T0);
    let store = common::open(&root, host.clone(), common::limits(1 << 32)).unwrap();
    (root, host, store)
}

#[test]
fn an_archived_chat_keeps_its_files_is_listed_with_its_expiry_and_is_restored_as_it_was() {
    let (root, _, store) = open("archive-chat");
    let id = new_chat(&store, &root, "Build a hut");
    let transcript: Vec<String> = store
        .conversation(&id)
        .unwrap()
        .unwrap()
        .lock()
        .unwrap()
        .messages
        .iter()
        .map(|m| m.public_text())
        .collect();
    let before = store.conversation(&id).unwrap().unwrap();
    store.archive_conversation(&id).unwrap();
    assert!(
        !store
            .list_conversations()
            .unwrap()
            .iter()
            .any(|r| r.id.0 == id)
    );
    assert!(store.conversation(&id).unwrap().is_none());
    assert_eq!(folders(&root, &id), FOLDERS, "nothing on disk is removed");
    let entry = store.list_archive().unwrap().conversations.remove(0);
    assert_eq!(
        (
            entry.id.as_str(),
            entry.title.as_str(),
            entry.provider.as_str(),
            entry.model.as_str()
        ),
        (id.as_str(), "A title", "openrouter", "chat/model")
    );
    assert_eq!(entry.archived_at, T0);
    assert_eq!(entry.expires_at, entry.archived_at + ARCHIVE_TTL_MS);
    store.restore_conversation(&id).unwrap();
    assert!(store.list_archive().unwrap().conversations.is_empty());
    let restored = store.conversation(&id).unwrap().unwrap();
    assert!(
        !Arc::ptr_eq(&restored, &before),
        "it is read back from what is stored"
    );
    let r = restored.lock().unwrap();
    assert_eq!(
        r.messages
            .iter()
            .map(|m| m.public_text())
            .collect::<Vec<_>>(),
        transcript
    );
    assert_eq!(r.world.size(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join("sandbox").join(&id).join("notes.txt")).unwrap(),
        "kept"
    );
    drop(r);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_archived_chat_deleted_for_good_leaves_no_row_no_search_entry_and_no_folder_and_one_not_archived_is_refused()
 {
    let (root, _, store) = open("archive-delete");
    let id = new_chat(&store, &root, "Build a well");
    let refused = store.delete_conversation(&id).unwrap_err();
    assert_eq!(
        (refused.status, refused.message.as_str()),
        (Some(409), "Archive it first")
    );
    assert_eq!(
        store.restore_conversation(&id).unwrap_err().status,
        Some(404),
        "what is not archived cannot be restored"
    );
    assert!(store.storage().used(&id).unwrap() > 0);
    store.archive_conversation(&id).unwrap();
    store.delete_conversation(&id).unwrap();
    assert_eq!(rows(&root, &id), [0; 5]);
    assert!(folders(&root, &id).is_empty());
    assert_eq!(
        store.storage().used(&id).unwrap(),
        0,
        "its measurement is forgotten"
    );
    assert!(
        !store
            .list_archive()
            .unwrap()
            .conversations
            .iter()
            .any(|r| r.id == id)
    );
    assert_eq!(
        store.delete_conversation(&id).unwrap_err().status,
        Some(404)
    );
    assert_eq!(
        store.restore_conversation(&id).unwrap_err().status,
        Some(404)
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_archived_bench_keeps_its_models_rounds_and_votes_which_leave_the_leaderboard_until_it_is_restored()
 {
    let (root, host, store) = open("archive-bench");
    let (id, members) = new_bench(&store, &root, &host, "A tower");
    let ranked = store.leaderboard().unwrap();
    assert_eq!(ranked.len(), 2);
    assert_eq!(store.archive_bench(&id).unwrap().len(), 2);
    assert!(store.leaderboard().unwrap().is_empty());
    for member in &members {
        assert_eq!(folders(&root, member), FOLDERS);
    }
    let archive = store.list_archive().unwrap();
    assert!(
        !archive
            .conversations
            .iter()
            .any(|r| members.contains(&r.id)),
        "its models are part of the bench"
    );
    let entry = &archive.benches[0];
    assert_eq!(
        (entry.id.as_str(), entry.models, entry.expires_at),
        (id.as_str(), 2, entry.archived_at + ARCHIVE_TTL_MS)
    );
    for refused in [
        store.restore_conversation(&members[0]).unwrap_err(),
        store.delete_conversation(&members[0]).unwrap_err(),
    ] {
        assert_eq!(
            (refused.status, refused.message.as_str()),
            (Some(409), "A bench chat is changed only through its bench")
        );
    }
    store.restore_bench(&id).unwrap();
    assert_eq!(
        store.leaderboard().unwrap(),
        ranked,
        "its votes count again"
    );
    let bench = store.bench(&id).unwrap().unwrap();
    assert_eq!((bench.rounds.len(), bench.votes.len()), (1, 1));
    let sizes: Vec<(String, usize)> = store
        .bench_members(&id)
        .unwrap()
        .iter()
        .map(|m| {
            let m = m.lock().unwrap();
            (m.id.0.clone(), m.world.size())
        })
        .collect();
    assert_eq!(
        sizes,
        members.iter().map(|m| (m.clone(), 1)).collect::<Vec<_>>()
    );
    assert_eq!(store.restore_bench(&id).unwrap_err().status, Some(404));
    let refused = store.delete_bench(&id).unwrap_err();
    assert_eq!(
        (refused.status, refused.message.as_str()),
        (Some(409), "Archive it first")
    );
    store.archive_bench(&id).unwrap();
    store.delete_bench(&id).unwrap();
    assert_eq!(rows(&root, &id), [0; 5]);
    for member in &members {
        assert_eq!(rows(&root, member), [0; 5]);
        assert!(folders(&root, member).is_empty());
    }
    assert!(
        folders(&root, &id).is_empty(),
        "the bench's own attachments go too"
    );
    assert_eq!(store.delete_bench(&id).unwrap_err().status, Some(404));
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_archive_lists_the_newest_first_and_the_purge_after_7_days_deletes_what_a_delete_does() {
    let (root, host, store) = open("archive-purge");
    let chat = new_chat(&store, &root, "Build a barn");
    let (bench, members) = new_bench(&store, &root, &host, "A bridge");
    store.archive_conversation(&chat).unwrap();
    host.set_now(T0 + 5);
    store.archive_bench(&bench).unwrap();
    let later = new_chat(&store, &root, "Build a mill");
    host.set_now(T0 + 10);
    store.archive_conversation(&later).unwrap();
    let list = store.list_archive().unwrap();
    assert_eq!(
        list.conversations
            .iter()
            .take(2)
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        [later.clone(), chat.clone()]
    );
    assert_eq!(list.benches[0].id, bench);
    let expires: Vec<i64> = list
        .conversations
        .iter()
        .map(|r| r.expires_at)
        .chain(list.benches.iter().map(|r| r.expires_at))
        .collect();
    let due = |now: i64| expires.iter().copied().filter(|at| *at > now).min();
    let first = list.conversations[1].expires_at;
    assert_eq!(
        store.next_archive_expiry(first - 1).unwrap(),
        due(first - 1)
    );
    store.clear_expired_archive(first - 1).unwrap();
    assert_eq!(
        folders(&root, &chat),
        FOLDERS,
        "nothing goes before its 7 days"
    );
    store
        .clear_expired_archive(list.benches[0].expires_at + 1)
        .unwrap();
    assert_eq!(
        store
            .next_archive_expiry(list.benches[0].expires_at + 1)
            .unwrap(),
        Some(list.conversations[0].expires_at)
    );
    assert_eq!(
        store
            .next_archive_expiry(list.conversations[0].expires_at)
            .unwrap(),
        None,
        "none due after the last"
    );
    for id in [&chat, &bench].into_iter().chain(&members) {
        assert_eq!(rows(&root, id), [0; 5], "{id}");
        assert!(folders(&root, id).is_empty(), "{id}");
    }
    assert_eq!(store.storage().used(&chat).unwrap(), 0);
    assert_eq!(
        folders(&root, &later),
        FOLDERS,
        "the one archived since is kept"
    );
    let remaining = store.list_archive().unwrap();
    assert_eq!(
        remaining
            .conversations
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>(),
        [later]
    );
    assert!(remaining.benches.is_empty());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn attachments_keep_their_names_and_types_and_every_url_resolves_to_its_file() {
    let (root, _, store) = open("serving");
    let chat = store
        .create_conversation("openrouter", "test/model", NewConversation::default())
        .unwrap()
        .lock()
        .unwrap()
        .id
        .0
        .clone();
    let files = [
        ("shot.png", "image/png", "png bytes", ".png"),
        ("photo.JPEG", "image/jpeg", "jpeg bytes", ".jpg"),
        ("plan.svg", "image/svg+xml", "<svg/>", ".svg"),
        ("spec.pdf", "application/pdf", "%PDF-1.7", ".pdf"),
        ("notes.txt", "text/plain", "notes", ".txt"),
        ("gadget.js", "text/javascript", "window.GADGET=1", ".js"),
        (
            "page.html",
            "text/html",
            "<script>parent.PWN=1</script>",
            ".html",
        ),
        ("build.schem", "application/octet-stream", "nbt", ".schem"),
    ];
    use base64::Engine;
    let upload: Vec<Json> = files
        .iter()
        .map(|(name, mime, text, _)| object! { "name" => *name, "data" => format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(text)) })
        .collect();
    let saved = store.save_attachments(&chat, &upload).unwrap();
    for (file, (name, mime, text, extension)) in saved.iter().zip(files) {
        assert_eq!(
            (file.name.as_str(), file.mime.as_str(), file.size),
            (name, mime, text.len() as u64)
        );
        assert!(file.url.ends_with(extension), "{}", file.url);
        let path = store.file_path(&file.url).unwrap();
        assert_eq!(
            path,
            root.join("attachments")
                .join(&chat)
                .join(Path::new(&file.url).file_name().unwrap())
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
    let error = store
        .file_path(&format!("/api/attachments/{chat}/../../mcbench.sqlite3"))
        .unwrap_err();
    assert_eq!(error.message, "Bad attachment path");
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_chats_images_resolve_to_their_files() {
    let (root, _, store) = open("serving-images");
    let chat = store
        .create_conversation("openrouter", "test/model", NewConversation::default())
        .unwrap()
        .lock()
        .unwrap()
        .id
        .0
        .clone();
    let urls = store
        .save_images(
            &chat,
            &[
                ImageData::Png(b"png bytes".to_vec()),
                ImageData::DataUrl("data:image/jpeg;base64,anBn".into()),
            ],
            "view-",
        )
        .unwrap();
    assert!(urls[0].ends_with(".png") && urls[1].ends_with(".jpg"));
    assert_eq!(
        std::fs::read(store.file_path(&urls[0]).unwrap()).unwrap(),
        b"png bytes"
    );
    assert_eq!(
        std::fs::read(store.file_path(&urls[1]).unwrap()).unwrap(),
        b"jpg"
    );
    assert_eq!(
        store.file_path("/api/images/../../x").unwrap_err().message,
        "Bad image path"
    );
    assert!(store.file_path("/elsewhere/x").is_err());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
