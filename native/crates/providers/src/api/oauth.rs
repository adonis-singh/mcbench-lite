//! What the `openai-oauth` 2.0.0 bridge did in-process for the ChatGPT
//! sign-in (`createOpenAIOAuthFetchHandler` with `codex.js`
//! `codexSessionFetch`): it reads the Codex login (`CODEX_HOME/auth.json`)
//! on every request, refreshes the tokens when they are near expiry and
//! writes them back to that file in the same format, looks up the model's
//! defaults in the account's Codex catalogue, and sends the request to the
//! ChatGPT backend with the account's headers and the chat's session
//! headers. A data folder signed in under Node is signed in here: the file,
//! its keys and its formatting are the ones `@openai-oauth/local` reads and
//! writes, and the Codex CLI's own login writes.
//!
//! A failure inside the bridge reached `llm.js` as the bridge's HTTP 500
//! answer, `{"error":{"message":…,"type":"server_error"}}`, which reads
//! `LLM request failed (HTTP 500): …`; it reads the same here.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use serde_json::{Map, Value, json};
use tokio::sync::OnceCell;
use tokio_util::sync::CancellationToken;

use super::http::{self, Deadline, Request};
use super::{Api, js, provider_error_message, v8json};
use crate::ProviderError;
use crate::catalog::encode_uri_component;

/// The OAuth client of the Codex CLI (`DEFAULT_OPENAI_OAUTH_CLIENT_ID`).
pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
/// The Codex version asked for when npm cannot say
/// (`DEFAULT_CODEX_CLIENT_VERSION`).
pub const FALLBACK_VERSION: &str = "0.144.1";
const REFRESH_EXPIRY_MARGIN_MS: i64 = 5 * 60 * 1000;
const REFRESH_INTERVAL_MS: i64 = 55 * 60 * 1000;
const VERSION_TTL: Duration = Duration::from_secs(60 * 60);
const CATALOG_TTL: Duration = Duration::from_secs(5 * 60);
const CATALOG_FAILURE_TTL: Duration = Duration::from_secs(60);
const LITE_HEADER: &str = "x-openai-internal-codex-responses-lite";
/// The bridge's own requests (npm, the catalogue, the token refresh) have
/// no timeout today; a year stands for none.
const NO_TIMEOUT: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// What the bridge keeps for the app's life: the Codex version (an hour)
/// and each account's catalogue (five minutes, a failure one minute).
#[derive(Default)]
pub struct Bridge {
    version: Mutex<Held<String>>,
    catalogs: Mutex<HashMap<String, (tokio::time::Instant, Catalogue)>>,
    loading: Mutex<HashMap<String, Arc<OnceCell<Catalogue>>>>,
}

/// An account's catalogue.
type Catalogue = Arc<Vec<ModelDefaults>>;

/// A value kept since an instant, and the fetch of it under way.
type Held<T> = (Option<(tokio::time::Instant, T)>, Option<Arc<OnceCell<T>>>);

/// A model's entry in the Codex catalogue, the fields the bridge reads
/// (`toCodexModelInfo`).
#[derive(Clone, Debug, Default)]
pub struct ModelDefaults {
    slug: String,
    use_responses_lite: Option<bool>,
    support_verbosity: Option<bool>,
    default_verbosity: Option<String>,
    default_reasoning_level: Option<String>,
}

/// The signed-in session (`toSession`).
#[derive(Clone, Debug)]
struct Session {
    access_token: String,
    account_id: String,
    is_fedramp: bool,
}

/// `parseJwtClaims`.
pub fn jwt_claims(token: Option<&str>) -> Option<Map<String, Value>> {
    let token = token?;
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    // atob after base64url's alphabet is mapped back, as forgiving as atob.
    let mut text: String = parts[1]
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\x0C' | '\r'))
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    while !text.len().is_multiple_of(4) {
        text.push('=');
    }
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::GeneralPurposeConfig::new()
            .with_decode_allow_trailing_bits(true)
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
    );
    let bytes = engine.decode(text.trim_end_matches('=')).ok()?;
    let mut decoder = super::sse::Utf8Decoder::default();
    let mut payload = decoder.decode(&bytes);
    payload.push_str(&decoder.finish());
    match v8json::parse(&payload).ok()? {
        Value::Object(claims) => Some(claims),
        _ => None,
    }
}

