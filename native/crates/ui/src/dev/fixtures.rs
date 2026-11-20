//! The dev binary's fixtures: chats and their stored messages, each a row's
//! id and its `payload_json` in today's shape, authored for the checks
//! (never anyone's chats), with every state the list shows: running,
//! finished and unseen, archived, and a blind bench with two models, one
//! failed. The stand-in decodes each payload with the store's decoder
//! (`StoredMessage::from_payload`).

use serde_json::{Value, json};

/// A chat of a fixture.
pub struct Chat {
    /// Its id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its stored messages, oldest first.
    pub messages: Vec<Row>,
    /// Finished and not looked at.
    pub unread: bool,
    /// A turn shown running since this epoch ms (the stand-in runs none:
    /// joining it finds it ended).
    pub running_since: Option<i64>,
    /// A bench model's chat: its bench and slot.
    pub bench: Option<(String, u32)>,
    /// Archived at this epoch ms.
    pub archived_at: Option<i64>,
    // sidebar agent: what the filters and the resolution tag read.
    /// Its resolution (1, 2, 5 or 10).
    pub resolution: u32,
    /// The library prompts its messages were sent from.
    pub prompts: Vec<String>,
    // bench agent: a bench model's own model.
    /// Its provider, model and effort; none is the fixtures' model.
    pub model: Option<(String, String, Option<String>)>,
}

impl Chat {
    /// A listed, idle Build chat.
    pub fn new(id: &str, title: &str, messages: Vec<Row>) -> Self {
        Chat {
            id: id.into(),
            title: title.into(),
            messages,
            unread: false,
            running_since: None,
            bench: None,
            archived_at: None,
            resolution: 1,
            prompts: Vec::new(),
            model: None,
        }
    }
}

/// A bench of a fixture.
pub struct Bench {
    /// Its id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Names hidden until revealed.
    pub blind: bool,
    /// Revealed.
    pub revealed: bool,
    /// Its members' chat ids, in their letters' order.
    pub order: Vec<String>,
}

/// A fixture: its chats and benches.
pub struct Fixture {
    /// The chats, newest first (bench models' and archived ones too).
    pub chats: Vec<Chat>,
    /// The benches.
    pub benches: Vec<Bench>,
    // sidebar agent: the prompt library and the titles of deleted prompts.
    /// The prompt library: id, title, text, tags.
    pub library: Vec<(String, String, String, Vec<String>)>,
    /// The titles prompts were last sent under (`promptTitles`).
    pub prompt_titles: Vec<(String, String)>,
    /// Archived benches: id, title, archived at, models.
    pub archived_benches: Vec<(String, String, i64, i64)>,
}

/// When the fixtures' archived chat was archived: 2026-11-15 10:00 UTC.
pub const ARCHIVED_AT: i64 = 1_794_736_800_000;

/// The fixtures' today, 2026-11-17 10:00 UTC: when `--running` starts a
/// chat's turn in the tests that fix the clock, and two days after
/// [`ARCHIVED_AT`].
pub const TODAY: i64 = 1_794_909_600_000;

/// The failed bench model's error.
pub const FAILURE_TEXT: &str = "The provider refused the request: overloaded (a fixture error)";

/// A stored message: its row's id and its payload.
pub struct Row {
    /// `id`.
    pub id: String,
    /// `payload_json`, as JSON.
    pub payload: Value,
}

/// The texts of `fixtures/gate`, with the picture's path filled in.
pub struct Texts {
    /// The streamed answer.
    pub streamed: String,
    answers: [String; 3],
}

impl Texts {
    /// Reads the texts compiled into the binary; `picture` is where the
    /// fixture's picture was written.
    pub fn new(picture: &std::path::Path) -> Self {
        Texts {
            streamed: include_str!("../../fixtures/gate/streamed.md").to_string(),
            answers: [
                include_str!("../../fixtures/gate/answer-1.md").to_string(),
                include_str!("../../fixtures/gate/answer-2.md").to_string(),
                include_str!("../../fixtures/gate/answer-3.md")
                    .replace("PICTURE", &picture.display().to_string()),
            ],
        }
    }
}

