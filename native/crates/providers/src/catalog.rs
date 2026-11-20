//! Model lists and model facts (`llm.js` `fetchCatalog`, `startCatalog`,
//! `catalogIds`, `infoFromCatalogEntry`, `claudeCodeModels`, `listModels`,
//! `modelInfo`; `codex-cli.js` `listModels`, `modelsFromCache`).
//!
//! Live model capabilities come from OpenRouter's public catalog (no key
//! needed): which models exist, whether they take tools, and exactly which
//! reasoning efforts they support. Native provider slugs are normalized to
//! catalog ids so the same source answers for every provider. The catalog is
//! fetched when the app starts and again every [`CATALOG_REFRESH`]
//! ([`Catalog::start`]); a turn reads the one fetched last and never waits
//! for a refresh. Until one fetch has succeeded, a reader waits for the
//! fetch under way, or starts one: a catalog that could not be fetched fails
//! the reader with the fetch's error. A refresh that fails is logged and
//! leaves the last catalog. Each provider's own model list is kept for
//! [`MODEL_LIST_TTL`]; the Codex catalogue (`codex debug models`, which the
//! ChatGPT sign-in and the Codex CLI share) for [`CODEX_MODELS_TTL`].

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use icu_collator::Collator;
use icu_collator::options::CollatorOptions;
use serde_json::Value;
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

use crate::api::http::{self, Deadline, Request};
use crate::api::{self, Api, Endpoints, js, provider_error_message, qwencloud};
use crate::{CliRuntime, ModelFacts, ModelInfo, ProviderConfig, ProviderError, ProviderId};

/// How often the catalog is fetched again (`CATALOG_REFRESH_MS`).
pub const CATALOG_REFRESH: Duration = Duration::from_secs(5 * 60);
/// How long a provider's model list is kept (`MODEL_LIST_TTL_MS`).
pub const MODEL_LIST_TTL: Duration = Duration::from_secs(5 * 60);
/// How long the Codex catalogue is kept (`codex-cli.js` `CATALOG_TTL_MS`).
pub const CODEX_MODELS_TTL: Duration = Duration::from_secs(5 * 60);

/// The effort scale, lowest first (`EFFORT_ORDER`).
pub const EFFORT_ORDER: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// The catalog's entries by id, in the order it lists them (a repeated id
/// keeps its first place and its last entry, as a JavaScript `Map` does).
#[derive(Debug, Default)]
pub struct Entries {
    order: Vec<String>,
    by_id: HashMap<String, Value>,
}

impl Entries {
    /// `new Map(data.map((m) => [m.id, m]))`.
    pub fn new(data: &[Value]) -> Entries {
        let mut entries = Entries::default();
        for entry in data {
            let id = entry
                .get("id")
                .map(js::to_string)
                .unwrap_or_else(|| "undefined".into());
            if entries.by_id.insert(id.clone(), entry.clone()).is_none() {
                entries.order.push(id);
            }
        }
        entries
    }

    /// The entry of `id`.
    pub fn get(&self, id: &str) -> Option<&Value> {
        self.by_id.get(id)
    }

    /// Every entry, in order.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.order.iter().map(|id| &self.by_id[id])
    }
}

/// `catalogIds`: the catalog ids a provider's model may have.
pub fn catalog_ids(provider: ProviderId, model: &str) -> Vec<String> {
    if matches!(provider, ProviderId::OpenRouter | ProviderId::Groq) {
        return vec![model.to_owned()];
    }
    // Date snapshots are stripped (`/-\d{8}$/`).
    let b = model.as_bytes();
    let bare = if b.len() >= 9
        && b[b.len() - 9] == b'-'
        && b[b.len() - 8..].iter().all(u8::is_ascii_digit)
    {
        &model[..model.len() - 9]
    } else {
        model
    };
    let vendor = match provider {
        ProviderId::Codex | ProviderId::CodexCli => "openai",
        ProviderId::ClaudeCode => "anthropic",
        other => other.key(),
    };
    let mut ids = vec![format!("{vendor}/{bare}")];
    // Anthropic's native ids use dashes where the catalog uses dots:
    // claude-sonnet-4-6 -> anthropic/claude-sonnet-4.6 (`/-(\d+)-(\d+)$/`).
    if let Some(dotted) = dotted(bare) {
        ids.push(format!("{vendor}/{dotted}"));
    }
    ids
}

