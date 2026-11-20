//! The stand-in core answers every command of the seam (`src/lib.rs`)
//! against its data: the authored fixture's list states, the archive,
//! search, models, favourites, preferences kept in a file, branches,
//! dismissed errors, joins, turns at zero delay, and a folder in the
//! shape `dev/export-data.py` writes.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Inbox, TempDir};
use mcbench_ui::dev::{fixtures, standin::StandIn};
use mcbench_ui::{
    Command, Core, MessageId, Outgoing, Preferences, Resolution, SearchScope, Subject, Target,
    TurnEvent, Update,
};

fn gate(dir: &TempDir, preferences: bool) -> (StandIn, Arc<Inbox>) {
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let core = StandIn::new(
        fixtures::gate(&texts),
        texts.streamed.clone(),
        Duration::ZERO,
        preferences.then(|| dir.0.join("preferences.json")),
    );
    let inbox = Arc::new(Inbox::default());
    core.attach(inbox.clone());
    (core, inbox)
}

/// An update's kind, for the order a turn publishes.
fn kind(update: &Update) -> &'static str {
    match update {
        Update::Conversations(_) => "list",
        Update::TurnJoined { .. } => "joined",
        Update::Transcript { .. } => "transcript",
        Update::Turn {
            event: TurnEvent::Done,
            ..
        } => "done",
        Update::Turn {
            event: TurnEvent::Error { .. },
            ..
        } => "error",
        Update::Turn { .. } => "event",
        Update::TurnNotRunning { .. } => "not running",
        _ => "other",
    }
}

fn take(inbox: &Inbox) -> Vec<Update> {
    std::mem::take(&mut *inbox.0.lock().expect("lock"))
}

fn lists(updates: &[Update]) -> mcbench_ui::ConversationList {
    updates
        .iter()
        .rev()
        .find_map(|u| match u {
            Update::Conversations(l) => Some(l.clone()),
            _ => None,
        })
        .expect("a list")
}

#[test]
fn the_fixture_has_every_list_state() {
    let dir = TempDir::new("standin-lists");
    let (_core, inbox) = gate(&dir, false);
    let updates = take(&inbox);
    let l = lists(&updates);
    let build: Vec<&str> = l
        .conversations
        .iter()
        .filter(|c| c.summary.bench_id.is_none())
        .map(|c| c.summary.id.0.as_str())
        .collect();
    assert_eq!(build, ["chat-lamp", "chat-gate", "chat-farm"]);
    let row = |id: &str| {
        l.conversations
            .iter()
            .find(|c| c.summary.id.0 == id)
            .expect("listed")
    };
    assert!(
        l.conversations.iter().all(|c| c.running_since.is_none()),
        "nothing runs unless asked (--running)"
    );
    assert!(row("chat-farm").summary.unread, "finished and not seen");
    assert_eq!(
        row("member-a").summary.bench_id.as_deref(),
        Some("bench-towers")
    );
    assert_eq!(row("member-a").progress, Some((1, None)));
    assert_eq!(row("member-b").progress, Some((1, Some("failed"))));
    assert_eq!(l.benches.len(), 1);
    let bench = &l.benches[0];
    assert!(bench.blind && !bench.revealed);
    // sidebar agent: and one archived bench.
    assert_eq!(l.archived, (1, 1), "one archived chat and one bench");
    assert!(
        updates
            .iter()
            .any(|u| matches!(u, Update::Providers(Ok(p)) if !p.is_empty()))
    );
    assert!(
        updates
            .iter()
            .any(|u| matches!(u, Update::Favorites(f) if !f.is_empty()))
    );
}