/// The picture of `fixtures/gate`.
pub const PICTURE: &[u8] = include_bytes!("../../fixtures/gate/lamp.png");

/// When every fixture message was written (`createdAt`).
pub const CREATED_AT: &str = "2026-11-17T10:00:00.000Z";

/// A row of `payload`, written at [`CREATED_AT`].
fn row(id: &str, mut payload: Value) -> Row {
    payload["createdAt"] = Value::String(CREATED_AT.to_string());
    Row {
        id: id.to_string(),
        payload,
    }
}

/// A prompt the user sent.
pub fn user(id: &str, text: &str) -> Row {
    row(
        id,
        json!({ "role": "user", "content": [{ "type": "text", "text": text }] }),
    )
}

/// A model's reply.
pub fn assistant(id: &str, text: &str) -> Row {
    row(id, json!({ "role": "assistant", "content": text }))
}

/// The pictures the app fed back after a `see_build` (`seeBuild: true`),
/// as stored addresses.
pub fn shots(id: &str, addresses: &[&str]) -> Row {
    let parts: Vec<Value> = addresses
        .iter()
        .map(|a| json!({ "type": "image_url", "image_url": { "url": a } }))
        .collect();
    row(
        id,
        json!({ "role": "user", "content": parts, "seeBuild": true }),
    )
}

/// The fixtures' picture that is not there: the core answers its address
/// with `Image not found`.
pub const MISSING_PICTURE: &str = "/api/images/chat-gate/missing.png";

/// A failed turn's error.
pub fn error(id: &str, text: &str) -> Row {
    row(id, json!({ "role": "error", "content": text }))
}

