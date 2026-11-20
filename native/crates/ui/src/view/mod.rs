//! The window's views, in GPUI: the shell ([`shell`]: the layout, places,
//! notices, live regions), the sidebar ([`sidebar`]), the chat pane
//! ([`chat`]: transcript and composer) with its markdown ([`markdown`]) and
//! text ([`text_leaf`]), the shared controls ([`controls`]), icons
//! ([`icons`]), the design tokens of `web/style.css` ([`theme`]), the
//! shortcuts and layers wired ([`keys`]), the actions and their keys.

// bench agent: Bench's places and the prompt library.
pub mod bench;
// pane agent: the blocks' icons (selection chips, the pane's markers).
pub mod block_icons;
pub mod chat;
pub mod controls;
pub mod dump;
pub mod fatal;
// linux-start: the font stacks with the app's own last entry.
pub mod fonts;
pub mod icons;
pub mod keys;
pub mod markdown;
pub mod metrics;
// chat-fix: animations for looks rest while inactive or on a software adapter.
pub mod motion;
// pickers: the composer's pickers and meters.
pub mod pickers;
// settings agent: the Settings layer and the setup screen.
pub mod settings;
// share agent: the header's Share button and its panel.
pub mod setup;
pub mod share;
pub mod shell;
pub mod sidebar;
pub mod text_leaf;
pub mod theme;
// pane agent: the 3D pane (the live world beside the chat).
pub mod world;

use std::rc::Rc;

use gpui_kit::*;

/// What a control does when activated.
pub type Press = Rc<dyn Fn(&mut Window, &mut App)>;
/// What a row's part does with its text or address (a link, Copy, a fold's key).
pub type Handler = Rc<dyn Fn(String, &mut Window, &mut App)>;
/// Opens a link: the browser in the app, only the log in the dev binary.
pub type Opener = Rc<dyn Fn(&str, &mut App)>;

pub use theme::colour;

// chat (stage four's chat agent)
/// What a file pick gives: the paths chosen (none when cancelled), or why
/// the platform's dialog could not be shown.
pub type Picked = futures::channel::oneshot::Receiver<Result<Vec<std::path::PathBuf>, String>>;
/// Picks files to attach (`Attach file`).
pub type Picker = Rc<dyn Fn(&mut App) -> Picked>;

/// The platform's file dialog, several files, any type (the page's hidden
/// `<input type="file" multiple>`).
pub fn platform_picker() -> Picker {
    Rc::new(|cx| {
        let (sender, receiver) = futures::channel::oneshot::channel();
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.background_executor()
            .spawn(async move {
                let paths = match prompt.await {
                    Ok(Ok(paths)) => Ok(paths.unwrap_or_default()),
                    Ok(Err(e)) => Err(format!("The file dialog could not open: {e}")),
                    Err(_) => Ok(Vec::new()),
                };
                let _ = sender.send(paths);
            })
            .detach();
        receiver
    })
}

/// Paths given ahead (the dev binary's `--pick`): each pick gives them.
pub fn fixed_picker(paths: Vec<std::path::PathBuf>) -> Picker {
    Rc::new(move |_| {
        let (sender, receiver) = futures::channel::oneshot::channel();
        let _ = sender.send(Ok(paths.clone()));
        receiver
    })
}

/// The focus ring every focus stop draws: 2 px of `--gold`, as today's
/// `.header-action:focus-visible`, shown when focus came from the keyboard.
/// Elements reserve the border so the ring never moves the layout.
pub fn ring<E: Styled + InteractiveElement>(element: E) -> E {
    element
        .border_2()
        .border_color(colour::none())
        .focus_visible(|style| style.border_color(colour::gold()))
}

actions!(
    mcbench,
    [
        /// Quits the app (Cmd-Q, Ctrl+Q, the app menu's `Quit
        /// mcbench-lite`): the person's quit, which asks first while builds
        /// run (quit-ask, `shell::quit`). A quit the code asks for calls
        /// `App::quit`, which never asks.
        Quit,
        /// Minimises the window (Cmd-M).
        Minimize,
        /// Closes the window, which quits (Cmd-W); asks first while builds
        /// run, as [`Quit`] does.
        CloseWindow,
        /// Full screen on and off (Ctrl-Cmd-F).
        ToggleFullScreen,
        /// The current message's next block (Right).
        NextBlock,
        /// The current message's previous block, or the message from its
        /// first block (Left).
        PreviousBlock,
        /// The previous chat or message.
        MovePrevious,
        /// The next chat or message.
        MoveNext,
        /// The first chat or message.
        MoveFirst,
        /// The last chat or message.
        MoveLast,
        /// A page of messages back.
        MovePageUp,
        /// A page of messages on.
        MovePageDown,
        /// Opens the current chat (Enter in the list).
        OpenChat,
        /// Activates a link, a button, a tool row (Enter, Space).
        Activate,
        /// Selects all of the transcript's text.
        SelectAll,
        /// Copies the transcript's selection.
        CopySelection,
        /// The next focus stop (Tab).
        FocusNext,
        /// The previous focus stop (Shift+Tab).
        FocusPrevious,
        /// Escape anywhere: closes the topmost layer, or cancels an edit
        /// (`keys::close_top`).
        CloseLayer,
        /// The previous place (Mod+[ on macOS, Alt+Left elsewhere, the
        /// mouse's back button).
        GoBack,
        /// The next place (Mod+] on macOS, Alt+Right elsewhere, the mouse's
        /// forward button).
        GoForward,
        /// Closes the popover that holds focus (Escape inside it), and goes
        /// no further.
        Dismiss,
    ]
);

