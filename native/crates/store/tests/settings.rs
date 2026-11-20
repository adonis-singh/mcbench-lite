//! `test/dotenv.test.js` (3), `test/settings-data-dir.test.js` (2), the
//! settings file's half of `test/settings.test.js` (7 of 11: favourites,
//! failed and planted saves, prototype names, Google's entry, the title
//! model and the share repository; the other four test connections, CLI
//! logins and the Settings screen's summaries, which are
//! `mcbench-providers`'), and the library's half of `test/prompts.test.js`
//! (5 of 7; the tool call and the tool list are `mcbench-agent`'s), ported.

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use mcbench_store::json::{self, Json};
use mcbench_store::{
    LibraryPrompt, PROVIDERS, PromptLibrary, SettingsFile, SystemHost, dot_env, object,
};

// ---- dotenv.test.js ----------------------------------------------------------

#[cfg(unix)]
fn dotenv(scratch: &std::path::Path, mode: u32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let folder = scratch.join(format!("checkout-{mode:o}"));
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join(".env");
    std::fs::write(&file, "MCBENCH_DOTENV_TEST=from-file\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
    file
}

#[cfg(unix)]
#[test]
fn a_env_only_its_owner_can_read_sets_the_variables_the_environment_does_not() {
    let scratch = common::scratch("dotenv");
    let set = dot_env(&dotenv(&scratch, 0o600), |_| None).unwrap();
    assert_eq!(
        set,
        [("MCBENCH_DOTENV_TEST".to_string(), "from-file".to_string())]
    );
    assert!(
        dot_env(&dotenv(&scratch, 0o600), |_| Some("set".into()))
            .unwrap()
            .is_empty()
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

#[cfg(unix)]
#[test]
fn a_env_the_group_or_other_accounts_can_read_or_write_fails_naming_it_and_saying_to_run_chmod_600()
{
    let scratch = common::scratch("dotenv-mode");
    for mode in [0o644, 0o640, 0o604, 0o620, 0o602] {
        let file = dotenv(&scratch, mode);
        let error = dot_env(&file, |_| None).unwrap_err();
        assert_eq!(
            error.message,
            format!(
                "{} can be read or changed by other accounts on this machine (mode {mode:o}). It holds API keys: run `chmod 600 .env` in {}, then start the app again.",
                file.display(),
                file.parent().unwrap().display()
            )
        );
    }
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn no_env_sets_nothing() {
    assert!(
        dot_env(
            &std::env::temp_dir().join("missing-mcbench").join(".env"),
            |_| None
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn dot_env_reads_lines_as_today() {
    let scratch = common::scratch("dotenv-lines");
    let file = scratch.join(".env");
    std::fs::write(
        &file,
        "# comment\n A = 'one' \nB=\"two\"\r\nC=x=y\nA=again\n#D=no\nbad line\n_E=\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let set = dot_env(&file, |_| None).unwrap();
    let pairs: Vec<(&str, &str)> = set.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(
        pairs,
        [("A", "one' "), ("B", "two"), ("C", "x=y"), ("_E", "")]
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

// ---- settings-data-dir.test.js -----------------------------------------------

#[test]
fn settings_are_read_from_and_written_to_the_data_folder() {
    let data = common::scratch("settings-data");
    std::fs::write(
        data.join("settings.json"),
        r#"{"providers":{"openai":{"apiKey":"scratch-key"}}}"#,
    )
    .unwrap();
    let settings = SettingsFile::open(SettingsFile::path(&data, |_| None)).unwrap();
    assert_eq!(settings.get().api_key("openai"), Some("scratch-key"));
    settings
        .update_model_favorites(&Json::from(vec!["google::gemini-3.8-flash"]))
        .unwrap();
    let written =
        json::parse(&std::fs::read_to_string(data.join("settings.json")).unwrap()).unwrap();
    assert_eq!(
        written.get("modelFavorites").unwrap(),
        &Json::from(vec!["google::gemini-3.8-flash"])
    );
    std::fs::remove_dir_all(data).unwrap();
}

#[test]
fn mcbench_settings_file_names_the_settings_file_whatever_the_data_folder() {
    let scratch = common::scratch("settings-file");
    let file = scratch.join("named.json");
    std::fs::write(&file, r#"{"providers":{"openai":{"apiKey":"named-key"}}}"#).unwrap();
    let path = SettingsFile::path(&scratch.join("data"), |name| {
        (name == "MCBENCH_SETTINGS_FILE").then(|| file.display().to_string())
    });
    assert_eq!(path, file);
    assert_eq!(
        SettingsFile::open(path).unwrap().get().api_key("openai"),
        Some("named-key")
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

// ---- settings.test.js (the file's half) --------------------------------------

fn fresh_settings(name: &str) -> (PathBuf, SettingsFile) {
    let scratch = common::scratch(name);
    let settings = SettingsFile::open(scratch.join("settings.json")).unwrap();
    (scratch, settings)
}

#[test]
fn model_favorites_are_normalized_before_persistence() {
    let value = json::parse(r#"["codex::gpt-5.6-sol","codex::gpt-5.6-sol","openrouter::openai/gpt-5.6-luna","unknown::model","openai::","codex::  gpt-5.5  ",null]"#).unwrap();
    assert_eq!(
        mcbench_store::normalize_model_favorites(Some(&value)),
        Json::from(vec![
            "codex::gpt-5.6-sol",
            "openrouter::openai/gpt-5.6-luna",
            "codex::gpt-5.5"
        ])
    );
}

#[test]
fn an_update_that_cannot_be_written_leaves_the_settings_in_memory_as_the_file_has_them() {
    let (scratch, settings) = fresh_settings("settings-failed");
    let before = settings.get();
    let temp = PathBuf::from(format!(
        "{}.{}.tmp",
        settings.file().display(),
        std::process::id()
    ));
    let refused = format!(
        "The settings were not saved: {} already exists. Remove it and save again.",
        temp.display()
    );
    // Something stands at the temporary file's name. On Windows a folder
    // there makes CreateFile answer "Access is denied", not "already
    // exists", so a file stands there instead.
    #[cfg(unix)]
    std::fs::create_dir(&temp).unwrap();
    #[cfg(windows)]
    std::fs::write(&temp, "").unwrap();
    let next = object! {
        "providers" => object! { "openai" => object! { "apiKey" => "sk-never-saved" } },
        "titleModel" => object! { "provider" => "openai", "model" => "gpt-fixture", "effort" => "high" },
        "shareRepo" => Json::Null,
    };
    assert_eq!(settings.update(&next).unwrap_err().message, refused);
    assert_eq!(settings.get(), before);
    assert_eq!(
        settings
            .update_model_favorites(&Json::from(vec!["codex::gpt-5.5"]))
            .unwrap_err()
            .message,
        refused
    );
    assert_eq!(settings.get(), before);
    #[cfg(unix)]
    std::fs::remove_dir(&temp).unwrap();
    #[cfg(windows)]
    std::fs::remove_file(&temp).unwrap();
    assert_eq!(
        settings
            .update_model_favorites(&Json::from(vec!["codex::gpt-5.5"]))
            .unwrap(),
        ["codex::gpt-5.5"]
    );
    assert_eq!(
        json::parse(&std::fs::read_to_string(settings.file()).unwrap()).unwrap(),
        settings.get().0
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

#[cfg(unix)]
#[test]
fn a_link_planted_at_the_temporary_files_name_fails_the_save_and_the_keys_never_reach_its_target() {
    let (scratch, settings) = fresh_settings("settings-planted");
    let before = settings.get();
    let temp = PathBuf::from(format!(
        "{}.{}.tmp",
        settings.file().display(),
        std::process::id()
    ));
    let planted = scratch.join("another-account.json");
    std::fs::write(&planted, "").unwrap();
    std::os::unix::fs::symlink(&planted, &temp).unwrap();
    let next = object! { "providers" => object! { "openai" => object! { "apiKey" => "sk-fixture-planted" } }, "titleModel" => Json::Null, "shareRepo" => Json::Null };
    assert_eq!(
        settings.update(&next).unwrap_err().message,
        format!(
            "The settings were not saved: {} already exists. Remove it and save again.",
            temp.display()
        )
    );
    assert_eq!(std::fs::read_to_string(&planted).unwrap(), "");
    assert!(
        !settings.file().exists()
            || !std::fs::symlink_metadata(settings.file())
                .unwrap()
                .file_type()
                .is_symlink()
    );
    assert_eq!(settings.get(), before);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn provider_names_inherited_from_object_prototype_are_not_providers() {
    let (scratch, settings) = fresh_settings("settings-proto");
    let providers = settings.get().0.get("providers").unwrap().clone();
    for name in ["constructor", "toString", "__proto__", "hasOwnProperty"] {
        let next = object! { "providers" => providers.clone(), "titleModel" => object! { "provider" => name, "model" => "m" }, "shareRepo" => Json::Null };
        assert_eq!(
            settings.update(&next).unwrap_err().message,
            format!("Unknown title provider: {name}")
        );
    }
    let value = Json::from(vec![
        "constructor::m",
        "toString::m",
        "__proto__::m",
        "openai::gpt-fixture",
    ]);
    assert_eq!(
        mcbench_store::normalize_model_favorites(Some(&value)),
        Json::from(vec!["openai::gpt-fixture"])
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn google_takes_an_api_key_from_settings_or_google_api_key() {
    let google = PROVIDERS.iter().find(|p| p.id == "google").unwrap();
    assert_eq!(
        (google.label, google.env_key, google.auth, google.harness),
        ("Google", Some("GOOGLE_API_KEY"), None, false)
    );
    let (scratch, settings) = fresh_settings("settings-google");
    assert_eq!(
        settings
            .get()
            .0
            .get("providers")
            .unwrap()
            .get("google")
            .unwrap(),
        &object! { "apiKey" => "", "enabled" => true, "hiddenModels" => Vec::<String>::new(), "modelOrder" => Vec::<String>::new() }
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn the_title_model_is_the_chats_own_until_one_is_chosen_and_a_chosen_one_is_kept() {
    let (scratch, settings) = fresh_settings("settings-title");
    let providers = settings.get().0.get("providers").unwrap().clone();
    let next = |title: Json| object! { "providers" => providers.clone(), "titleModel" => title, "shareRepo" => Json::Null };
    assert_eq!(settings.get().title_model(), &Json::Null);
    assert!(
        settings
            .update(&next(object! { "provider" => "nowhere", "model" => "m" }))
            .unwrap_err()
            .message
            .contains("Unknown title provider: nowhere")
    );
    assert!(
        settings
            .update(&next(object! { "provider" => "google", "model" => " " }))
            .unwrap_err()
            .message
            .contains("Choose a title model")
    );
    settings
        .update(&next(
            object! { "provider" => "google", "model" => "gemini-3.8-flash", "effort" => "low" },
        ))
        .unwrap();
    assert_eq!(
        settings.get().title_model(),
        &object! { "provider" => "google", "model" => "gemini-3.8-flash", "effort" => "low" }
    );
    settings.update(&next(Json::Null)).unwrap();
    assert_eq!(settings.get().title_model(), &Json::Null);
    let written = json::parse(&std::fs::read_to_string(settings.file()).unwrap()).unwrap();
    assert_eq!(written.get("titleModel").unwrap(), &Json::Null);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn the_repository_for_shared_builds_is_null_until_one_is_set_and_is_owner_name() {
    let (scratch, settings) = fresh_settings("settings-share");
    let providers = settings.get().0.get("providers").unwrap().clone();
    assert_eq!(settings.get().share_repo(), None);
    let with = |share: Option<Json>| {
        let mut next = object! { "providers" => providers.clone(), "titleModel" => Json::Null };
        if let Some(share) = share {
            next.as_object_mut()
                .unwrap()
                .insert("shareRepo".into(), share);
        }
        next
    };
    assert!(
        settings
            .update(&with(Some(json::str("not a repo"))))
            .unwrap_err()
            .message
            .contains("must be owner/name")
    );
    assert!(
        settings
            .update(&with(None))
            .unwrap_err()
            .message
            .contains("must be owner/name")
    );
    settings
        .update(&with(Some(json::str(" owner/builds.site "))))
        .unwrap();
    assert_eq!(settings.get().share_repo(), Some("owner/builds.site"));
    let written = json::parse(&std::fs::read_to_string(settings.file()).unwrap()).unwrap();
    assert_eq!(
        written.get("shareRepo").and_then(Json::as_str),
        Some("owner/builds.site")
    );
    settings.update(&with(Some(Json::Null))).unwrap();
    assert_eq!(settings.get().share_repo(), None);
    std::fs::remove_dir_all(scratch).unwrap();
}

// ---- prompts.test.js (the library's half) ------------------------------------

fn library(name: &str) -> (PathBuf, PromptLibrary) {
    let scratch = common::scratch(name);
    let prompts = PromptLibrary::new(scratch.join("prompts.json"), Arc::new(SystemHost));
    (scratch, prompts)
}

/// Prompts in their stored form.
fn jsons(prompts: Vec<LibraryPrompt>) -> Vec<Json> {
    prompts.iter().map(LibraryPrompt::to_json).collect()
}

/// The library as `list()` reads it, in its stored form.
fn listed(prompts: &PromptLibrary) -> Vec<Json> {
    jsons(prompts.list().unwrap())
}

fn ids(prompts: &[Json]) -> Vec<String> {
    prompts
        .iter()
        .map(|p| p.get("id").unwrap().as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_library_starts_from_the_seed_prompts_and_saves_the_order_the_user_gives_it() {
    let (scratch, prompts) = library("prompts-seed");
    let seeded = listed(&prompts);
    assert!(!seeded.is_empty());
    let reversed: Vec<Json> = seeded.iter().rev().cloned().collect();
    let saved = jsons(prompts.save(&reversed).unwrap());
    assert_eq!(ids(&listed(&prompts)), ids(&saved));
    assert_eq!(
        ids(&listed(&prompts))[0],
        ids(&seeded).last().unwrap().clone()
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn a_prompt_keeps_attachments_that_carry_their_bytes_and_nothing_else() {
    let (scratch, prompts) = library("prompts-attachments");
    let data = "data:image/png;base64,cG5n";
    let saved = jsons(prompts
        .save(&[object! { "id" => "ref", "title" => "Ref", "text" => "x", "attachments" => vec![object! { "name" => "ref.png", "type" => "text/html", "size" => 3i64, "data" => data, "url" => "javascript:alert(1)" }] }])
        .unwrap());
    assert_eq!(
        saved[0].get("attachments").unwrap(),
        &Json::Array(vec![
            object! { "name" => "ref.png", "type" => "image/png", "size" => 3i64, "data" => data }
        ])
    );
    for attachment in [
        object! { "name" => "a.png", "data" => "https://example.com/a.png" },
        object! { "name" => "a.png", "url" => "/api/attachments/x/a.png" },
        object! { "data" => data },
    ] {
        let error = prompts.save(&[object! { "id" => "ref", "title" => "Ref", "text" => "x", "attachments" => vec![attachment] }]).unwrap_err();
        assert_eq!(error.status, Some(400));
    }
    std::fs::remove_dir_all(scratch).unwrap();
}

fn chat_1() -> Json {
    object! { "chat" => "chat-1", "model" => "test/model" }
}

#[test]
fn a_model_reads_the_library_and_adds_prompts_which_are_marked_with_its_chat_and_model() {
    let (scratch, prompts) = library("prompts-model");
    prompts
        .save(&[
            object! { "id" => "a", "title" => "Castle", "text" => "Build a castle", "tags" => vec!["Medieval"], "attachments" => vec![object! { "name" => "ref.png", "type" => "image/png", "data" => "data:image/png;base64,AA==" }] },
            object! { "id" => "b", "title" => "Tower", "text" => "Build a tower", "tags" => Vec::<String>::new() },
        ])
        .unwrap();
    let read = prompts.apply(&object! {}, &chat_1()).unwrap();
    assert_eq!(ids(&read), ["a", "b"]);
    assert_eq!(
        read[0].get("attachments").unwrap(),
        &Json::from(vec!["ref.png"])
    );
    assert_eq!(read[0].get("tags").unwrap(), &Json::from(vec!["medieval"]));
    assert!(read[0].get("addedBy").is_none());
    let after = prompts.apply(&object! { "save" => vec![object! { "title" => "Moat", "text" => "Dig a {{shape}} moat", "tags" => vec!["medieval"] }] }, &chat_1()).unwrap();
    assert_eq!(
        after
            .iter()
            .map(|p| p.get("title").unwrap().as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Castle", "Tower", "Moat"]
    );
    assert_eq!(after[2].get("addedBy").unwrap(), &chat_1());
    assert_eq!(listed(&prompts)[2].get("addedBy").unwrap(), &chat_1());
    assert!(
        prompts
            .apply(
                &object! { "save" => vec![object! { "title" => "Empty" }] },
                &chat_1()
            )
            .unwrap_err()
            .message
            .contains("needs text")
    );
    assert!(
        prompts
            .apply(&object! { "remove" => "b" }, &chat_1())
            .unwrap_err()
            .message
            .contains("remove must be an array")
    );
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn a_model_cannot_change_remove_or_reorder_a_prompt_it_did_not_add() {
    let (scratch, prompts) = library("prompts-others");
    prompts
        .save(&[
            object! { "id" => "user", "title" => "Castle", "text" => "Build a castle" },
            object! { "id" => "other-chat", "title" => "Hut", "text" => "Build a hut", "addedBy" => object! { "chat" => "chat-2", "model" => "test/model" } },
            object! { "id" => "other-model", "title" => "Barn", "text" => "Build a barn", "addedBy" => object! { "chat" => "chat-1", "model" => "other/model" } },
        ])
        .unwrap();
    let before = listed(&prompts);
    for id in ["user", "other-chat", "other-model"] {
        let expected = format!("Prompt {id} was not added by you in this chat");
        assert!(
            prompts
                .apply(
                    &object! { "save" => vec![object! { "id" => id, "text" => "Something else" }] },
                    &chat_1()
                )
                .unwrap_err()
                .message
                .contains(&expected)
        );
        assert!(
            prompts
                .apply(&object! { "remove" => vec![id] }, &chat_1())
                .unwrap_err()
                .message
                .contains(&expected)
        );
    }
    assert!(
        prompts
            .apply(
                &object! { "order" => vec!["other-model", "user"] },
                &chat_1()
            )
            .unwrap_err()
            .message
            .contains("takes only save and remove; not order")
    );
    assert!(
        prompts
            .apply(
                &object! { "save" => vec![object! { "id" => "missing", "title" => "x" }] },
                &chat_1()
            )
            .unwrap_err()
            .message
            .contains("No prompt with id missing")
    );
    for (key, value) in [
        ("addedBy", Json::Null),
        ("favorite", Json::Bool(true)),
        ("attachments", Json::Array(vec![])),
    ] {
        let mut entry = object! { "text" => "Build a well" };
        entry
            .as_object_mut()
            .unwrap()
            .insert((key).to_string(), value);
        assert!(
            prompts
                .apply(&object! { "save" => vec![entry] }, &chat_1())
                .unwrap_err()
                .message
                .contains("has only the fields id, title, text, tags")
        );
    }
    assert_eq!(listed(&prompts), before);
    std::fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn a_model_changes_and_removes_the_prompts_it_added_and_the_user_keeps_or_edits_them_as_any_other()
{
    let (scratch, prompts) = library("prompts-own");
    prompts
        .save(&[object! { "id" => "user", "title" => "Castle", "text" => "Build a castle" }])
        .unwrap();
    let added = prompts
        .apply(
            &object! { "save" => vec![object! { "title" => "Moat", "text" => "Dig a moat" }] },
            &chat_1(),
        )
        .unwrap();
    let id = added[1].get("id").unwrap().as_str().unwrap().to_string();
    prompts.apply(&object! { "save" => vec![object! { "id" => id.as_str(), "text" => "Dig a deep moat", "tags" => vec!["Water"] }] }, &chat_1()).unwrap();
    assert_eq!(
        listed(&prompts)[1],
        object! { "id" => id.as_str(), "title" => "Moat", "text" => "Dig a deep moat", "tags" => vec!["water"], "favorite" => false, "attachments" => Json::Array(vec![]), "addedBy" => chat_1() }
    );
    let edited: Vec<Json> = listed(&prompts)
        .into_iter()
        .map(|mut p| {
            if p.get("id").unwrap().as_str() == Some(id.as_str()) {
                let object = p.as_object_mut().unwrap();
                object.insert("title".into(), json::str("Deep moat"));
                object.insert("favorite".into(), Json::Bool(true));
            }
            p
        })
        .collect();
    prompts.save(&edited).unwrap();
    assert_eq!(listed(&prompts)[1].get("addedBy").unwrap(), &chat_1());
    prompts
        .apply(&object! { "remove" => vec![id.as_str()] }, &chat_1())
        .unwrap();
    assert_eq!(ids(&listed(&prompts)), ["user"]);
    std::fs::remove_dir_all(scratch).unwrap();
}
