//! The rebindable shortcuts (`web/app.js` `MAC`, `BIND_ACTIONS`,
//! `DEFAULT_BINDS`, `comboOf`, `formatBind`, lines 2619-2646; inventory
//! `components.md` K.3 and `app.md` 11). The keymap is data: a map from
//! action to combination that Settings rebinds later (stored by the core as
//! a preference, the page's `mcb-binds`), merged over the defaults.
//!
//! A combination is written as the page writes it: the parts present in the
//! order `Mod`, `Ctrl` (macOS only), `Alt`, `Shift`, then the key, joined by
//! `+` (`Mod+Shift+O`). Mod is Cmd on macOS and Ctrl elsewhere. A
//! combination needs Mod, Alt, or (on macOS) Ctrl, or is an F key, so
//! typing never triggers one.
//!
//! The page reads letters and digits from the physical key (`KeyA`,
//! `Digit1`), so a layout or Shift does not change them. GPUI gives the key
//! as its ASCII equivalent with Shift taken off (`Keystroke::key`), which is
//! the same for letters and digits on QWERTY-like layouts; on a layout whose
//! digit row needs Shift for digits (AZERTY) GPUI reports the unshifted
//! character, so Mod+1 there is Mod+& (a stated limit: GPUI exposes no scan
//! code).

use std::collections::BTreeMap;

/// A rebindable action (`BIND_ACTIONS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BindAction {
    /// `build`: switch to Build.
    Build,
    /// `bench`: switch to Bench.
    Bench,
    /// `search`: the command panel, open or closed.
    Search,
    /// `new`: a new chat (Build) or bench (Bench).
    New,
    /// `sidebar`: show or hide the sidebar (the drawer when narrow).
    Sidebar,
    /// `select`: the viewer's selection tool.
    Select,
}

impl BindAction {
    /// Every action, in the page's order.
    pub const ALL: [BindAction; 6] = [
        BindAction::Build,
        BindAction::Bench,
        BindAction::Search,
        BindAction::New,
        BindAction::Sidebar,
        BindAction::Select,
    ];

    /// Its id (`mcb-binds`' keys).
    pub fn id(self) -> &'static str {
        match self {
            BindAction::Build => "build",
            BindAction::Bench => "bench",
            BindAction::Search => "search",
            BindAction::New => "new",
            BindAction::Sidebar => "sidebar",
            BindAction::Select => "select",
        }
    }

    /// Its label in Settings and the palette.
    pub fn label(self) -> &'static str {
        match self {
            BindAction::Build => "Switch to Build",
            BindAction::Bench => "Switch to Bench",
            BindAction::Search => "Search everything",
            BindAction::New => "New chat or bench",
            BindAction::Sidebar => "Show or hide the sidebar",
            BindAction::Select => "Select blocks in the viewer",
        }
    }

    /// Its default combination (`DEFAULT_BINDS`).
    pub fn default_bind(self) -> &'static str {
        match self {
            BindAction::Build => "Mod+1",
            BindAction::Bench => "Mod+2",
            BindAction::Search => "Mod+K",
            BindAction::New => "Mod+Shift+O",
            BindAction::Sidebar => "Mod+B",
            BindAction::Select => "Alt+S",
        }
    }

    /// The action with this id.
    pub fn from_id(id: &str) -> Option<BindAction> {
        BindAction::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// The binds in force: each action's combination (empty is not set).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keymap(pub BTreeMap<BindAction, String>);

impl Default for Keymap {
    fn default() -> Self {
        Keymap(
            BindAction::ALL
                .into_iter()
                .map(|a| (a, a.default_bind().to_string()))
                .collect(),
        )
    }
}

impl Keymap {
    /// The stored binds merged over the defaults (`{ ...DEFAULT_BINDS,
    /// ...stored }`); an id that names no action is an error.
    pub fn merged(stored: &BTreeMap<String, String>) -> Keymap {
        let mut map = Keymap::default();
        for (id, combo) in stored {
            let action = BindAction::from_id(id)
                .unwrap_or_else(|| panic!("a stored shortcut for no action: {id}"));
            map.0.insert(action, combo.clone());
        }
        map
    }

    /// The action bound to `combo`, if any (the first, in the page's order,
    /// when two share it).
    pub fn action(&self, combo: &str) -> Option<BindAction> {
        BindAction::ALL
            .into_iter()
            .find(|a| self.0.get(a).is_some_and(|c| c == combo))
    }

    /// An action's combination (empty when not set).
    pub fn bind(&self, action: BindAction) -> &str {
        self.0.get(&action).map(String::as_str).unwrap_or("")
    }

    /// Whether any bind differs from its default (Settings' `Reset all`).
    pub fn changed(&self) -> bool {
        *self != Keymap::default()
    }
}

/// A key press as GPUI gives it: the modifiers and the key's name.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Press {
    /// Cmd (macOS) or the Windows key.
    pub platform: bool,
    /// Control.
    pub control: bool,
    /// Alt or Option.
    pub alt: bool,
    /// Shift.
    pub shift: bool,
    /// GPUI's key name: `a`, `1`, `up`, `enter`, `f5`, `,`.
    pub key: String,
}