/// `gate`: three chats; the first has every markdown block, two tool calls
/// (one `build` with a program), reasoning, a picture, links, right-to-left
/// text and CJK.
pub fn gate(texts: &Texts) -> Fixture {
    let program = "world.fill((0, 0, 0), (2, 0, 2), \"stone\");\nworld.set((1, 1, 1), \"redstone_lamp\");\nworld.set((0, 1, 1), \"lever[face=floor,facing=west]\");";
    let build_args =
        json!({ "summary": "Build a lamp with a lever", "program": program }).to_string();
    let bash_args = json!({ "command": "ls -la /work\necho done" }).to_string();
    let lamp = vec![
        user(
            "m1",
            "Build a redstone lamp that turns on with a lever, and explain what you placed.",
        ),
        row(
            "m2",
            json!({
                "role": "assistant", "content": "I will place a stone base first, then the lamp and its lever.",
                "reasoning": "The user wants a lamp on a lever. A lamp lights when the block under it is powered, so the lever can sit on the base.",
                "tool_calls": [{ "id": "call-build", "type": "function", "function": { "name": "build", "arguments": build_args } }]
            }),
        ),
        row(
            "m3",
            json!({ "role": "tool", "tool_call_id": "call-build",
                "content": json!({ "ok": true, "diff": { "added": 11, "removed": 0, "changed": 1 } }).to_string() }),
        ),
        row(
            "m4",
            json!({
                "role": "assistant", "content": "",
                "tool_calls": [{ "id": "call-bash", "type": "function", "function": { "name": "bash", "arguments": bash_args } }]
            }),
        ),
        row(
            "m5",
            json!({ "role": "tool", "tool_call_id": "call-bash",
                "content": "total 8\ndrwxr-xr-x 2 work work 4096 Nov 17 10:02 .\n-rw-r--r-- 1 work work  212 Nov 17 10:02 build.js\ndone" }),
        ),
        assistant("m6", &texts.answers[0]),
        user(
            "m7",
            "Put a label on the second lamp in English, Arabic, Hebrew, Japanese and Chinese.",
        ),
        assistant("m8", &texts.answers[1]),
        user("m9", "Show me how it looks now."),
        assistant("m10", &texts.answers[2]),
        user("m11", "Thanks. Is the base still three by three?"),
        assistant(
            "m12",
            "Yes: the base is still **three by three**, nine `stone` blocks, and nothing else moved.",
        ),
    ];
    let chats = vec![
        Chat::new("chat-lamp", "Redstone lamp with a lever", lamp),
        // Shown running only with `--running chat-gate` or `--simulate-ms`
        // (`StandIn::mark_running`, `StandIn::set_turn`).
        Chat {
            ..Chat::new(
                "chat-gate",
                "Castle gate",
                vec![
                    user("g1", "Build a castle gate with a portcullis."),
                    assistant(
                        "g2",
                        "The gate is two towers with an iron-bar portcullis between them.",
                    ),
                    // A stored picture whose file is gone (the error state).
                    shots("g3", &[MISSING_PICTURE]),
                ],
            )
        },
        Chat {
            // Finished and not looked at.
            unread: true,
            // sidebar agent: a resolution tag and two prompts, one deleted
            // from the library.
            resolution: 5,
            prompts: vec!["prompt-farm".into(), "prompt-barn".into()],
            ..Chat::new(
                "chat-farm",
                "Wheat farm",
                vec![
                    user("f1", "A small wheat farm with water in the middle."),
                    assistant(
                        "f2",
                        "Done: a nine by nine field with one water block in the centre.",
                    ),
                ],
            )
        },
        // A bench's two models (not Build chats: the Build list does not
        // show them).
        Chat {
            bench: Some(("bench-towers".into(), 0)),
            ..Chat::new(
                "member-a",
                "Towers and a bridge",
                vec![
                    user("ma-1", "Two stone towers joined by a bridge."),
                    assistant(
                        "ma-2",
                        "Two towers of stone bricks, eight high, and an oak bridge.",
                    ),
                ],
            )
        },
        Chat {
            bench: Some(("bench-towers".into(), 1)),
            ..Chat::new(
                "member-b",
                "Towers and a bridge",
                vec![
                    user("mb-1", "Two stone towers joined by a bridge."),
                    error("mb-2", FAILURE_TEXT),
                ],
            )
        },
        // Archived two days before the fixtures' today.
        Chat {
            archived_at: Some(ARCHIVED_AT),
            ..Chat::new(
                "chat-windmill",
                "Old windmill",
                vec![
                    user("w1", "A windmill with four sails."),
                    assistant("w2", "The windmill stands on a cobblestone base."),
                ],
            )
        },
    ];
    Fixture {
        chats,
        library: vec![
            (
                "prompt-farm".into(),
                "Wheat field".into(),
                "A small wheat farm with water in the middle.".into(),
                vec!["farm".into()],
            ),
            (
                "prompt-tower".into(),
                "Two towers".into(),
                "Two stone towers joined by a bridge.".into(),
                vec!["castle".into()],
            ),
        ],
        prompt_titles: vec![("prompt-barn".into(), "Red barn".into())],
        archived_benches: vec![("bench-old".into(), "Old lighthouse".into(), ARCHIVED_AT, 3)],
        benches: vec![Bench {
            id: "bench-towers".into(),
            title: "Towers and a bridge".into(),
            blind: true,
            revealed: false,
            order: vec!["member-b".into(), "member-a".into()],
        }],
    }
}

