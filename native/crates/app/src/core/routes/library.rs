//! The prompt library, Improve with AI, the model favourites and the
//! model lists (`server/index.js`: `GET` and `PUT /api/prompts`,
//! `POST /api/prompts/generate`, `PUT /api/model-favorites`,
//! `GET /api/models`, `GET /api/model-info`).

use mcbench_agent::lifecycle::Refusal;
use mcbench_agent::texts::{Improve, improve_request, improved};
use mcbench_providers::CompletionRequest;
use mcbench_store::json::Json;
use mcbench_store::{LibraryPrompt, PromptLibrary};
use tokio_util::sync::CancellationToken;

use crate::core::routes::chats::known_provider;
use crate::core::{Core, refusal};

impl Core {
    fn library(&self) -> PromptLibrary {
        PromptLibrary::new(
            self.0.store.folder().prompts(),
            std::sync::Arc::clone(self.0.store.host()),
        )
    }

    /// `GET /api/prompts`: the library (`{ prompts }`, each
    /// `LibraryPrompt::to_json`, is the route's body).
    pub async fn prompts(&self) -> Result<Vec<LibraryPrompt>, Refusal> {
        self.blocking(|core| core.library().list().map_err(Refusal::from))
            .await
    }

    /// `PUT /api/prompts` with `{ prompts }`: the library saved whole, in
    /// this order; answers it as saved.
    pub async fn save_prompts(&self, prompts: &Json) -> Result<Vec<LibraryPrompt>, Refusal> {
        let Json::Array(prompts) = prompts.clone() else {
            return Err(refusal(400, "prompts must be an array"));
        };
        self.blocking(move |core| core.library().save(&prompts).map_err(Refusal::from))
            .await
    }

    /// `POST /api/prompts/generate` with `{ provider, model, effort, text }`:
    /// the text rewritten as a prompt by that model (`generatePrompt`).
    /// `cancel` stops the model's call (the page's abort of the request).
    pub async fn generate_prompt(
        &self,
        provider: &str,
        model: &str,
        effort: &str,
        text: &str,
        cancel: &CancellationToken,
    ) -> Result<String, Refusal> {
        let key = mcbench_providers::api::js::trim(provider).to_string();
        if !known_provider(&key) {
            return Err(refusal(400, "Choose a configured provider"));
        }
        let providers = std::sync::Arc::clone(&self.0.providers);
        let connected = self
            .blocking(move |_| -> Result<bool, String> {
                let settings = providers.settings();
                providers.connected(&settings, &key)
            })
            .await?;
        let key = mcbench_providers::api::js::trim(provider).to_string();
        if !connected {
            let label = crate::core::providers::info(&key).expect("known").label;
            return Err(refusal(400, format!("{label} is not configured")));
        }
        let (provider, model, effort, messages) = improve_request(&Improve {
            provider: key,
            model: model.to_string(),
            effort: effort.to_string(),
            text: text.to_string(),
        })
        .map_err(|e| refusal(500, e))?;
        let chosen = self
            .0
            .backend
            .provider(&provider)
            .map_err(|e| refusal(500, e))?;
        let reply = chosen
            .complete(
                &CompletionRequest {
                    conversation_id: None,
                    model,
                    effort: (!effort.is_empty()).then_some(effort),
                    messages,
                    tools: Vec::new(),
                    chat: false,
                    transcript: None,
                },
                &mut |_| {},
                cancel,
            )
            .await
            .map_err(|e| refusal(500, e.message))?;
        improved(&reply.content).map_err(|e| refusal(500, e))
    }

    /// `PUT /api/model-favorites` with `{ favorites }`: answers them as
    /// saved.
    pub async fn model_favorites(&self, favorites: &Json) -> Result<Vec<String>, Refusal> {
        let favorites = favorites.clone();
        self.blocking(move |core| {
            core.0
                .providers
                .settings_file()
                .update_model_favorites(&favorites)
                .map_err(Refusal::from)
        })
        .await
    }

    /// The starred models as `(provider, model)` pairs: `modelFavorites`'
    /// `provider::model` keys split at their first `::` (every stored key
    /// has one: `normalizeModelFavorites` keeps no other).
    pub async fn favorites(&self) -> Vec<(String, String)> {
        let providers = std::sync::Arc::clone(&self.0.providers);
        self.blocking(move |_| pairs(&providers.settings().model_favorites()))
            .await
    }

    /// `PUT /api/model-favorites` with the starred models as pairs: answers
    /// them as saved.
    pub async fn set_favorites(
        &self,
        favorites: &[(String, String)],
    ) -> Result<Vec<(String, String)>, Refusal> {
        let keys = Json::Array(
            favorites
                .iter()
                .map(|(provider, model)| Json::String(format!("{provider}::{model}")))
                .collect(),
        );
        Ok(pairs(&self.model_favorites(&keys).await?))
    }

    /// `GET /api/models?provider=`'s `models`.
    pub async fn model_list(
        &self,
        provider: &str,
    ) -> Result<Vec<mcbench_providers::ModelInfo>, Refusal> {
        let key = mcbench_providers::api::js::trim(provider).to_string();
        if !known_provider(&key) {
            return Err(refusal(400, "Unknown provider"));
        }
        self.0
            .providers
            .models(&key)
            .await
            .map_err(|e| refusal(500, e))
    }

    /// `GET /api/models?provider=`: `{ provider, models }`.
    pub async fn models(&self, provider: &str) -> Result<Json, Refusal> {
        let models = self.model_list(provider).await?;
        Ok(mcbench_store::object! {
            "provider" => mcbench_providers::api::js::trim(provider),
            "models" => Json::Array(models.iter().map(|m| m.to_json()).collect()),
        })
    }

    /// `GET /api/model-info?provider=&model=`'s facts, typed.
    pub async fn model_facts(
        &self,
        provider: &str,
        model: &str,
    ) -> Result<mcbench_providers::ModelFacts, Refusal> {
        let key = mcbench_providers::api::js::trim(provider).to_string();
        let model = mcbench_providers::api::js::trim(model).to_string();
        if !known_provider(&key) {
            return Err(refusal(400, "provider query parameter is required"));
        }
        if model.is_empty() {
            return Err(refusal(400, "model query parameter is required"));
        }
        self.0
            .providers
            .model_info(&key, &model)
            .await
            .map_err(|e| refusal(500, e))
    }

    /// `GET /api/model-info?provider=&model=`: the model's facts.
    pub async fn model_info(&self, provider: &str, model: &str) -> Result<Json, Refusal> {
        Ok(self.model_facts(provider, model).await?.to_json())
    }
}

/// `provider::model` keys as pairs, split at the first `::`.
fn pairs(keys: &[String]) -> Vec<(String, String)> {
    keys.iter()
        .map(|key| {
            let (provider, model) = key
                .split_once("::")
                .unwrap_or_else(|| panic!("a starred model's key {key} has no ::"));
            (provider.to_string(), model.to_string())
        })
        .collect()
}
