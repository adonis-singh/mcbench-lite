//! The typed message and the writers. Every synthetic row of today's writer
//! orders decodes (the two orders no single struct gives, the bench prompt
//! since 4ecb45ac and the v14 migration's, included), and each constructor
//! writes its writer's exact text. The write differential
//! (`differential.rs`) holds every constructor against Node's own writer
//! code on top of this.

mod common;

use std::sync::Arc;

use common::DetHost;
use mcbench_store::{
    Attachment, Content, FunctionCall, Message, NewConversation, NewMessage, Part, PromptLink,
    Reply, Selection, Telemetry, ToolCall,
};

include!("synthetic.in");

const T0: i64 = 1794873600000;

#[test]
fn every_synthetic_row_of_todays_writers_decodes() {
    for (label, text) in SYNTHETIC {
        Message::decode(text, label).unwrap_or_else(|error| panic!("{error}"));
    }
}

#[test]
fn a_stand_in_makes_stored_messages_from_payload_text() {
    for (index, (_, text)) in SYNTHETIC.iter().enumerate() {
        let message = mcbench_store::StoredMessage::from_payload(
            &format!("m{index}"),
            (index > 0).then(|| format!("m{}", index - 1)).as_deref(),
            index as u32,
            text.to_string(),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(message.payload_text(), *text);
        assert_eq!(message.message, Message::decode(text, "").unwrap());
        assert_eq!(message.position, index as u32);
    }
    let refused = mcbench_store::StoredMessage::from_payload(
        "m9",
        None,
        0,
        r#"{"role":"user","content":"hi","createdAt":"2026-11-17T00:00:00.000Z","extra":1}"#.into(),
    )
    .unwrap_err();
    assert!(refused.starts_with("Message m9: "), "{refused}");
}

#[test]
fn unknown_fields_are_refused_naming_the_chat_and_message() {
    let error = Message::decode(
        r#"{"role":"user","content":"x","createdAt":"t","colour":"red"}"#,
        "Conversation c message m",
    )
    .unwrap_err();
    assert!(
        error.starts_with("Conversation c message m: unknown field `colour`"),
        "{error}"
    );
}

fn telemetry(extra: &str) -> Telemetry {
    serde_json::from_str(&format!(
        r#"{{"provider":"openrouter","model":"m","effort":null,"startedAt":"2026-11-17T00:00:00.000Z","wallMs":12,"firstDeltaMs":null,"contextBytes":100,"messageCount":3,"usage":null{extra}}}"#
    ))
    .unwrap()
}

#[test]
fn each_constructor_writes_its_writers_text() {
    let root = common::scratch("writers");
    let host = DetHost::new(T0);
    let store = common::open(&root, host, common::limits(1 << 30)).unwrap();
    let live = store
        .create_conversation("openrouter", "m", NewConversation::default())
        .unwrap();
    let mut chat = live.lock().unwrap();
    let text = |label: &str| SYNTHETIC.iter().find(|(l, _)| *l == label).unwrap().1;
    let link = vec![PromptLink {
        id: "p1".into(),
        title: "Castle".into(),
    }];
    let x = Content::Text("x".into());
    let mut check = |message: NewMessage, label: &str| {
        let stored = store.append_message(&mut chat, message, None).unwrap();
        assert_eq!(stored.payload_text(), text(label), "{label}");
    };
    check(
        NewMessage::prompt(&x, &link),
        "user chat prompt with prompts",
    );
    let parts = Content::Parts(vec![
        Part::Text { text: "x".into() },
        Part::Attachment {
            attachment: Attachment {
                name: "a.png".into(),
                mime: "image/png".into(),
                size: 10,
                url: "/api/attachments/c/1-a.png".into(),
                path: "/work/attachments/1-a.png".into(),
            },
        },
        Part::Selection {
            selection: Selection {
                letter: "A".into(),
                label: "Stone, top".into(),
                min: [0, 0, 0],
                max: [1, 1, 1],
                blocks: 8,
                block: Some("minecraft:stone".into()),
            },
        },
    ]);
    check(
        NewMessage::prompt(&parts, &[]),
        "user chat prompt with parts",
    );
    check(
        NewMessage::bench_prompt("r1", Some(&link), &x),
        "user bench prompt (new order)",
    );
    check(
        NewMessage::bench_prompt("r1", None, &x),
        "user bench prompt (new order, no prompts)",
    );
    check(
        NewMessage::unanswered("c1", true, "ignored"),
        "tool unanswered",
    );
    check(
        NewMessage::stopped(
            "(stopped by user)",
            Some("partial"),
            Some(&telemetry(r#","failed":true,"cancelled":true"#)),
        ),
        "assistant stopped with reply",
    );
    check(
        NewMessage::reply(&Reply {
            content: "x".into(),
            reasoning: Some("r".into()),
            finish_reason: Some("tool_calls".into()),
            telemetry: Some(telemetry(r#","contextTokens":null"#)),
            notices: Some(vec![mcbench_store::Notice {
                level: "warning".into(),
                text: "n".into(),
            }]),
            plan: Some(vec![mcbench_store::PlanItem {
                text: "t".into(),
                status: "pending".into(),
            }]),
            rate_limit: Some(serde_json::json!({ "status": "allowed", "resetsAt": 1 })),
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                kind: "function".into(),
                function: FunctionCall {
                    name: "build".into(),
                    arguments: "{}".into(),
                },
                extra_content: None,
            }],
            ..Reply::default()
        }),
        "assistant harness with notices",
    );
    check(
        NewMessage::error("Title generation failed: x", None),
        "error title failure",
    );
    let error = store
        .append_message(
            &mut chat,
            NewMessage::error("x", Some(&telemetry(r#","failed":true,"cancelled":false"#))),
            None,
        )
        .unwrap();
    store.save(&mut chat).unwrap();
    store.dismiss_error(&mut chat, &error.id.0).unwrap();
    let dismissed = chat.messages.iter().find(|m| m.id == error.id).unwrap();
    assert_eq!(dismissed.payload_text(), text("error dismissed"));
    let Message::Error(e) = &dismissed.message else {
        panic!()
    };
    assert!(e.dismissed);
    let in_memory = dismissed.in_memory_text();
    assert!(in_memory.starts_with(&text("error dismissed")[..text("error dismissed").len() - 1]));
    assert!(in_memory.ends_with(&format!(
        r#","id":"{}","parentId":"{}","position":{}}}"#,
        error.id.0,
        error.parent.as_ref().unwrap().0,
        error.position
    )));
    drop(chat);
    drop(store);
    let _ = Arc::strong_count(&live);
    std::fs::remove_dir_all(root).unwrap();
}
