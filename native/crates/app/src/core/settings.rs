//! What the settings routes answer (`server/settings.js` `publicSettings`,
//! `details`, `summary`; `server/index.js` `GET /api/settings`'s endpoints
//! and protocols, `GET /api/config`): JSON in today's key order. Cursor is
//! not in the native app: it is listed, as stored settings name it, and
//! reads as never signed in, with no version.

use std::collections::HashMap;
use std::sync::Mutex;

use mcbench_providers::login::{Login, SignIns};
use mcbench_providers::{ProviderId, api};
use mcbench_store::json::{Json, Object};
use mcbench_store::{PROVIDERS, Settings};

use super::providers::{Providers, info_runtime};

/// `llm.js` `PROVIDERS[id].protocol` read through `index.js` `PROTOCOLS`,
/// and its `baseUrl`.
fn endpoint_and_protocol(id: &str) -> (Option<&'static str>, &'static str) {
    match id {
        "openrouter" => (Some("https://openrouter.ai/api/v1"), "Chat Completions"),
        "openai" => (Some("https://api.openai.com/v1"), "Chat Completions"),
        "codex" => (None, "Responses"),
        "anthropic" => (Some("https://api.anthropic.com/v1"), "Messages"),
        "groq" => (Some("https://api.groq.com/openai/v1"), "Chat Completions"),
        "google" => (
            Some("https://generativelanguage.googleapis.com/v1beta/openai"),
            "Chat Completions",
        ),
        "qwencloud" => (
            Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
            "Chat Completions",
        ),
        "cursor" => (None, "Cursor SDK agent"),
        "claude-code" => (None, "Claude Code CLI over MCP"),
        "codex-cli" => (None, "Codex CLI over MCP"),
        other => panic!("no protocol for {other}"),
    }
}

fn provider_settings(settings: &Settings, id: &str) -> Json {
    settings
        .0
        .get("providers")
        .and_then(|p| p.get(id))
        .cloned()
        .unwrap_or(Json::Null)
}

/// `publicSettings()`.
pub fn public_settings(providers: &Providers, settings: &Settings) -> Result<Json, String> {
    let mut list = Object::new();
    for info in &PROVIDERS {
        let saved = provider_settings(settings, info.id);
        let mut entry = Object::new();
        entry.insert("label".into(), info.label.into());
        entry.insert(
            "connected".into(),
            providers.connected(settings, info.id)?.into(),
        );
        if let Some(env) = info.env_key {
            entry.insert("envKey".into(), env.into());
        }
        entry.insert("auth".into(), info.auth.unwrap_or("api-key").into());
        entry.insert("harness".into(), info.harness.into());
        for key in ["enabled", "hiddenModels", "modelOrder"] {
            entry.insert(key.into(), saved.get(key).cloned().unwrap_or(Json::Null));
        }
        list.insert(info.id.into(), Json::Object(entry));
    }
    let mut out = Object::new();
    for key in ["titleModel", "modelFavorites", "shareRepo"] {
        out.insert(
            key.into(),
            settings.0.get(key).cloned().unwrap_or(Json::Null),
        );
    }
    out.insert("providers".into(), Json::Object(list));
    Ok(Json::Object(out))
}

