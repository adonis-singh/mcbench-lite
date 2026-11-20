//! The settings file (`server/settings.js`): per provider its key, whether it
//! is offered, and which of its models the picker hides and in what order;
//! the title model, the model favourites and the share repository. Keys are
//! the user's alone (mode 0600, written whole then renamed into place). What
//! the Settings screen shows of each provider's connection (logins, CLI
//! versions) is `mcbench-providers`'.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::Error;
use crate::json::Truthy;
use crate::json::{self, Json, Object};
use crate::storage::js_space;

/// A provider of `server/settings.js` `PROVIDERS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderInfo {
    /// The key settings and stored chats name it by.
    pub id: &'static str,
    /// `label`.
    pub label: &'static str,
    /// `envKey`: the variable its API key may come from.
    pub env_key: Option<&'static str>,
    /// `auth` (`api-key` when unset).
    pub auth: Option<&'static str>,
    /// `harness`: the vendor's own agent loop with mcbench's tools.
    pub harness: bool,
    /// `runtime`: `(binary, binaryEnv, homeEnv, home)` of a CLI harness.
    pub runtime: Option<(&'static str, &'static str, &'static str, &'static str)>,
}

/// `PROVIDERS`, in its order.
pub const PROVIDERS: [ProviderInfo; 10] = [
    ProviderInfo {
        id: "openrouter",
        label: "OpenRouter",
        env_key: Some("OPENROUTER_API_KEY"),
        auth: None,
        harness: false,
        runtime: None,
    },
    ProviderInfo {
        id: "openai",
        label: "OpenAI",
        env_key: Some("OPENAI_API_KEY"),
        auth: None,
        harness: false,
        runtime: None,
    },
    ProviderInfo {
        id: "codex",
        label: "ChatGPT",
        env_key: None,
        auth: Some("cli-login"),
        harness: false,
        runtime: None,
    },
    ProviderInfo {
        id: "anthropic",
        label: "Anthropic",
        env_key: Some("ANTHROPIC_API_KEY"),
        auth: None,
        harness: false,
        runtime: None,
    },
    ProviderInfo {
        id: "groq",
        label: "Groq",
        env_key: Some("GROQ_API_KEY"),
        auth: None,
        harness: false,
        runtime: None,
    },
    ProviderInfo {
        id: "google",
        label: "Google",
        env_key: Some("GOOGLE_API_KEY"),
        auth: None,
        harness: false,
        runtime: None,
    },
    ProviderInfo {
        id: "qwencloud",
        label: "QwenCloud",
        env_key: Some("QWENCLOUD_API_KEY"),
        auth: None,
        harness: false,
        runtime: None,
    },
    ProviderInfo {
        id: "cursor",
        label: "Cursor",
        env_key: Some("CURSOR_API_KEY"),
        auth: Some("api-key-or-login"),
        harness: true,
        runtime: None,
    },
    ProviderInfo {
        id: "claude-code",
        label: "Claude Code",
        env_key: None,
        auth: Some("cli-login"),
        harness: true,
        runtime: Some(("claude", "MCBENCH_CLAUDE_BINARY", "CLAUDE_CONFIG_DIR", "~")),
    },
    ProviderInfo {
        id: "codex-cli",
        label: "Codex CLI",
        env_key: None,
        auth: Some("cli-login"),
        harness: true,
        runtime: Some(("codex", "MCBENCH_CODEX_BINARY", "CODEX_HOME", "~/.codex")),
    },
];

fn provider(id: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|provider| provider.id == id)
}

/// The settings as `settings.get()` returns them: `{ providers, titleModel,
/// modelFavorites, shareRepo }`.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings(pub Json);

impl Settings {
    fn provider(&self, id: &str) -> Option<&Json> {
        self.0.get("providers")?.get(id)
    }

    /// The saved API key of provider `id` (`''` when none).
    pub fn api_key(&self, id: &str) -> Option<&str> {
        self.provider(id)?.get("apiKey")?.as_str()
    }

    /// Whether provider `id` is offered.
    pub fn enabled(&self, id: &str) -> bool {
        self.provider(id).and_then(|p| p.get("enabled")) == Some(&Json::Bool(true))
    }

    /// `titleModel`, `null` until the user picks one.
    pub fn title_model(&self) -> &Json {
        self.0.get("titleModel").unwrap_or(&Json::Null)
    }

