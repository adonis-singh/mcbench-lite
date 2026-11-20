//! `preferences.json` across builds: every field is read on its own, so a
//! file of an earlier build (`fixtures/preferences/`) loads with what it has
//! and the defaults for the rest; an unknown field is ignored; an unreadable
//! value takes its default and is noted.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mcbench_ui::state::layout::{Placement, Rect};
use mcbench_ui::{BenchChoice, Preferences, Resolution};
use serde_json::{Value, json};

fn folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/preferences")
}

/// The preferences of the fixtures: every field non-default.
fn full() -> Preferences {
    Preferences {
        light: true,
        sidebar_open: false,
        sidebar_width: 300.,
        chat_width: Some(480.),
        binds: BTreeMap::from([("send".to_string(), "cmd-enter".to_string())]),
        bench_mode: true,
        last_model: Some(("anthropic".into(), "claude-opus-5-5".into())),
        last_resolution: Resolution::VeryHigh,
        place: Some("/chat/c1".into()),
        window: Some(Placement {
            display: "0F4B1A7E-2C3D-4E5F-8A9B-1C2D3E4F5A6B".into(),
            bounds: Rect {
                x: 10.,
                y: 20.,
                width: 1200.,
                height: 800.,
            },
        }),
        open_benches: vec!["bench-towers".into()],
        prompts_open: false,
        night: true,
        bench_models: vec![BenchChoice {
            provider: "openai".into(),
            model: "gpt-5.5".into(),
            effort: Some("high".into()),
        }],
        bench_blind: false,
        bench_resolution: Resolution::High,
        prompt_ai_model: Some(BenchChoice {
            provider: "anthropic".into(),
            model: "claude-opus-5-5".into(),
            effort: None,
        }),
        improve_source_note: true,
    }
}

/// [`full`] with the fields named in `lacking` set to the default's.
fn lacking(lacking: &[&str]) -> Preferences {
    let d = Preferences::default();
    let mut p = full();
    for name in lacking {
        match *name {
            "night" => p.night = d.night,
            "openBenches" => p.open_benches = d.open_benches.clone(),
            "promptsOpen" => p.prompts_open = d.prompts_open,
            "benchModels" => p.bench_models = d.bench_models.clone(),
            "benchBlind" => p.bench_blind = d.bench_blind,
            "benchResolution" => p.bench_resolution = d.bench_resolution,
            "promptAiModel" => p.prompt_ai_model = d.prompt_ai_model.clone(),
            "improveSource" => p.improve_source_note = d.improve_source_note,
            other => panic!("no fixture lacks {other}"),
        }
    }
    p
}

fn read(name: &str) -> (Preferences, Vec<String>) {
    let text = std::fs::read_to_string(folder().join(name)).expect("the fixture");
    Preferences::from_text_noting(&text).expect("the fixture loads")
}

const BENCH: [&str; 5] = [
    "benchModels",
    "benchBlind",
    "benchResolution",
    "promptAiModel",
    "improveSource",
];