#[test]
fn a_turn_streams_at_once_under_test_and_is_stored() {
    let dir = TempDir::new("standin-turn");
    let (core, inbox) = gate(&dir, false);
    take(&inbox);
    core.send(Command::Send(Outgoing {
        target: Target::Chat {
            id: "chat-lamp".into(),
            provider: "anthropic".into(),
            model: mcbench_ui::dev::standin::MODEL.into(),
        },
        text: "One more lamp".into(),
        attachments: Vec::new(),
        selections: Vec::new(),
        prompts: Vec::new(),
        effort: None,
        resolution: Resolution::Standard,
        edit_of: None,
    }));
    let updates = take(&inbox);
    // The seam's order: the list with the turn running, `TurnJoined`, the
    // chat at the turn's start, the turn's events, `Done`, the chat as
    // stored, the list with the turn ended.
    let kinds: Vec<&str> = updates.iter().map(kind).collect();
    let n = kinds.len();
    assert_eq!(&kinds[..3], ["list", "joined", "transcript"], "{kinds:?}");
    assert_eq!(&kinds[n - 3..], ["done", "transcript", "list"], "{kinds:?}");
    assert!(kinds[3..n - 3].iter().all(|k| *k == "event"), "{kinds:?}");
    let Update::Transcript {
        messages, header, ..
    } = &updates[2]
    else {
        unreachable!()
    };
    assert_eq!(messages.len(), 13, "the 12 stored and the message sent");
    assert!(header.running_since.is_some());
    let Update::Transcript { header, .. } = &updates[n - 2] else {
        unreachable!()
    };
    assert_eq!(
        header.running_since, None,
        "the end's transcript says no turn runs"
    );
    // No list shows the chat idle before that transcript.
    for update in &updates[..n - 2] {
        if let Update::Conversations(l) = update {
            assert!(
                l.conversations
                    .iter()
                    .find(|c| c.summary.id.0 == "chat-lamp")
                    .is_some_and(|c| c.running_since.is_some())
            );
        }
    }
    let Update::Conversations(ended) = &updates[n - 1] else {
        unreachable!()
    };
    assert!(
        ended
            .conversations
            .iter()
            .find(|c| c.summary.id.0 == "chat-lamp")
            .is_some_and(|c| c.running_since.is_none())
    );
    core.send(Command::Open {
        chat: "chat-lamp".into(),
    });
    let Some(Update::Transcript {
        messages,
        tree,
        header,
        ..
    }) = take(&inbox)
        .into_iter()
        .find(|u| matches!(u, Update::Transcript { .. }))
    else {
        panic!("a transcript");
    };
    assert_eq!(header.title, "Redstone lamp with a lever");
    assert_eq!(
        messages.len(),
        14,
        "the 12 stored, the message and its answer"
    );
    assert_eq!(
        tree.head_id,
        Some(messages.last().expect("one").id.0.clone())
    );
    // A new chat: listed once it has its message, then answered.
    core.send(Command::Send(Outgoing {
        target: Target::NewChat {
            provider: "anthropic".into(),
            model: "claude-opus-5-5".into(),
        },
        text: "A tower".into(),
        attachments: Vec::new(),
        selections: Vec::new(),
        prompts: Vec::new(),
        effort: None,
        resolution: Resolution::High,
        edit_of: None,
    }));
    let updates = take(&inbox);
    let l = lists(&updates);
    assert_eq!(l.conversations[0].summary.title, "A tower");
    assert_eq!(l.conversations[0].summary.resolution, 2);
}