fn dotted(bare: &str) -> Option<String> {
    let (head, last) = bare.rsplit_once('-')?;
    let (start, middle) = head.rsplit_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    (digits(last) && digits(middle)).then(|| format!("{start}-{middle}.{last}"))
}

/// `findCatalogEntry`.
pub fn find_entry<'a>(
    entries: &'a Entries,
    provider: ProviderId,
    model: &str,
) -> Option<&'a Value> {
    catalog_ids(provider, model)
        .iter()
        .find_map(|id| entries.get(id))
}

/// `infoFromCatalogEntry`.
pub fn info_from_entry(entry: Option<&Value>) -> ModelFacts {
    let Some(entry) = entry else {
        return ModelFacts::unknown();
    };
    let params = entry
        .get("supported_parameters")
        .filter(|v| js::truthy(Some(v)));
    let architecture = entry.get("architecture").filter(|v| js::truthy(Some(v)));
    let input_modalities = js::get(architecture, "input_modalities");
    let modality = js::get(architecture, "modality")
        .filter(|v| js::truthy(Some(v)))
        .map(js::to_string)
        .unwrap_or_default();
    let input_side = modality.split("->").next().unwrap_or_default().to_owned();
    let reasoning = entry.get("reasoning").filter(|v| js::truthy(Some(v)));
    let mut efforts: Vec<String> = js::get(reasoning, "supported_efforts")
        .filter(|v| js::truthy(Some(v)))
        .and_then(Value::as_array)
        .map(|list| list.iter().map(js::to_string).collect())
        .unwrap_or_default();
    let rank = |effort: &String| -> i64 {
        EFFORT_ORDER
            .iter()
            .position(|e| e == effort)
            .map_or(-1, |at| at as i64)
    };
    efforts.sort_by_key(rank);
    let includes = |value: Option<&Value>, needle: &str| match value {
        Some(Value::Array(items)) => items.iter().any(|item| item.as_str() == Some(needle)),
        Some(Value::String(text)) => text.contains(needle),
        _ => false,
    };
    ModelFacts {
        known: true,
        image_input: match input_modalities {
            Some(Value::Array(items)) => {
                Some(items.iter().any(|item| item.as_str() == Some("image")))
            }
            _ if !input_side.is_empty() => Some(input_side.split('+').any(|side| side == "image")),
            _ => None,
        },
        image_output: includes(
            js::get(architecture, "output_modalities").filter(|v| js::truthy(Some(v))),
            "image",
        ),
        tools: Some(includes(params, "tools")),
        reasoning: reasoning.is_some(),
        efforts,
        default_effort: js::get(reasoning, "default_effort")
            .filter(|v| js::truthy(Some(v)))
            .map(js::to_string),
        mandatory_reasoning: js::truthy(js::get(reasoning, "mandatory")),
        context_window: Some(
            entry
                .get("context_length")
                .filter(|v| js::truthy(Some(v)))
                .and_then(Value::as_f64),
        ),
        fast: None,
    }
}

/// `claudeCodeModels`: Claude Code takes Anthropic's native ids; the catalog
/// knows them dotted (`anthropic/claude-opus-5.5` -> `claude-opus-5-5`).
/// Variants such as `:batch` are catalog routing, not models. Each comes
/// with its catalog entry.
pub fn claude_code_models(entries: &Entries) -> Vec<(String, String, f64, &Value)> {
    entries
        .values()
        .filter_map(|entry| {
            let id = entry.get("id").and_then(Value::as_str)?;
            (id.starts_with("anthropic/claude-") && !id.contains(':')).then(|| {
                (
                    id["anthropic/".len()..].replace('.', "-"),
                    name_or_id(entry),
                    created(entry),
                    entry,
                )
            })
        })
        .collect()
}

