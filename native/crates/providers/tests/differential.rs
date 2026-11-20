//! The differential test of the API providers: the cases of
//! `tests/node/cases.js` through `mcbench_providers::api` against the replay
//! server `tests/node/replay.js`, written as `tests/node/diff-node.js`
//! writes today's `llm.js` run, and compared with that run.
//!
//! It needs Node and a folder, so it runs only when
//! `MCBENCH_PROVIDERS_DIFF=<folder>` is set (and then fails, never skips,
//! when something is missing). In the folder: `cases.json` is written from
//! `cases.js`, `rust/requests.jsonl` and `rust/results.jsonl` are this run,
//! and when `node/` holds `diff-node.js`'s files (the Node run, made on any
//! machine with `node diff-node.js --server <checkout>/server --cases
//! <folder>/cases.json --out <folder>/node`) every request, delta and message
//! is compared and a table per provider is printed. Header names are
//! compared without case: undici keeps the case a caller wrote, and the
//! `http` crate's names are lower case (see `src/api/http.rs`).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use mcbench_providers::api::{Api, Endpoints};
use mcbench_providers::{
    ApiMessage, CliRuntime, CompletionRequest, Delta, ProviderConfig, ProviderId, ToolSchema,
};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

fn node_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node")
}

fn provider_id(key: &str) -> ProviderId {
    serde_json::from_value(Value::String(key.into())).expect("a provider key")
}

fn normalize_telemetry(telemetry: &mut Value) {
    telemetry["startedAt"] = "<startedAt>".into();
    telemetry["wallMs"] = 0.into();
    if !telemetry["firstDeltaMs"].is_null() {
        telemetry["firstDeltaMs"] = 0.into();
    }
}