#[test]
fn seen_archive_restore_and_delete() {
    let dir = TempDir::new("standin-archive");
    let (core, inbox) = gate(&dir, false);
    take(&inbox);
    core.send(Command::MarkSeen {
        chat: "chat-farm".into(),
    });
    assert!(
        !lists(&take(&inbox))
            .conversations
            .iter()
            .any(|c| c.summary.unread)
    );
    core.send(Command::Archive(Subject::Chat("chat-gate".into())));
    let l = lists(&take(&inbox));
    assert!(
        !l.conversations
            .iter()
            .any(|c| c.summary.id.0 == "chat-gate")
    );
    assert_eq!(l.archived, (2, 1));
    core.send(Command::ReadArchive);
    let Some(Update::Archive(Ok(archive))) = take(&inbox).pop() else {
        panic!("the archive");
    };
    let chats = &archive.conversations;
    let ids: Vec<&str> = chats.iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&"chat-gate") && ids.contains(&"chat-windmill"));
    let windmill = chats
        .iter()
        .find(|c| c.id == "chat-windmill")
        .expect("archived");
    assert_eq!(
        windmill.expires_at - windmill.archived_at,
        7 * 24 * 60 * 60 * 1000
    );
    core.send(Command::Restore(Subject::Chat("chat-gate".into())));
    let updates = take(&inbox);
    assert!(
        lists(&updates)
            .conversations
            .iter()
            .any(|c| c.summary.id.0 == "chat-gate")
    );
    core.send(Command::Delete(Subject::Chat("chat-windmill".into())));
    let updates = take(&inbox);
    let [Update::Conversations(_), Update::Archive(Ok(archive))] = &updates[..] else {
        panic!("the list, then the archive: {updates:?}");
    };
    assert!(archive.conversations.is_empty());
}

#[test]
fn an_archive_change_publishes_the_list_then_the_archive_once_read() {
    // As the core does (`end_in_lists`): Archive, Restore and Delete
    // publish `Conversations`, then `Archive(Ok(..))` once the archive was
    // read.
    let dir = TempDir::new("standin-archive-rule");
    let (core, inbox) = gate(&dir, false);
    take(&inbox);
    let kinds = |updates: &[Update]| -> Vec<&'static str> {
        updates
            .iter()
            .map(|u| match u {
                Update::Archive(Ok(_)) => "archive",
                Update::Conversations(_) => "list",
                _ => "other",
            })
            .collect()
    };
    core.send(Command::Archive(Subject::Chat("chat-gate".into())));
    assert_eq!(kinds(&take(&inbox)), ["list"], "not read yet");
    core.send(Command::ReadArchive);
    take(&inbox);
    for command in [
        Command::Archive(Subject::Chat("chat-farm".into())),
        Command::Delete(Subject::Chat("chat-windmill".into())),
        Command::Restore(Subject::Chat("chat-gate".into())),
        Command::Archive(Subject::Bench("bench-towers".into())),
        Command::Restore(Subject::Bench("bench-towers".into())),
        Command::Delete(Subject::Bench("bench-old".into())),
    ] {
        let updates = take_after(&core, &inbox, command);
        assert_eq!(kinds(&updates), ["list", "archive"]);
    }
    // After deleting one and restoring another, the archive keeps the rest.
    let updates = take_after(&core, &inbox, Command::ReadArchive);
    let [Update::Archive(Ok(archive))] = &updates[..] else {
        panic!("the archive: {updates:?}");
    };
    let ids: Vec<&str> = archive
        .conversations
        .iter()
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(ids, ["chat-farm"]);
    assert!(archive.benches.is_empty());
}

fn take_after(core: &StandIn, inbox: &Inbox, command: Command) -> Vec<Update> {
    core.send(command);
    take(inbox)
}