/// The CLI versions read once per process (`versions`), cleared by a
/// refresh, a settings update and a CLI install.
#[derive(Default)]
pub struct Versions(Mutex<HashMap<&'static str, Option<String>>>);

impl Versions {
    /// Forgets every version read.
    pub fn clear(&self) {
        self.0.lock().expect("the versions").clear();
    }

    async fn get(&self, name: &'static str, providers: &Providers) -> Option<String> {
        if let Some(known) = self.0.lock().expect("the versions").get(name) {
            return known.clone();
        }
        let read = match name {
            "claude" => providers.claude().ok()?.version().await.ok(),
            "codex" => providers.codex().version().await.ok(),
            _ => None,
        };
        // `/(\d+\.\d+\.\d+)/`'s first match.
        let value = read.and_then(|text| {
            mcbench_store::regex!(r"(\d+\.\d+\.\d+)")
                .captures(&text)
                .map(|c| c[1].to_string())
        });
        self.0
            .lock()
            .expect("the versions")
            .insert(name, value.clone());
        value
    }
}

/// `summary(id, entry)`.
fn summary(id: &str, entry: &Object) -> String {
    let get = |key: &str| entry.get(key).cloned().unwrap_or(Json::Null);
    let truthy = |key: &str| mcbench_store::json::truthy(entry.get(key));
    if !truthy("enabled") {
        return "Disabled".into();
    }
    if get("auth") == "cli-login" && truthy("signedIn") && !truthy("connected") {
        return "CLI not installed".into();
    }
    if truthy("account") {
        return match get("plan") {
            Json::String(plan) if !plan.is_empty() => format!("Signed in · {plan}"),
            _ => format!(
                "Signed in as {}",
                mcbench_store::json::to_js_string(entry.get("account"))
            ),
        };
    }
    match get("keySource").as_str() {
        Some("saved") => return "Key saved".into(),
        Some("env") => {
            return format!(
                "Key from {}",
                mcbench_store::json::to_js_string(entry.get("envKey"))
            );
        }
        _ => {}
    }
    if id == "cursor" && truthy("signedIn") {
        return "Signed in with Cursor".into();
    }
    if entry.contains_key("envKey") {
        return if id == "cursor" {
            "No key or login".into()
        } else {
            "No key".into()
        };
    }
    "Not signed in".into()
}

/// `provider-login.js` `describe(provider)`.
fn describe(id: &str, sign_ins: &SignIns) -> Result<Option<Json>, String> {
    let Some(provider) = serde_json::from_value::<ProviderId>(Json::String(id.into())).ok() else {
        return Ok(None);
    };
    let Ok(login) = Login::of(provider) else {
        return Ok(None);
    };
    let program = match login {
        Login::Claude => "claude",
        Login::Codex => "codex",
    };
    let pin = mcbench_providers::harness::install::pins()
        .into_iter()
        .find(|p| p.name == program)
        .expect("a pin for every login's CLI");
    let platform = mcbench_providers::harness::install::platform();
    let program = match pin.asset(&platform) {
        Some(asset) => mcbench_store::object! {
            "name" => pin.name.as_str(), "title" => pin.title.as_str(), "bytes" => asset.bytes,
        },
        None => mcbench_store::object! {
            "name" => pin.name.as_str(), "title" => pin.title.as_str(), "bytes" => Json::Null,
        },
    };
    let current = sign_ins
        .state(provider)
        .map_err(|e| e.message)?
        .map_or(Json::Null, |view| view.to_json());
    Ok(Some(mcbench_store::object! {
        "methods" => login.methods().map(|m| m.to_vec()),
        "program" => program,
        "current" => current,
    }))
}

/// `details({ refresh })` with `GET /api/settings`'s endpoints and
/// protocols; `checked_at` is `new Date().toISOString()`.
pub async fn details(
    providers: &Providers,
    sign_ins: &SignIns,
    versions: &Versions,
    refresh: bool,
    checked_at: String,
) -> Result<Json, String> {
    if refresh {
        versions.clear();
        providers.api().catalog.codex.forget();
    }
    let settings = providers.settings();
    let Json::Object(mut public) = public_settings(providers, &settings)? else {
        unreachable!("publicSettings is an object")
    };
    let codex = providers.codex_config().account()?;
    let claude = providers.claude_config().account()?;
    let field = |account: &Option<Json>, key: &str| {
        account
            .as_ref()
            .and_then(|a| a.get(key))
            .filter(|v| mcbench_store::json::truthy(Some(v)))
            .cloned()
            .unwrap_or(Json::Null)
    };
    let Some(Json::Object(list)) = public.get_mut("providers") else {
        unreachable!("publicSettings has providers")
    };
    for info in &PROVIDERS {
        let id = info.id;
        let Some(Json::Object(entry)) = list.get_mut(id) else {
            unreachable!("every provider is listed")
        };
        if info.env_key.is_some() {
            entry.insert(
                "keySource".into(),
                providers
                    .key_source(&settings, id)
                    .map_or(Json::Null, Json::from),
            );
        }
        if info.runtime.is_some() {
            let (binary, binary_env, home_env, home) = info_runtime(id);
            let resolved = if id == "claude-code" {
                providers.claude_config().config_dir()
            } else {
                providers.codex_config().codex_home()
            };
            entry.insert(
                "runtime".into(),
                mcbench_store::object! {
                    "binary" => binary, "binaryEnv" => binary_env, "homeEnv" => home_env, "home" => home,
                    "binaryValue" => std::env::var(binary_env).unwrap_or_default(),
                    "resolvedHome" => resolved.to_string_lossy().into_owned(),
                },
            );
        }
        if info.auth == Some("cli-login") {
            entry.insert("signedIn".into(), providers.stored_login(id)?.into());
        }
        match id {
            "codex" | "codex-cli" => {
                entry.insert("account".into(), field(&codex, "email"));
                entry.insert("plan".into(), field(&codex, "plan"));
                entry.insert(
                    "version".into(),
                    versions.get("codex", providers).await.into(),
                );
            }
            "claude-code" => {
                entry.insert("account".into(), field(&claude, "email"));
                entry.insert("plan".into(), field(&claude, "plan"));
                entry.insert(
                    "version".into(),
                    versions.get("claude", providers).await.into(),
                );
            }
            "cursor" => {
                entry.insert("signedIn".into(), false.into());
                entry.insert("version".into(), Json::Null);
            }
            _ => {}
        }
        if let Some(Json::Object(mut sign_in)) = describe(id, sign_ins)? {
            let installed = mcbench_store::json::truthy(entry.get("version"));
            sign_in.insert("installed".into(), installed.into());
            entry.insert("signIn".into(), Json::Object(sign_in));
        }
        let summary = summary(id, entry);
        entry.insert("summary".into(), summary.into());
        let (endpoint, protocol) = endpoint_and_protocol(id);
        entry.insert("endpoint".into(), endpoint.map_or(Json::Null, Json::from));
        entry.insert("protocol".into(), protocol.into());
    }
    public.insert("checkedAt".into(), checked_at.into());
    Ok(Json::Object(public))
}

fn text(value: &Json, what: &str) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("The settings' {what} is not text: {value}"))
}

