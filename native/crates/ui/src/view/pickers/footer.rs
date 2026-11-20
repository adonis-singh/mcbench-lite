//! The composer footer's pickers in Build (`App`'s composer controls,
//! inventory `app.md` 6.6), in the page's order: the limit chip and the
//! context meter (when the latest usage is the chosen model's), the
//! resolution picker, the chat model picker (once the providers are
//! known), and the effort picker (when the model reasons and lists
//! efforts). It holds the message's [`Choice`] and where it comes from
//! (`state::choice`): a send reads [`FooterPickers::choice`] and
//! [`FooterPickers::effort_to_send`].

use std::rc::Rc;

use gpui_kit::*;

use super::catalog::{Catalog, Remember};
use super::effort::effort_picker;
use super::meters::{context_meter, limit_chip};
use super::model::ModelPicker;
use super::resolution::resolution_picker;
use super::{Bounds, bounds};
use crate::state::choice::{Choice, Parked};
use crate::state::meters::{self, Usage};
use crate::state::models::{effort_for, effort_shown};
use crate::state::popover::{EFFORT_EXIT, RESOLUTION_EXIT};
use crate::view::controls::popover::PopoverState;
use crate::{Preferences, Resolution};

/// The draft key of the new chat (`chat:new`).
pub const NEW_CHAT: &str = "chat:new";

/// The footer's pickers and the choice they make.
pub struct FooterPickers {
    catalog: Entity<Catalog>,
    model: Entity<ModelPicker>,
    effort: Entity<PopoverState>,
    resolution: Entity<PopoverState>,
    track: Bounds,
    choice: Choice,
    /// The place's draft key, and its own choice (an open chat's row; none
    /// for the new chat).
    place: (String, Option<Choice>),
    parked: Parked,
    preferences: Preferences,
    usage: Option<Usage>,
    _subscriptions: Vec<Subscription>,
}

impl FooterPickers {
    /// The new chat's pickers, from the stored preferences.
    pub fn new(
        catalog: Entity<Catalog>,
        preferences: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let this = cx.entity().downgrade();
        let model = cx.new(|cx| {
            ModelPicker::new(
                catalog.clone(),
                Rc::new(move |provider, model, _, cx| {
                    let _ = this.update(cx, |f, cx| f.pick_model(provider, model, cx));
                }),
                window,
                cx,
            )
        });
        let observed = cx.observe(&catalog, |_, _, cx| cx.notify());
        let choice = Choice::new_chat(&preferences);
        let mut pickers = FooterPickers {
            catalog,
            model,
            effort: cx.new(|cx| PopoverState::new(EFFORT_EXIT, cx)),
            resolution: cx.new(|cx| PopoverState::new(RESOLUTION_EXIT, cx)),
            track: bounds(),
            choice: choice.clone(),
            place: (NEW_CHAT.to_string(), None),
            parked: Parked::default(),
            preferences,
            usage: None,
            _subscriptions: vec![observed],
        };
        pickers.set_choice(choice, cx);
        pickers
    }

    /// The message's choice.
    pub fn choice(&self) -> &Choice {
        &self.choice
    }

    /// The effort a send carries: `effortFor` when the model's facts are
    /// known, else the choice as it is.
    pub fn effort_to_send(&self, cx: &App) -> Option<String> {
        let facts = self
            .catalog
            .read(cx)
            .facts(&self.choice.provider, &self.choice.model)
            .cloned();
        match facts {
            Some(facts) => effort_for(Some(&facts), self.choice.effort.as_deref()),
            None => self.choice.effort.clone(),
        }
    }

    /// The model picker.
    pub fn model_picker(&self) -> Entity<ModelPicker> {
        self.model.clone()
    }

    /// The effort picker's popover.
    pub fn effort_popover(&self) -> Entity<PopoverState> {
        self.effort.clone()
    }

    /// The resolution picker's popover.
    pub fn resolution_popover(&self) -> Entity<PopoverState> {
        self.resolution.clone()
    }

    fn set_choice(&mut self, choice: Choice, cx: &mut Context<Self>) {
        if choice.chosen() {
            let (p, m) = (choice.provider.clone(), choice.model.clone());
            self.catalog.update(cx, |c, _| c.ask_facts(&p, &m));
        }
        let value = choice
            .chosen()
            .then(|| (choice.provider.clone(), choice.model.clone()));
        self.model.update(cx, |m, cx| m.set_value(value, cx));
        self.choice = choice;
        cx.notify();
    }