#[test]
fn search_models_favourites_branches_errors_and_joins() {
    let dir = TempDir::new("standin-misc");
    let (core, inbox) = gate(&dir, false);
    take(&inbox);
    core.send(Command::Search {
        scope: SearchScope::Build,
        query: "portcullis".into(),
        request: 7,
    });
    let Some(Update::Found {
        request,
        results: Ok(results),
    }) = take(&inbox).pop()
    else {
        panic!("results");
    };
    assert_eq!(request, 7);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "chat-gate");
    let snippet = results[0].snippet.clone().expect("a snippet");
    assert!(
        mcbench_ui::state::seam::snippet_parts(&snippet)
            .iter()
            .any(|(t, matched)| *matched && t == "portcullis"),
        "{snippet:?}"
    );
    core.send(Command::Models {
        provider: "anthropic".into(),
    });
    assert!(
        matches!(take(&inbox).pop(), Some(Update::Models { models: Ok(m), .. }) if m.len() == 2)
    );
    core.send(Command::ModelInfo {
        provider: "anthropic".into(),
        model: "claude-haiku-5-5".into(),
    });
    assert!(
        matches!(take(&inbox).pop(), Some(Update::ModelInfo { facts: Ok(f), .. }) if f.reasoning)
    );
    core.send(Command::Models {
        provider: "openai".into(),
    });
    assert!(matches!(
        take(&inbox).pop(),
        Some(Update::Models { models: Err(_), .. })
    ));
    core.send(Command::SetFavorites(vec![(
        "claude-code".into(),
        "claude-haiku-5-5".into(),
    )]));
    assert!(matches!(take(&inbox).pop(), Some(Update::Favorites(f)) if f.len() == 1));
    core.send(Command::JoinTurn {
        chat: "chat-gate".into(),
    });
    assert!(matches!(
        take(&inbox).pop(),
        Some(Update::TurnNotRunning { .. })
    ));
    core.send(Command::Open {
        chat: "nowhere".into(),
    });
    assert!(matches!(
        take(&inbox).pop(),
        Some(Update::TranscriptFailed { .. })
    ));
    // A failed turn's error, dismissed: stored as dismissed.
    core.send(Command::Send(Outgoing {
        target: Target::Chat {
            id: "chat-farm".into(),
            provider: "anthropic".into(),
            model: mcbench_ui::dev::standin::MODEL.into(),
        },
        text: "again".into(),
        attachments: Vec::new(),
        selections: Vec::new(),
        prompts: Vec::new(),
        effort: None,
        resolution: Resolution::Standard,
        edit_of: None,
    }));
    let error = take(&inbox)
        .into_iter()
        .find_map(|u| match u {
            Update::Turn {
                event: TurnEvent::Error { message_id, .. },
                ..
            } => message_id,
            _ => None,
        })
        .expect("the error's id");
    core.send(Command::DismissError {
        chat: "chat-farm".into(),
        message: MessageId(error.clone()),
    });
    core.send(Command::Open {
        chat: "chat-farm".into(),
    });
    let Some(Update::Transcript { messages, .. }) = take(&inbox)
        .into_iter()
        .find(|u| matches!(u, Update::Transcript { .. }))
    else {
        panic!("a transcript");
    };
    let stored = messages
        .iter()
        .find(|m| m.id.0 == error)
        .expect("the error");
    assert!(stored.payload_text().contains("\"dismissed\":true"));
    // A branch: the line through an earlier message, to its newest end.
    core.send(Command::Branch {
        chat: "chat-lamp".into(),
        message: MessageId("m6".into()),
    });
    let Some(Update::Transcript { messages, .. }) = take(&inbox).pop() else {
        panic!("a transcript");
    };
    assert_eq!(messages.last().expect("one").id.0, "m12");
}

#[test]
fn preferences_are_kept_in_their_file() {
    let dir = TempDir::new("standin-preferences");
    let (core, _inbox) = gate(&dir, true);
    // chat: the first run's last model is the stand-in's.
    assert_eq!(
        core.preferences(),
        Preferences {
            last_model: Some(("anthropic".into(), mcbench_ui::dev::standin::MODEL.into())),
            ..Preferences::default()
        }
    );
    let saved = Preferences {
        sidebar_width: 300.,
        chat_width: Some(500.),
        place: Some("/chat/chat-gate".into()),
        window: Some(mcbench_ui::state::layout::Placement {
            display: "d".into(),
            bounds: mcbench_ui::state::layout::Rect {
                x: 10.,
                y: 20.,
                width: 1100.,
                height: 700.,
            },
        }),
        ..Preferences::default()
    };
    core.send(Command::SavePreferences(saved.clone()));
    let (again, _) = gate(&dir, true);
    assert_eq!(again.preferences(), saved, "a new stand-in reads them back");
}

