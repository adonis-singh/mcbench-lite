//! The tool contract: what a form of attempt or chat turn gives a model,
//! the schemas, the system prompt and the budgets: `toolsFor` and the
//! prompt library's tool (`server/agent.js`, `server/prompts.js`),
//! `systemPrompt` (`server/prompt.js`) and `BUDGETS`
//! (`server/verification-contract.js`), byte for byte.
//!
//! The text is in `contract/` beside `src/`: files `contract/generate.js`
//! writes by running today's `server/`, never edited by hand (its header
//! says what each file is). The budgets a schema or the prompt quotes are
//! holes there, `⟦key⟧` or `⟦key/divisor⟧`, filled with JavaScript's
//! `Number::toString` of the budget divided by the divisor
//! ([`mcbench_json::number_to_string`]); the generator finds where the
//! holes are by loading `server/` under sentinel budgets and proves the
//! filled templates give what `server/` gives under others.
//!
//! The tests: [`TOOL_CONTRACT_SHA256`] for the benchmark's 8 forms, always;
//! with `MCBENCH_NODE_DIFF=<checkout with node_modules>` and
//! `MCBENCH_BENCH_DIR=<the private benchmark's folder>`, `tests/node_diff.rs`
//! runs `generate.js --check` and compares every form (placement, image
//! input, network access, chat, each resolution) under the default, the
//! unrestricted and 40 random budget environments (5 of them refused) with Node's output.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use mcbench_tools::verify::Budgets;

use super::ToolSchema;

mod files {
    include!("../contract/files.rs");
}

/// The sweep contract's sha256 that the benchmark's tool contract script
/// prints on `main` 5398055 (and on `native-app`): 8 forms, standard and
/// unrestricted budgets.
pub const TOOL_CONTRACT_SHA256: &str =
    "efc4d688d3dc7feff0f437bcd7a3bac8f79a1ddd86e892f26b992002ff755645";

/// The budgets in force: today's, or lifted to the protocol's integer
/// ceiling for an unrestricted study (`tool-contract.js` `LIFTED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Regime {
    /// `standard`: no `MCBENCH_VERIFY_MAX_*` set.
    Standard,
    /// `unrestricted`: `LIFTED` set.
    Unrestricted,
}

impl Regime {
    /// Both, in the sweep's order.
    pub const ALL: [Regime; 2] = [Regime::Standard, Regime::Unrestricted];

    /// The name the sweep gives it.
    pub fn name(self) -> &'static str {
        match self {
            Regime::Standard => "standard",
            Regime::Unrestricted => "unrestricted",
        }
    }

    /// The environment variables the regime sets (`LIFTED` for the
    /// unrestricted one, from `contract/manifest.json`).
    pub fn vars(self) -> Vec<(String, String)> {
        match self {
            Regime::Standard => Vec::new(),
            Regime::Unrestricted => MANIFEST
                .lifted
                .iter()
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        }
    }

    /// The budgets of an environment that sets only [`Regime::vars`].
    pub fn budgets(self) -> Budgets {
        let vars = self.vars();
        Budgets::from_vars(|name| {
            vars.iter()
                .find(|(n, _)| n == name)
                .map(|(_, value)| value.into())
        })
        .expect("LIFTED's budgets are valid")
    }
}

/// How `test_build` places a build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Placement {
    /// `updates`.
    Updates,
    /// `strict`: `scheduled_ticks` is offered.
    Strict,
}

impl Placement {
    /// `placementMode`'s value.
    pub fn name(self) -> &'static str {
        match self {
            Placement::Updates => "updates",
            Placement::Strict => "strict",
        }
    }
}

/// One form an attempt or a chat turn can take (`toolsFor`'s and
/// `systemPrompt`'s inputs, and whether `runTurn` adds the chat tools). The
/// budgets are not part of it: they are the process's ([`Budgets`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Form {
    /// The placement mode.
    pub placement: Placement,
    /// Whether the model takes images (`see_build` and `view_image`).
    pub image_input: bool,
    /// Whether `bash` offers its networked command.
    pub network_access: bool,
    /// A chat turn: the prompt library's tool follows the others
    /// (`runTurn`'s `chat`).
    pub chat: bool,
    /// The chat's resolution (`web/resolution.mjs` `RESOLUTIONS`' values;
    /// 1 is Standard, which adds no section to the prompt).
    pub resolution: u32,
}