    /// The place on screen changed: the chat `chat` with its row's choice
    /// (`own`), or the new chat (none: the model of the place left). The choice left behind is parked
    /// while it differs from its place's own.
    pub fn show_place(&mut self, chat: Option<(String, Choice)>, cx: &mut Context<Self>) {
        let (key, own) = std::mem::take(&mut self.place);
        self.parked.leave(&key, self.choice.clone(), own.as_ref());
        let (key, own, arrived) = match chat {
            Some((id, own)) => {
                let key = format!("chat:{id}");
                let arrived = self.parked.arrive(&key, own.clone());
                (key, Some(own), arrived)
            }
            // A new chat with no choice of its own keeps the model and
            // effort of the chat just left and takes the last resolution
            // chosen (`showNewChat`); at the app's start that is the last
            // model of the preferences (the choice it starts with).
            None => {
                let kept = Choice {
                    resolution: self.preferences.last_resolution,
                    ..self.choice.clone()
                };
                let arrived = self.parked.arrive(NEW_CHAT, kept);
                (NEW_CHAT.to_string(), None, arrived)
            }
        };
        self.place = (key, own);
        self.usage = None;
        self.set_choice(arrived, cx);
    }

    /// The chat's stored messages arrived: the latest usage for the meter
    /// and the chip.
    pub fn transcript(
        &mut self,
        messages: &[mcbench_store::StoredMessage],
        cx: &mut Context<Self>,
    ) {
        self.usage = meters::latest_usage(messages);
        cx.notify();
    }

    /// A model picked in the composer: the effort resets, the resolution
    /// stays, and it is remembered as the last model.
    pub fn pick_model(&mut self, provider: String, model: String, cx: &mut Context<Self>) {
        let choice = self.choice.picked(&provider, &model);
        self.preferences.last_model = Some((provider.clone(), model.clone()));
        let hooks = self.catalog.read(cx).hooks();
        (hooks.remember)(Remember::Model(provider, model), cx);
        self.set_choice(choice, cx);
    }

    /// An effort chosen.
    pub fn pick_effort(&mut self, effort: String, cx: &mut Context<Self>) {
        let mut choice = self.choice.clone();
        choice.effort = Some(effort);
        self.set_choice(choice, cx);
    }

    /// A resolution chosen; remembered as the last resolution.
    pub fn pick_resolution(&mut self, resolution: Resolution, cx: &mut Context<Self>) {
        let mut choice = self.choice.clone();
        choice.resolution = resolution;
        self.preferences.last_resolution = resolution;
        let hooks = self.catalog.read(cx).hooks();
        (hooks.remember)(Remember::Resolution(resolution), cx);
        self.set_choice(choice, cx);
    }
}

impl Render for FooterPickers {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let catalog = self.catalog.read(cx);
        let facts = catalog
            .facts(&self.choice.provider, &self.choice.model)
            .cloned();
        let arrived = catalog.providers_arrived();
        let window_size = facts.as_ref().and_then(|f| f.context_window.flatten());
        let (chip, meter) = meters::shown(
            self.usage.as_ref(),
            &self.choice.provider,
            &self.choice.model,
            window_size,
            meters::clock,
        );
        let this = cx.entity().downgrade();
        let mut row = div().flex().items_center().gap(px(3.)).min_w_0();
        if let Some(chip) = &chip {
            row = row.child(
                div()
                    .flex_none()
                    .child(limit_chip("limit-chip", chip, window, cx)),
            );
        }
        if let Some(meter) = &meter {
            row = row.child(div().flex_none().child(context_meter(
                "context-meter",
                meter,
                window,
                cx,
            )));
        }
        let on_resolution = {
            let this = this.clone();
            Rc::new(move |r: Resolution, _: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |f, cx| f.pick_resolution(r, cx));
            })
        };
        row = row.child(div().flex_none().child(resolution_picker(
            &self.resolution,
            &self.track,
            self.choice.resolution,
            false,
            on_resolution,
            cx,
        )));
        // pane agent (first real look): the model's name is what gives way
        // in a narrow composer, ending in an ellipsis
        // (`.composer-model { min-width: 0; flex: 0 1 auto }`); the other
        // pickers keep their size.
        if arrived {
            row = row.child(div().flex().min_w_0().child(self.model.clone()));
        }
        if arrived && effort_shown(facts.as_ref()) {
            let facts = facts.expect("shown with facts");
            let on_effort = Rc::new(move |e: String, _: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |f, cx| f.pick_effort(e, cx));
            });
            row = row.child(div().flex_none().child(effort_picker(
                &self.effort,
                &facts,
                self.choice.effort.as_deref(),
                on_effort,
                cx,
            )));
        }
        row
    }
}