#[test]
fn a_folder_in_the_exporters_shape_loads() {
    let dir = TempDir::new("standin-folder");
    let folder = &dir.0;
    std::fs::create_dir_all(folder.join("chats")).expect("a folder");
    let write = |name: &str, value: serde_json::Value| {
        std::fs::write(folder.join(name), value.to_string()).expect("written")
    };
    write(
        "lists.json",
        serde_json::json!({
            "chats": [{ "id": "c1", "title": "Tower", "provider": "claude-code", "model": "claude-opus-5-5",
                        "effort": null, "resolution": 10, "createdAt": "2026-11-14T13:07:21.488Z", "bench": null,
                        "unread": true, "messageCount": 2, "prompts": [], "runningSince": null }],
            "benches": [], "promptTitles": {}, "archived": { "conversations": 0, "benches": 0 }
        }),
    );
    write(
        "archive.json",
        serde_json::json!({ "conversations": [], "benches": [] }),
    );
    write(
        "settings.json",
        serde_json::json!({
            "providers": { "claude-code": { "enabled": true, "hiddenModels": [], "modelOrder": [], "keySaved": false } },
            "modelFavorites": ["claude-code::claude-opus-5-5", "cursor::auto"]
        }),
    );
    write(
        "chats/c1.json",
        serde_json::json!({
            "header": { "id": "c1", "title": "Tower", "provider": "claude-code", "model": "claude-opus-5-5", "effort": null, "runningSince": null },
            "messages": [
                { "id": "a", "parentId": null, "position": 0, "payload": "{\"role\":\"user\",\"content\":\"A tower\",\"createdAt\":\"2026-11-14T13:07:21.488Z\"}" },
                { "id": "b", "parentId": "a", "position": 1, "payload": "{\"role\":\"assistant\",\"content\":\"Done.\",\"createdAt\":\"2026-11-14T13:07:22.000Z\"}" }
            ],
            "tree": { "headId": "b", "messages": [] }
        }),
    );
    let core = StandIn::from_folder(folder, "Fine.".into(), Duration::ZERO);
    let inbox = Arc::new(Inbox::default());
    core.attach(inbox.clone());
    let updates = take(&inbox);
    let l = lists(&updates);
    assert_eq!(l.conversations.len(), 1);
    assert_eq!(l.conversations[0].summary.resolution, 10);
    assert!(
        updates
            .iter()
            .any(|u| matches!(u, Update::Favorites(f) if f.len() == 1)),
        "no Cursor"
    );
    core.send(Command::Open { chat: "c1".into() });
    let Some(Update::Transcript { messages, .. }) = take(&inbox)
        .into_iter()
        .find(|u| matches!(u, Update::Transcript { .. }))
    else {
        panic!("a transcript");
    };
    // Rows without an id in their payload take the row's.
    let typed = mcbench_ui::state::seam::from_stored(&messages);
    assert_eq!(typed.len(), 2);
    assert!(
        matches!(&typed[0], mcbench_ui::state::seam::StoredMessage::User { id: Some(id), .. } if id == "a")
    );
}

#[test]
fn preferences_text_has_one_shape() {
    let p = Preferences {
        chat_width: Some(480.),
        last_model: Some(("anthropic".into(), "claude-opus-5-5".into())),
        open_benches: vec!["bench-towers".into()],
        prompts_open: false,
        // bench agent: Bench's and the prompt editor's keys.
        bench_models: vec![mcbench_ui::BenchChoice {
            provider: "openai".into(),
            model: "gpt-5.5".into(),
            effort: Some("high".into()),
        }],
        bench_blind: false,
        bench_resolution: Resolution::High,
        prompt_ai_model: Some(mcbench_ui::BenchChoice {
            provider: "anthropic".into(),
            model: "claude-opus-5-5".into(),
            effort: None,
        }),
        improve_source_note: true,
        ..Preferences::default()
    };
    assert_eq!(Preferences::from_text(&p.to_text()), Ok(p.clone()));
    // A field the file lacks takes its default, an unknown one is ignored,
    // an unreadable one takes its default and is noted (tests/preferences.rs).
    let mut changed: serde_json::Value = serde_json::from_str(&p.to_text()).expect("JSON");
    changed["improveSource"] = serde_json::json!("both");
    changed["theme"] = serde_json::json!("dark");
    let object = changed.as_object_mut().expect("an object");
    object.remove("binds");
    object.remove("openBenches");
    assert_eq!(
        Preferences::from_text_noting(&changed.to_string()),
        Ok((
            Preferences {
                improve_source_note: false,
                open_benches: Vec::new(),
                ..p.clone()
            },
            vec!["improveSource".to_string()]
        ))
    );
    assert!(Preferences::from_text("{").is_err());
}