fn name_or_id(entry: &Value) -> String {
    entry
        .get("name")
        .filter(|v| js::truthy(Some(v)))
        .or_else(|| entry.get("id"))
        .map(js::to_string)
        .unwrap_or_default()
}

fn created(entry: &Value) -> f64 {
    entry
        .get("created")
        .filter(|v| js::truthy(Some(v)))
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

type Loading = Arc<OnceCell<Result<Arc<Entries>, String>>>;

#[derive(Default)]
struct State {
    entries: Option<Arc<Entries>>,
    loading: Option<Loading>,
    lists: HashMap<ProviderId, (tokio::time::Instant, Vec<ModelInfo>)>,
}

/// The OpenRouter catalog, the providers' model lists and the Codex
/// catalogue, held for the app's life.
pub struct Catalog {
    http: reqwest::Client,
    endpoints: Endpoints,
    state: Mutex<State>,
    refresher: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// The Codex catalogue, which the Codex CLI's provider shares
    /// (`harness::codex_cli::CodexCli::new`).
    pub codex: Arc<CodexModels>,
}

impl Catalog {
    /// An empty catalog that fetches from `endpoints.catalog`.
    pub fn new(http: reqwest::Client, endpoints: Endpoints) -> Catalog {
        Catalog {
            http,
            endpoints,
            state: Mutex::default(),
            refresher: Mutex::new(None),
            codex: Arc::new(CodexModels::default()),
        }
    }

    /// `fetchCatalog`: readers and the timer share the fetch under way.
    pub async fn fetch(&self) -> Result<Arc<Entries>, String> {
        let cell = self.loading();
        self.drive(cell).await
    }

    /// The fetch under way, or a new one (`catalogLoading ||= ...`), taken
    /// at once so that a reader coming before it runs shares it.
    fn loading(&self) -> Loading {
        let mut state = self.state.lock().expect("the catalog");
        state.loading.get_or_insert_with(Loading::default).clone()
    }

    async fn drive(&self, cell: Loading) -> Result<Arc<Entries>, String> {
        let result = cell
            .get_or_init(|| async {
                let fetched = self.download().await;
                if let Ok(entries) = &fetched {
                    self.state.lock().expect("the catalog").entries = Some(entries.clone());
                }
                fetched
            })
            .await
            .clone();
        let mut state = self.state.lock().expect("the catalog");
        if state
            .loading
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, &cell))
        {
            state.loading = None;
        }
        result
    }

    async fn download(&self) -> Result<Arc<Entries>, String> {
        let deadline = Deadline::new(&CancellationToken::new(), http::CATALOG_TIMEOUT);
        let answer = http::send(
            &self.http,
            Request {
                method: reqwest::Method::GET,
                url: &self.endpoints.catalog,
                headers: Vec::new(),
                body: None,
            },
            &deadline,
        )
        .await
        .map_err(|failure| failure.message().to_owned())?;
        if !answer.ok() {
            return Err(format!(
                "Model catalog fetch failed with status {}",
                answer.status
            ));
        }
        let text = answer
            .text()
            .await
            .map_err(|failure| failure.message().to_owned())?;
        let payload = api::v8json::parse(&text)?;
        let data = payload
            .get("data")
            .and_then(Value::as_array)
            .ok_or("Cannot read properties of undefined (reading 'map')")?;
        Ok(Arc::new(Entries::new(data)))
    }

    /// `catalog()`: the one fetched last, or a fetch.
    pub async fn entries(&self) -> Result<Arc<Entries>, String> {
        let held = self.state.lock().expect("the catalog").entries.clone();
        match held {
            Some(entries) => Ok(entries),
            None => self.fetch().await,
        }
    }

    /// `startCatalog`: fetches now and every [`CATALOG_REFRESH`] until
    /// [`Catalog::stop`]; a failed refresh is logged as `[model-catalog]`.
    pub fn start(self: &Arc<Catalog>) {
        self.start_every(CATALOG_REFRESH);
    }

    /// [`Catalog::start`] with another period (the refresh test's).
    pub(crate) fn start_every(self: &Arc<Catalog>, period: Duration) {
        // The first fetch is under way before this returns (`refresh()`
        // before `setInterval`), so a turn that comes next waits for it.
        let first = self.loading();
        let started = tokio::time::Instant::now();
        let catalog = self.clone();
        let task = tokio::spawn(async move {
            let log = |result: Result<Arc<Entries>, String>| {
                if let Err(message) = result {
                    eprintln!("[model-catalog] {message}");
                }
            };
            log(catalog.drive(first).await);
            let mut every = tokio::time::interval_at(started + period, period);
            loop {
                every.tick().await;
                let catalog = catalog.clone();
                tokio::spawn(async move { log(catalog.fetch().await) });
            }
        });
        if let Some(old) = self.refresher.lock().expect("the refresher").replace(task) {
            old.abort();
        }
    }

    /// `stopCatalog`.
    pub fn stop(&self) {
        if let Some(task) = self.refresher.lock().expect("the refresher").take() {
            task.abort();
        }
    }

    /// The catalog's facts of a provider's model (`modelInfo` for the
    /// providers it describes).
    pub async fn facts(&self, provider: ProviderId, model: &str) -> Result<ModelFacts, String> {
        let entries = self.entries().await?;
        Ok(info_from_entry(find_entry(&entries, provider, model)))
    }

    /// `modelInfo(provider, model)` for an API provider.
    pub async fn model_info(
        &self,
        api: &Api,
        config: &ProviderConfig,
        model: &str,
    ) -> Result<ModelFacts, ProviderError> {
        let _ = api;
        match config.id {
            ProviderId::QwenCloud => {
                Ok(qwencloud::model_info(model).unwrap_or_else(ModelFacts::unknown))
            }
            ProviderId::Codex | ProviderId::CodexCli => {
                let models = self
                    .codex
                    .list(config.cli.as_ref().expect("the Codex CLI's runtime"))
                    .await?;
                Ok(models
                    .into_iter()
                    .find(|m| m.id == model)
                    .map(|m| m.facts)
                    .unwrap_or_else(ModelFacts::unknown))
            }
            id => self.facts(id, model).await.map_err(ProviderError::new),
        }
    }

    /// `listModels(provider)` for an API provider: newest first, without
    /// image-generation models, kept for [`MODEL_LIST_TTL`].
    pub async fn list_models(
        &self,
        api: &Api,
        config: &ProviderConfig,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        if config.id == ProviderId::Codex {
            api::resolve(config)?;
            return self
                .codex
                .list(config.cli.as_ref().expect("the Codex CLI's runtime"))
                .await;
        }
        {
            let state = self.state.lock().expect("the catalog");
            if let Some((at, models)) = state.lists.get(&config.id)
                && at.elapsed() < MODEL_LIST_TTL
            {
                return Ok(models.clone());
            }
        }
        api::resolve(config)?;
        let models = if config.id == ProviderId::QwenCloud {
            let payload = self.provider_json(api, config, "/models").await?;
            qwencloud::recent_models(
                payload
                    .get("data")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            )
        } else {
            let entries = self.entries().await.map_err(ProviderError::new)?;
            let native = self.native_models(api, config, &entries).await?;
            let mut models: Vec<ModelInfo> = native
                .into_iter()
                .map(|(id, name, created, entry)| {
                    let entry = entry.or_else(|| find_entry(&entries, config.id, &id).cloned());
                    ModelInfo {
                        facts: info_from_entry(entry.as_ref()),
                        id,
                        name,
                        created,
                    }
                })
                .filter(|m| !m.facts.image_output && !m.id.starts_with("gpt-image-"))
                .collect();
            let collator = Collator::try_new(Default::default(), CollatorOptions::default())
                .expect("ICU's root collation is compiled in");
            models.sort_by(|a, b| {
                let by_date = b.created - a.created;
                if by_date != 0.0 && !by_date.is_nan() {
                    return if by_date > 0.0 {
                        Ordering::Greater
                    } else {
                        Ordering::Less
                    };
                }
                collator.compare(&a.name, &b.name)
            });
            models
        };
        self.state
            .lock()
            .expect("the catalog")
            .lists
            .insert(config.id, (tokio::time::Instant::now(), models.clone()));
        Ok(models)
    }

    /// Each provider's own list: `(id, name, created, catalog entry)`.
    async fn native_models(
        &self,
        api: &Api,
        config: &ProviderConfig,
        entries: &Entries,
    ) -> Result<Vec<(String, String, f64, Option<Value>)>, ProviderError> {
        let mut native = Vec::new();
        match config.id {
            // Claude Code has no address of its own to ask: its models are
            // Anthropic's as the catalog lists them (`claudeCodeModels`).
            ProviderId::ClaudeCode => {
                for (id, name, created, entry) in claude_code_models(entries) {
                    native.push((id, name, created, Some(entry.clone())));
                }
            }
            ProviderId::OpenRouter => {
                for entry in entries.values() {
                    native.push((
                        entry.get("id").map(js::to_string).unwrap_or_default(),
                        name_or_id(entry),
                        created(entry),
                        Some(entry.clone()),
                    ));
                }
            }
            ProviderId::Anthropic => {
                let mut path = "/models?limit=100".to_owned();
                loop {
                    let payload = self.provider_json(api, config, &path).await?;
                    for entry in payload
                        .get("data")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        let at = entry
                            .get("created_at")
                            .filter(|v| js::truthy(Some(v)))
                            .map(js::to_string)
                            .and_then(|text| js::parse_iso(&text));
                        native.push((
                            entry.get("id").map(js::to_string).unwrap_or_default(),
                            entry
                                .get("display_name")
                                .filter(|v| js::truthy(Some(v)))
                                .or_else(|| entry.get("id"))
                                .map(js::to_string)
                                .unwrap_or_default(),
                            at.map_or(0.0, |ms| (ms as f64 / 1000.0).floor()),
                            None,
                        ));
                    }
                    if !js::truthy(payload.get("has_more")) {
                        break;
                    }
                    let last = payload
                        .get("last_id")
                        .map(js::to_string)
                        .unwrap_or_else(|| "undefined".into());
                    path = format!("/models?limit=100&after_id={}", encode_uri_component(&last));
                }
            }
            ProviderId::Google => {
                // Only the model's name and methods come from Google: it
                // publishes no vision, tool or effort facts, so those are the
                // catalog's, as is the release date that orders the list.
                let mut token = String::new();
                loop {
                    let url = if token.is_empty() {
                        format!("{}?pageSize=1000", api.endpoints.google_models)
                    } else {
                        format!(
                            "{}?pageSize=1000&pageToken={}",
                            api.endpoints.google_models,
                            encode_uri_component(&token)
                        )
                    };
                    let payload = self
                        .get_json(
                            api,
                            config,
                            &url,
                            vec![("x-goog-api-key".into(), config.api_key.clone())],
                        )
                        .await?;
                    for entry in payload
                        .get("models")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        let methods = entry
                            .get("supportedGenerationMethods")
                            .and_then(Value::as_array);
                        if !methods.is_some_and(|m| {
                            m.iter().any(|x| x.as_str() == Some("generateContent"))
                        }) {
                            continue;
                        }
                        let name = entry.get("name").map(js::to_string).unwrap_or_default();
                        let id = name.chars().skip("models/".len()).collect::<String>();
                        let created = find_entry(entries, config.id, &id).map_or(0.0, created);
                        native.push((
                            id,
                            entry
                                .get("displayName")
                                .map(js::to_string)
                                .unwrap_or_default(),
                            created,
                            None,
                        ));
                    }
                    token = payload
                        .get("nextPageToken")
                        .filter(|v| js::truthy(Some(v)))
                        .map(js::to_string)
                        .unwrap_or_default();
                    if token.is_empty() {
                        break;
                    }
                }
            }
            _ => {
                let payload = self.provider_json(api, config, "/models").await?;
                for entry in payload
                    .get("data")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    native.push((
                        entry.get("id").map(js::to_string).unwrap_or_default(),
                        name_or_id(entry),
                        created(entry),
                        None,
                    ));
                }
            }
        }
        Ok(native)
    }

    /// `providerJson`: a GET of the provider's `path` with its headers.
    async fn provider_json(
        &self,
        api: &Api,
        config: &ProviderConfig,
        path: &str,
    ) -> Result<Value, ProviderError> {
        let headers = match config.id {
            ProviderId::Anthropic => vec![
                ("x-api-key".into(), config.api_key.clone()),
                ("anthropic-version".into(), "2023-06-01".into()),
                (
                    "anthropic-beta".into(),
                    "thinking-binding-controls-2026-08-01".into(),
                ),
            ],
            _ => api::chat::bearer(&config.api_key),
        };
        let url = format!("{}{path}", api.endpoints.base(config.id));
        self.get_json(api, config, &url, headers).await
    }

    async fn get_json(
        &self,
        api: &Api,
        config: &ProviderConfig,
        url: &str,
        headers: Vec<(String, String)>,
    ) -> Result<Value, ProviderError> {
        let deadline = Deadline::new(&CancellationToken::new(), http::LIST_TIMEOUT);
        let answer = http::send(
            &api.http,
            Request {
                method: reqwest::Method::GET,
                url,
                headers,
                body: None,
            },
            &deadline,
        )
        .await
        .map_err(|failure| ProviderError::new(failure.message()))?;
        let status = answer.status;
        let ok = answer.ok();
        let text = answer
            .text()
            .await
            .map_err(|failure| ProviderError::new(failure.message()))?;
        if !ok {
            return Err(ProviderError::new(provider_error_message(
                &config.api_key,
                status,
                &text,
            )));
        }
        api::v8json::parse(&text).map_err(ProviderError::new)
    }
}