/// The benchmark's forms of one regime, by the names `tool-contract.js`
/// gives them: every placement and image input, network access off, not a
/// chat, the standard resolution.
pub fn benchmark_forms() -> [(&'static str, Form); 4] {
    let form = |placement, image_input| Form {
        placement,
        image_input,
        network_access: false,
        chat: false,
        resolution: MANIFEST.standard_resolution,
    };
    [
        ("updates/image", form(Placement::Updates, true)),
        ("updates/text", form(Placement::Updates, false)),
        ("strict/image", form(Placement::Strict, true)),
        ("strict/text", form(Placement::Strict, false)),
    ]
}

/// What a form gives a model.
#[derive(Clone, Debug)]
pub struct Contract {
    /// The tools, in `toolsFor`'s order, then the chat tools.
    pub tools: Vec<ToolSchema>,
    /// The system prompt.
    pub system_prompt: String,
    /// The budgets, as `JSON.stringify(BUDGETS)` writes them.
    pub budgets: String,
}

impl Contract {
    /// `JSON.stringify` of the tools.
    pub fn tools_json(&self) -> String {
        let texts: Vec<&str> = self.tools.iter().map(|tool| tool.0.as_str()).collect();
        format!("[{}]", texts.join(","))
    }
}

/// The tools, system prompt and budgets of `form` under `budgets`; an error,
/// worded as `resolutionStop`'s, for a resolution that is not one.
pub fn of(form: Form, budgets: &Budgets) -> Result<Contract, String> {
    let manifest = &*MANIFEST;
    if !manifest.resolutions.contains(&form.resolution) {
        let known: Vec<String> = manifest.resolutions.iter().map(u32::to_string).collect();
        return Err(format!(
            "{} is not a resolution ({})",
            form.resolution,
            known.join(", ")
        ));
    }
    let values = match serde_json::to_value(budgets).expect("budgets serialize") {
        serde_json::Value::Object(values) => values,
        _ => unreachable!("Budgets is a struct"),
    };
    let network = if form.network_access {
        "network"
    } else {
        "offline"
    };
    let mut tools = Vec::new();
    for tool in &manifest.tools {
        if tool.vision && !form.image_input {
            continue;
        }
        let mut name = tool.name.clone();
        for axis in &tool.by {
            name.push('.');
            name.push_str(match axis.as_str() {
                "placement" => form.placement.name(),
                "network" => network,
                other => panic!("contract/manifest.json: {} varies by {other}", tool.name),
            });
        }
        tools.push(ToolSchema(fill(
            file(&format!("tools/{name}.json")),
            &values,
        )));
    }
    if form.chat {
        for name in &manifest.chat_tools {
            tools.push(ToolSchema(fill(
                file(&format!("tools/{name}.json")),
                &values,
            )));
        }
    }
    let image = if form.image_input { "image" } else { "text" };
    let mut system_prompt = fill(file(&format!("prompt/{image}.{network}.txt")), &values);
    if form.resolution != manifest.standard_resolution {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(&fill(
            file(&format!(
                "prompt/resolution-{}.{image}.txt",
                form.resolution
            )),
            &values,
        ));
    }
    Ok(Contract {
        tools,
        system_prompt,
        budgets: mcbench_json::to_string(budgets).expect("budgets serialize"),
    })
}

/// The import order revision a contract records (the benchmark's import
/// order revision, `IMPORT_ORDER`, from `contract/manifest.json`).
pub fn import_order() -> u64 {
    MANIFEST.import_order
}

/// What an attempt records of the contract it saw (`tool-contract.js`
/// `attemptContract`): the sha256 of each part and of the parts together.
/// Serialized, it is `attemptContract`'s object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    /// sha256 of `JSON.stringify` of the parts below, in this order.
    pub sha256: String,
    /// sha256 of `JSON.stringify(tools)`.
    pub tools: String,
    /// sha256 of `JSON.stringify(BUDGETS)`.
    pub budgets: String,
    /// sha256 of the system prompt.
    pub system_prompt: String,
    /// The import order revision.
    pub import_order: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Parts<'a> {
    tools: &'a str,
    budgets: &'a str,
    system_prompt: &'a str,
    import_order: u64,
}

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `attemptContract` of `contract`.
pub fn attempt(contract: &Contract) -> Attempt {
    let tools = sha256(&contract.tools_json());
    let budgets = sha256(&contract.budgets);
    let system_prompt = sha256(&contract.system_prompt);
    let parts = Parts {
        tools: &tools,
        budgets: &budgets,
        system_prompt: &system_prompt,
        import_order: import_order(),
    };
    Attempt {
        sha256: sha256(&mcbench_json::to_string(&parts).expect("parts serialize")),
        tools,
        budgets,
        system_prompt,
        import_order: import_order(),
    }
}

/// The sweep's sha256 (`sweepContract`) of its attempts' sha256s, named
/// `<regime>/<form>` in the sweep's order.
pub fn sweep_of(attempts: &[(String, String)]) -> String {
    let object: serde_json::Map<String, serde_json::Value> = attempts
        .iter()
        .map(|(name, sha)| (name.clone(), serde_json::Value::String(sha.clone())))
        .collect();
    sha256(&mcbench_json::stringify(&serde_json::Value::Object(object)))
}

/// The sweep's sha256 of both regimes' benchmark forms, as this crate gives
/// them; [`TOOL_CONTRACT_SHA256`] while no text has changed.
pub fn sweep() -> String {
    let mut attempts = Vec::new();
    for regime in Regime::ALL {
        let budgets = regime.budgets();
        for (name, form) in benchmark_forms() {
            let contract = of(form, &budgets).expect("the benchmark's forms are valid");
            attempts.push((
                format!("{}/{name}", regime.name()),
                attempt(&contract).sha256,
            ));
        }
    }
    sweep_of(&attempts)
}