#[test]
fn preferences_that_do_not_parse_are_an_error_then_the_defaults() {
    let dir = TempDir::new("standin-bad-preferences");
    std::fs::write(dir.0.join("preferences.json"), "{ not json").expect("written");
    let (core, inbox) = gate(&dir, true);
    assert_eq!(core.preferences(), Preferences::default());
    let errors: Vec<String> = take(&inbox)
        .into_iter()
        .filter_map(|u| match u {
            Update::Error(e) => Some(e),
            _ => None,
        })
        .collect();
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].contains("preferences.json could not be read"),
        "{errors:?}"
    );
}

#[test]
fn stored_pictures_and_attachments_resolve_or_say_why_not() {
    let dir = TempDir::new("standin-files");
    let (core, _inbox) = gate(&dir, false);
    assert_eq!(
        core.image_file(fixtures::MISSING_PICTURE),
        Err("Image not found".to_string())
    );
    assert_eq!(
        core.attachment_file("/api/attachments/chat-gate/plan.pdf"),
        Err("Attachment not found".to_string())
    );
    // A folder in the exporter's shape holds them.
    let folder = &dir.0;
    std::fs::create_dir_all(folder.join("chats")).expect("a folder");
    std::fs::create_dir_all(folder.join("images/c1")).expect("a folder");
    std::fs::create_dir_all(folder.join("attachments/c1")).expect("a folder");
    std::fs::write(folder.join("images/c1/shot.png"), fixtures::PICTURE).expect("written");
    std::fs::write(folder.join("attachments/c1/notes.txt"), "notes").expect("written");
    std::fs::write(
        folder.join("lists.json"),
        r#"{"chats":[],"benches":[],"promptTitles":{},"archived":{"conversations":0,"benches":0}}"#,
    )
    .expect("written");
    std::fs::write(
        folder.join("archive.json"),
        r#"{"conversations":[],"benches":[]}"#,
    )
    .expect("written");
    std::fs::write(
        folder.join("settings.json"),
        r#"{"providers":{},"modelFavorites":[]}"#,
    )
    .expect("written");
    let core = StandIn::from_folder(folder, "Fine.".into(), Duration::ZERO);
    assert_eq!(
        core.image_file("/api/images/c1/shot.png"),
        Ok(folder.join("images/c1/shot.png"))
    );
    let file = core
        .attachment_file("/api/attachments/c1/notes.txt")
        .expect("the attachment");
    assert_eq!(
        (file.content_type, file.inline, file.file_name.as_str()),
        ("text/plain", true, "notes.txt")
    );
    // A path that is a file already is drawn as it is; an address goes
    // through the core.
    let plain = std::path::Path::new("/data/shot.png");
    assert_eq!(
        mcbench_ui::state::files::picture(plain, &core),
        Ok(plain.to_path_buf())
    );
    assert_eq!(
        mcbench_ui::state::files::picture(std::path::Path::new("/api/images/c1/gone.png"), &core),
        Err("Image not found".to_string())
    );
}

