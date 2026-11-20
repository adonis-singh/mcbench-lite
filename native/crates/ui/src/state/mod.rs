//! The interface's state with no GPUI types: tested on every host without a
//! window. The transcript's items, rows, markdown and selection; the
//! places and their history ([`place`], [`address`]); the layout
//! ([`layout`]); the layers ([`layers`]), the shortcuts ([`keymap`]) and the
//! command panel's highlight ([`command`]); the popovers' and confirm
//! buttons' timing ([`popover`], [`confirm`]); the vendor logos
//! ([`vendor`]); and [`seam`], the one place that reads the core's JSON.

pub mod address;
pub mod code;
pub mod command;
pub mod confirm;
pub mod doc;
pub mod files;
pub mod keymap;
pub mod layers;
pub mod layout;
pub mod place;
pub mod popover;
pub mod rows;
pub mod seam;
pub mod selection;
pub mod transcript;
pub mod vendor;
// chat (stage four's chat agent): the turn's phases, the draft, the
// pickers' values a send uses, attachments, the clipboard's message form,
// branch arrows, the placeholder's ideas, the held send's line.
pub mod branches;
pub mod choice;
pub mod clipboard;
pub mod draft;
pub mod ideas;
pub mod setup;
pub mod turn;
pub mod upload;
// pickers: the composer's pickers and meters.
pub mod favorites;
pub mod meters;
pub mod models;
// (resolution: shared with the sidebar agent, below)
// sidebar agent: the sidebar's rows, filters, archive and the palette.
pub mod archive;
pub mod filters;
pub mod palette;
pub mod resolution;
pub mod sidebar;
// seen: when the window marks a chat seen.
pub mod seen;
// settings agent: the Settings panel and its texts, a provider's sign-in
// rows, its models list, the published builds.
pub mod model_list;
pub mod published;
pub mod settings;
pub mod sign_in;
// bench agent: a bench's details and the prompt library's sync.
pub mod bench;
pub mod bench_details;
pub mod prompt_sync;
pub mod prompts;
// share agent: a subject's share (`web/share.mjs`).
pub mod share;
// quit-ask: asking before a quit while builds run.
pub mod quit;

/// Moves a current index in a list of `count` (Up, Down, Home, End, Page Up,
/// Page Down by `page`): the chat list and the transcript's messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// One back.
    Previous,
    /// One on.
    Next,
    /// The first.
    First,
    /// The last.
    Last,
    /// A page back.
    PageUp(usize),
    /// A page on.
    PageDown(usize),
}

/// The index after `step` from `current` in a list of `count`.
pub fn step(current: Option<usize>, count: usize, step: Step) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let last = count - 1;
    Some(match (current, step) {
        (_, Step::First) => 0,
        (_, Step::Last) => last,
        (None, Step::Previous | Step::PageUp(_)) => last,
        (None, Step::Next | Step::PageDown(_)) => 0,
        (Some(i), Step::Previous) => i.saturating_sub(1),
        (Some(i), Step::Next) => (i + 1).min(last),
        (Some(i), Step::PageUp(page)) => i.saturating_sub(page.max(1)),
        (Some(i), Step::PageDown(page)) => (i + page.max(1)).min(last),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_stay_inside_the_list() {
        assert_eq!(step(Some(0), 3, Step::Previous), Some(0));
        assert_eq!(step(Some(2), 3, Step::Next), Some(2));
        assert_eq!(step(None, 3, Step::Previous), Some(2));
        assert_eq!(step(Some(1), 10, Step::PageDown(4)), Some(5));
        assert_eq!(step(Some(1), 10, Step::Last), Some(9));
        assert_eq!(step(Some(1), 0, Step::Next), None);
    }
}
// pane agent: the 3D pane's pure rules.
pub mod live;
