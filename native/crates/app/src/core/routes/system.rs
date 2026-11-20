//! Setup, activity and the settings (`server/index.js`: `GET /api/setup`,
//! `POST /api/setup/retry`, `GET /api/activity`, `GET /api/config`,
//! `GET` and `PUT /api/settings`).

use mcbench_agent::lifecycle::Refusal;
use mcbench_setup::Step;
use mcbench_store::json::Json;
use mcbench_ui::{SettingsChange, SettingsView};

use crate::core::{Core, refusal, settings};

impl Core {
    /// `GET /api/setup`: whether every step that is not optional is done,
    /// and every step in order.
    pub fn setup_view(&self) -> (bool, Vec<Step>) {
        self.0.steps.view()
    }

    /// `POST /api/setup/retry`: the steps' work again after one failed
    /// (409 when none did); answers the steps as they are then.
    pub fn retry_setup(&self) -> Result<(bool, Vec<Step>), Refusal> {
        self.0.steps.retry().map_err(|e| refusal(409, e))?;
        self.start_setup();
        Ok(self.0.steps.view())
    }

    /// `GET /api/activity`'s `busy`: a turn, an edit or a queued bench
    /// round, or a title under way (`Turns::busy`).
    pub async fn busy(&self) -> Result<bool, Refusal> {
        self.blocking(|core| {
            let store = &core.0.store;
            if core.0.turns.busy() {
                return Ok(true);
            }
            for bench in store.list_benches()? {
                let rounds = bench.rounds as i64;
                for member in store.bench_members(&bench.id)? {
                    let member = member.lock().expect("a chat's lock").id.0.clone();
                    let progress = store.bench_progress(&member)?;
                    if progress.halted.is_none() && progress.round < rounds {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        })
        .await
    }

    /// `GET /api/config`.
    pub async fn config(&self) -> Result<Json, Refusal> {
        self.blocking(|core| settings::config(&core.0.providers).map_err(|e| refusal(500, e)))
            .await
    }

    /// `GET /api/settings` (`refresh=1` reads the CLIs' versions and the
    /// Codex catalogue again).
    pub async fn settings(&self, refresh: bool) -> Result<Json, Refusal> {
        let checked_at = mcbench_store::iso(self.0.store.host().now_ms());
        settings::details(
            &self.0.providers,
            &self.0.sign_ins,
            &self.0.versions,
            refresh,
            checked_at,
        )
        .await
        .map_err(|e| refusal(500, e))
    }

    /// `GET /api/settings` typed for the window (`Command::ReadSettings`):
    /// [`settings::view`] of [`Core::settings`]'s body, read at one time.
    pub async fn settings_view(&self, refresh: bool) -> Result<SettingsView, Refusal> {
        let now = self.0.store.host().now_ms();
        let details = settings::details(
            &self.0.providers,
            &self.0.sign_ins,
            &self.0.versions,
            refresh,
            mcbench_store::iso(now),
        )
        .await
        .map_err(|e| refusal(500, e))?;
        settings::view(
            &details,
            now,
            self.0.providers.settings_file().file().to_path_buf(),
            &self.0.sign_ins,
            &self.0.installs,
        )
        .map_err(|e| refusal(500, e))
    }

    /// `PUT /api/settings` with the window's whole draft
    /// (`Command::SaveSettings`, [`settings::change_body`]).
    pub async fn save_settings(&self, change: &SettingsChange) -> Result<(), Refusal> {
        let body = settings::change_body(change, &self.0.providers.settings());
        self.update_settings(body).await.map(drop)
    }

    /// `PUT /api/settings`: `{ ok: true, ...publicSettings() }`.
    pub async fn update_settings(&self, body: Json) -> Result<Json, Refusal> {
        let json = self
            .blocking(move |core| -> Result<Json, Refusal> {
                let providers = &core.0.providers;
                let settings = providers.settings_file().update(&body)?;
                let Json::Object(public) =
                    settings::public_settings(providers, &settings).map_err(|e| refusal(500, e))?
                else {
                    unreachable!("an object")
                };
                let mut out = mcbench_store::json::Object::new();
                out.insert("ok".into(), true.into());
                out.extend(public);
                Ok(Json::Object(out))
            })
            .await?;
        self.0.versions.clear();
        self.list_changed();
        Ok(json)
    }
}