#[test]
fn a_join_replays_the_snapshot_and_the_events_so_far_then_the_live_ones() {
    let dir = TempDir::new("standin-join");
    let (core, inbox) = gate(&dir, false);
    core.turns_by_hand();
    take(&inbox);
    // A turn no window sent: the list, then its events, joined or not.
    core.start_turn("chat-gate");
    assert_eq!(take(&inbox).iter().map(kind).collect::<Vec<_>>(), ["list"]);
    assert!(core.advance("chat-gate", 2));
    assert_eq!(
        take(&inbox).iter().map(kind).collect::<Vec<_>>(),
        ["event", "event"]
    );
    core.send(Command::JoinTurn {
        chat: "chat-gate".into(),
    });
    let joined = take(&inbox);
    assert_eq!(
        joined.iter().map(kind).collect::<Vec<_>>(),
        ["joined", "transcript", "event", "event"]
    );
    let Update::Transcript { header, .. } = &joined[1] else {
        unreachable!()
    };
    assert!(
        header.running_since.is_some(),
        "the snapshot at the turn's start"
    );
    assert!(core.advance("chat-gate", 1));
    assert_eq!(take(&inbox).iter().map(kind).collect::<Vec<_>>(), ["event"]);
    core.finish("chat-gate");
    let end: Vec<_> = take(&inbox).iter().map(kind).collect();
    assert_eq!(&end[end.len() - 3..], ["done", "transcript", "list"]);
    // After the last event: not running.
    core.send(Command::JoinTurn {
        chat: "chat-gate".into(),
    });
    assert_eq!(
        take(&inbox).iter().map(kind).collect::<Vec<_>>(),
        ["not running"]
    );
}

#[test]
fn each_command_publishes_what_the_seam_says() {
    use mcbench_ui::dev::standin::{STAR_FAILS, STAR_FAILURE};
    let dir = TempDir::new("standin-each");
    let (core, inbox) = gate(&dir, false);
    let kinds = |updates: &[Update]| -> Vec<&'static str> {
        updates
            .iter()
            .map(|u| match u {
                Update::Providers(_) => "providers",
                Update::Favorites(_) => "favorites",
                Update::Error(_) => "error",
                Update::Archive(_) => "archive",
                other => kind(other),
            })
            .collect()
    };
    // After attach: the list, the providers, the favourites.
    assert_eq!(kinds(&take(&inbox)), ["list", "providers", "favorites"]);
    // Open: the transcript and nothing else (only MarkSeen marks seen).
    core.send(Command::Open {
        chat: "chat-farm".into(),
    });
    assert_eq!(kinds(&take(&inbox)), ["transcript"]);
    core.send(Command::Open {
        chat: "no-such-chat".into(),
    });
    assert!(matches!(
        &take(&inbox)[..],
        [Update::TranscriptFailed { .. }]
    ));
    // MarkSeen: the list, the chat no longer unread.
    core.send(Command::MarkSeen {
        chat: "chat-farm".into(),
    });
    let marked = take(&inbox);
    assert_eq!(kinds(&marked), ["list"]);
    assert!(
        !lists(&marked)
            .conversations
            .iter()
            .any(|c| c.summary.id.0 == "chat-farm" && c.summary.unread),
        "MarkSeen marks it seen"
    );
    // A failed mark: the error, then the unchanged list.
    core.set_turn("chat-farm", false, true, None);
    take(&inbox);
    core.fail_next_seen("The chat could not be marked seen");
    core.send(Command::MarkSeen {
        chat: "chat-farm".into(),
    });
    let failed = take(&inbox);
    assert_eq!(kinds(&failed), ["error", "list"]);
    assert!(
        lists(&failed)
            .conversations
            .iter()
            .any(|c| c.summary.id.0 == "chat-farm" && c.summary.unread),
        "a failed mark leaves it unread"
    );
    // With the archive read, a mark and an archive end in the list, then
    // the archive (the core's order).
    core.send(Command::ReadArchive);
    take(&inbox);
    core.send(Command::MarkSeen {
        chat: "chat-farm".into(),
    });
    assert_eq!(kinds(&take(&inbox)), ["list", "archive"]);
    core.send(Command::Archive(mcbench_ui::Subject::Chat(
        "chat-gate".into(),
    )));
    assert_eq!(kinds(&take(&inbox)), ["list", "archive"]);
    // SetFavorites refused: the notice, then the unchanged favourites.
    core.send(Command::SetFavorites(vec![(
        STAR_FAILS.0.to_string(),
        STAR_FAILS.1.to_string(),
    )]));
    let starred = take(&inbox);
    assert_eq!(kinds(&starred), ["error", "favorites"]);
    assert!(matches!(&starred[0], Update::Error(e) if e == STAR_FAILURE));
    // settings agent: a save: the providers, the favourites, then its end.
    core.send(Command::SaveSettings(mcbench_ui::SettingsChange {
        title_model: None,
        share_repo: None,
        providers: Vec::new(),
    }));
    assert_eq!(kinds(&take(&inbox)), ["providers", "favorites", "other"]);
    // SavePreferences: nothing.
    core.send(Command::SavePreferences(mcbench_ui::Preferences::default()));
    assert!(take(&inbox).is_empty());
}

