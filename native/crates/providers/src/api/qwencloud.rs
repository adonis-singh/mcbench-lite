//! QwenCloud's curated models and thinking parameters (`server/qwencloud.js`).
//!
//! Curated September 2026 releases, intersected with the authenticated
//! `/models` response. Native creation timestamps also change when old
//! aliases are redeployed, so a date cutoff alone would bring legacy models
//! back. Capabilities and thinking parameters follow QwenCloud, not
//! OpenRouter.

use serde_json::{Map, Value};

use crate::{ModelFacts, ModelInfo, ProviderError};

/// One curated model.
struct Model {
    id: &'static str,
    name: &'static str,
    image_input: bool,
    native_effort: bool,
    preserve_thinking: bool,
    mandatory_reasoning: bool,
}

const MODELS: [Model; 8] = [
    Model {
        id: "qwen3.8-max-0902",
        name: "Qwen3.8 Max (0902)",
        image_input: true,
        native_effort: true,
        preserve_thinking: true,
        mandatory_reasoning: false,
    },
    Model {
        id: "qwen3.8-max",
        name: "Qwen3.8 Max",
        image_input: true,
        native_effort: true,
        preserve_thinking: true,
        mandatory_reasoning: false,
    },
    Model {
        id: "qwen3.8-flash",
        name: "Qwen3.8 Flash",
        image_input: true,
        native_effort: true,
        preserve_thinking: true,
        mandatory_reasoning: false,
    },
    Model {
        id: "qwen3.8-2.4t-a95b",
        name: "Qwen3.8 2.4T A95B",
        image_input: false,
        native_effort: true,
        preserve_thinking: false,
        mandatory_reasoning: true,
    },
    Model {
        id: "qwen3.8-27b",
        name: "Qwen3.8 27B",
        image_input: true,
        native_effort: true,
        preserve_thinking: false,
        mandatory_reasoning: false,
    },
    Model {
        id: "qwen3.7-max",
        name: "Qwen3.7 Max",
        image_input: false,
        native_effort: false,
        preserve_thinking: true,
        mandatory_reasoning: false,
    },
    Model {
        id: "qwen3.7-plus",
        name: "Qwen3.7 Plus",
        image_input: true,
        native_effort: false,
        preserve_thinking: true,
        mandatory_reasoning: false,
    },
    Model {
        id: "qwen3.7-flash",
        name: "Qwen3.7 Flash",
        image_input: true,
        native_effort: false,
        preserve_thinking: true,
        mandatory_reasoning: false,
    },
];

/// UI effort levels for models that expose a token budget instead of an
/// enum (`THINKING_BUDGETS`).
const THINKING_BUDGETS: [(&str, u64); 5] = [
    ("low", 4096),
    ("medium", 16384),
    ("high", 32768),
    ("xhigh", 65536),
    ("max", 262_144),
];

fn model(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|m| m.id == id)
}

/// `modelInfo(id)`: a curated model's facts (no `contextWindow` key), or
/// `None`.
pub fn model_info(id: &str) -> Option<ModelFacts> {
    let model = model(id)?;
    let mut efforts: Vec<String> = Vec::new();
    if !model.mandatory_reasoning {
        efforts.push("none".into());
    }
    if model.native_effort {
        efforts.extend(["low", "medium", "xhigh"].map(String::from));
    } else {
        efforts.extend(THINKING_BUDGETS.iter().map(|(name, _)| name.to_string()));
    }
    Some(ModelFacts {
        known: true,
        image_input: Some(model.image_input),
        image_output: false,
        tools: Some(true),
        reasoning: true,
        efforts,
        default_effort: Some(if model.native_effort { "xhigh" } else { "max" }.into()),
        mandatory_reasoning: model.mandatory_reasoning,
        context_window: None,
        fast: None,
    })
}

/// `recentModels(entries)`: the curated models the account's `/models`
/// lists, in the curated order, with its `created`.
pub fn recent_models(entries: &[Value]) -> Vec<ModelInfo> {
    // `new Map(entries.map(e => [e.id, e]))`: the last entry of an id wins.
    let created = |id: &str| -> Option<f64> {
        entries
            .iter()
            .rev()
            .find(|entry| entry.get("id").and_then(Value::as_str) == Some(id))
            .map(|entry| {
                entry
                    .get("created")
                    .filter(|v| super::js::truthy(Some(v)))
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
            })
    };
    MODELS
        .iter()
        .filter_map(|model| {
            created(model.id).map(|created| ModelInfo {
                id: model.id.into(),
                name: model.name.into(),
                created,
                facts: model_info(model.id).expect("a curated model"),
            })
        })
        .collect()
}