fn optional_text(value: Option<&Json>, what: &str) -> Result<Option<String>, String> {
    match value {
        None | Some(Json::Null) => Ok(None),
        Some(value) => text(value, what).map(Some),
    }
}

fn flag(value: Option<&Json>, what: &str) -> Result<bool, String> {
    value
        .and_then(Json::as_bool)
        .ok_or_else(|| format!("The settings' {what} is not true or false"))
}

fn texts(value: &Json, what: &str) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("The settings' {what} is not a list"))?
        .iter()
        .map(|item| text(item, what))
        .collect()
}

/// The title model of the settings (`titleModel`): none, or its provider,
/// model and effort.
pub fn title_model(value: &Json) -> Result<Option<mcbench_ui::TitleModel>, String> {
    if value.is_null() {
        return Ok(None);
    }
    Ok(Some(mcbench_ui::TitleModel {
        provider: text(&value["provider"], "title provider")?,
        model: text(&value["model"], "title model")?,
        effort: text(&value["effort"], "title effort")?,
    }))
}

/// `GET /api/settings`'s body ([`details`]) typed for the window, Cursor
/// left out: every line in it is the body's (the summaries as
/// [`details`] makes them), with each sign-in's latest view and its CLI's
/// latest install, `checked_at` the read's time in ms and `settings_file`
/// where the keys are saved. A field of the wrong shape is an error naming
/// it.
pub fn view(
    details: &Json,
    checked_at: i64,
    settings_file: std::path::PathBuf,
    sign_ins: &SignIns,
    installs: &mcbench_providers::harness::install::CliInstaller,
) -> Result<mcbench_ui::SettingsView, String> {
    use mcbench_ui::{Auth, KeySource, Program, ProviderSettings, Runtime, SignInOffer};
    let mut providers = Vec::new();
    for info in PROVIDERS.iter().filter(|info| info.id != "cursor") {
        let id = info.id;
        let entry = &details["providers"][id];
        let what = |field: &str| format!("{id} {field}");
        let runtime = match entry.get("runtime") {
            None => None,
            Some(runtime) => Some(Runtime {
                binary: text(&runtime["binary"], &what("binary"))?,
                binary_env: text(&runtime["binaryEnv"], &what("binaryEnv"))?,
                home_env: text(&runtime["homeEnv"], &what("homeEnv"))?,
                binary_value: Some(text(&runtime["binaryValue"], &what("binaryValue"))?)
                    .filter(|value| !value.is_empty()),
                resolved_home: text(&runtime["resolvedHome"], &what("resolvedHome"))?,
            }),
        };
        let sign_in = match entry.get("signIn") {
            None => None,
            Some(offer) => {
                let program = &offer["program"];
                let name = text(&program["name"], &what("program"))?;
                let provider = serde_json::from_value::<ProviderId>(Json::String(id.into()))
                    .map_err(|e| format!("{id} is not a provider: {e}"))?;
                Some(SignInOffer {
                    methods: match &offer["methods"] {
                        Json::Null => Vec::new(),
                        methods => texts(methods, &what("methods"))?,
                    },
                    program: Program {
                        title: text(&program["title"], &what("program title"))?,
                        // No pinned download for this platform (Windows
                        // today): `bytes` is null in the body, 0 here.
                        bytes: match &program["bytes"] {
                            Json::Null => 0,
                            bytes => bytes
                                .as_u64()
                                .ok_or_else(|| format!("{id}'s download size is {bytes}"))?,
                        },
                        name: name.clone(),
                    },
                    installed: flag(offer.get("installed"), &what("installed"))?,
                    current: sign_ins.state(provider).map_err(|e| e.message)?,
                    install: installs.state(&name).map_err(|e| e.message)?,
                })
            }
        };
        providers.push(ProviderSettings {
            id: id.to_string(),
            label: text(&entry["label"], &what("label"))?,
            auth: match entry["auth"].as_str() {
                Some("api-key") => Auth::ApiKey,
                Some("cli-login") => Auth::CliLogin,
                other => return Err(format!("{id} signs in by {other:?}")),
            },
            harness: flag(entry.get("harness"), &what("harness"))?,
            env_key: optional_text(entry.get("envKey"), &what("envKey"))?,
            connected: flag(entry.get("connected"), &what("connected"))?,
            enabled: flag(entry.get("enabled"), &what("enabled"))?,
            key_source: match entry.get("keySource").and_then(Json::as_str) {
                Some("saved") => Some(KeySource::Saved),
                Some("env") => Some(KeySource::Env),
                None => None,
                Some(other) => return Err(format!("{id}'s key comes from {other}")),
            },
            signed_in: match entry.get("signedIn") {
                None => false,
                signed_in => flag(signed_in, &what("signedIn"))?,
            },
            account: optional_text(entry.get("account"), &what("account"))?,
            plan: optional_text(entry.get("plan"), &what("plan"))?,
            version: optional_text(entry.get("version"), &what("version"))?,
            summary: text(&entry["summary"], &what("summary"))?,
            hidden_models: texts(&entry["hiddenModels"], &what("hiddenModels"))?,
            model_order: texts(&entry["modelOrder"], &what("modelOrder"))?,
            runtime,
            sign_in,
            endpoint: optional_text(entry.get("endpoint"), &what("endpoint"))?,
            protocol: text(&entry["protocol"], &what("protocol"))?,
        });
    }
    Ok(mcbench_ui::SettingsView {
        checked_at,
        title_model: title_model(&details["titleModel"])?,
        share_repo: optional_text(details.get("shareRepo"), "shareRepo")?,
        providers,
        settings_file,
    })
}