/// `encodeURIComponent`.
pub fn encode_uri_component(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

type CodexLoading = Arc<OnceCell<Result<Vec<ModelInfo>, String>>>;

/// The Codex catalogue (`codex-cli.js` `listModels`): what `codex debug
/// models` prints, kept for [`CODEX_MODELS_TTL`]; callers while it runs
/// share the one run.
#[derive(Default)]
pub struct CodexModels {
    state: Mutex<HeldModels>,
}

/// The catalogue kept since an instant, and the run under way.
type HeldModels = (
    Option<(tokio::time::Instant, Vec<ModelInfo>)>,
    Option<CodexLoading>,
);

impl CodexModels {
    /// The models, from the cache or a run of `runtime`'s program.
    pub async fn list(&self, runtime: &CliRuntime) -> Result<Vec<ModelInfo>, ProviderError> {
        let cell = {
            let mut state = self.state.lock().expect("the Codex catalogue");
            if let Some((at, models)) = &state.0
                && at.elapsed() <= CODEX_MODELS_TTL
            {
                return Ok(models.clone());
            }
            state.1.get_or_insert_with(CodexLoading::default).clone()
        };
        let result = cell
            .get_or_init(|| async {
                let models = run_debug_models(runtime).await?;
                self.state.lock().expect("the Codex catalogue").0 =
                    Some((tokio::time::Instant::now(), models.clone()));
                Ok(models)
            })
            .await
            .clone();
        let mut state = self.state.lock().expect("the Codex catalogue");
        if state
            .1
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, &cell))
        {
            state.1 = None;
        }
        result.map_err(ProviderError::new)
    }

    /// `forgetModels`: Settings' re-check asks again.
    pub fn forget(&self) {
        self.state.lock().expect("the Codex catalogue").0 = None;
    }
}