/// `contract/manifest.json`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    import_order: u64,
    lifted: ordered::Pairs,
    /// `BUDGET_ENV`: the tests hold `Budgets::ENV` to it.
    #[cfg_attr(not(test), allow(dead_code))]
    budget_env: ordered::Pairs,
    tools: Vec<ManifestTool>,
    chat_tools: Vec<String>,
    resolutions: Vec<u32>,
    standard_resolution: u32,
    /// `resolutionStop`'s error for 3: the tests hold [`of`]'s to it.
    #[cfg_attr(not(test), allow(dead_code))]
    resolution_error: String,
}

/// String-to-string objects of the manifest, kept in their order.
mod ordered {
    use serde::Deserialize;

    /// An object of strings as (key, value) pairs in order.
    #[derive(Debug)]
    pub(super) struct Pairs(Vec<(String, String)>);

    impl Pairs {
        pub(super) fn iter(&self) -> impl Iterator<Item = &(String, String)> {
            self.0.iter()
        }
    }

    impl<'de> Deserialize<'de> for Pairs {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let map = serde_json::Map::<String, serde_json::Value>::deserialize(d)?;
            map.into_iter()
                .map(|(key, value)| match value {
                    serde_json::Value::String(value) => Ok((key, value)),
                    other => Err(serde::de::Error::custom(format!(
                        "{key} is {other}, not a string"
                    ))),
                })
                .collect::<Result<_, _>>()
                .map(Pairs)
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestTool {
    name: String,
    vision: bool,
    by: Vec<String>,
}

static MANIFEST: LazyLock<Manifest> = LazyLock::new(|| {
    serde_json::from_str(file("manifest.json"))
        .unwrap_or_else(|error| panic!("contract/manifest.json: {error}"))
});

/// The resolutions a chat or bench may have (`web/resolution.mjs`
/// `RESOLUTIONS`' values, in order).
pub fn resolutions() -> Vec<u32> {
    MANIFEST.resolutions.clone()
}

/// A generated file of `contract/` by its name (the other texts, for
/// [`crate::texts`]).
pub(crate) fn generated(name: &str) -> &'static str {
    file(name)
}

fn file(name: &str) -> &'static str {
    files::FILES
        .iter()
        .find(|(n, _)| *n == name)
        .unwrap_or_else(|| panic!("contract/{name} is not one of generate.js's files"))
        .1
}

/// `template` with each hole `⟦key⟧` or `⟦key/divisor⟧` replaced by
/// `Number::toString` of `values[key] / divisor`.
fn fill(template: &str, values: &serde_json::Map<String, serde_json::Value>) -> String {
    const OPEN: char = '⟦';
    const CLOSE: char = '⟧';
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find(OPEN) {
        out.push_str(&rest[..at]);
        let after = &rest[at + OPEN.len_utf8()..];
        let end = after.find(CLOSE).expect("a hole closes");
        let hole = &after[..end];
        let (key, divisor) = match hole.split_once('/') {
            Some((key, divisor)) => (key, divisor.parse::<u64>().expect("a divisor")),
            None => (hole, 1),
        };
        let value = values
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| panic!("no budget {key}"));
        out.push_str(&mcbench_json::number_to_string(
            value as f64 / divisor as f64,
        ));
        rest = &after[end + CLOSE.len_utf8()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_benchmarks_eight_forms_give_the_pinned_sweep_sha256() {
        assert_eq!(sweep(), TOOL_CONTRACT_SHA256);
    }

    #[test]
    fn the_manifest_names_the_budgets_as_mcbench_tools_does() {
        let names: Vec<&str> = MANIFEST
            .budget_env
            .iter()
            .map(|(_, env)| env.as_str())
            .collect();
        assert_eq!(names, Budgets::ENV);
        let keys: Vec<String> = match serde_json::to_value(Budgets::DEFAULT).unwrap() {
            serde_json::Value::Object(map) => map.keys().cloned().collect(),
            _ => unreachable!(),
        };
        let manifest_keys: Vec<String> =
            MANIFEST.budget_env.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, manifest_keys);
    }

    #[test]
    fn an_unknown_resolution_is_refused_as_resolution_stop_refuses_it() {
        let form = Form {
            resolution: 3,
            ..benchmark_forms()[0].1
        };
        assert_eq!(
            of(form, &Budgets::DEFAULT).unwrap_err(),
            MANIFEST.resolution_error
        );
    }

    #[test]
    fn chat_turns_add_the_prompt_library_after_the_other_tools() {
        let budgets = Budgets::DEFAULT;
        let form = benchmark_forms()[0].1;
        let plain = of(form, &budgets).unwrap();
        let chat = of(Form { chat: true, ..form }, &budgets).unwrap();
        assert_eq!(chat.tools.len(), plain.tools.len() + 1);
        assert!(
            chat.tools
                .last()
                .unwrap()
                .0
                .contains(r#""name":"prompt_library""#)
        );
        assert_eq!(chat.system_prompt, plain.system_prompt);
    }
}