// par-chat (stage four's parity pass)
/// The open chat's model: its row's provider and model in `updates`' list.
fn row_model(updates: &[Update], chat: &str) -> (String, String) {
    let list = lists(updates);
    let row = list
        .conversations
        .iter()
        .find(|c| c.summary.id.0 == chat)
        .unwrap_or_else(|| panic!("{chat} is listed"));
    (row.summary.provider.clone(), row.summary.model.clone())
}

#[test]
fn a_chat_message_sets_the_chats_model_and_a_refused_pair_changes_nothing() {
    let dir = TempDir::new("standin-pair");
    let (core, inbox) = gate(&dir, false);
    let at_start = row_model(&take(&inbox), "chat-lamp");
    assert_eq!(
        at_start,
        ("anthropic".into(), mcbench_ui::dev::standin::MODEL.into())
    );
    let to = |provider: &str, model: &str| Outgoing {
        target: Target::Chat {
            id: "chat-lamp".into(),
            provider: provider.into(),
            model: model.into(),
        },
        text: "One more lamp".into(),
        attachments: Vec::new(),
        selections: Vec::new(),
        prompts: Vec::new(),
        effort: None,
        resolution: Resolution::Standard,
        edit_of: None,
    };
    // The route's two refusals, in its order, and nothing changes.
    for (provider, model, error) in [
        ("nobody", "", mcbench_ui::dev::standin::UNKNOWN_PROVIDER),
        ("claude-code", " ", mcbench_ui::dev::standin::NO_MODEL),
    ] {
        let updates = take_after(&core, &inbox, Command::Send(to(provider, model)));
        assert!(
            matches!(&updates[..], [Update::SendRefused { error: e, .. }] if e == error),
            "{updates:?}"
        );
    }
    // Accepted: the chat takes the message's pair.
    let updates = take_after(
        &core,
        &inbox,
        Command::Send(to("claude-code", "claude-haiku-5-5")),
    );
    assert_eq!(
        row_model(&updates, "chat-lamp"),
        ("claude-code".into(), "claude-haiku-5-5".into())
    );
}

#[test]
fn a_providers_read_fails_once_when_asked_and_read_providers_answers_once() {
    let dir = TempDir::new("standin-providers");
    let (core, inbox) = gate(&dir, false);
    take(&inbox);
    core.fail_providers_read("unreadable");
    let updates = take_after(&core, &inbox, Command::ReadProviders);
    assert!(
        matches!(&updates[..], [Update::Providers(Err(e))] if e == "unreadable"),
        "{updates:?}"
    );
    let updates = take_after(&core, &inbox, Command::ReadProviders);
    assert!(
        matches!(&updates[..], [Update::Providers(Ok(p))] if !p.is_empty()),
        "{updates:?}"
    );
}