/// `execFile(program, ['debug', 'models'], {env, maxBuffer: 64 MB, timeout:
/// 60 s})`, then `modelsFromCache(JSON.parse(stdout))`.
async fn run_debug_models(runtime: &CliRuntime) -> Result<Vec<ModelInfo>, String> {
    let mut command = tokio::process::Command::from(mcbench_spawn::command(&runtime.program));
    command
        .args(&runtime.args)
        .args(["debug", "models"])
        .env_clear()
        .envs(runtime.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let failed = |stderr: &[u8]| {
        format!(
            "Command failed: {} debug models\n{}",
            runtime.program,
            String::from_utf8_lossy(stderr)
        )
    };
    let child = command.output();
    let output = match tokio::time::timeout(Duration::from_secs(60), child).await {
        Err(_) => return Err(failed(b"")),
        Ok(Err(error)) => {
            let code = match error.kind() {
                std::io::ErrorKind::NotFound => "ENOENT".to_owned(),
                std::io::ErrorKind::PermissionDenied => "EACCES".to_owned(),
                _ => error.to_string(),
            };
            return Err(format!("spawn {} {code}", runtime.program));
        }
        Ok(Ok(output)) => output,
    };
    if !output.status.success() {
        return Err(failed(&output.stderr));
    }
    if output.stdout.len() > 64 * 1024 * 1024 {
        return Err("stdout maxBuffer length exceeded".into());
    }
    let mut decoder = api::sse::Utf8Decoder::default();
    let mut text = decoder.decode(&output.stdout);
    text.push_str(&decoder.finish());
    let cache = api::v8json::parse(&text)?;
    Ok(models_from_cache(&cache))
}

/// `modelsFromCache`: the listed models by priority, each with its efforts,
/// image input, context window and fast tier.
pub fn models_from_cache(cache: &Value) -> Vec<ModelInfo> {
    let mut models: Vec<&Value> = cache
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
        .collect();
    models.sort_by(|a, b| {
        let priority = |m: &Value| {
            m.get("priority")
                .and_then(Value::as_f64)
                .unwrap_or(f64::NAN)
        };
        (priority(a) - priority(b))
            .partial_cmp(&0.0)
            .unwrap_or(Ordering::Equal)
    });
    models
        .into_iter()
        .map(|m| {
            let efforts: Vec<String> = m
                .get("supported_reasoning_levels")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|level| level.get("effort").map(js::to_string).unwrap_or_default())
                .collect();
            let fast = m
                .get("additional_speed_tiers")
                .and_then(Value::as_array)
                .is_some_and(|tiers| tiers.iter().any(|t| t.as_str() == Some("fast")));
            let note = fast
                .then(|| {
                    m.get("service_tiers")
                        .and_then(Value::as_array)
                        .and_then(|tiers| {
                            tiers
                                .iter()
                                .find(|t| t.get("name").and_then(Value::as_str) == Some("Fast"))
                        })
                        .and_then(|tier| tier.get("description"))
                        .filter(|v| js::truthy(Some(v)))
                        .map(js::to_string)
                })
                .flatten();
            let reasoning = !efforts.is_empty();
            ModelInfo {
                id: m.get("slug").map(js::to_string).unwrap_or_default(),
                name: m.get("display_name").map(js::to_string).unwrap_or_default(),
                created: 0.0,
                facts: ModelFacts {
                    known: true,
                    image_input: Some(
                        m.get("input_modalities")
                            .and_then(Value::as_array)
                            .is_some_and(|list| list.iter().any(|x| x.as_str() == Some("image"))),
                    ),
                    image_output: false,
                    tools: Some(true),
                    reasoning,
                    efforts,
                    default_effort: m
                        .get("default_reasoning_level")
                        .filter(|v| js::truthy(Some(v)))
                        .map(js::to_string),
                    mandatory_reasoning: reasoning,
                    context_window: Some(
                        m.get("context_window")
                            .filter(|v| js::truthy(Some(v)))
                            .and_then(Value::as_f64),
                    ),
                    fast: Some((fast, note)),
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalog_ids_maps_native_provider_slugs_onto_catalog_ids() {
        assert_eq!(
            catalog_ids(ProviderId::OpenRouter, "openai/gpt-5.5"),
            ["openai/gpt-5.5"]
        );
        assert_eq!(
            catalog_ids(ProviderId::Groq, "openai/gpt-oss-120b"),
            ["openai/gpt-oss-120b"]
        );
        assert_eq!(
            catalog_ids(ProviderId::OpenAi, "gpt-5.5"),
            ["openai/gpt-5.5"]
        );
        assert_eq!(
            catalog_ids(ProviderId::Codex, "gpt-5.6-sol"),
            ["openai/gpt-5.6-sol"]
        );
        assert_eq!(
            catalog_ids(ProviderId::Anthropic, "claude-sonnet-4-6-20261001"),
            ["anthropic/claude-sonnet-4-6", "anthropic/claude-sonnet-4.6"]
        );
    }

    #[test]
    fn harness_providers_resolve_native_ids_through_their_vendor_catalog() {
        assert_eq!(
            catalog_ids(ProviderId::ClaudeCode, "claude-opus-5-5"),
            ["anthropic/claude-opus-5-5", "anthropic/claude-opus-5.5"]
        );
        assert_eq!(
            catalog_ids(ProviderId::CodexCli, "gpt-6-astra"),
            ["openai/gpt-6-astra"]
        );
        let entry = |id: &str, created: u64| json!({"id": id, "name": format!("Anthropic: {id}"), "created": created});
        let entries = Entries::new(&[
            entry("anthropic/claude-opus-5.5", 5),
            entry("anthropic/claude-opus-5.5:batch", 5),
            entry("anthropic/claude-sonnet-4-6", 4),
            json!({"id": "openai/gpt-5.5", "created": 9}),
        ]);
        let models: Vec<(String, f64)> = claude_code_models(&entries)
            .into_iter()
            .map(|(id, _, created, _)| (id, created))
            .collect();
        assert_eq!(
            models,
            [
                ("claude-opus-5-5".to_string(), 5.0),
                ("claude-sonnet-4-6".to_string(), 4.0)
            ]
        );
    }

    #[test]
    fn catalog_metadata_normalizes_model_picker_capabilities() {
        let info = info_from_entry(Some(&json!({
            "supported_parameters": ["tools", "reasoning"],
            "reasoning": {"mandatory": false, "default_effort": "medium", "supported_efforts": ["high", "low", "medium"]},
        })));
        assert_eq!(
            info,
            ModelFacts {
                known: true,
                image_input: None,
                image_output: false,
                tools: Some(true),
                reasoning: true,
                efforts: vec!["low".into(), "medium".into(), "high".into()],
                default_effort: Some("medium".into()),
                mandatory_reasoning: false,
                context_window: Some(None),
                fast: None,
            }
        );
        assert!(
            info_from_entry(Some(
                &json!({"architecture": {"output_modalities": ["image", "text"]}})
            ))
            .image_output
        );
        assert_eq!(
            info_from_entry(Some(
                &json!({"architecture": {"input_modalities": ["text", "image"]}})
            ))
            .image_input,
            Some(true)
        );
        assert_eq!(
            info_from_entry(Some(
                &json!({"architecture": {"input_modalities": ["text"]}})
            ))
            .image_input,
            Some(false)
        );
        assert_eq!(info_from_entry(None), ModelFacts::unknown());
        assert_eq!(
            info_from_entry(Some(&json!({"context_length": 1000000}))).context_window,
            Some(Some(1_000_000.0))
        );
    }

    /// `localeCompare` as Node 24.13.1 (ICU 78.2, locale en-US) answers it
    /// for every pair of `tests/collation.json`'s names, written by
    /// `tests/node/collation.js`, against the collator the lists sort with.
    #[test]
    fn names_compare_as_javascripts_locale_compare_does() {
        let golden: Value = serde_json::from_str(include_str!("../tests/collation.json")).unwrap();
        let names: Vec<&str> = golden["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        let collator = Collator::try_new(Default::default(), CollatorOptions::default()).unwrap();
        let mut wrong = Vec::new();
        for (i, row) in golden["signs"].as_array().unwrap().iter().enumerate() {
            for (j, sign) in row.as_str().unwrap().chars().enumerate() {
                let got = match collator.compare(names[i], names[j]) {
                    Ordering::Less => '-',
                    Ordering::Equal => '0',
                    Ordering::Greater => '+',
                };
                if got != sign {
                    wrong.push(format!(
                        "{:?} vs {:?}: node {sign} rust {got}",
                        names[i], names[j]
                    ));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "{} pairs differ:\n{}",
            wrong.len(),
            wrong[..wrong.len().min(20)].join("\n")
        );
    }

    #[test]
    fn uri_components_are_encoded_as_javascript_encodes_them() {
        assert_eq!(
            encode_uri_component("page 2/=é~*"),
            "page%202%2F%3D%C3%A9~*"
        );
    }
}