/// The combination of a key press (`comboOf`), or none: a bare modifier, a
/// key with no modifier that is not an F key. `mac` says whether Mod is Cmd.
pub fn combo_of(press: &Press, mac: bool) -> Option<String> {
    let key = page_key(&press.key)?;
    let (modifier, ctrl) = if mac {
        (press.platform, press.control)
    } else {
        (press.control, false)
    };
    let f_key =
        key.len() > 1 && key.starts_with('F') && key[1..].chars().all(|c| c.is_ascii_digit());
    if !(modifier || press.alt || ctrl || f_key) {
        return None;
    }
    let mut parts = Vec::new();
    if modifier {
        parts.push("Mod".to_string());
    }
    if ctrl {
        parts.push("Ctrl".to_string());
    }
    if press.alt {
        parts.push("Alt".to_string());
    }
    if press.shift {
        parts.push("Shift".to_string());
    }
    parts.push(key);
    Some(parts.join("+"))
}

/// GPUI's key name as the page's: a letter uppercased, a digit, a named key
/// as the browser names it (`ArrowUp`, `Enter`, `F5`), another single
/// character uppercased; none for a bare modifier.
fn page_key(key: &str) -> Option<String> {
    Some(match key {
        "" | "shift" | "control" | "alt" | "platform" | "cmd" | "fn" | "capslock" => return None,
        "up" => "ArrowUp".into(),
        "down" => "ArrowDown".into(),
        "left" => "ArrowLeft".into(),
        "right" => "ArrowRight".into(),
        "enter" => "Enter".into(),
        "escape" => "Escape".into(),
        "tab" => "Tab".into(),
        "space" => " ".into(),
        "backspace" => "Backspace".into(),
        "delete" => "Delete".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "insert" => "Insert".into(),
        k if k.len() > 1 && k.starts_with('f') && k[1..].chars().all(|c| c.is_ascii_digit()) => {
            k.to_uppercase()
        }
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => k.to_string(),
    })
}

/// A combination as Settings shows it (`formatBind`): `Not set` for none;
/// on macOS the symbols with no separators (`⌘⇧O`); elsewhere `Ctrl`, `Alt`,
/// `Shift` joined with `+` (`Ctrl+Shift+O`).
pub fn format_bind(combo: &str, mac: bool) -> String {
    if combo.is_empty() {
        return "Not set".into();
    }
    let parts: Vec<String> = combo
        .split('+')
        .map(|part| match (part, mac) {
            ("Mod", true) => "⌘".into(),
            ("Ctrl", true) => "⌃".into(),
            ("Alt", true) => "⌥".into(),
            ("Shift", true) => "⇧".into(),
            ("Mod", false) => "Ctrl".into(),
            (other, _) => other.to_string(),
        })
        .collect();
    parts.join(if mac { "" } else { "+" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(platform: bool, control: bool, alt: bool, shift: bool, key: &str) -> Press {
        Press {
            platform,
            control,
            alt,
            shift,
            key: key.into(),
        }
    }

    #[test]
    fn the_defaults_are_the_pages() {
        let map = Keymap::default();
        assert_eq!(map.bind(BindAction::Build), "Mod+1");
        assert_eq!(map.bind(BindAction::Bench), "Mod+2");
        assert_eq!(map.bind(BindAction::Search), "Mod+K");
        assert_eq!(map.bind(BindAction::New), "Mod+Shift+O");
        assert_eq!(map.bind(BindAction::Sidebar), "Mod+B");
        assert_eq!(map.bind(BindAction::Select), "Alt+S");
        assert!(!map.changed());
    }

    #[test]
    fn a_press_becomes_the_pages_combination() {
        // Cmd on a Mac, Ctrl elsewhere, is Mod.
        assert_eq!(
            combo_of(&press(true, false, false, false, "k"), true).as_deref(),
            Some("Mod+K")
        );
        assert_eq!(
            combo_of(&press(false, true, false, false, "k"), false).as_deref(),
            Some("Mod+K")
        );
        assert_eq!(
            combo_of(&press(true, false, false, true, "o"), true).as_deref(),
            Some("Mod+Shift+O")
        );
        assert_eq!(
            combo_of(&press(false, false, true, false, "s"), false).as_deref(),
            Some("Alt+S")
        );
        // Ctrl on a Mac is its own part, after Mod.
        assert_eq!(
            combo_of(&press(true, true, false, false, "1"), true).as_deref(),
            Some("Mod+Ctrl+1")
        );
        // Typing is never a combination; an F key alone is.
        assert_eq!(
            combo_of(&press(false, false, false, true, "k"), false),
            None
        );
        assert_eq!(
            combo_of(&press(false, false, false, false, "f5"), false).as_deref(),
            Some("F5")
        );
        assert_eq!(combo_of(&press(true, false, false, false, ""), true), None);
        assert_eq!(
            combo_of(&press(true, false, false, false, "up"), true).as_deref(),
            Some("Mod+ArrowUp")
        );
    }

    #[test]
    fn a_combination_shows_as_the_platform_writes_it() {
        assert_eq!(format_bind("Mod+Shift+O", true), "⌘⇧O");
        assert_eq!(format_bind("Mod+Shift+O", false), "Ctrl+Shift+O");
        assert_eq!(format_bind("Alt+S", true), "⌥S");
        assert_eq!(format_bind("", false), "Not set");
    }

    #[test]
    fn stored_binds_merge_over_the_defaults() {
        let stored: BTreeMap<String, String> = [("search".to_string(), "Mod+P".to_string())].into();
        let map = Keymap::merged(&stored);
        assert_eq!(map.action("Mod+P"), Some(BindAction::Search));
        assert_eq!(map.action("Mod+K"), None);
        assert_eq!(map.action("Mod+B"), Some(BindAction::Sidebar));
        assert!(map.changed());
    }
}