/// `requestOptions(id, effort)`: the body's thinking fields, in order.
pub fn request_options(
    id: &str,
    effort: Option<&str>,
) -> Result<Map<String, Value>, ProviderError> {
    let mut options = Map::new();
    let Some(model) = model(id) else {
        return Ok(options);
    };
    if model.mandatory_reasoning && effort == Some("none") {
        return Err(ProviderError::new(format!(
            "{id} requires thinking; choose low, medium, or xhigh"
        )));
    }
    options.insert(
        "enable_thinking".into(),
        Value::Bool(effort != Some("none")),
    );
    if model.preserve_thinking {
        options.insert("preserve_thinking".into(), Value::Bool(true));
    }
    let Some(effort) = effort.filter(|e| *e != "none") else {
        return Ok(options);
    };
    if model.native_effort {
        let normalized = match effort {
            "minimal" => "low",
            "high" | "max" => "xhigh",
            other => other,
        };
        if !["low", "medium", "xhigh"].contains(&normalized) {
            return Err(ProviderError::new(format!(
                "Unsupported QwenCloud reasoning effort: {effort}"
            )));
        }
        options.insert("reasoning_effort".into(), Value::String(normalized.into()));
    } else {
        let level = if effort == "minimal" { "low" } else { effort };
        let Some((_, budget)) = THINKING_BUDGETS.iter().find(|(name, _)| *name == level) else {
            return Err(ProviderError::new(format!(
                "Unsupported QwenCloud reasoning effort: {effort}"
            )));
        };
        options.insert("thinking_budget".into(), Value::from(*budget));
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    //! `test/qwencloud.test.js`: the three tests without HTTP; the two that
    //! list models and stream a completion are in `tests/ported.rs`.

    use super::*;
    use serde_json::json;

    fn options(id: &str, effort: Option<&str>) -> Value {
        Value::Object(request_options(id, effort).unwrap())
    }

    #[test]
    fn qwencloud_picker_excludes_old_aliases_snapshots_other_vendors_and_non_chat_models() {
        let models = recent_models(&[
            json!({"id": "qwen3.7-plus", "created": 100}),
            json!({"id": "qwen3.8-max-0902", "created": 200}),
            json!({"id": "qwen3.8-flash", "created": 150}),
            json!({"id": "qwen3.5-plus", "created": 999}),
            json!({"id": "qwen3.7-max-2026-05-17", "created": 999}),
            json!({"id": "qwen-plus", "created": 999}),
            json!({"id": "qwen3.7-text-embedding", "created": 999}),
            json!({"id": "qwen-image-3.0", "created": 999}),
            json!({"id": "qwen-audio-3.0-asr-flash", "created": 999}),
            json!({"id": "kimi-k3", "created": 999}),
        ]);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["qwen3.8-max-0902", "qwen3.8-flash", "qwen3.7-plus"]);
        assert!(
            models
                .iter()
                .all(|m| m.facts.tools == Some(true) && !m.facts.image_output)
        );
        assert_eq!(models[0].created, 200.0);
        assert!(recent_models(&[]).is_empty());
    }

    #[test]
    fn qwencloud_capabilities_gate_vision_and_thinking_independently_of_openrouter() {
        let text_only = model_info("qwen3.8-2.4t-a95b").unwrap();
        assert_eq!(text_only.image_input, Some(false));
        assert!(text_only.mandatory_reasoning);
        assert_eq!(text_only.efforts, ["low", "medium", "xhigh"]);
        let vision = model_info("qwen3.8-flash").unwrap();
        assert_eq!(vision.image_input, Some(true));
        assert_eq!(vision.efforts, ["none", "low", "medium", "xhigh"]);
        assert_eq!(vision.default_effort.as_deref(), Some("xhigh"));
        assert_eq!(model_info("qwen3.7-max").unwrap().image_input, Some(false));
        // An unknown model has no curated facts (llm.js then answers with
        // infoFromCatalogEntry(null), whose `known` is false).
        assert!(model_info("unrecognized-model").is_none());
    }

    #[test]
    fn qwencloud_thinking_controls_never_mix_native_effort_with_token_budgets() {
        assert_eq!(
            options("qwen3.8-flash", Some("none")),
            json!({"enable_thinking": false, "preserve_thinking": true})
        );
        for (input, expected) in [
            ("low", "low"),
            ("minimal", "low"),
            ("medium", "medium"),
            ("high", "xhigh"),
            ("max", "xhigh"),
        ] {
            assert_eq!(
                options("qwen3.8-max", Some(input)),
                json!({"enable_thinking": true, "preserve_thinking": true, "reasoning_effort": expected})
            );
        }
        assert_eq!(
            options("qwen3.7-plus", Some("medium")),
            json!({"enable_thinking": true, "preserve_thinking": true, "thinking_budget": 16384})
        );
        assert_eq!(
            options("qwen3.8-27b", None),
            json!({"enable_thinking": true})
        );
        assert!(
            request_options("qwen3.8-2.4t-a95b", Some("none"))
                .unwrap_err()
                .message
                .contains("requires thinking")
        );
        assert!(
            request_options("qwen3.7-plus", Some("invalid"))
                .unwrap_err()
                .message
                .contains("Unsupported")
        );
    }
}