/// Sets up the app for the chat screen: the component library, the dark
/// theme, the caret, the keys. `run` and the in-process tests call it.
pub fn setup(cx: &mut App) {
    // The fonts first: the component library's theme looks up
    // `.SystemUIFont` as it starts.
    let fonts = fonts::install(cx);
    gpui_kit::init(cx);
    gpui_kit::component::Theme::change(gpui_kit::component::ThemeMode::Dark, None, cx);
    // The caret blinks for 10 s after the last key, pointer press or focus
    // change, then stays lit (GTK's gtk-cursor-blink-timeout), so a window
    // nobody touches draws nothing.
    gpui_kit::component::Theme::update(cx, |theme| {
        // The page's stacks as this platform resolves them (theme.rs).
        theme.font_family = fonts.ui.clone();
        theme.mono_font_family = fonts.data.clone();
        theme.cursor_blink = gpui_kit::component::input::CursorBlink::default()
            .with_idle_timeout(std::time::Duration::from_secs(10));
    });
    bind_keys(cx);
    keys::install(cx);
    world::set_locker(cx, world::platform_locker());
}

/// Sets the theme (the stored preference; the page's `mcb-theme`) and
/// redraws: the tokens of [`theme`] and gpui-kit's own components.
pub fn set_theme(mode: theme::Mode, cx: &mut App) {
    theme::set_mode(mode);
    let kit = match mode {
        theme::Mode::Dark => gpui_kit::component::ThemeMode::Dark,
        theme::Mode::Light => gpui_kit::component::ThemeMode::Light,
    };
    gpui_kit::component::Theme::change(kit, None, cx);
    cx.refresh_windows();
}

/// The keys of the chat screen.
pub fn bind_keys(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let (quit, select_all, copy) = ("cmd-q", "cmd-a", "cmd-c");
    #[cfg(not(target_os = "macos"))]
    let (quit, select_all, copy) = ("ctrl-q", "ctrl-a", "ctrl-c");
    let mut bindings = vec![
        KeyBinding::new(quit, Quit, None),
        KeyBinding::new("tab", FocusNext, Some("Screen")),
        KeyBinding::new("shift-tab", FocusPrevious, Some("Screen")),
        KeyBinding::new("up", MovePrevious, Some("ChatList")),
        KeyBinding::new("down", MoveNext, Some("ChatList")),
        KeyBinding::new("home", MoveFirst, Some("ChatList")),
        KeyBinding::new("end", MoveLast, Some("ChatList")),
        KeyBinding::new("enter", OpenChat, Some("ChatList")),
        KeyBinding::new("up", MovePrevious, Some("Transcript")),
        KeyBinding::new("down", MoveNext, Some("Transcript")),
        KeyBinding::new("right", NextBlock, Some("Transcript")),
        KeyBinding::new("left", PreviousBlock, Some("Transcript")),
        KeyBinding::new("home", MoveFirst, Some("Transcript")),
        KeyBinding::new("end", MoveLast, Some("Transcript")),
        KeyBinding::new("pageup", MovePageUp, Some("Transcript")),
        KeyBinding::new("pagedown", MovePageDown, Some("Transcript")),
        KeyBinding::new(select_all, SelectAll, Some("Transcript")),
        KeyBinding::new(copy, CopySelection, Some("Transcript")),
        KeyBinding::new(select_all, SelectAll, Some("Fatal")),
        KeyBinding::new(copy, CopySelection, Some("Fatal")),
        // chat-fix: a selectable failure line (`controls::selectable`).
        KeyBinding::new(select_all, SelectAll, Some("PickerNote")),
        KeyBinding::new(copy, CopySelection, Some("PickerNote")),
        KeyBinding::new("escape", Dismiss, Some("Popover")),
        KeyBinding::new("up", MovePrevious, Some("Menu")),
        KeyBinding::new("down", MoveNext, Some("Menu")),
        KeyBinding::new("home", MoveFirst, Some("Menu")),
        KeyBinding::new("end", MoveLast, Some("Menu")),
        KeyBinding::new("enter", Activate, Some("Part")),
        KeyBinding::new("space", Activate, Some("Part")),
    ];
    if cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("cmd-[", GoBack, Some("Screen")));
        bindings.push(KeyBinding::new("cmd-]", GoForward, Some("Screen")));
    } else {
        bindings.push(KeyBinding::new("alt-left", GoBack, Some("Screen")));
        bindings.push(KeyBinding::new("alt-right", GoForward, Some("Screen")));
    }
    if cfg!(target_os = "macos") {
        bindings.push(KeyBinding::new("cmd-m", Minimize, None));
        bindings.push(KeyBinding::new("cmd-w", CloseWindow, None));
        bindings.push(KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None));
    }
    cx.bind_keys(bindings);
    // sidebar agent: the search fields' and the palette's keys.
    sidebar::bind_keys(cx);
    // settings agent: Settings' navigation and grips.
    settings::bind_keys(cx);
    // quit-ask: the shell's screen handles `Quit` first (it asks while
    // builds run); this quits where no shell is (the gallery, the error
    // screen).
    cx.on_action(|_: &Quit, cx| cx.quit());
}

/// The application menu on macOS: Quit, and the Window menu.
pub fn set_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "mcbench-lite".into(),
            items: vec![MenuItem::action("Quit mcbench-lite", Quit)],
            disabled: false,
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Close", CloseWindow),
                MenuItem::separator(),
                MenuItem::action("Enter Full Screen", ToggleFullScreen),
            ],
            disabled: false,
        },
    ]);
}
