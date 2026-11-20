//! The layers that open over the app, topmost first (`web/layers.mjs`,
//! tests ported from `test/layers.test.js`, same cases and names). A modal
//! layer takes the keyboard from the app under it; the viewer filling the
//! window and the chat's Share panel are still the app.
//!
//! Escape closes only the topmost open layer ([`top_layer`]); with none
//! open it cancels an edit. A rebindable shortcut runs only when
//! [`bind_runs`] allows it with the layers open.

use super::keymap::BindAction;

/// A layer, in the order they stack (quit-ask: the quit question over
/// everything; then style.css: `.cmdk-overlay`,
/// `.lightbox`, `.overlay`, the narrow window's list drawer, `.viewer.full`,
/// `.share-panel`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    /// The question before a quit while builds run (quit-ask).
    Quit,
    /// The command panel (Mod+K).
    Command,
    /// The lightbox.
    Lightbox,
    /// Settings.
    Settings,
    /// The narrow window's list drawer.
    Drawer,
    /// The viewer filling the window.
    Viewer,
    /// The Share panel.
    Share,
}

/// Every layer, topmost first, with whether it is modal (`LAYERS`).
pub const LAYERS: [(Layer, bool); 7] = [
    (Layer::Quit, true),
    (Layer::Command, true),
    (Layer::Lightbox, true),
    (Layer::Settings, true),
    (Layer::Drawer, true),
    (Layer::Viewer, false),
    (Layer::Share, false),
];

impl Layer {
    /// Its id in `web/layers.mjs`.
    pub fn id(self) -> &'static str {
        match self {
            Layer::Quit => "quit",
            Layer::Command => "command",
            Layer::Lightbox => "lightbox",
            Layer::Settings => "settings",
            Layer::Drawer => "drawer",
            Layer::Viewer => "viewer",
            Layer::Share => "share",
        }
    }
}

/// Which layers are open.
pub type Open = std::collections::HashSet<Layer>;

/// The topmost open layer, the one Escape closes; none when none is open
/// (`topLayer`).
pub fn top_layer(open: &Open) -> Option<Layer> {
    LAYERS.iter().map(|(l, _)| *l).find(|l| open.contains(l))
}

/// Whether a modal layer is open (`modalOpen`).
pub fn modal_open(open: &Open) -> bool {
    LAYERS.iter().any(|(l, modal)| *modal && open.contains(l))
}

/// Whether a bind's action runs now (`bindRuns`): the search bind opens and
/// closes the command panel, the topmost layer, from anywhere; the sidebar
/// bind closes the drawer it opens; every other bind acts on the app and
/// waits while a modal layer covers it. Under the quit question no bind
/// runs (quit-ask).
pub fn bind_runs(action: BindAction, open: &Open) -> bool {
    if open.contains(&Layer::Quit) {
        return false;
    }
    action == BindAction::Search
        || (action == BindAction::Sidebar && top_layer(open) == Some(Layer::Drawer))
        || !modal_open(open)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(layers: &[Layer]) -> Open {
        layers.iter().copied().collect()
    }

    #[test]
    fn escape_closes_the_topmost_open_layer_in_the_order_they_stack() {
        use Layer::*;
        assert_eq!(
            LAYERS.map(|(l, _)| l.id()),
            [
                "quit", "command", "lightbox", "settings", "drawer", "viewer", "share"
            ]
        );
        // The narrow window's list drawer is under Settings and the search
        // panel opened from it.
        assert_eq!(top_layer(&open(&[Drawer])), Some(Drawer));
        assert_eq!(top_layer(&open(&[Drawer, Settings])), Some(Settings));
        assert_eq!(top_layer(&open(&[])), None);
        assert_eq!(top_layer(&open(&[Viewer])), Some(Viewer));
        assert_eq!(top_layer(&open(&[Viewer, Settings])), Some(Settings));
        assert_eq!(top_layer(&open(&[Settings, Lightbox])), Some(Lightbox));
        // The command panel is over whatever it was opened on.
        assert_eq!(top_layer(&open(&[Settings, Command])), Some(Command));
        assert_eq!(top_layer(&open(&[Viewer, Command])), Some(Command));
        assert_eq!(
            top_layer(&open(&[Command, Lightbox, Settings, Drawer, Viewer, Share])),
            Some(Command)
        );
        // The Share panel sits in the chat header, under the viewer filling
        // the window.
        assert_eq!(top_layer(&open(&[Share])), Some(Share));
        assert_eq!(top_layer(&open(&[Share, Viewer])), Some(Viewer));
        // quit-ask: the quit question is over everything.
        assert_eq!(top_layer(&open(&[Command, Settings, Quit])), Some(Quit));
        // Closing the top one uncovers the next.
        assert_eq!(
            top_layer(&open(&[Lightbox, Settings, Viewer])),
            Some(Lightbox)
        );
    }

    #[test]
    fn binds_that_act_on_the_app_wait_while_a_modal_layer_covers_it() {
        use BindAction as A;
        use Layer::*;
        let actions = [A::Build, A::Bench, A::New, A::Sidebar, A::Select];
        for action in actions {
            assert!(bind_runs(action, &open(&[])));
        }
        for layer in [Command, Lightbox, Settings, Drawer] {
            let o = open(&[layer]);
            assert!(modal_open(&o));
            for action in actions {
                assert_eq!(
                    bind_runs(action, &o),
                    layer == Drawer && action == A::Sidebar,
                    "{action:?} under {layer:?}"
                );
            }
            // The search bind opens the command panel over a layer, and
            // closes it.
            assert!(bind_runs(A::Search, &o));
        }
        // The sidebar bind that opened the drawer closes it; under another
        // layer it waits.
        assert!(bind_runs(A::Sidebar, &open(&[Drawer])));
        assert!(!bind_runs(A::Sidebar, &open(&[Drawer, Settings])));
        // The viewer filling the window is still the app.
        assert!(!modal_open(&open(&[Viewer])));
        for action in actions.into_iter().chain([A::Search]) {
            assert!(bind_runs(action, &open(&[Viewer])));
        }
        // quit-ask: under the quit question no bind runs, search included.
        for action in actions.into_iter().chain([A::Search]) {
            assert!(!bind_runs(action, &open(&[Quit])));
            assert!(!bind_runs(action, &open(&[Quit, Drawer])));
        }
        // So is the Share panel.
        assert!(!modal_open(&open(&[Share])));
        for action in actions.into_iter().chain([A::Search]) {
            assert!(bind_runs(action, &open(&[Share])));
        }
    }
}