fn auth_claim(claims: &Map<String, Value>) -> Option<&Map<String, Value>> {
    claims
        .get("https://api.openai.com/auth")
        .and_then(Value::as_object)
}

fn non_empty(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// `deriveAccountId`.
pub fn derive_account_id(token: Option<&str>) -> Option<String> {
    let claims = jwt_claims(token)?;
    if let Some(id) = auth_claim(&claims).and_then(|auth| non_empty(auth.get("chatgpt_account_id")))
    {
        return Some(id);
    }
    if let Some(id) = non_empty(claims.get("chatgpt_account_id")) {
        return Some(id);
    }
    let first = claims.get("organizations")?.as_array()?.first()?;
    non_empty(first.get("id"))
}

/// `deriveChatGptAccountIsFedRamp`.
pub fn derive_fedramp(token: Option<&str>) -> bool {
    jwt_claims(token)
        .as_ref()
        .and_then(auth_claim)
        .is_some_and(|auth| auth.get("chatgpt_account_is_fedramp") == Some(&Value::Bool(true)))
}

/// `toAuthFile`: the file's object with `OPENAI_API_KEY`, `tokens` and
/// `last_refresh` kept only when they are what the login holds.
fn to_auth_file(input: &Map<String, Value>) -> Map<String, Value> {
    let mut auth = input.clone();
    if non_empty(input.get("OPENAI_API_KEY")).is_none() {
        auth.shift_remove("OPENAI_API_KEY");
    }
    match input.get("tokens") {
        Some(Value::Object(tokens)) if !tokens.is_empty() => {
            let mut kept = Map::new();
            for key in ["id_token", "access_token", "refresh_token", "account_id"] {
                if let Some(value @ Value::String(_)) = tokens.get(key) {
                    kept.insert(key.into(), value.clone());
                }
            }
            auth.insert("tokens".into(), Value::Object(kept));
        }
        _ => {
            auth.shift_remove("tokens");
        }
    }
    if non_empty(input.get("last_refresh")).is_none() {
        auth.shift_remove("last_refresh");
    }
    auth
}

/// `shouldRefreshAccessToken`.
fn should_refresh(access_token: Option<&str>, last_refresh: Option<&str>, now: i64) -> bool {
    let Some(access_token) = access_token else {
        return true;
    };
    if let Some(exp) =
        jwt_claims(Some(access_token)).and_then(|c| c.get("exp").and_then(Value::as_f64))
        && exp * 1000.0 <= (now + REFRESH_EXPIRY_MARGIN_MS) as f64
    {
        return true;
    }
    match last_refresh.and_then(js::parse_iso) {
        Some(at) => at <= now - REFRESH_INTERVAL_MS,
        None => false,
    }
}

/// `writeAuthFile`: `JSON.stringify(data, null, 2)`, a new file readable by
/// its owner only.
fn write_auth_file(path: &Path, data: &Map<String, Value>) -> Result<(), String> {
    use std::io::Write as _;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    file.write_all(mcbench_json::stringify_pretty(&Value::Object(data.clone()), 2).as_bytes())
        .map_err(|e| e.to_string())
}

/// `requestOpenAIOAuthTokens` for a refresh.
async fn refresh(api: &Api, refresh_token: &str) -> Result<Map<String, Value>, String> {
    let body = json!({"grant_type": "refresh_token", "refresh_token": refresh_token, "client_id": CLIENT_ID});
    let answer = http::send(
        &api.http,
        Request {
            method: reqwest::Method::POST,
            url: &api.endpoints.codex_token,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Some(mcbench_json::stringify(&body)),
        },
        &Deadline::new(&CancellationToken::new(), NO_TIMEOUT),
    )
    .await
    .map_err(|failure| failure.message().to_owned())?;
    let status = answer.status;
    let ok = answer.ok();
    let text = answer
        .text()
        .await
        .map_err(|failure| failure.message().to_owned())?;
    if !ok {
        let mut detail = String::new();
        if let Ok(Value::Object(parsed)) = v8json::parse(&text) {
            let message = js::coalesce(
                js::coalesce(parsed.get("error_description"), parsed.get("message")),
                parsed.get("detail"),
            );
            if let Some(Value::String(message)) = message {
                detail = format!(" {message}");
            }
        }
        return Err(format!(
            "OpenAI OAuth token request failed with HTTP {status}.{detail}"
        ));
    }
    match v8json::parse(&text) {
        Err(_) => Err("OpenAI OAuth token response was not valid JSON.".into()),
        Ok(Value::Object(payload)) => {
            if non_empty(payload.get("access_token")).is_none() {
                return Err("OpenAI OAuth token response did not include access_token.".into());
            }
            Ok(payload)
        }
        Ok(_) => Err("OpenAI OAuth token response must be a JSON object.".into()),
    }
}

/// `loadAuthTokens`: the session, refreshed and written back when needed.
async fn load(api: &Api, path: &Path) -> Result<Session, String> {
    let shown = path.display();
    let data = match std::fs::read(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Map::new(),
        Err(error) => return Err(error.to_string()),
        Ok(bytes) => {
            let mut decoder = super::sse::Utf8Decoder::default();
            let mut text = decoder.decode(&bytes);
            text.push_str(&decoder.finish());
            match v8json::parse(&text) {
                Err(_) => return Err(format!("Auth file at {shown} is not valid JSON.")),
                Ok(Value::Object(object)) => to_auth_file(&object),
                Ok(_) => return Err(format!("Auth file at {shown} must contain a JSON object.")),
            }
        }
    };
    let tokens = data.get("tokens");
    let token = |key: &str| non_empty(js::get(tokens, key));
    let mut access_token = token("access_token");
    let mut id_token = token("id_token");
    let mut refresh_token = token("refresh_token");
    let mut account_id = token("account_id").or_else(|| derive_account_id(id_token.as_deref()));
    let mut is_fedramp =
        derive_fedramp(id_token.as_deref()) || derive_fedramp(access_token.as_deref());
    let last_refresh = data.get("last_refresh").and_then(Value::as_str);
    let now = js::now_ms();
    if let Some(current) = refresh_token.clone()
        && should_refresh(access_token.as_deref(), last_refresh, now)
    {
        let payload = refresh(api, &current).await?;
        let new_access = payload
            .get("access_token")
            .and_then(Value::as_str)
            .map(String::from);
        let new_id = payload
            .get("id_token")
            .and_then(Value::as_str)
            .map(String::from);
        let new_refresh = payload
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(String::from);
        let new_account = derive_account_id(new_id.as_deref())
            .or_else(|| derive_account_id(new_access.as_deref()));
        let refreshed_fedramp =
            derive_fedramp(new_id.as_deref()) || derive_fedramp(new_access.as_deref());
        access_token = new_access;
        id_token = new_id.or(id_token);
        refresh_token = new_refresh.or(refresh_token);
        account_id = new_account.or(account_id);
        is_fedramp = is_fedramp
            || refreshed_fedramp
            || derive_fedramp(id_token.as_deref())
            || derive_fedramp(access_token.as_deref());
        let mut written = data.clone();
        written.insert("auth_mode".into(), "chatgpt".into());
        let mut tokens = Map::new();
        for (key, value) in [
            ("id_token", &id_token),
            ("access_token", &access_token),
            ("refresh_token", &refresh_token),
            ("account_id", &account_id),
        ] {
            if let Some(value) = value {
                tokens.insert(key.into(), Value::String(value.clone()));
            }
        }
        written.insert("tokens".into(), Value::Object(tokens));
        written.insert(
            "last_refresh".into(),
            Value::String(js::iso_string(js::now_ms())),
        );
        write_auth_file(path, &written)?;
    }
    let access_token = access_token
        .ok_or("ChatGPT access token not found. Run `npx openai-oauth login` to sign in.")?;
    let account_id = account_id.ok_or(
        "ChatGPT account id not found in auth.json. Run `npx openai-oauth login` to sign in again.",
    )?;
    Ok(Session {
        access_token,
        account_id,
        is_fedramp,
    })
}

/// `applyAuthHeaders`.
fn auth_headers(session: &Session) -> Vec<(String, String)> {
    let mut headers = vec![
        (
            "Authorization".into(),
            format!("Bearer {}", session.access_token),
        ),
        ("chatgpt-account-id".into(), session.account_id.clone()),
    ];
    if session.is_fedramp {
        headers.push(("X-OpenAI-Fedramp".into(), "true".into()));
    }
    headers
}

/// `normalizeVersion`: the first `\b\d+\.\d+\.\d+\b` of the trimmed text.
fn normalize_version(text: &str) -> Option<String> {
    let text = js::trim(text);
    let b = text.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let digits = |mut at: usize| -> usize {
        while b.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        at
    };
    (0..b.len()).find_map(|start| {
        if !b[start].is_ascii_digit() || (start > 0 && word(b[start - 1])) {
            return None;
        }
        let mut at = digits(start);
        for _ in 0..2 {
            if b.get(at) != Some(&b'.') || !b.get(at + 1).is_some_and(u8::is_ascii_digit) {
                return None;
            }
            at = digits(at + 1);
        }
        (!b.get(at).is_some_and(|c| word(*c))).then(|| text[start..at].to_owned())
    })
}

/// `resolveCodexClientVersion`: npm's latest `@openai/codex`, or the
/// fallback, either kept for an hour.
async fn version(api: &Api) -> String {
    let cell = {
        let mut state = api.oauth.version.lock().expect("the version");
        if let Some((at, version)) = &state.0
            && at.elapsed() < VERSION_TTL
        {
            return version.clone();
        }
        state.1.get_or_insert_with(Default::default).clone()
    };
    let version = cell
        .get_or_init(|| async {
            let version = match http::send(
                &api.http,
                Request {
                    method: reqwest::Method::GET,
                    url: &api.endpoints.codex_registry,
                    headers: vec![("accept".into(), "application/json".into())],
                    body: None,
                },
                &Deadline::new(&CancellationToken::new(), NO_TIMEOUT),
            )
            .await
            {
                Ok(answer) if answer.ok() => match answer.text().await {
                    Ok(text) => v8json::parse(&text).ok().and_then(|parsed| {
                        parsed
                            .get("version")
                            .and_then(Value::as_str)
                            .and_then(normalize_version)
                    }),
                    Err(_) => None,
                },
                _ => None,
            };
            let version = version.unwrap_or_else(|| FALLBACK_VERSION.to_owned());
            api.oauth.version.lock().expect("the version").0 =
                Some((tokio::time::Instant::now(), version.clone()));
            version
        })
        .await
        .clone();
    let mut state = api.oauth.version.lock().expect("the version");
    if state
        .1
        .as_ref()
        .is_some_and(|held| Arc::ptr_eq(held, &cell))
    {
        state.1 = None;
    }
    version
}

/// `toUpstreamErrorMessage`.
fn upstream_error(text: &str) -> String {
    if text.is_empty() {
        return "Failed to load models from Codex.".into();
    }
    if let Ok(Value::Object(parsed)) = v8json::parse(text) {
        if let Some(Value::String(detail)) = parsed.get("detail")
            && !detail.is_empty()
        {
            return detail.clone();
        }
        if let Some(Value::String(message)) = js::get(parsed.get("error"), "message") {
            return message.clone();
        }
    }
    text.to_owned()
}

/// `fetchCodexModelCatalog`.
async fn fetch_catalog(api: &Api, session: &Session) -> Result<Vec<ModelDefaults>, String> {
    let version = version(api).await;
    let url = format!(
        "{}/models?client_version={}",
        api.endpoints.codex,
        encode_uri_component(&version)
    );
    let answer = http::send(
        &api.http,
        Request {
            method: reqwest::Method::GET,
            url: &url,
            headers: auth_headers(session),
            body: None,
        },
        &Deadline::new(&CancellationToken::new(), NO_TIMEOUT),
    )
    .await
    .map_err(|failure| failure.message().to_owned())?;
    let ok = answer.ok();
    let text = answer
        .text()
        .await
        .map_err(|failure| failure.message().to_owned())?;
    if !ok {
        return Err(upstream_error(&text));
    }
    let parsed = v8json::parse(&text)
        .map_err(|_| "Codex returned an invalid models response.".to_owned())?;
    let Some(models) = parsed
        .get("models")
        .and_then(Value::as_array)
        .filter(|_| parsed.is_object())
    else {
        return Err("Codex returned a malformed models response.".into());
    };
    let string = |m: &Value, key: &str| non_empty(m.get(key));
    let boolean = |m: &Value, key: &str| m.get(key).and_then(Value::as_bool);
    let models: Vec<ModelDefaults> = models
        .iter()
        .filter(|m| m.is_object())
        .filter_map(|m| {
            Some(ModelDefaults {
                slug: string(m, "slug")?,
                use_responses_lite: boolean(m, "use_responses_lite"),
                support_verbosity: boolean(m, "support_verbosity"),
                default_verbosity: string(m, "default_verbosity"),
                default_reasoning_level: string(m, "default_reasoning_level"),
            })
        })
        .collect();
    if models.is_empty() {
        return Err("Codex returned an empty models list.".into());
    }
    Ok(models)
}

/// The account's catalogue (`createModelCatalogResolver`): kept five
/// minutes, a failure one minute as an empty list.
async fn catalog(api: &Api, session: &Session) -> Arc<Vec<ModelDefaults>> {
    let key = format!("{}:{}", session.account_id, session.is_fedramp);
    if let Some((expires, models)) = api.oauth.catalogs.lock().expect("the catalogues").get(&key)
        && tokio::time::Instant::now() < *expires
    {
        return models.clone();
    }
    let cell = api
        .oauth
        .loading
        .lock()
        .expect("the catalogues")
        .entry(key.clone())
        .or_default()
        .clone();
    let models = cell
        .get_or_init(|| async {
            let (ttl, models) = match fetch_catalog(api, session).await {
                Ok(models) => (CATALOG_TTL, models),
                Err(_) => (CATALOG_FAILURE_TTL, Vec::new()),
            };
            let models = Arc::new(models);
            api.oauth.catalogs.lock().expect("the catalogues").insert(
                key.clone(),
                (tokio::time::Instant::now() + ttl, models.clone()),
            );
            models
        })
        .await
        .clone();
    let mut loading = api.oauth.loading.lock().expect("the catalogues");
    if loading
        .get(&key)
        .is_some_and(|held| Arc::ptr_eq(held, &cell))
    {
        loading.remove(&key);
    }
    models
}

/// `normalizeCodexResponsesBody` with the model's catalogue defaults
/// (`applyModelDefaults`) and `forceStream`; whether the request takes the
/// responses-lite header.
fn normalize(body: &Value, defaults: Option<&ModelDefaults>) -> (Value, bool) {
    let mut normalized = body
        .as_object()
        .expect("the request body is an object")
        .clone();
    if !normalized.get("instructions").is_some_and(Value::is_string) {
        normalized.insert("instructions".into(), Value::String(String::new()));
    }
    if let Some(Value::String(text)) = normalized.get("input").cloned() {
        normalized.insert(
            "input".into(),
            json!([{"role": "user", "content": [{"type": "input_text", "text": text}]}]),
        );
    }
    normalized.insert("store".into(), Value::Bool(false));
    let mut include: Vec<Value> = normalized
        .get("include")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter(|v| v.is_string()).cloned().collect())
        .unwrap_or_default();
    if !include
        .iter()
        .any(|v| v.as_str() == Some("reasoning.encrypted_content"))
    {
        include.push("reasoning.encrypted_content".into());
    }
    normalized.insert("include".into(), Value::Array(include));
    let mut lite = false;
    if let Some(info) = defaults {
        let mut reasoning = normalized
            .get("reasoning")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if !reasoning.contains_key("effort")
            && let Some(level) = &info.default_reasoning_level
        {
            reasoning.insert("effort".into(), Value::String(level.clone()));
        }
        if info.use_responses_lite == Some(true) {
            reasoning.insert("context".into(), "all_turns".into());
        }
        if !reasoning.is_empty() {
            normalized.insert("reasoning".into(), Value::Object(reasoning));
        }
        if info.support_verbosity == Some(true)
            && let Some(verbosity) = &info.default_verbosity
        {
            let mut text = normalized
                .get("text")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            if !text.contains_key("verbosity") {
                text.insert("verbosity".into(), Value::String(verbosity.clone()));
            }
            normalized.insert("text".into(), Value::Object(text));
        }
        if info.use_responses_lite == Some(true) {
            lite = true;
            let input = normalized
                .get("input")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let tools = normalized
                .get("tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut prefix = Vec::new();
            if !tools.is_empty()
                && !input.iter().any(|item| {
                    item.get("type").and_then(Value::as_str) == Some("additional_tools")
                })
            {
                prefix
                    .push(json!({"type": "additional_tools", "role": "developer", "tools": tools}));
            }
            if let Some(Value::String(instructions)) = normalized.get("instructions")
                && !instructions.is_empty()
            {
                prefix.push(json!({"role": "developer", "content": [{"type": "input_text", "text": instructions}]}));
            }
            prefix.extend(input);
            normalized.insert("input".into(), Value::Array(prefix));
            normalized.insert("instructions".into(), Value::String(String::new()));
            normalized.insert("parallel_tool_calls".into(), Value::Bool(false));
            normalized.shift_remove("tools");
        }
    }
    normalized.insert("stream".into(), Value::Bool(true));
    normalized.shift_remove("max_output_tokens");
    (Value::Object(normalized), lite)
}

/// The bridge's handling of one Responses request: the session, the
/// model's defaults, and the request to the ChatGPT backend with the
/// account's and the chat's headers. A failure is the bridge's HTTP 500.
pub async fn send(
    api: &Api,
    auth_file: &Path,
    body: &Value,
    cancel: &CancellationToken,
) -> Result<http::Answer, ProviderError> {
    let server_error = |message: String| {
        let answer = mcbench_json::stringify(
            &json!({"error": {"message": message, "type": "server_error"}}),
        );
        ProviderError::new(provider_error_message("", 500, &answer))
    };
    let session = load(api, auth_file).await.map_err(server_error)?;
    let defaults = match body.get("model").and_then(Value::as_str) {
        Some(model) => catalog(api, &session)
            .await
            .iter()
            .find(|m| m.slug == model)
            .cloned(),
        None => None,
    };
    let (normalized, lite) = normalize(body, defaults.as_ref());
    let mut headers = vec![("content-type".to_owned(), "application/json".to_owned())];
    headers.extend(auth_headers(&session));
    if lite {
        headers.push((LITE_HEADER.into(), "true".into()));
    }
    // codexSessionFetch: the session headers from the body's key.
    if let Some(key) = normalized.get("prompt_cache_key") {
        let key = js::to_string(key);
        headers.push(("session-id".into(), key.clone()));
        headers.push(("thread-id".into(), key));
    }
    let url = format!("{}/responses", api.endpoints.codex);
    let deadline = Deadline::new(cancel, http::COMPLETION_TIMEOUT);
    http::send(
        &api.http,
        Request {
            method: reqwest::Method::POST,
            url: &url,
            headers,
            body: Some(mcbench_json::stringify(&normalized)),
        },
        &deadline,
    )
    .await
    .map_err(|failure| server_error(failure.message().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_read_as_the_bridge_reads_them() {
        assert_eq!(normalize_version(" 0.169.0 ").as_deref(), Some("0.169.0"));
        assert_eq!(
            normalize_version("codex-cli 0.169.0-alpha.1").as_deref(),
            Some("0.169.0")
        );
        assert_eq!(normalize_version("v0.169.0"), None);
        assert_eq!(normalize_version("1.2"), None);
    }

    #[test]
    fn jwt_claims_and_account_ids() {
        let token = |claims: Value| {
            let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
            format!(
                "{}.{}.sig",
                b64.encode("{}"),
                b64.encode(mcbench_json::stringify(&claims))
            )
        };
        let id = token(
            json!({"https://api.openai.com/auth": {"chatgpt_account_id": "acct", "chatgpt_account_is_fedramp": true}}),
        );
        assert_eq!(derive_account_id(Some(&id)).as_deref(), Some("acct"));
        assert!(derive_fedramp(Some(&id)));
        assert_eq!(
            derive_account_id(Some(&token(json!({"organizations": [{"id": "org-1"}]})))).as_deref(),
            Some("org-1")
        );
        assert_eq!(derive_account_id(Some("not-a-jwt")), None);
    }
}