    /// `modelFavorites`.
    pub fn model_favorites(&self) -> Vec<String> {
        self.0
            .get("modelFavorites")
            .and_then(Json::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Json::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `shareRepo`.
    pub fn share_repo(&self) -> Option<&str> {
        self.0.get("shareRepo")?.as_str()
    }
}

/// The settings file of a data folder, and the settings it holds.
pub struct SettingsFile {
    file: PathBuf,
    settings: Mutex<Settings>,
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(js_space)
}

/// `String(value || '')`.
fn string_or_empty(value: Option<&Json>) -> String {
    match value {
        Some(value) if value.truthy() => json::to_js_string(Some(value)),
        _ => String::new(),
    }
}

/// `strings(value)`: the distinct non-empty strings of an array.
fn strings(value: Option<&Json>) -> Json {
    let mut out: Vec<String> = Vec::new();
    if let Some(Json::Array(items)) = value {
        for item in items {
            if let Json::String(text) = item
                && !text.is_empty()
                && !out.contains(text)
            {
                out.push(text.clone());
            }
        }
    }
    out.into()
}

/// `providerSettings(id, saved, previous)`.
fn provider_settings(id: &str, saved: Option<&Json>, previous: Option<&Json>) -> Json {
    let info = provider(id).expect("a provider of PROVIDERS");
    let saved_key = saved
        .and_then(|s| s.get("apiKey"))
        .and_then(Json::as_str)
        .map(js_trim)
        .filter(|key| !key.is_empty());
    let key = match saved_key {
        Some(key) => key.to_string(),
        None => previous
            .and_then(|p| p.get("apiKey"))
            .filter(|key| key.truthy())
            .map_or(String::new(), |key| json::to_js_string(Some(key))),
    };
    let mut out = Object::new();
    if info.env_key.is_some() {
        out.insert("apiKey".into(), Json::String(key));
    }
    out.insert(
        "enabled".into(),
        Json::Bool(saved.and_then(|s| s.get("enabled")) != Some(&Json::Bool(false))),
    );
    out.insert(
        "hiddenModels".into(),
        strings(saved.and_then(|s| s.get("hiddenModels"))),
    );
    out.insert(
        "modelOrder".into(),
        strings(saved.and_then(|s| s.get("modelOrder"))),
    );
    Json::Object(out)
}

/// `normalizeModelFavorites(value)`.
pub fn normalize_model_favorites(value: Option<&Json>) -> Json {
    let Some(Json::Array(items)) = value else {
        return Json::Array(Vec::new());
    };
    let mut favorites: Vec<String> = Vec::new();
    for entry in items {
        let Json::String(entry) = entry else { continue };
        // `indexOf` and `slice` count UTF-16 units; `::` is ASCII, so byte
        // offsets name the same places.
        let separator = entry.find("::");
        let (provider_id, model) = match separator {
            Some(at) => (&entry[..at], js_trim(&entry[at + 2..])),
            None => continue,
        };
        let key = format!("{provider_id}::{model}");
        if separator.is_none_or(|at| at < 1)
            || provider(provider_id).is_none()
            || model.is_empty()
            || favorites.contains(&key)
        {
            continue;
        }
        favorites.push(key);
    }
    favorites.truncate(500);
    favorites.into()
}

/// `titleModelSetting(value)`.
fn title_model_setting(value: Option<&Json>) -> Result<Json, Error> {
    let provider_id = js_trim(&string_or_empty(value.and_then(|v| v.get("provider")))).to_string();
    let model = js_trim(&string_or_empty(value.and_then(|v| v.get("model")))).to_string();
    if provider(&provider_id).is_none() {
        return Err(Error::plain(format!(
            "Unknown title provider: {provider_id}"
        )));
    }
    if model.is_empty() {
        return Err(Error::plain("Choose a title model"));
    }
    let effort = js_trim(&string_or_empty(value.and_then(|v| v.get("effort")))).to_string();
    Ok(crate::object! { "provider" => provider_id, "model" => model, "effort" => effort })
}

/// `shareRepoSetting(value)`.
fn share_repo_setting(value: Option<&Json>) -> Result<Json, Error> {
    let repo = js_trim(&json::to_js_string(value)).to_string();
    let pattern = crate::regex!(r"^[\w.-]+/[\w.-]+$");
    // JavaScript's `\w` is ASCII.
    if !repo.is_ascii() || !pattern.is_match(&repo) {
        return Err(Error::plain(
            "The repository for shared builds must be owner/name",
        ));
    }
    Ok(Json::String(repo))
}

impl SettingsFile {
    /// Where the settings are: `MCBENCH_SETTINGS_FILE` when set (`lookup` is
    /// `std::env::var`'s), else `settings.json` in the data folder.
    pub fn path(data: &Path, lookup: impl Fn(&str) -> Option<String>) -> PathBuf {
        match lookup("MCBENCH_SETTINGS_FILE").filter(|file| !file.is_empty()) {
            Some(file) => {
                crate::files::normalize(&std::path::absolute(file).expect("the working folder"))
            }
            None => data.join("settings.json"),
        }
    }

    /// Reads the settings in `file` (none when it does not exist).
    pub fn open(file: PathBuf) -> Result<SettingsFile, Error> {
        let saved = if file.exists() {
            json::parse(&std::fs::read_to_string(&file)?)?
        } else {
            Json::Object(Object::new())
        };
        let mut providers = Object::new();
        for info in &PROVIDERS {
            providers.insert(
                (info.id).to_string(),
                provider_settings(
                    info.id,
                    saved.get("providers").and_then(|p| p.get(info.id)),
                    None,
                ),
            );
        }
        let title = match saved.get("titleModel").filter(|title| title.truthy()) {
            Some(Json::Object(title)) => {
                let mut title = title.clone();
                let effort = title
                    .get("effort")
                    .filter(|effort| effort.truthy())
                    .cloned()
                    .unwrap_or(json::str("low"));
                title.insert("effort".into(), effort);
                Json::Object(title)
            }
            Some(other) => other.clone(),
            None => Json::Null,
        };
        let settings = crate::object! {
            "providers" => providers,
            "titleModel" => title,
            "modelFavorites" => normalize_model_favorites(saved.get("modelFavorites")),
            "shareRepo" => saved.get("shareRepo").cloned().unwrap_or(Json::Null),
        };
        Ok(SettingsFile {
            file,
            settings: Mutex::new(Settings(settings)),
        })
    }

    /// The file.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// `get()`.
    pub fn get(&self) -> Settings {
        self.settings.lock().expect("the settings' lock").clone()
    }

    /// `save(next)`: the settings in memory become `next` only once the
    /// file holds it. The temporary file is created new, so anything at its
    /// name (a link another account planted) fails the save.
    fn save(&self, current: &mut Settings, next: Json) -> Result<(), Error> {
        std::fs::create_dir_all(self.file.parent().expect("a folder"))?;
        let temporary = PathBuf::from(format!(
            "{}.{}.tmp",
            self.file.display(),
            std::process::id()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut handle = match options.open(&temporary) {
            Ok(handle) => handle,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(Error::plain(format!(
                    "The settings were not saved: {} already exists. Remove it and save again.",
                    temporary.display()
                )));
            }
            Err(error) => return Err(error.into()),
        };
        std::io::Write::write_all(&mut handle, json::stringify_indented(&next).as_bytes())?;
        drop(handle);
        std::fs::rename(&temporary, &self.file)?;
        *current = Settings(next);
        Ok(())
    }

    /// `update(next)`: providers' keys and switches, the title model and the
    /// share repository, checked as today. A key left empty keeps the saved
    /// one.
    pub fn update(&self, next: &Json) -> Result<Settings, Error> {
        let mut current = self.settings.lock().expect("the settings' lock");
        let mut providers = Object::new();
        for info in &PROVIDERS {
            providers.insert(
                (info.id).to_string(),
                provider_settings(
                    info.id,
                    next.get("providers").and_then(|p| p.get(info.id)),
                    current.provider(info.id),
                ),
            );
        }
        let title = match next.get("titleModel") {
            Some(Json::Null) => Json::Null,
            value => title_model_setting(value)?,
        };
        let share = match next.get("shareRepo") {
            Some(Json::Null) => Json::Null,
            value => share_repo_setting(value)?,
        };
        let settings = crate::object! {
            "providers" => providers,
            "titleModel" => title,
            "modelFavorites" => current.0.get("modelFavorites").cloned().unwrap_or(Json::Array(Vec::new())),
            "shareRepo" => share,
        };
        self.save(&mut current, settings)?;
        Ok(current.clone())
    }

    /// `updateModelFavorites(value)`.
    pub fn update_model_favorites(&self, value: &Json) -> Result<Vec<String>, Error> {
        if value.as_array().is_none() {
            return Err(Error::plain("favorites must be an array"));
        }
        let mut current = self.settings.lock().expect("the settings' lock");
        let mut next = current.0.clone();
        next.as_object_mut()
            .expect("settings are an object")
            .insert(
                "modelFavorites".into(),
                normalize_model_favorites(Some(value)),
            );
        self.save(&mut current, next)?;
        Ok(current.model_favorites())
    }
}