/// `long`: one chat of 400 messages that alternate user and model, of
/// varying length and markdown, for scrolling.
pub fn long() -> Fixture {
    let sentences = [
        "The lamp sits on a stone base.",
        "Redstone dust carries power for fifteen blocks.",
        "A repeater adds a delay of one to four ticks.",
        "Observers fire a pulse when the block in front changes.",
        "Pistons push up to twelve blocks.",
        "A comparator reads the fill level of a chest.",
        "Hoppers move one item every eight ticks.",
    ];
    let mut messages = Vec::new();
    for index in 0..400usize {
        let lines = 1 + index % 5;
        let mut text = String::new();
        for line in 0..lines {
            let sentence = sentences[(index + line) % sentences.len()];
            match (index % 7, line) {
                (3, 0) => text.push_str(&format!("## Step {index}\n\n")),
                (5, _) => text.push_str("- "),
                _ => {}
            }
            text.push_str(sentence);
            text.push_str(if index % 7 == 5 { "\n" } else { " " });
        }
        if index % 11 == 6 {
            text.push_str("\n\n```js\nworld.set([0, 1, 0], 'redstone_lamp');\n```\n");
        }
        let id = format!("l{index}");
        messages.push(if index % 2 == 0 {
            user(&id, &text)
        } else {
            assistant(&id, &text)
        });
    }
    Fixture {
        chats: vec![Chat::new("chat-long", "Four hundred messages", messages)],
        benches: Vec::new(),
        library: Vec::new(),
        prompt_titles: Vec::new(),
        archived_benches: Vec::new(),
    }
}

// sidebar agent: the sidebar's measured list.
/// `many`: 500 Build chats of 10 messages each (5,000 messages to search),
/// none running, for the sidebar's timings; every 7th chat has a
/// resolution above Standard.
pub fn many() -> Fixture {
    let subjects = [
        "lamp", "gate", "farm", "tower", "bridge", "windmill", "castle", "barn", "well", "mill",
    ];
    let chats = (0..500usize)
        .map(|index| {
            let subject = subjects[index % subjects.len()];
            let messages = (0..10usize)
                .map(|m| {
                    let id = format!("n{index}-{m}");
                    if m % 2 == 0 {
                        user(&id, &format!("Build a {subject} number {index}, step {m}."))
                    } else {
                        assistant(
                            &id,
                            &format!(
                                "Placed the {subject}'s stone base and {m} redstone lamps for build {index}."
                            ),
                        )
                    }
                })
                .collect();
            Chat {
                resolution: if index % 7 == 0 { 2 } else { 1 },
                ..Chat::new(
                    &format!("chat-many-{index}"),
                    &format!("A {subject}, build {index}"),
                    messages,
                )
            }
        })
        .collect();
    Fixture {
        chats,
        benches: Vec::new(),
        library: Vec::new(),
        prompt_titles: Vec::new(),
        archived_benches: Vec::new(),
    }
}