/// `PUT /api/settings`'s body for `change`, as the page's draft is: each
/// provider's `apiKey` (`''` keeps the saved key), `enabled`,
/// `hiddenModels` and `modelOrder`, the title model and the share
/// repository. Cursor, which the window does not list, keeps what is saved,
/// as the page's draft carries it.
pub fn change_body(change: &mcbench_ui::SettingsChange, saved: &Settings) -> Json {
    let mut providers = Object::new();
    let cursor = provider_settings(saved, "cursor");
    providers.insert(
        "cursor".into(),
        mcbench_store::object! {
            "apiKey" => "",
            "enabled" => cursor.get("enabled").cloned().unwrap_or(Json::Null),
            "hiddenModels" => cursor.get("hiddenModels").cloned().unwrap_or(Json::Null),
            "modelOrder" => cursor.get("modelOrder").cloned().unwrap_or(Json::Null),
        },
    );
    for provider in &change.providers {
        providers.insert(
            provider.id.clone(),
            mcbench_store::object! {
                "apiKey" => provider.api_key.as_str(),
                "enabled" => provider.enabled,
                "hiddenModels" => provider.hidden_models.clone(),
                "modelOrder" => provider.model_order.clone(),
            },
        );
    }
    mcbench_store::object! {
        "titleModel" => change.title_model.as_ref().map_or(Json::Null, |title| mcbench_store::object! {
            "provider" => title.provider.as_str(), "model" => title.model.as_str(), "effort" => title.effort.as_str(),
        }),
        "shareRepo" => change.share_repo.clone(),
        "providers" => Json::Object(providers),
    }
}

/// `GET /api/config`: the providers' public state, the model favourites
/// and Minecraft's version (no `hosted`: the hosted version is dropped).
pub fn config(providers: &Providers) -> Result<Json, String> {
    let settings = providers.settings();
    let public = public_settings(providers, &settings)?;
    Ok(mcbench_store::object! {
        "providers" => public["providers"].clone(),
        "modelFavorites" => settings.0.get("modelFavorites").cloned().unwrap_or(Json::Null),
        "mcVersion" => mc_version()?,
    })
}

/// `assets.js` `MC_VERSION`: the version the shipped data's lock names.
pub fn mc_version() -> Result<String, String> {
    Ok(mcbench_setup::bundle::Lock::parse(mcbench_setup::bundle::LOCK)?.minecraft)
}

/// `resolveProvider`'s refusal of a provider, for routes that check it
/// first (`POST /api/prompts/generate`).
pub fn refuse(providers: &Providers, key: &str) -> Result<(), String> {
    let config = providers.config(key)?;
    api::resolve(&config).map_err(|e| e.message)
}