async fn run_case(
    api: &Arc<Api>,
    keys: &Map<String, Value>,
    codex_home: &Path,
    entry: &Value,
) -> Value {
    let id = provider_id(entry["provider"].as_str().unwrap());
    let mut api_key = keys
        .get(id.key())
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if let Some(Value::String(value)) = entry["env"].as_object().and_then(|env| env.values().next())
    {
        api_key = value.clone();
    }
    let auth = codex_home.join("auth.json");
    let connected = match id {
        ProviderId::Codex => auth.exists(),
        _ => !api_key.is_empty(),
    };
    let cli = (id == ProviderId::Codex).then(|| CliRuntime {
        program: node_dir()
            .join("fake-codex-models.js")
            .display()
            .to_string(),
        args: Vec::new(),
        home: codex_home.to_owned(),
        env: vec![
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
            ("HOME".into(), std::env::var("HOME").unwrap_or_default()),
            ("CODEX_HOME".into(), codex_home.display().to_string()),
        ],
    });
    let provider = api.provider(ProviderConfig {
        id,
        api_key,
        enabled: true,
        connected,
        cli,
    });
    let mut record = Map::new();
    record.insert("case".into(), entry["id"].clone());
    let error_record = |message: String, reply: Value, telemetry: Value| json!({"message": message, "reply": reply, "telemetry": telemetry});
    match entry["kind"].as_str().unwrap() {
        "complete" => {
            let request = CompletionRequest {
                conversation_id: entry["conversationId"].as_str().map(String::from),
                model: entry["model"].as_str().unwrap().into(),
                effort: entry["effort"].as_str().map(String::from),
                messages: entry["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| ApiMessage(m.as_object().unwrap().clone()))
                    .collect(),
                tools: entry["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| ToolSchema(t.as_str().unwrap().into()))
                    .collect(),
                chat: false,
                transcript: None,
            };
            let cancel = CancellationToken::new();
            let stop_after = entry["cancelAfterDeltas"].as_u64();
            let mut deltas: Vec<Value> = Vec::new();
            let result = {
                let stopper = cancel.clone();
                let mut on_delta = |delta: Delta| {
                    deltas.push(delta.to_json().expect("an API delta"));
                    if stop_after == Some(deltas.len() as u64) {
                        stopper.cancel();
                    }
                };
                provider.complete(&request, &mut on_delta, &cancel).await
            };
            record.insert("deltas".into(), Value::Array(deltas));
            match result {
                Ok(message) => {
                    let mut message = Value::Object(message.to_json());
                    normalize_telemetry(&mut message["telemetry"]);
                    record.insert("message".into(), message);
                }
                Err(error) => {
                    let (reply, telemetry) = match error.telemetry {
                        Some(telemetry) => {
                            let mut telemetry = telemetry.to_json();
                            normalize_telemetry(&mut telemetry);
                            (Value::String(error.reply), telemetry)
                        }
                        None => (Value::Null, Value::Null),
                    };
                    record.insert(
                        "error".into(),
                        error_record(error.message, reply, telemetry),
                    );
                }
            }
        }
        "listModels" => match provider.models().await {
            Ok(models) => {
                record.insert(
                    "models".into(),
                    Value::Array(models.iter().map(|m| m.to_json()).collect()),
                );
            }
            Err(error) => {
                record.insert(
                    "error".into(),
                    error_record(error.message, Value::Null, Value::Null),
                );
            }
        },
        "modelInfo" => match provider.model_info(entry["model"].as_str().unwrap()).await {
            Ok(facts) => {
                record.insert("info".into(), facts.to_json());
            }
            Err(error) => {
                record.insert(
                    "error".into(),
                    error_record(error.message, Value::Null, Value::Null),
                );
            }
        },
        other => panic!("unknown case kind {other}"),
    }
    if id == ProviderId::Codex {
        record.insert(
            "authAfter".into(),
            std::fs::read_to_string(&auth)
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
    }
    Value::Object(record)
}

fn read_lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// A request with its header names in lower case, as compared.
fn folded(request: &Value) -> Value {
    let mut request = request.clone();
    let headers: Vec<Value> = request["headers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| json!([h[0].as_str().unwrap().to_ascii_lowercase(), h[1]]))
        .collect();
    request["headers"] = Value::Array(headers);
    request
}

/// The record with the login file's `last_refresh` written as a fixed value
/// when the run refreshed the tokens (each side wrote its own clock's time),
/// the file's text otherwise as written.
fn without_refresh_time(record: &Value) -> Value {
    let mut record = record.clone();
    if let Some(text) = record.get("authAfter").and_then(Value::as_str)
        && let Ok(mut auth) = mcbench_json::parse(text)
        && auth
            .get("last_refresh")
            .and_then(Value::as_str)
            .is_some_and(|t| !t.starts_with("2100-"))
    {
        assert_eq!(
            mcbench_json::stringify_pretty(&auth, 2),
            text,
            "the login file is JSON.stringify(data, null, 2)"
        );
        auth["last_refresh"] = "<refreshed>".into();
        record["authAfter"] = mcbench_json::stringify_pretty(&auth, 2).into();
    }
    record
}

#[derive(Default)]
struct Row {
    conversations: usize,
    requests: usize,
    requests_same: usize,
    deltas_same: usize,
    messages_same: usize,
    other: usize,
    other_same: usize,
}

fn compare(folder: &Path, cases: &[Value]) {
    let node = folder.join("node");
    let rust = folder.join("rust");
    let by_case = |path: &Path| {
        let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for line in read_lines(path) {
            out.entry(line["case"].as_str().unwrap().to_owned())
                .or_default()
                .push(line);
        }
        out
    };
    let node_requests = by_case(&node.join("requests.jsonl"));
    let rust_requests = by_case(&rust.join("requests.jsonl"));
    let node_results = by_case(&node.join("results.jsonl"));
    let rust_results = by_case(&rust.join("results.jsonl"));
    let mut rows: BTreeMap<String, Row> = BTreeMap::new();
    let mut differences = Vec::new();
    let mut case_only = 0;
    for entry in cases {
        let id = entry["id"].as_str().unwrap();
        let row = rows
            .entry(entry["provider"].as_str().unwrap().to_owned())
            .or_default();
        let empty = Vec::new();
        let nreq = node_requests.get(id).unwrap_or(&empty);
        let rreq = rust_requests.get(id).unwrap_or(&empty);
        let requests_same =
            nreq.len() == rreq.len() && nreq.iter().zip(rreq).all(|(a, b)| folded(a) == folded(b));
        if requests_same && nreq != rreq {
            case_only += 1;
        }
        if !requests_same {
            differences.push(format!(
                "{id}: requests differ\n  node {}\n  rust {}",
                serde_json::to_string(nreq).unwrap(),
                serde_json::to_string(rreq).unwrap()
            ));
        }
        let n = &without_refresh_time(&node_results[id][0]);
        let r = &without_refresh_time(&rust_results[id][0]);
        // Each side's record as its own JSON text: the bytes JSON.stringify
        // and mcbench-json wrote.
        let text = |v: &Value| mcbench_json::stringify(v);
        if entry["kind"] == "complete" {
            row.conversations += 1;
            row.requests += nreq.len();
            row.requests_same += usize::from(requests_same);
            let deltas_same = text(&n["deltas"]) == text(&r["deltas"]);
            row.deltas_same += usize::from(deltas_same);
            let mut n_rest = n.clone();
            let mut r_rest = r.clone();
            n_rest.as_object_mut().unwrap().shift_remove("deltas");
            r_rest.as_object_mut().unwrap().shift_remove("deltas");
            let messages_same = text(&n_rest) == text(&r_rest);
            row.messages_same += usize::from(messages_same);
            if !deltas_same {
                differences.push(format!(
                    "{id}: deltas differ\n  node {}\n  rust {}",
                    text(&n["deltas"]),
                    text(&r["deltas"])
                ));
            }
            if !messages_same {
                differences.push(format!(
                    "{id}: results differ\n  node {}\n  rust {}",
                    text(&n_rest),
                    text(&r_rest)
                ));
            }
        } else {
            row.other += 1;
            let same = requests_same && text(n) == text(r);
            row.other_same += usize::from(same);
            if text(n) != text(r) {
                differences.push(format!(
                    "{id}: results differ\n  node {}\n  rust {}",
                    text(n),
                    text(r)
                ));
            }
        }
    }
    let mut table = String::from(
        "| provider | conversations | requests | conversations with identical requests | identical deltas | identical messages or errors | model lists and facts identical |\n|---|---|---|---|---|---|---|\n",
    );
    for (provider, row) in &rows {
        table.push_str(&format!(
            "| {provider} | {} | {} | {} | {} | {} | {} of {} |\n",
            row.conversations,
            row.requests,
            row.requests_same,
            row.deltas_same,
            row.messages_same,
            row.other_same,
            row.other
        ));
    }
    table.push_str(&format!(
        "\nCases whose requests differ only in header-name case: {case_only}\n"
    ));
    std::fs::write(folder.join("table.md"), &table).unwrap();
    println!("{table}");
    assert!(
        differences.is_empty(),
        "{} differences:\n{}",
        differences.len(),
        differences[..differences.len().min(12)].join("\n")
    );
}

/// The replay server, stopped if the test fails before it ends it.
struct Replay(std::process::Child);

impl Drop for Replay {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "needs Node and a folder: set MCBENCH_PROVIDERS_DIFF=<folder>"]
fn the_api_providers_match_todays_llm_js() {
    let folder = PathBuf::from(
        std::env::var("MCBENCH_PROVIDERS_DIFF").expect("MCBENCH_PROVIDERS_DIFF names the folder"),
    );
    let rust = folder.join("rust");
    let _ = std::fs::remove_dir_all(&rust);
    std::fs::create_dir_all(rust.join("codex")).unwrap();
    let cases_text = Command::new("node")
        .arg(node_dir().join("cases.js"))
        .output()
        .expect("node runs (MCBENCH_PROVIDERS_DIFF needs Node on PATH)");
    assert!(cases_text.status.success(), "cases.js failed");
    std::fs::write(folder.join("cases.json"), &cases_text.stdout).unwrap();
    let cases: Value = serde_json::from_slice(&cases_text.stdout).unwrap();
    let mut replay = Replay(
        Command::new("node")
            .arg(node_dir().join("replay.js"))
            .arg("--log")
            .arg(rust.join("requests.jsonl"))
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(rust.join("replay.stderr")).unwrap())
            .spawn()
            .expect("the replay server starts"),
    );
    let mut first = String::new();
    BufReader::new(replay.0.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    let port = serde_json::from_str::<Value>(&first).unwrap()["port"]
        .as_u64()
        .unwrap();
    let origin = format!("http://127.0.0.1:{port}");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let api = Api::new(Endpoints::under(&origin));
        let control = mcbench_providers::api::http::client();
        let codex_home = rust.join("codex");
        let keys = cases["keys"].as_object().unwrap().clone();
        let mut results = std::fs::File::create(rust.join("results.jsonl")).unwrap();
        for entry in cases["cases"].as_array().unwrap() {
            if let Some(auth) = entry["codexAuth"].as_str() {
                std::fs::write(codex_home.join("auth.json"), auth).unwrap();
            }
            control
                .post(format!("{origin}/__case"))
                .body(mcbench_json::stringify(
                    &json!({"id": entry["id"], "routes": entry["routes"]}),
                ))
                .send()
                .await
                .unwrap();
            let record = run_case(&api, &keys, &codex_home, entry).await;
            writeln!(results, "{}", mcbench_json::stringify(&record)).unwrap();
        }
        control
            .post(format!("{origin}/__end"))
            .body("{}")
            .send()
            .await
            .unwrap();
    });
    assert!(replay.0.wait().unwrap().success());
    if folder.join("node/results.jsonl").exists() {
        compare(&folder, cases["cases"].as_array().unwrap());
    } else {
        println!(
            "no Node run in {}: nothing compared",
            folder.join("node").display()
        );
    }
}