// chat (stage four's chat agent)
/// `long` with twenty pictures, one in every twentieth model message, each
/// its own file (`pictures`): the memory after scrolling them all.
pub fn long_with_pictures(pictures: &[std::path::PathBuf]) -> Fixture {
    let mut fixture = long();
    let chat = &mut fixture.chats[0];
    for (n, picture) in pictures.iter().enumerate() {
        let message = &mut chat.messages[20 * n + 1];
        let text = message.payload["content"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        message.payload["content"] =
            Value::String(format!("{text}\n\n![Picture {n}]({})", picture.display()));
    }
    fixture
}

// bench agent: the `bench` fixture.

/// The rounds of `bench`'s castle bench.
pub const CASTLE_ROUNDS: [&str; 3] = [
    "A castle with four corner towers and a moat.",
    "Add a drawbridge over the moat on the south side.",
    "Light the courtyard with {{light}} lanterns.",
];

/// A stopped turn's stored answer.
fn stopped(id: &str) -> Row {
    row(
        id,
        json!({ "role": "assistant", "content": "(stopped by user)", "stopped": true }),
    )
}

/// `bench`: `gate`'s chats and its two-model bench, then a blind castle
/// bench of eight models on its third round (five done, one running, one
/// failed, one stopped; two of them the same model at two efforts), and a
/// revealed bench of three on two finished rounds. Its library, votes and
/// leaderboard are the stand-in's (`StandIn::bench_fixture`).
pub fn bench(texts: &Texts) -> Fixture {
    let mut fixture = gate(texts);
    let members: [(&str, &str, &str, Option<&str>, &str); 8] = [
        ("castle-1", "anthropic", "claude-opus-5-5", None, "done"),
        ("castle-2", "openai", "gpt-5.5", Some("high"), "done"),
        ("castle-3", "openai", "gpt-5.5", Some("low"), "running"),
        ("castle-4", "google", "gemini-3-pro", None, "done"),
        ("castle-5", "openrouter", "x-ai/grok-5", None, "done"),
        (
            "castle-6",
            "openrouter",
            "anthropic/claude-sonnet-5",
            None,
            "done",
        ),
        (
            "castle-7",
            "claude-code",
            "claude-haiku-5-5",
            None,
            "failed",
        ),
        ("castle-8", "groq", "llama-5-70b", None, "stopped"),
    ];
    let answers = [
        "Four round towers of stone bricks at the corners, a water moat three wide.",
        "An oak drawbridge on chains, lowered over the moat on the south side.",
        "Lanterns hang on the courtyard walls every four blocks.",
    ];
    for (slot, (id, provider, model, effort, last)) in members.iter().enumerate() {
        let mut messages = Vec::new();
        for (round, prompt) in CASTLE_ROUNDS.iter().enumerate() {
            let prompt = prompt.replace("{{light}}", "soul");
            messages.push(user(&format!("{id}-u{round}"), &prompt));
            if round < 2 {
                messages.push(assistant(&format!("{id}-a{round}"), answers[round]));
                continue;
            }
            match *last {
                "done" => messages.push(assistant(&format!("{id}-a{round}"), answers[round])),
                "failed" => messages.push(error(&format!("{id}-a{round}"), FAILURE_TEXT)),
                "stopped" => messages.push(stopped(&format!("{id}-a{round}"))),
                _ => {}
            }
        }
        fixture.chats.insert(
            0,
            Chat {
                bench: Some(("bench-castle".into(), slot as u32)),
                // Started 95 s before the fixture is made, on the
                // stand-in's own clock.
                running_since: (*last == "running").then(|| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("after 1970")
                        .as_millis() as i64
                        - 95_000
                }),
                unread: *last == "done" && slot % 2 == 0,
                model: Some((
                    provider.to_string(),
                    model.to_string(),
                    effort.map(str::to_string),
                )),
                ..Chat::new(id, "Castle with a moat", messages)
            },
        );
    }
    let houses = [
        ("house-1", "anthropic", "claude-opus-5-5"),
        ("house-2", "openai", "gpt-5.5"),
        ("house-3", "google", "gemini-3-pro"),
    ];
    for (slot, (id, provider, model)) in houses.iter().enumerate() {
        fixture.chats.insert(
            0,
            Chat {
                bench: Some(("bench-houses".into(), slot as u32)),
                model: Some((provider.to_string(), model.to_string(), None)),
                ..Chat::new(
                    id,
                    "Village houses",
                    vec![
                        user(&format!("{id}-u0"), "Three village houses around a well."),
                        assistant(
                            &format!("{id}-a0"),
                            "Three oak houses face a cobblestone well.",
                        ),
                        user(&format!("{id}-u1"), "Give each house a garden."),
                        assistant(
                            &format!("{id}-a1"),
                            "Each house has a fenced garden of flowers.",
                        ),
                    ],
                )
            },
        );
    }
    fixture.benches.insert(
        0,
        Bench {
            id: "bench-houses".into(),
            title: "Village houses".into(),
            blind: true,
            revealed: true,
            order: vec!["house-2".into(), "house-3".into(), "house-1".into()],
        },
    );
    fixture.benches.insert(
        0,
        Bench {
            id: "bench-castle".into(),
            title: "Castle with a moat".into(),
            blind: true,
            revealed: false,
            order: [5, 2, 7, 0, 3, 6, 1, 4]
                .iter()
                .map(|i: &usize| format!("castle-{}", i + 1))
                .collect(),
        },
    );
    fixture
}
