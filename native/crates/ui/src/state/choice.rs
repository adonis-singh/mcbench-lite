//! Where a message's model, effort and resolution come from (inventory
//! `app.md` 1 `provider`, `model`, `effort`, `resolution`; 6.6; 6.10
//! `choice`): one [`Choice`] with exactly the fields [`crate::Outgoing`]
//! needs, which the composer's pickers change and a send reads.
//!
//! At the app's start the choice is the last model and resolution of the
//! preferences (`mcb-last-model`, `mcb-last-resolution`), with no effort; a
//! new chat with no choice of its own keeps the model and effort of the
//! chat just left, with the last resolution (`showNewChat`;
//! `view::pickers::footer::FooterPickers::show_place`). A chat opened
//! takes its header's values (`setModelChoice(row)`). A draft parked in a
//! Build place keeps its own choice only while it differs from the chat's
//! (`draft.choice ?? row`). A model picked in the composer resets the
//! effort and keeps the resolution, and is remembered as the last model;
//! a resolution chosen is remembered as the last resolution (both saved
//! with the window's preferences). An empty provider and model is no model
//! chosen.

use std::collections::HashMap;

use crate::{ConversationHeader, Preferences, Resolution};

/// A message's choice of model, effort and resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The provider's key; empty when no model is chosen.
    pub provider: String,
    /// The model; empty when none is chosen.
    pub model: String,
    /// The effort chosen; none is the model's default (`''`).
    pub effort: Option<String>,
    /// The resolution.
    pub resolution: Resolution,
}

impl Choice {
    /// A new chat's: the last model and resolution, no effort.
    pub fn new_chat(preferences: &Preferences) -> Choice {
        let (provider, model) = preferences.last_model.clone().unwrap_or_default();
        Choice {
            provider,
            model,
            effort: None,
            resolution: preferences.last_resolution,
        }
    }

    /// An open chat's: its header's values.
    pub fn of_chat(header: &ConversationHeader) -> Choice {
        Choice::of_row(
            &header.provider,
            &header.model,
            header.effort.as_deref(),
            header.resolution,
        )
    }

    /// A chat's from its row's fields (the list's row before its header
    /// arrives, `setModelChoice(row)`).
    pub fn of_row(provider: &str, model: &str, effort: Option<&str>, resolution: u32) -> Choice {
        Choice {
            provider: provider.to_string(),
            model: model.to_string(),
            effort: effort.filter(|e| !e.is_empty()).map(str::to_string),
            resolution: Resolution::of(resolution),
        }
    }

    /// A model is chosen.
    pub fn chosen(&self) -> bool {
        !self.provider.is_empty() && !self.model.is_empty()
    }

    /// A model picked in the composer: the effort resets, the resolution
    /// stays.
    pub fn picked(&self, provider: &str, model: &str) -> Choice {
        Choice {
            provider: provider.to_string(),
            model: model.to_string(),
            effort: None,
            resolution: self.resolution,
        }
    }
}

/// The preferences after a model was picked in the composer
/// (`mcb-last-model`).
pub fn remember_model(preferences: &Preferences, choice: &Choice) -> Preferences {
    Preferences {
        last_model: Some((choice.provider.clone(), choice.model.clone())),
        ..preferences.clone()
    }
}

/// The preferences after a resolution was chosen (`mcb-last-resolution`).
pub fn remember_resolution(preferences: &Preferences, resolution: Resolution) -> Preferences {
    Preferences {
        last_resolution: resolution,
        ..preferences.clone()
    }
}

/// The choices of the drafts parked in places not on screen, by draft key
/// (`chat:new`, `chat:<id>`).
#[derive(Clone, Debug, Default)]
pub struct Parked(HashMap<String, Choice>);

impl Parked {
    /// Leaving the place `key` with `current` on screen: kept when it
    /// differs from the place's own (`own`: the chat's header, or for the
    /// new chat none, which always keeps it).
    pub fn leave(&mut self, key: &str, current: Choice, own: Option<&Choice>) {
        if own == Some(&current) {
            self.0.remove(key);
        } else {
            self.0.insert(key.to_string(), current);
        }
    }

    /// Arriving at `key`: its parked choice, else its own (`own`).
    pub fn arrive(&mut self, key: &str, own: Choice) -> Choice {
        self.0.remove(key).unwrap_or(own)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(
        provider: &str,
        model: &str,
        effort: Option<&str>,
        resolution: u32,
    ) -> ConversationHeader {
        ConversationHeader {
            id: "c".into(),
            title: "t".into(),
            provider: provider.into(),
            model: model.into(),
            effort: effort.map(str::to_string),
            resolution,
            bench: None,
            running_since: None,
            share: None,
        }
    }

    #[test]
    fn a_new_chat_starts_at_the_last_model_and_resolution() {
        let fresh = Choice::new_chat(&Preferences::default());
        assert!(!fresh.chosen());
        assert_eq!(fresh.resolution, Resolution::Standard);
        let kept = Preferences {
            last_model: Some(("anthropic".into(), "claude-opus-5-5".into())),
            last_resolution: Resolution::VeryHigh,
            ..Preferences::default()
        };
        let choice = Choice::new_chat(&kept);
        assert_eq!(
            choice,
            Choice {
                provider: "anthropic".into(),
                model: "claude-opus-5-5".into(),
                effort: None,
                resolution: Resolution::VeryHigh,
            }
        );
    }

    #[test]
    fn an_open_chat_takes_its_header_and_a_pick_resets_the_effort_only() {
        let chat = Choice::of_chat(&header("claude-code", "claude-haiku-5-5", Some("high"), 2));
        assert_eq!(chat.effort.as_deref(), Some("high"));
        assert_eq!(chat.resolution, Resolution::High);
        let picked = chat.picked("anthropic", "claude-opus-5-5");
        assert_eq!(picked.effort, None);
        assert_eq!(picked.resolution, Resolution::High);
        let remembered = remember_model(&Preferences::default(), &picked);
        assert_eq!(
            remembered.last_model,
            Some(("anthropic".into(), "claude-opus-5-5".into()))
        );
        assert_eq!(
            remember_resolution(&remembered, Resolution::Maximum).last_resolution,
            Resolution::Maximum
        );
        assert_eq!(Choice::of_chat(&header("a", "b", Some(""), 1)).effort, None);
    }

    #[test]
    fn a_parked_draft_keeps_its_choice_only_while_it_differs_from_the_chats() {
        let mut parked = Parked::default();
        let own = Choice::of_chat(&header("anthropic", "claude-opus-5-5", None, 1));
        let other = own.picked("claude-code", "claude-haiku-5-5");
        parked.leave("chat:a", other.clone(), Some(&own));
        assert_eq!(parked.arrive("chat:a", own.clone()), other);
        parked.leave("chat:a", own.clone(), Some(&own));
        assert_eq!(parked.arrive("chat:a", own.clone()), own);
        // The new chat has no choice of its own: its draft keeps it.
        parked.leave("chat:new", own.clone(), None);
        let fresh = Choice::new_chat(&Preferences::default());
        assert_eq!(parked.arrive("chat:new", fresh), own);
    }
}