#[test]
fn every_file_of_the_folder_loads_with_nothing_noted() {
    let mut read_files = 0;
    for entry in std::fs::read_dir(folder()).expect("the folder") {
        let path = entry.expect("an entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("the file");
        let (_, noted) = Preferences::from_text_noting(&text)
            .unwrap_or_else(|e| panic!("{} is refused: {e}", path.display()));
        assert_eq!(noted, Vec::<String>::new(), "{}", path.display());
        read_files += 1;
    }
    assert!(read_files >= 5, "{read_files} files");
}

#[test]
fn the_first_file_lacks_the_folds_the_night_and_bench() {
    let mut fields = vec!["openBenches", "promptsOpen", "night"];
    fields.extend(BENCH);
    assert_eq!(read("before-openBenches.json"), (lacking(&fields), vec![]));
}

#[test]
fn the_file_with_the_folds_lacks_the_night_and_bench() {
    let mut fields = vec!["night"];
    fields.extend(BENCH);
    assert_eq!(
        read("before-benchModels-and-night.json"),
        (lacking(&fields), vec![])
    );
}

#[test]
fn the_file_with_the_night_lacks_bench() {
    assert_eq!(read("before-benchModels.json"), (lacking(&BENCH), vec![]));
}

#[test]
fn the_file_with_bench_lacks_the_night() {
    assert_eq!(read("before-night.json"), (lacking(&["night"]), vec![]));
}

#[test]
fn todays_file_keeps_every_field() {
    assert_eq!(read("today.json"), (full(), vec![]));
}

#[test]
fn an_unknown_field_is_ignored() {
    let mut text: Value = serde_json::from_str(&full().to_text()).expect("JSON");
    text["theme"] = json!("dark");
    assert_eq!(
        Preferences::from_text_noting(&text.to_string()),
        Ok((full(), vec![]))
    );
}

/// `full()`'s text with `field` set to `value`, read.
fn with(field: &str, value: Value) -> (Preferences, Vec<String>) {
    let mut text: Value = serde_json::from_str(&full().to_text()).expect("JSON");
    text[field] = value;
    Preferences::from_text_noting(&text.to_string()).expect("loads")
}

fn noted(field: &str) -> Vec<String> {
    vec![field.to_string()]
}

#[test]
fn a_value_of_the_wrong_type_takes_the_default() {
    let d = Preferences::default();
    let cases: [(&str, Value, Preferences); 8] = [
        (
            "light",
            json!("light"),
            Preferences {
                light: d.light,
                ..full()
            },
        ),
        (
            "sidebarWidth",
            json!("wide"),
            Preferences {
                sidebar_width: d.sidebar_width,
                ..full()
            },
        ),
        (
            "chatWidth",
            json!(true),
            Preferences {
                chat_width: d.chat_width,
                ..full()
            },
        ),
        (
            "place",
            json!(7),
            Preferences {
                place: d.place.clone(),
                ..full()
            },
        ),
        (
            "night",
            json!(1),
            Preferences {
                night: d.night,
                ..full()
            },
        ),
        (
            "benchBlind",
            json!("yes"),
            Preferences {
                bench_blind: d.bench_blind,
                ..full()
            },
        ),
        (
            "lastResolution",
            json!("5"),
            Preferences {
                last_resolution: d.last_resolution,
                ..full()
            },
        ),
        (
            "promptAiModel",
            json!("claude"),
            Preferences {
                prompt_ai_model: d.prompt_ai_model.clone(),
                ..full()
            },
        ),
    ];
    for (field, value, expected) in cases {
        assert_eq!(with(field, value), (expected, noted(field)), "{field}");
    }
}

#[test]
fn a_resolution_that_is_not_a_stop_is_standard() {
    for field in ["lastResolution", "benchResolution"] {
        let (p, n) = with(field, json!(3));
        let expected = Preferences {
            last_resolution: if field == "lastResolution" {
                Resolution::Standard
            } else {
                full().last_resolution
            },
            bench_resolution: if field == "benchResolution" {
                Resolution::Standard
            } else {
                full().bench_resolution
            },
            ..full()
        };
        assert_eq!((p, n), (expected, noted(field)));
    }
}

#[test]
fn a_window_without_its_five_readable_members_is_none() {
    let mut extra = json!({"display": "d", "x": 1, "y": 2, "width": 3, "height": 4});
    extra["z"] = json!(0);
    for window in [
        json!({"display": "d", "x": 1, "y": 2, "width": 3}),
        extra,
        json!({"display": 5, "x": 1, "y": 2, "width": 3, "height": 4}),
        json!({"display": "d", "x": "1", "y": 2, "width": 3, "height": 4}),
        json!([1, 2]),
    ] {
        assert_eq!(
            with("window", window.clone()),
            (
                Preferences {
                    window: None,
                    ..full()
                },
                noted("window")
            ),
            "{window}"
        );
    }
}

#[test]
fn a_last_model_that_is_not_a_pair_of_strings_is_none() {
    for model in [
        json!(["anthropic"]),
        json!(["anthropic", 5]),
        json!({"provider": "anthropic", "model": "claude-opus-5-5"}),
    ] {
        assert_eq!(
            with("lastModel", model.clone()),
            (
                Preferences {
                    last_model: None,
                    ..full()
                },
                noted("lastModel")
            ),
            "{model}"
        );
    }
}

#[test]
fn an_improve_source_that_is_not_text_or_note_is_text() {
    assert_eq!(
        with("improveSource", json!("both")),
        (
            Preferences {
                improve_source_note: false,
                ..full()
            },
            noted("improveSource")
        )
    );
}

#[test]
fn a_collection_drops_the_entries_it_cannot_read() {
    let (p, n) = with("binds", json!({"send": "cmd-enter", "stop": 5}));
    assert_eq!((p, n), (full(), noted("binds")));
    let (p, n) = with("openBenches", json!(["bench-towers", 3, null]));
    assert_eq!((p, n), (full(), noted("openBenches")));
    let (p, n) = with(
        "benchModels",
        json!([
            {"provider": "openai", "model": "gpt-5.5", "effort": "high"},
            {"provider": "openai", "model": "gpt-5.5"},
            "gpt",
        ]),
    );
    assert_eq!((p, n), (full(), noted("benchModels")));
}

#[test]
fn a_collection_of_the_wrong_kind_is_the_default() {
    let d = Preferences::default();
    assert_eq!(
        with("binds", json!(["cmd-enter"])),
        (
            Preferences {
                binds: d.binds.clone(),
                ..full()
            },
            noted("binds")
        )
    );
    assert_eq!(
        with("openBenches", json!("bench-towers")),
        (
            Preferences {
                open_benches: d.open_benches.clone(),
                ..full()
            },
            noted("openBenches")
        )
    );
    assert_eq!(
        with("benchModels", json!({})),
        (
            Preferences {
                bench_models: d.bench_models.clone(),
                ..full()
            },
            noted("benchModels")
        )
    );
}

#[test]
fn only_text_that_is_not_a_json_object_is_refused() {
    assert!(
        Preferences::from_text("{ not json")
            .unwrap_err()
            .starts_with("the preferences are not JSON: ")
    );
    assert_eq!(
        Preferences::from_text("[1, 2]"),
        Err("the preferences are not a JSON object".to_string())
    );
}

#[test]
fn the_text_round_trips() {
    let p = full();
    assert_ne!(p.to_text(), Preferences::default().to_text());
    assert_eq!(Preferences::from_text(&p.to_text()), Ok(p));
}
