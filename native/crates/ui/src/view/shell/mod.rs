//! The window: today's whole page around the chat screen (inventory
//! `app.md` 3, `style-viewer-routes.md` 1.4). The shell owns the layout
//! (the sidebar, the chat column with its pane header, the divider, the
//! world pane), the place on screen and its history, the lists the core
//! publishes, the window's notices and live regions, and the fatal screen.
//!
//! | Part | Where |
//! |---|---|
//! | the sidebar and its sections | `view::sidebar` (a section per file) |
//! | the pane header's places for controls | [`header`] |
//! | the chat pane (transcript, composer) | `view::chat` |
//! | the world pane (the 3D view's place; empty until its milestone) | [`panes::WorldPane`] |
//! | a place not built yet | [`panes::placeholder`] |
//! | the lists | [`lists::Lists`] |
//! | the live regions | [`announce::Announcer`] |
//!
//! A navigation is [`Shell::go`] (a place the user opens: a new history
//! entry) or [`Shell::visit`] with [`Visit::Replace`]. The chat pane and the
//! sidebar are cached views: dragging the sidebar's edge or the divider, or
//! resizing the window, redraws the shell, and the transcript lays out only
//! the rows it shows.

pub mod announce;
// bench agent: what Bench's places ask of the shell.
mod bench;
pub mod header;
pub mod lists;
pub mod panes;
// quit-ask: the question before a quit while builds run.
mod quit;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::chat::{ChatOptions, ChatPane};
use super::controls::pointer_tip;
use super::sidebar::Sidebar;
use super::theme::{colour, shadow};
use super::{CloseLayer, FocusNext, FocusPrevious, Minimize, ToggleFullScreen, keys};
use crate::state::address::ListedChat;
use crate::state::keymap::BindAction;
use crate::state::layers::Layer;
use crate::state::layout::{self, Layout, SidebarDrag};
use crate::state::place::{self, History, Mode, Place, Visit};
use crate::{ActionLog, Core, Update};
use announce::Announcer;
use lists::Lists;

/// What a shortcut does to the shell.
type ShellAction = fn(&mut Shell, &mut Window, &mut Context<Shell>);

/// The app's name: the window's title is `<place> · mcbench-lite`.
pub const APP_NAME: &str = "mcbench-lite";

/// What the shell needs from outside.
pub struct ShellOptions {
    /// The core.
    pub core: Arc<dyn Core>,
    /// Where user-visible actions are recorded, when given.
    pub log: Option<ActionLog>,
    /// Opens a link (the browser in the app; only the log in the dev binary).
    pub opener: super::Opener,
    /// Where frames are counted and timed.
    pub metrics: Rc<RefCell<super::metrics::Metrics>>,
    /// The place to open once the first list arrives (the landing when
    /// none).
    pub start: Option<Place>,
    /// The stored preferences (`Core::preferences`): the layout, the
    /// landing's mode; saved back whole when one changes.
    pub preferences: crate::Preferences,
    // chat (stage four's chat agent)
    /// Picks files to attach.
    pub picker: super::Picker,
    /// Selections the new chat's draft starts with (the stand-in's).
    pub selections: Vec<crate::Selection>,
}

/// The window's root view.
pub struct Shell {
    options: Rc<ShellOptions>,
    lists: Entity<Lists>,
    announcer: Entity<Announcer>,
    sidebar: Entity<Sidebar>,
    chat: Entity<ChatPane>,
    // pane agent: the 3D pane (`view::world`).
    world: Entity<super::world::Pane>,
    place: Place,
    /// The place's mode.
    mode: Mode,
    history: History,
    /// The last place of each mode (`leftRef`).
    left: HashMap<Mode, Place>,
    layout: Layout,
    sidebar_drag: Option<SidebarDrag>,
    chat_drag: bool,
    notices: Vec<String>,
    fatal: Option<Entity<super::fatal::FatalScreen>>,
    started: bool,
    // start-place agent: a chat, bench or member to start at, opened before
    // the first lists arrive; they then decide whether it is there.
    pending_start: Option<Place>,
    /// The start place is being opened (its visit is not the person's).
    opening_start: bool,
    /// The person went somewhere before the first lists arrived: they stay
    /// there (the page's `navigationRef`).
    moved_early: bool,
    // settings agent: the Settings layer (it owns the `settings` layer of
    // `view::keys` while its panel is open).
    settings: Entity<super::settings::SettingsLayer>,
    renders: usize,
    preferences: crate::Preferences,
    /// Bumped by each window move or resize; a save waits for it to settle.
    moves: u64,
    // sidebar agent: the running dots the pulse layer draws.
    pulses: super::sidebar::dot::Pulses,
    /// Where the sidebar held focus at the last render (chat-fix, W1).
    sidebar_focus: Option<super::sidebar::archived::SidebarFocus>,
    // bench agent: Bench's places and the prompt library.
    bench_pane: Entity<super::bench::BenchPane>,
    library: Entity<super::bench::library::Library>,
    prompt_editor: Entity<super::bench::prompts::PromptEditor>,
    // narrow agent: the narrow window (980 px and below) and its drawer.
    narrow: Narrow,
    // quit-ask: the quit question, while it is up, and its focus stops.
    asking: Option<quit::Asking>,
    quit_focus: quit::QuitFocus,
    _subscriptions: Vec<Subscription>,
}

// narrow agent
/// The narrow window's state in the shell.
struct Narrow {
    /// The window's width at the last render.
    width: f32,
    /// The list drawer, asked open or not.
    drawer: crate::state::layout::Drawer,
    /// Whether the drawer was shown at the last render (its layer open).
    shown: bool,
    /// What had focus when the drawer opened: it gets focus back when the
    /// drawer closes.
    opener: Option<FocusHandle>,
    /// The drawer's region, to keep Tab inside it.
    region: FocusHandle,
    /// The sidebar's own button (`.sidebar-toggle`), one handle in the
    /// bar, the drawer and the wide sidebar.
    toggle: FocusHandle,
}

impl Shell {
    /// The window, empty until the core publishes.
    pub fn new(options: ShellOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // pane agent: the core the blocks' icons are asked of.
        super::block_icons::install(options.core.clone(), cx);
        let lists = cx.new(|_| Lists::default());
        let announcer = cx.new(|_| Announcer::default());
        // pickers: the window's catalogue of providers, models and
        // favourites, and what the pickers' actions do in the shell.
        {
            let (settings, bench, remember, notice, asked) = (
                cx.entity().downgrade(),
                cx.entity().downgrade(),
                cx.entity().downgrade(),
                cx.entity().downgrade(),
                cx.entity().downgrade(),
            );
            super::pickers::catalog::Catalog::install(
                options.core.clone(),
                super::pickers::catalog::Hooks {
                    // settings agent: `onSetup('connect')`.
                    open_settings: Rc::new(move |window, cx| {
                        let _ = settings.update(cx, |s, cx| {
                            s.open_settings_on(
                                Some(crate::state::settings::Pane::Connect),
                                None,
                                window,
                                cx,
                            )
                        });
                    }),
                    // par-bench: `Compare several in Bench` is `benchFromBuild`.
                    compare_in_bench: Rc::new(move |window, cx| {
                        let _ = bench.update(cx, |s, cx| s.bench_from_build(window, cx));
                    }),
                    remember: Rc::new(move |choice, cx| {
                        let _ = remember.update(cx, |s, _| s.remember_choice(choice));
                    }),
                    notice: Rc::new(move |text, cx| {
                        let _ = notice.update(cx, |s, cx| s.notice(text, cx));
                    }),
                    // chat: the picker that opened by itself after Settings
                    // closed hands focus to the composer once a model is
                    // chosen, the next frame.
                    chosen_after_ask: Rc::new(move |window, cx| {
                        let shell = asked.clone();
                        window.defer(cx, move |window, cx| {
                            let _ = shell.update(cx, |s, cx| {
                                s.chat.update(cx, |c, cx| c.focus_composer(window, cx))
                            });
                        });
                    }),
                },
                cx,
            );
        }
        // chat (stage four's chat agent): the chat pane's notices, its
        // links to Settings and to Bench, its picker and starting choice.
        let shell_for_chat = cx.entity().downgrade();
        let (notice_shell, connect_shell, bench_shell) = (
            shell_for_chat.clone(),
            shell_for_chat.clone(),
            shell_for_chat,
        );
        let chat_options = ChatOptions {
            core: options.core.clone(),
            log: options.log.clone(),
            opener: options.opener.clone(),
            lists: lists.clone(),
            announcer: announcer.clone(),
            notice: Rc::new(move |text, _, cx| {
                let shell = notice_shell.clone();
                cx.defer(move |cx| {
                    let _ = shell.update(cx, |s, cx| s.notice(text, cx));
                });
            }),
            picker: options.picker.clone(),
            preferences: options.preferences.clone(),
            connect: Rc::new(move |window, cx| {
                let shell = connect_shell.clone();
                window.defer(cx, move |window, cx| {
                    // settings agent: `connectAI` opens Connect an AI.
                    let _ = shell.update(cx, |s, cx| {
                        s.open_settings_on(
                            Some(crate::state::settings::Pane::Connect),
                            None,
                            window,
                            cx,
                        )
                    });
                });
            }),
            // par-bench: the empty chat's `Bench` link is `benchFromBuild`.
            to_bench: Rc::new(move |window, cx| {
                let shell = bench_shell.clone();
                window.defer(cx, move |window, cx| {
                    let _ = shell.update(cx, |s, cx| s.bench_from_build(window, cx));
                });
            }),
            selections: options.selections.clone(),
            // bench agent: the new bench's Build link, its line-up kept, and
            // the comparison a new bench opens.
            to_build: {
                let shell = cx.entity().downgrade();
                Rc::new(move |window, cx| {
                    let shell = shell.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = shell.update(cx, |s, cx| s.build_from_bench(window, cx));
                    });
                })
            },
            keep_bench: {
                let shell = cx.entity().downgrade();
                Rc::new(move |line, blind, resolution, cx| {
                    let shell = shell.clone();
                    cx.defer(move |cx| {
                        let _ = shell.update(cx, |s, _| s.keep_bench(line, blind, resolution));
                    });
                })
            },
            bench_opened: {
                let shell = cx.entity().downgrade();
                Rc::new(move |bench, window, cx| {
                    let shell = shell.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = shell.update(cx, |s, cx| {
                            s.sidebar.update(cx, |b, cx| b.open_bench_fold(&bench, cx));
                            s.go(Place::Compare { bench }, window, cx)
                        });
                    });
                })
            },
            edit_prompt: {
                let shell = cx.entity().downgrade();
                Rc::new(move |id, window, cx| {
                    let shell = shell.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = shell.update(cx, |s, cx| s.open_prompt(id, window, cx));
                    });
                })
            },
            open_prompts: {
                let shell = cx.entity().downgrade();
                Rc::new(move |window, cx| {
                    let shell = shell.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = shell.update(cx, |s, cx| {
                            let id = s.library.read(cx).active().map(str::to_string);
                            if let Some(id) = id {
                                s.open_prompt(id, window, cx);
                            }
                        });
                    });
                })
            },
            // seen: the sidebar marks a chat whose turn ended seen.
            turn_ended: {
                let shell = cx.entity().downgrade();
                Rc::new(move |chat, window, cx| {
                    let shell = shell.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = shell.update(cx, |s, cx| {
                            s.sidebar.update(cx, |b, cx| b.turn_ended(chat, window, cx))
                        });
                    });
                })
            },
        };
        let chat = cx.new(|cx| ChatPane::new(chat_options, window, cx));
        // par-shell (finding 13): the window opens ready to type in the
        // interim chat, before the first lists name the start place (W1:
        // focus is never on nothing; a draft whose selection chips load
        // their block icons drew the window three times before the lists
        // came, with focus on nothing).
        chat.update(cx, |c, cx| c.focus_composer(window, cx));
        // par-shell: where the error screen opens after a panic.
        {
            let bounds = window.bounds();
            crate::crash::remember_window([
                f32::from(bounds.origin.x),
                f32::from(bounds.origin.y),
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ]);
        }
        let shell = cx.entity().downgrade();
        let sidebar = cx.new(|cx| Sidebar::new(shell, lists.clone(), cx));
        // narrow agent: the sidebar's button, one handle wherever it is drawn.
        let toggle_focus = cx.focus_handle();
        // sidebar agent: the sidebar sends to the core, logs, and announces.
        // sidebar agent: and records its running dots for the pulse layer.
        let pulses = super::sidebar::dot::Pulses::default();
        sidebar.update(cx, |s, cx| {
            s.attach(
                options.core.clone(),
                options.log.clone(),
                announcer.clone(),
                pulses.clone(),
                &options.preferences,
                cx,
            )
        });
        // pane agent: the 3D pane, its night kept in the preferences.
        let world = {
            let (core, log, night) = (
                options.core.clone(),
                options.log.clone(),
                options.preferences.night,
            );
            let announcer = announcer.clone();
            cx.new(|cx| super::world::Pane::new(core, log, announcer, night, window, cx))
        };
        super::world::install_hook(window.window_handle(), cx.entity().downgrade(), cx);
        {
            let this = cx.entity().downgrade();
            world.update(cx, |pane, _| {
                pane.on_night = Some(Rc::new(move |night, cx| {
                    let _ = this.update(cx, |shell, _| {
                        shell.preferences.night = night;
                        shell.save();
                    });
                }));
            });
        }
        let this = cx.entity().downgrade();
        let binds: [(BindAction, ShellAction); 5] = [
            (BindAction::Sidebar, |s, w, cx| s.toggle_sidebar(w, cx)),
            (BindAction::Build, |s, w, cx| {
                s.switch_mode(Mode::Build, w, cx)
            }),
            (BindAction::Bench, |s, w, cx| {
                s.switch_mode(Mode::Bench, w, cx)
            }),
            (BindAction::New, |s, w, cx| {
                let place = match s.mode() {
                    Mode::Build => Place::NewChat { id: None },
                    Mode::Bench => Place::NewBench,
                };
                // par-chat: New chat's model check.
                s.new_here(place, w, cx)
            }),
            // pane agent: the selection tool on or off in the shown viewer
            // (`select: () => setSelectToggle(..)`, the bridge's
            // `toggle-select`, which only a selectable chat heeds).
            (BindAction::Select, |s, _, cx| {
                s.world.update(cx, |pane, cx| pane.toggle_select(cx))
            }),
        ];
        for (action, run) in binds {
            let this = this.clone();
            keys::on_bind(cx, action, move |window, cx| {
                let _ = this.update(cx, |shell, cx| run(shell, window, cx));
            });
        }
        let moved =
            cx.observe_window_bounds(window, |this, window, cx| this.window_moved(window, cx));
        let preferences = options.preferences.clone();
        let layout = Layout {
            sidebar_open: preferences.sidebar_open,
            sidebar_width: preferences.sidebar_width,
            chat_width: preferences.chat_width,
        };
        let lost = cx.on_focus_lost(window, |this: &mut Self, window, cx| {
            this.focus_lost(window, cx)
        });
        // settings agent: the Settings layer and what it asks of the shell.
        let settings = {
            let shell = cx.entity().downgrade();
            let (notice, closed, theme, binds, go_to) = (
                shell.clone(),
                shell.clone(),
                shell.clone(),
                shell.clone(),
                shell.clone(),
            );
            let hooks = super::settings::Hooks {
                notice: Rc::new(move |text, cx| {
                    let shell = notice.clone();
                    cx.defer(move |cx| {
                        let _ = shell.update(cx, |s, cx| s.notice(text, cx));
                    });
                }),
                closed: Rc::new(move |window, cx| {
                    let _ = closed.update(cx, |s, cx| s.settings_closed(window, cx));
                }),
                theme: Rc::new(move |light, cx| {
                    let _ = theme.update(cx, |s, cx| s.set_light(light, cx));
                }),
                binds: Rc::new(move |keymap, cx| {
                    let _ = binds.update(cx, |s, cx| s.set_binds(keymap, cx));
                }),
                go_to: Rc::new(move |subject, window, cx| {
                    let _ = go_to.update(cx, |s, cx| s.go_to_published(subject, window, cx));
                }),
                light: Rc::new(|_| super::theme::mode() == super::theme::Mode::Light),
            };
            let (core, log, opener) = (
                options.core.clone(),
                options.log.clone(),
                options.opener.clone(),
            );
            let announcer = announcer.clone();
            cx.new(|cx| {
                super::settings::SettingsLayer::new(core, log, opener, announcer, hooks, cx)
            })
        };
        // chat-fix (W6): the pulse layer rests or moves with the window's
        // activity: draw again when it changes.
        let activation = cx.observe_window_activation(window, |_: &mut Self, _, cx| cx.notify());
        // seen: the open chat or comparison is looked at again when the
        // window becomes visible.
        let visibility = cx.observe_window_visibility(window, |s: &mut Self, _, window, cx| {
            s.sidebar
                .update(cx, |b, cx| b.visibility_changed(window, cx))
        });
        // bench agent: Bench's places and the library.
        let (bench_pane, library, prompt_editor) = super::bench::install(
            &options,
            &lists,
            &announcer,
            &chat,
            &sidebar,
            cx.entity().downgrade(),
            window,
            cx,
        );
        // quit-ask: the window's close request asks while builds run.
        Self::ask_on_close(window, cx);
        Shell {
            asking: None,
            quit_focus: quit::QuitFocus::new(cx),
            bench_pane,
            library,
            prompt_editor,
            options: Rc::new(options),
            lists,
            announcer,
            sidebar,
            chat,
            world,
            place: Place::NewChat { id: None },
            mode: Mode::Build,
            history: History::default(),
            left: HashMap::new(),
            layout,
            sidebar_drag: None,
            chat_drag: false,
            narrow: Narrow {
                width: f32::from(window.viewport_size().width),
                drawer: Default::default(),
                shown: false,
                opener: None,
                region: cx.focus_handle(),
                toggle: toggle_focus,
            },
            notices: Vec::new(),
            fatal: None,
            started: false,
            pending_start: None,
            opening_start: false,
            moved_early: false,
            settings,
            renders: 0,
            preferences,
            moves: 0,
            pulses,
            sidebar_focus: None,
            _subscriptions: vec![moved, lost, activation, visibility],
        }
    }

    // chat-fix: one rule for focus whose element went (W1).
    /// The focused element left the window (a dismissed notice, an
    /// archived or deleted row, a place left): focus goes to its defined
    /// neighbour, so the keyboard is never left on nothing. A row of the
    /// sidebar's lists: its neighbour there ([`Sidebar::restore_focus`]);
    /// anything else: the composer on a chat or the new chat (a notice or
    /// an error dismissed, a place changed to a chat), else the sidebar's
    /// current row (a place with no control of its own yet).
    fn focus_lost(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.fatal.is_some() {
            return;
        }
        // settings agent: a control of open Settings went (a row changed
        // under it): focus goes to the panel's active navigation row.
        if self.settings.read(cx).is_open() {
            self.settings.update(cx, |s, cx| s.focus_active(window, cx));
            self.log("focus restored", serde_json::json!({}));
            return;
        }
        if let Some(held) = self.sidebar_focus.take() {
            self.sidebar
                .update(cx, |s, cx| s.restore_focus(held, window, cx));
        } else if matches!(self.place, Place::Chat { .. } | Place::NewChat { .. }) {
            self.chat.update(cx, |c, cx| c.focus_composer(window, cx));
        } else {
            self.sidebar.update(cx, |s, cx| s.focus_current(window, cx));
        }
        self.log("focus restored", serde_json::json!({}));
    }

    fn log(&self, action: &str, fields: serde_json::Value) {
        if let Some(log) = &self.options.log {
            log.record(action, fields);
        }
    }

    /// The chat pane.
    pub fn chat(&self) -> Entity<ChatPane> {
        self.chat.clone()
    }

    /// The place on screen.
    pub fn place(&self) -> &Place {
        &self.place
    }

    /// The layout.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// The mode of the place on screen.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// How many times the shell has rendered.
    pub fn render_count(&self) -> usize {
        self.renders
    }

    /// The newest Build chat, or the new chat (`openLanding` in Build).
    fn landing(&self, cx: &App) -> Place {
        match self.lists.read(cx).chats().first() {
            Some(chat) => Place::Chat {
                id: chat.id.0.clone(),
            },
            None => Place::NewChat { id: None },
        }
    }

    // par-bench: Bench's landing.
    /// The newest bench, or the new bench when there is none or a prompt
    /// was typed for the new bench before the lists came (`openLanding` in
    /// Bench).
    fn bench_landing(&self, cx: &App) -> Place {
        let typed = !crate::state::draft::draft_empty(&self.chat.read(cx).current_draft(cx));
        match self.lists.read(cx).benches().first() {
            Some(bench) if !typed => Place::Compare {
                bench: bench.id.clone(),
            },
            _ => Place::NewBench,
        }
    }

    /// Opens `place` as the user does (a new history entry): a chat's
    /// title is announced, the new chat takes the composer's focus.
    pub fn go(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        let place = match place {
            // par-bench: the landing in the mode on screen (`openLanding`).
            Place::Landing if self.mode == Mode::Bench => self.bench_landing(cx),
            Place::Landing => self.landing(cx),
            other => other,
        };
        // narrow agent: the drawer closes on what is chosen in it.
        self.set_drawer(false, window, cx);
        if place == self.place && !matches!(place, Place::NewChat { .. }) {
            // par-chat: the chat on screen chosen again reads it again
            // after a failed read (`openConversation`).
            if matches!(place, Place::Chat { .. } | Place::Member { .. }) {
                self.chat.update(cx, |c, cx| c.chosen_again(cx));
            }
            return;
        }
        self.visit(place, Visit::Push, true, window, cx);
    }

    /// Switches to `mode`'s remembered place (`switchMode`).
    pub fn switch_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if mode == self.mode() {
            return;
        }
        let place = match (mode, self.left.get(&mode)) {
            (_, Some(place)) => place.clone(),
            // par-bench: Build's first place is the new chat (`leftRef`'s
            // `new-chat`, `goBuild(null)`), not its landing.
            (Mode::Build, None) => Place::NewChat { id: None },
            (Mode::Bench, None) => Place::NewBench,
        };
        self.visit(place, Visit::Push, true, window, cx);
    }

    /// Records the navigation to `place` in the history and shows it;
    /// `announce` says a chat's title (the user opened it).
    pub fn visit(
        &mut self,
        place: Place,
        visit: Visit,
        announce: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // start-place agent: a visit before the first lists that is not the
        // start place's own is the person's.
        if !self.started && !self.opening_start {
            self.moved_early = true;
        }
        // par-chat: the new chat is shown with its row when it has one
        // (`showNewChat`'s `arrive({ kind: 'new-chat', id })`).
        let place = match place {
            Place::NewChat { id: None } => Place::NewChat {
                id: self.chat.read(cx).new_row_id(),
            },
            other => other,
        };
        self.history.visit(place.clone(), visit);
        self.arrive(place, announce, window, cx);
    }

    /// Shows `place` (no history change: Back and Forward come here).
    fn arrive(
        &mut self,
        place: Place,
        announce: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match &place {
            Place::Missing { .. } | Place::Landing => {}
            other => {
                let mode = place::place_keys(other, |_| Some(1)).mode;
                self.left.insert(mode, other.clone());
            }
        }
        self.mode = place::place_keys(&place, |_| Some(1)).mode;
        let previous = std::mem::replace(&mut self.place, place.clone());
        // chat: type-ahead reaches the composer only while it is on screen.
        let chat_shown = matches!(
            place,
            Place::Chat { .. } | Place::NewChat { .. } | Place::NewBench | Place::Compare { .. }
        );
        self.chat.update(cx, |c, _| c.set_shown(chat_shown));
        // par-bench: a navigation, for a new bench's comparison (`runBench`).
        self.chat.update(cx, |c, _| c.bench.navigated());
        match &place {
            Place::Chat { id } => {
                let same = self.chat.read(cx).open_chat() == Some(id.as_str());
                if !same {
                    let title = self
                        .lists
                        .read(cx)
                        .chat(id)
                        .map(|c| c.title.clone())
                        .unwrap_or_default();
                    self.log(
                        "opened chat",
                        serde_json::json!({ "chat": id, "title": title }),
                    );
                    let chat = id.clone();
                    self.chat
                        .update(cx, |c, cx| c.show_chat(Some(chat), window, cx));
                    self.announcer.update(cx, |a, cx| {
                        a.clear(cx);
                        if announce {
                            a.polite(title, cx);
                        }
                    });
                }
            }
            Place::NewChat { id } => {
                if !matches!(previous, Place::NewChat { .. }) || id.is_none() {
                    if announce {
                        self.log("new chat", serde_json::json!({}));
                    }
                    // par-chat: the new chat, with or without its row (the
                    // row is the world's, the draft stays the new chat's).
                    self.chat.update(cx, |c, cx| {
                        c.show_chat(None, window, cx);
                        c.focus_composer(window, cx);
                    });
                    self.announcer.update(cx, |a, cx| a.clear(cx));
                }
            }
            Place::Missing { archived: None, .. } => {
                // Whether it is archived decides the screen's texts.
                self.options.core.send(crate::Command::ReadArchive);
            }
            other => {
                self.log(
                    "opened",
                    serde_json::json!({ "place": crate::state::address::address(other) }),
                );
                // bench agent: Bench's places.
                self.arrive_bench(other.clone(), window, cx);
            }
        }
        self.tell_sidebar(window, cx);
        self.sync_title(window, cx);
        // pane agent: the place's world.
        self.sync_world(window, cx);
        if self.started {
            self.save();
        }
        cx.notify();
    }

    /// Saves the preferences with the layout and the place as they are now.
    fn save(&mut self) {
        let place = crate::state::address::address(&self.place_to_remember());
        let bench_mode = self.mode == Mode::Bench;
        let p = &mut self.preferences;
        p.sidebar_open = self.layout.sidebar_open;
        p.sidebar_width = self.layout.sidebar_width;
        p.chat_width = self.layout.chat_width;
        p.bench_mode = bench_mode;
        p.place = Some(place);
        self.options
            .core
            .send(crate::Command::SavePreferences(self.preferences.clone()));
    }

    /// The window moved or was resized: its display and bounds are saved
    /// once they have not changed for half a second.
    fn window_moved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(display) = window.display(cx) else {
            return;
        };
        let bounds = window.bounds();
        // par-shell: where the error screen opens after a panic.
        crate::crash::remember_window([
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        ]);
        self.preferences.window = Some(crate::state::layout::Placement {
            display: crate::display_name(&*display),
            bounds: crate::state::layout::Rect {
                x: f32::from(bounds.origin.x),
                y: f32::from(bounds.origin.y),
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
            },
        });
        self.moves += 1;
        let at = self.moves;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(500))
                .await;
            let _ = this.update(cx, |this, _| {
                if this.moves == at {
                    this.save();
                }
            });
        })
        .detach();
    }

    /// Tells the sidebar the place on screen, after the update under way
    /// (the sidebar may be the one navigating).
    fn tell_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) {
        let (place, mode, sidebar) = (self.place.clone(), self.mode(), self.sidebar.clone());
        cx.defer_in(window, move |_, window, cx| {
            sidebar.update(cx, |s, cx| s.set_place(place, mode, window, cx))
        });
    }

    /// Restore on the missing screen: asks the core to restore it; it opens
    /// once the lists have it (the seam's restore, `state/seam.rs`).
    pub fn restore_missing(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.log(
            "restore",
            serde_json::json!({ "place": crate::state::address::address(&self.place) }),
        );
        // sidebar agent: the sidebar restores it and opens it once listed.
        if let Place::Missing { mode, id, .. } = self.place.clone() {
            self.sidebar.update(cx, |s, cx| s.restore(mode, id, cx));
        }
    }

    /// The place to remember at quit and open at the next start: the place
    /// on screen (the new chat without its row).
    pub fn place_to_remember(&self) -> Place {
        match &self.place {
            Place::NewChat { .. } => Place::NewChat { id: None },
            other => other.clone(),
        }
    }

    /// Opens Settings over the place on their first pane (Connect an AI
    /// while no AI is usable, General once one is): the `settings` layer
    /// (Escape closes it, shortcuts wait under it), with no history entry,
    /// as the page's overlay (`openSettings()`).
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_settings_on(None, None, window, cx);
    }

    // settings agent
    /// Opens Settings on `pane` (none: the first pane), flashing the
    /// General row `flash` (`openSettings(view)`, `openSetting`).
    pub fn open_settings_on(
        &mut self,
        pane: Option<crate::state::settings::Pane>,
        flash: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings
            .update(cx, |s, cx| s.open(pane, flash, window, cx));
    }

    /// Closes open Settings, dropping the draft; focus goes back to what
    /// opened them.
    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.update(cx, |s, cx| s.close(window, cx));
    }

    /// The Settings layer.
    pub fn settings(&self) -> Entity<super::settings::SettingsLayer> {
        self.settings.clone()
    }

    /// Settings closed (`app.md` 2.5): with an AI connected and no model
    /// chosen in Build, the chat model picker opens by itself.
    fn settings_closed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Build {
            super::pickers::catalog::Catalog::installed(cx).update(cx, |c, cx| c.ask_model(cx));
        }
        cx.notify();
    }

    /// The theme from Settings: applied and saved at once.
    fn set_light(&mut self, light: bool, cx: &mut Context<Self>) {
        if self.preferences.light != light {
            self.flip_theme(cx);
        }
    }

    /// The shortcuts from Settings: in force and saved at once
    /// (`mcb-binds`; an unbound action is stored as empty).
    fn set_binds(&mut self, keymap: crate::state::keymap::Keymap, cx: &mut Context<Self>) {
        self.preferences.binds = keymap
            .0
            .iter()
            .filter(|(action, combo)| combo.as_str() != action.default_bind())
            .map(|(action, combo)| (action.id().to_string(), combo.clone()))
            .collect();
        keys::set_keymap(cx, keymap);
        self.save();
        cx.notify();
    }

    /// A published build's chat or bench, opened (Settings' `Go to`).
    fn go_to_published(
        &mut self,
        subject: crate::state::published::Subject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::state::published::Subject;
        let place = match subject {
            Subject::Chat(id) => Place::Chat { id },
            Subject::Bench(bench) => Place::Compare { bench },
        };
        self.go(place, window, cx);
    }

    /// Back: the previous place in the history.
    pub fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(place) = self.history.back().cloned() {
            self.arrive(place, true, window, cx);
        }
    }

    /// Forward: the next place in the history.
    pub fn forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(place) = self.history.forward().cloned() {
            self.arrive(place, true, window, cx);
        }
    }

    fn sync_title(&self, window: &mut Window, cx: &App) {
        window.set_window_title(&place::window_title(&self.place, self.lists.read(cx)));
    }

    /// Shows or hides the sidebar (Mod+B, the toggle, Open sidebar); in a
    /// narrow window opens or closes the list's drawer instead.
    pub fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // narrow agent
        if layout::narrow(self.narrow.width) {
            let open = !self.narrow.drawer.open;
            self.set_drawer(open, window, cx);
            return;
        }
        self.layout.toggle_sidebar();
        self.log(
            "sidebar",
            serde_json::json!({ "open": self.layout.sidebar_open }),
        );
        self.save();
        cx.notify();
    }

    /// Takes one update from the core.
    pub fn apply(&mut self, update: Update, window: &mut Window, cx: &mut Context<Self>) {
        // mac-real: when each update applied, for the frame-pacing runs.
        self.options
            .metrics
            .borrow_mut()
            .update(super::metrics::kind(&update));
        // start-place agent: the core spoke before its lists (its setup, the
        // pane): a chat, bench or member start place opens now.
        if !self.started && !matches!(update, Update::Conversations(_)) {
            self.open_start(window, cx);
        }
        match update {
            Update::Conversations(published) => {
                // sidebar agent: the benches before this list.
                let known_benches: Vec<String> = self
                    .lists
                    .read(cx)
                    .benches()
                    .iter()
                    .map(|b| b.id.clone())
                    .collect();
                let known: Vec<String> = self
                    .lists
                    .read(cx)
                    .chats()
                    .iter()
                    .map(|c| c.id.0.clone())
                    .collect();
                let created = published
                    .conversations
                    .iter()
                    .map(|c| &c.summary)
                    .find(|c| c.bench_id.is_none() && !known.contains(&c.id.0))
                    .map(|c| c.id.0.clone());
                self.lists.update(cx, |l, cx| l.set(published, cx));
                // pane agent: a title or a bench's rounds may have changed.
                self.sync_world(window, cx);
                // seen: the marks this list answers end.
                self.sidebar.update(cx, |s, cx| s.seen_listed(cx));
                if let Some(id) = created.filter(|_| self.chat.read(cx).creating()) {
                    // The new chat became its chat: the same entry, renamed.
                    self.chat.update(cx, |c, cx| c.adopt(id.clone(), cx));
                    self.history
                        .visit(Place::Chat { id: id.clone() }, Visit::Replace);
                    self.place = Place::Chat { id: id.clone() };
                    self.left.insert(Mode::Build, self.place.clone());
                    self.tell_sidebar(window, cx);
                }
                // par-chat: the new chat's row settles once listed.
                self.settle_new_row(window, cx);
                if !self.started && self.pending_start.is_some() {
                    self.started = true;
                    let target = self.pending_start.take().expect("a pending start");
                    // start-place agent: the person went elsewhere meanwhile:
                    // they stay there. Listed: the place continues as it is.
                    // Not listed: the missing place.
                    if !self.moved_early {
                        let listed = self.listed_start(&target, cx);
                        if listed == target {
                            self.continue_start(&target, window, cx);
                        } else {
                            self.visit(listed, Visit::Replace, false, window, cx);
                        }
                    }
                }
                if !self.started {
                    self.started = true;
                    let start = self.options.start.clone().unwrap_or(Place::Landing);
                    let start = match start {
                        // The landing in the mode last used.
                        // par-bench: Bench's landing is its newest bench
                        // (`openLanding`).
                        Place::Landing if self.preferences.bench_mode => self.bench_landing(cx),
                        Place::Landing => self.landing(cx),
                        // A chat or bench the lists do not have is missing,
                        // never another one in its place.
                        target => {
                            let lists = self.lists.read(cx);
                            let chats: Vec<ListedChat> = lists
                                .all()
                                .iter()
                                .map(|c| ListedChat {
                                    id: &c.summary.id.0,
                                    bench: c.summary.bench_id.as_deref(),
                                })
                                .collect();
                            let benches: Vec<&str> =
                                lists.benches().iter().map(|b| b.id.as_str()).collect();
                            let address = crate::state::address::address(&target);
                            crate::state::address::listed_place(&target, &address, &chats, &benches)
                        }
                    };
                    // start-place agent: the page's exception: one who moved
                    // before the lists stays where they went.
                    if !self.moved_early {
                        self.visit(start, Visit::Replace, false, window, cx);
                        // The app opens ready to type, as today's page does.
                        self.chat.update(cx, |c, cx| c.focus_composer(window, cx));
                    }
                }
                if self.started
                    && let Place::Chat { id } = &self.place
                    && self.lists.read(cx).chat(id).is_none()
                {
                    // The chat left the list: its neighbour stands in for it
                    // (the same entry, replaced), or the new chat.
                    // sidebar agent: the page's `successor`.
                    let listed: Vec<String> = self
                        .lists
                        .read(cx)
                        .chats()
                        .iter()
                        .map(|c| c.id.0.clone())
                        .collect();
                    let next = crate::state::sidebar::successor(&known, &listed, id)
                        .map(|id| Place::Chat { id: id.clone() })
                        .unwrap_or(Place::NewChat { id: None });
                    self.visit(next, Visit::Replace, false, window, cx);
                }
                // sidebar agent: a bench that left the list (archived here or
                // elsewhere): Bench's place moves to its successor's
                // comparison, or the new bench (`receiveBenches`).
                self.bench_left(&known_benches, window, cx);
                // After this update: the sidebar may navigate (a restored
                // item opens), which updates the shell.
                let sidebar = self.sidebar.clone();
                window.defer(cx, move |window, cx| {
                    sidebar.update(cx, |s, cx| s.lists_changed(window, cx))
                });
                self.sync_title(window, cx);
                // chat: a running turn to join, or a detached one ended.
                self.chat.update(cx, |c, cx| c.lists_changed(window, cx));
            }
            Update::Transcript { .. }
            | Update::Turn { .. }
            | Update::TurnJoined { .. }
            | Update::TurnNotRunning { .. } => {
                self.chat.update(cx, |c, cx| c.apply(update, window, cx));
            }
            Update::Error(text) => {
                // seen: a failed mark ends in this, then the list.
                self.sidebar.update(cx, |s, _| s.seen_error());
                // A notice per distinct text, the newest last (`showError`).
                self.notices.retain(|n| *n != text);
                self.notices.push(text.clone());
                self.announcer.update(cx, |a, cx| a.assertive(text, cx));
            }
            // The core stopped: the whole window becomes the fatal screen.
            Update::Fatal(text) => {
                self.fatal = Some(cx.new(|cx| super::fatal::FatalScreen::new(text, window, cx)));
            }
            // chat: the transcript shows its failed read itself.
            Update::TranscriptFailed { .. } => {
                self.chat.update(cx, |c, cx| c.apply(update, window, cx));
            }
            Update::SendRefused { error, target } => {
                // bench agent: a bench send's refusal ends its wait.
                self.chat.update(cx, |c, _| c.bench_refused(&target));
                self.chat.update(cx, |c, cx| c.refused(&target, window, cx));
                self.notices.retain(|n| *n != error);
                self.notices.push(error.clone());
                self.announcer.update(cx, |a, cx| a.assertive(error, cx));
            }
            // The archive answers the missing screen (and later the
            // sidebar's archived section).
            Update::Archive(Err(error)) => {
                // sidebar agent: the archived section says so; the missing
                // screen cannot tell, and the error is a notice.
                self.sidebar
                    .update(cx, |s, cx| s.archive_failed(error.clone(), cx));
                if matches!(self.place, Place::Missing { archived: None, .. }) {
                    self.notices.retain(|n| *n != error);
                    self.notices.push(error);
                }
            }
            Update::Archive(Ok(archive)) => {
                // sidebar agent: the archived section reads it too.
                let read = archive.clone();
                self.sidebar.update(cx, |s, cx| s.archive_read(read, cx));
                if let Place::Missing {
                    mode, id, address, ..
                } = &self.place
                {
                    let archived = match mode {
                        Mode::Build => archive.conversations.iter().any(|c| c.id == *id),
                        Mode::Bench => archive.benches.iter().any(|b| b.id == *id),
                    };
                    self.place = Place::Missing {
                        mode: *mode,
                        id: id.clone(),
                        address: address.clone(),
                        archived: Some(archived),
                    };
                    self.history.visit(self.place.clone(), Visit::Replace);
                    self.sync_title(window, cx);
                }
            }
            // The search panel, the pickers and Settings are later
            // milestones; their answers have no reader yet.
            // sidebar agent: the sidebar's search and the palette.
            Update::Found { request, results } => {
                self.sidebar
                    .update(cx, |s, cx| s.found(request, results, cx));
            }
            // pickers: the catalogue takes the providers (the sidebar's
            // palette lists them too), models and favourites.
            // par-chat: a failed read (`config.phase === 'failed'`): the
            // sidebar's sentence, the page's `showError(error)` notice, the
            // log line; the providers held stay, and no AI is said to be
            // missing (the page's `noAI` needs a loaded read).
            Update::Providers(Err(error)) => {
                self.log("providers failed", serde_json::json!({ "error": error }));
                self.chat.update(cx, |c, cx| c.providers_failed(cx));
                self.sidebar
                    .update(cx, |s, cx| s.set_models_failed(true, cx));
                self.notice(error, cx);
            }
            Update::Providers(Ok(providers)) => {
                // par-chat: a read that succeeds ends the failure.
                self.sidebar
                    .update(cx, |s, cx| s.set_models_failed(false, cx));
                // chat: whether an AI is connected (the notice of a send
                // without a model).
                self.chat
                    .update(cx, |c, cx| c.set_providers(&providers, cx));
                self.sidebar
                    .update(cx, |s, cx| s.set_providers(providers.clone(), cx));
                super::pickers::catalog::Catalog::installed(cx)
                    .update(cx, |c, cx| c.apply(Update::Providers(Ok(providers)), cx));
            }
            update @ (Update::Models { .. } | Update::ModelInfo { .. } | Update::Favorites(_)) => {
                super::pickers::catalog::Catalog::installed(cx)
                    .update(cx, |c, cx| c.apply(update, cx));
            }
            // chat: a held send goes once setup is ready.
            Update::Setup(steps) => {
                self.chat.update(cx, |c, cx| c.set_setup(steps, window, cx));
            }
            // pane agent: the live view's updates go to the pane.
            update @ (Update::LivePane(_)
            | Update::Live(_)
            | Update::Clicked { .. }
            | Update::PlayerNote(_)
            | Update::SelectionDescribed { .. }
            | Update::Exported(_)) => self.live_update(update, window, cx),
            // sidebar agent: the library.
            // bench agent: the library's sync reads it; the sidebar shows
            // the sync's list.
            Update::Prompts(library) => {
                self.library.update(cx, |l, cx| l.read(library, cx));
            }
            // bench agent: Settings' vote count and the leaderboard.
            Update::Leaderboard(board) => {
                self.bench_pane.update(cx, |b, cx| {
                    b.apply(Update::Leaderboard(board.clone()), window, cx)
                });
                self.settings
                    .update(cx, |s, cx| s.apply(Update::Leaderboard(board), window, cx));
            }
            // settings agent: Settings' updates.
            update @ (Update::Settings(_)
            | Update::SettingsSaved(_)
            | Update::SignIn { .. }
            | Update::Install { .. }
            | Update::Shares(_)
            | Update::ShareRemoved { .. }) => {
                self.settings
                    .update(cx, |s, cx| s.apply(update, window, cx));
            }
            // par-chat: the new chat's row.
            Update::ChatCreated(result) => self.chat_created(result, window, cx),
            // share agent: the Share panel's updates.
            update @ (Update::ShareRead { .. }
            | Update::PublishStep { .. }
            | Update::ShareChanged { .. }) => crate::view::share::apply(update, cx),
            // bench agent: Bench's and the library's answers.
            update @ (Update::Bench { .. }
            | Update::BenchSent { .. }
            | Update::Forked { .. }
            | Update::PromptsSaved(_)
            | Update::Generated { .. }) => self.apply_bench(update, window, cx),
        }
        cx.notify();
    }

    fn pointer_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let x = f32::from(event.position.x);
        if let Some(drag) = self.sidebar_drag.as_mut() {
            drag.move_to(&mut self.layout, x);
            cx.notify();
        } else if self.chat_drag {
            let width = f32::from(window.viewport_size().width);
            self.layout.chat_width =
                Some(layout::chat_drag(x, self.layout.sidebar_column(), width));
            cx.notify();
        }
    }

    fn pointer_up(&mut self, cx: &mut Context<Self>) {
        // Saved once the drag ends (the page writes on each move).
        let ended = self.sidebar_drag.is_some() || self.chat_drag;
        if let Some(drag) = self.sidebar_drag.take() {
            drag.end(&mut self.layout);
            self.log(
                "sidebar",
                serde_json::json!({ "open": self.layout.sidebar_open, "width": self.layout.sidebar_width }),
            );
            cx.notify();
        }
        if self.chat_drag {
            self.chat_drag = false;
            self.log(
                "split",
                serde_json::json!({ "chat_width": self.layout.chat_width }),
            );
            cx.notify();
        }
        if ended {
            self.save();
        }
    }

    /// The sidebar's edge (or the strip of a closed one): a drag handle;
    /// a double-click on the edge collapses the sidebar.
    fn edge(&self, reveal: bool, cx: &mut Context<Self>) -> AnyElement {
        let dragging = self.sidebar_drag.is_some();
        let element = div()
            .id(if reveal {
                "sidebar-reveal"
            } else {
                "sidebar-edge"
            })
            .group("edge")
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(7.))
            .map(|d| if reveal { d.left_0() } else { d.right_0() })
            .cursor(if reveal {
                CursorStyle::ResizeRight
            } else {
                CursorStyle::ResizeLeftRight
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if event.click_count == 2 && this.layout.sidebar_open {
                        this.sidebar_drag = None;
                        this.layout.sidebar_open = false;
                        this.log("sidebar", serde_json::json!({ "open": false }));
                        this.save();
                    } else {
                        this.sidebar_drag = Some(SidebarDrag::start(
                            &this.layout,
                            f32::from(event.position.x),
                            0.,
                        ));
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .w(px(2.))
                    .map(|d| if reveal { d.left_0() } else { d.right_0() })
                    .when(dragging, |d| d.bg(colour::edge_line()))
                    .group_hover("edge", |s| s.bg(colour::edge_line())),
            );
        let title = if reveal {
            "Show sidebar"
        } else {
            "Drag to resize \u{b7} double-click to collapse"
        };
        pointer_tip(element, title, title).into_any_element()
    }

    fn divider(&self, cx: &mut Context<Self>) -> AnyElement {
        let element = div()
            .id("divider")
            .group("divider")
            .relative()
            .flex_none()
            .w(px(layout::DIVIDER))
            .h_full()
            .bg(colour::bedrock())
            .when(self.chat_drag, |d| d.bg(colour::hover()))
            .hover(|s| s.bg(colour::hover()))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.chat_drag = true;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(2.))
                    .w(px(1.))
                    .bg(colour::line())
                    .when(self.chat_drag, |d| {
                        d.bg(colour::gold()).shadow(shadow::divider_glow())
                    })
                    .group_hover("divider", |s| {
                        s.bg(colour::gold()).shadow(shadow::divider_glow())
                    }),
            );
        // pane agent (first real look): by the pointer, as the page's title.
        super::controls::cursor_tip(element, "divider-tip", "Drag to resize").into_any_element()
    }
}

// sidebar agent: the theme the palette flips, and a bench that left.
impl Shell {
    /// Keeps the sidebar's folds in the preferences (`mcb-open-benches`,
    /// `mcb-prompts-open`) and saves them.
    pub fn keep_sidebar(&mut self, open_benches: Vec<String>, prompts_open: bool) {
        if self.preferences.open_benches == open_benches
            && self.preferences.prompts_open == prompts_open
        {
            return;
        }
        self.preferences.open_benches = open_benches;
        self.preferences.prompts_open = prompts_open;
        self.save();
    }

    /// The preferences as they will be saved next.
    pub fn preferences(&self) -> &crate::Preferences {
        &self.preferences
    }

    /// The sidebar.
    pub fn sidebar(&self) -> Entity<Sidebar> {
        self.sidebar.clone()
    }

    /// Flips the theme (the palette's `Light theme` / `Dark theme`) and
    /// saves it.
    pub fn flip_theme(&mut self, cx: &mut Context<Self>) {
        self.preferences.light = !self.preferences.light;
        super::set_theme(
            if self.preferences.light {
                super::theme::Mode::Light
            } else {
                super::theme::Mode::Dark
            },
            cx,
        );
        self.log(
            "theme",
            serde_json::json!({ "light": self.preferences.light }),
        );
        self.save();
        cx.notify();
    }

    /// A bench left the list: the place on it (on screen, or remembered for
    /// Bench) moves to its successor's comparison, or the new bench; the
    /// entry is replaced when it is on screen.
    fn bench_left(&mut self, before: &[String], window: &mut Window, cx: &mut Context<Self>) {
        let listed: Vec<String> = self
            .lists
            .read(cx)
            .benches()
            .iter()
            .map(|b| b.id.clone())
            .collect();
        let gone = |place: &Place| match place {
            Place::Compare { bench } | Place::Member { bench, .. } => {
                (!listed.contains(bench) && before.contains(bench)).then(|| bench.clone())
            }
            _ => None,
        };
        let next = |bench: &str| {
            crate::state::sidebar::successor(before, &listed, bench)
                .map(|b| Place::Compare { bench: b.clone() })
                .unwrap_or(Place::NewBench)
        };
        if let Some(bench) = gone(&self.place) {
            let place = next(&bench);
            self.visit(place, Visit::Replace, false, window, cx);
        } else if let Some(bench) = self.left.get(&Mode::Bench).and_then(gone) {
            let place = next(&bench);
            self.left.insert(Mode::Bench, place);
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(fatal) = &self.fatal {
            return div().size_full().child(fatal.clone()).into_any_element();
        }
        self.renders += 1;
        // par-shell: the dev binary's `--crash draw`.
        if crate::dev::PANIC_WHILE_DRAWING.load(std::sync::atomic::Ordering::SeqCst) {
            panic!("a panic on purpose while drawing (--crash draw)");
        }
        // Focus is never left on nothing after an update (W1): a handle
        // that was dropped has no element and no neighbour.
        debug_assert!(
            self.renders < 3 || window.focused(cx).is_some(),
            "the window's focus was left on nothing"
        );
        self.sidebar_focus = self.sidebar.read(cx).sidebar_focus(window);
        self.options.metrics.borrow_mut().rendering();
        let metrics = self.options.metrics.clone();
        let wheel = self.options.metrics.clone();
        let first_log = self.options.log.clone();
        let probe = canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                // pane agent: the first frame's line, written as it paints
                // (two sessions' timing rows read it): from the process's
                // start (`crash::process_started`) and the wall clock.
                if metrics.borrow_mut().painted()
                    && let Some(log) = &first_log
                {
                    let epoch = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("the clock is after 1970");
                    log.record(
                        "first_frame",
                        serde_json::json!({
                            "since_start_ms": crate::crash::process_started().elapsed().as_secs_f64() * 1e3,
                            "epoch_ms": epoch.as_millis() as u64,
                        }),
                    );
                }
                // Every wheel event as it reaches the window, before any
                // element takes it (the scroll-to-paint numbers).
                let wheel = wheel.clone();
                window.on_mouse_event(move |_: &ScrollEvent, phase, _, _| {
                    if phase == DispatchPhase::Capture {
                        wheel.borrow_mut().scroll();
                    }
                });
            },
        )
        .size_0();
        let width = f32::from(window.viewport_size().width);
        // narrow agent: the width the narrow layout and its drawer follow.
        if width != self.narrow.width {
            self.resized(width, window, cx);
        }
        // The drawer closes under Settings and the search panel.
        if self.narrow.drawer.open
            && (keys::is_open(cx, Layer::Settings) || keys::is_open(cx, Layer::Command))
        {
            self.narrow.drawer.open = false;
            self.narrow.opener = None;
            self.sync_drawer(window, cx);
        }
        let narrow = layout::narrow(width);
        let drawer = self.drawer_shown();
        let height = f32::from(window.viewport_size().height);
        let chat_width = self.layout.chat_column(width);
        let open = self.layout.sidebar_open && !narrow;
        let content = match &self.place {
            Place::Chat { .. } | Place::NewChat { .. } => self
                .chat
                .clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
            Place::Missing { .. } => panes::missing(self, cx),
            // bench agent: Bench's places.
            Place::NewBench | Place::Compare { .. } | Place::Member { .. } => self
                .chat
                .clone()
                .cached(StyleRefinement::default().size_full())
                .into_any_element(),
            Place::Leaderboard => self.bench_pane.clone().into_any_element(),
            Place::Prompt => super::bench::prompts::place(self, window, cx),
            other => panes::placeholder(other, self.lists.read(cx), cx),
        };
        // share agent: the header's Share panel needs the window.
        let header = header::pane_header(self, window, cx);
        let notices = self.notices(cx);
        let dragging = self.sidebar_drag.is_some() || self.chat_drag;
        div()
            .id("screen")
            .key_context("Screen")
            .on_action(
                cx.listener(|this, _: &FocusNext, window, cx| this.focus_step(true, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FocusPrevious, window, cx| {
                    this.focus_step(false, window, cx)
                }),
            )
            .on_action(|_: &CloseLayer, window, cx| keys::close_top(window, cx))
            // The Window menu's items and their keys (macOS).
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &ToggleFullScreen, window, _| window.toggle_fullscreen())
            // quit-ask: the person's quit and the Window menu's Close ask
            // while builds run.
            .on_action(cx.listener(|this, _: &super::Quit, window, cx| {
                if this.may_quit("key", window, cx) {
                    cx.quit();
                }
            }))
            .on_action(cx.listener(|this, _: &super::CloseWindow, window, cx| {
                if this.may_quit("close-window", window, cx) {
                    window.remove_window();
                }
            }))
            .on_action(cx.listener(|this, _: &super::GoBack, window, cx| this.back(window, cx)))
            .on_action(
                cx.listener(|this, _: &super::GoForward, window, cx| this.forward(window, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Back),
                cx.listener(|this, _: &MouseDownEvent, window, cx| this.back(window, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(NavigationDirection::Forward),
                cx.listener(|this, _: &MouseDownEvent, window, cx| this.forward(window, cx)),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.pointer_move(event, window, cx)
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.pointer_up(cx)),
            )
            .when(dragging, |d| d.cursor(CursorStyle::ResizeLeftRight))
            .size_full()
            .relative()
            .flex()
            .bg(colour::bedrock())
            .text_color(colour::bone())
            .font_family(super::controls::fonts(cx).ui)
            .child(probe)
            // narrow agent: the wide window's columns, or the narrow
            // window's bar, world, line and chat with the drawer over them.
            .map(|d| {
                if narrow {
                    d.flex_col().children(
                        self.narrow_body(drawer, width, height, header, content, notices, cx),
                    )
                } else {
                    d.children(self.wide_body(open, chat_width, header, content, notices, cx))
                }
            })
            // sidebar agent: the running dots, over the sidebar; a step
            // notifies the shell alone, its cached views are reused.
            .map(|d| {
                let dots = super::sidebar::dot::layer(self.pulses.clone(), cx.entity_id());
                // narrow agent: over the drawer while it shows (it is drawn
                // after the page).
                if drawer {
                    d.child(deferred(dots).with_priority(1))
                } else {
                    d.child(dots)
                }
            })
            .when(!open && !narrow, |d| d.child(self.edge(true, cx)))
            // settings agent: the Settings layer (it draws only while open).
            .child(self.settings.clone())
            // sidebar agent: search everything, over everything.
            .child(self.sidebar.read(cx).palette())
            // quit-ask: the quit question, over everything.
            .children(self.quit_layer(window, cx))
            .child(self.announcer.clone())
            .into_any_element()
    }
}

impl Shell {
    /// The notices (`.app-notices`): over the chat column's top, one per
    /// distinct error, each with `Dismiss error`, kept until dismissed.
    fn notices(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.notices.is_empty() {
            return None;
        }
        let fonts = super::controls::fonts(cx);
        let mut stack = div()
            .tab_group()
            .tab_index(1)
            .absolute()
            .top(px(10.))
            .left(px(16.))
            .right(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.));
        for (index, text) in self.notices.iter().enumerate() {
            let dismissed = text.clone();
            stack = stack.child(
                super::theme::text(
                    div()
                        .id(("notice", index))
                        .role(Role::Article)
                        .aria_label(format!("Error, {text}"))
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(px(10.))
                        .px(px(12.))
                        .py(px(10.))
                        .rounded(px(super::theme::radius::ERROR))
                        .border_1()
                        .border_color(colour::error_border())
                        .bg(colour::error_fill())
                        .shadow(shadow::menu()),
                    super::theme::TextKind::Error,
                    &fonts,
                )
                .child(div().flex_1().min_w_0().child(text.clone()))
                .child(
                    super::controls::Button::icon(
                        ("dismiss-notice", index),
                        super::icons::Icon::X,
                        "Dismiss error",
                    )
                    .on_press({
                        let this = cx.entity().downgrade();
                        move |_, cx| {
                            let dismissed = dismissed.clone();
                            let _ = this.update(cx, |s, cx| {
                                s.notices.retain(|n| *n != dismissed);
                                cx.notify();
                            });
                        }
                    }),
                ),
            );
        }
        Some(stack.into_any_element())
    }
}

// narrow agent: the window's two arrangements.
impl Shell {
    /// The wide window: the sidebar, the chat column, the divider and the
    /// world, side by side.
    fn wide_body(
        &mut self,
        open: bool,
        chat_width: f32,
        header: AnyElement,
        content: AnyElement,
        notices: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if open {
            out.push(
                div()
                    .relative()
                    // The sidebar, then the chat column, in the Tab
                    // order (chat-fix, W3: tab groups place the
                    // column's notices before its transcript).
                    .tab_group()
                    .tab_index(0)
                    .flex_none()
                    .w(px(self.layout.sidebar_width))
                    .h_full()
                    .child(
                        self.sidebar
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    )
                    .child(self.edge(false, cx))
                    .into_any_element(),
            );
        }
        out.push(
            div()
                .relative()
                .tab_group()
                .tab_index(0)
                .flex_none()
                // bench agent: the prompt library hides the divider and
                // the viewer (`app.md` 9.2): the column takes the rest.
                .map(|d| {
                    if self.place == Place::Prompt {
                        d.flex_1().min_w_0()
                    } else {
                        d.w(px(chat_width))
                    }
                })
                .h_full()
                .flex()
                .flex_col()
                .bg(colour::cave())
                .border_r_1()
                .border_color(colour::line())
                // In the Tab order as on screen: the header, the
                // notices over the column's top, the transcript and
                // composer (the notices are drawn last, over them).
                .child(div().flex_none().tab_group().tab_index(0).child(header))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .tab_group()
                        .tab_index(2)
                        .child(content),
                )
                .children(notices)
                .into_any_element(),
        );
        if self.place != Place::Prompt {
            // pane agent: the 3D pane, a Tab group after the chat
            // column's transcript and composer (their group is the
            // column's index 2).
            out.push(self.divider(cx).into_any_element());
            out.push(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .tab_group()
                    .tab_index(3)
                    .child(self.world.clone())
                    .into_any_element(),
            );
        }
        out
    }
}

// narrow agent: the narrow window and its list drawer.
impl Shell {
    /// The narrow window (`style.css` S:1478-1528): the sidebar as a bar
    /// over the world, the world, a 1 px line and the chat column, one under
    /// the other (the prompt library: the bar and the column); the drawer,
    /// while it shows, over the page on its scrim, the bar's row left
    /// empty under it as the page's.
    #[allow(clippy::too_many_arguments)]
    fn narrow_body(
        &mut self,
        drawer: bool,
        width: f32,
        height: f32,
        header: AnyElement,
        content: AnyElement,
        notices: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let prompts = self.place == Place::Prompt;
        let rows = layout::narrow_rows(height, prompts);
        let sidebar = || {
            self.sidebar
                .clone()
                .cached(StyleRefinement::default().size_full())
        };
        let mut out = Vec::new();
        // The bar (row 1). Like the wide column it has no element id, so
        // the sidebar's controls keep their focus handles across 980 px.
        out.push(
            div()
                .tab_group()
                .tab_index(0)
                .flex_none()
                .w_full()
                .h(px(rows.bar))
                .bg(colour::bedrock())
                .when(!drawer, |d| d.child(sidebar()))
                .into_any_element(),
        );
        if !prompts {
            // pane agent's pane, placed by the narrow agent: row 2, then the
            // line (row 3).
            out.push(
                div()
                    .flex_none()
                    .w_full()
                    .h(px(rows.world))
                    .tab_group()
                    .tab_index(3)
                    .child(self.world.clone())
                    .into_any_element(),
            );
            out.push(
                div()
                    .flex_none()
                    .w_full()
                    .h(px(rows.divider))
                    .bg(colour::line())
                    .into_any_element(),
            );
        }
        out.push(
            div()
                .relative()
                .tab_group()
                .tab_index(0)
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .bg(colour::cave())
                .child(div().flex_none().tab_group().tab_index(0).child(header))
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .tab_group()
                        .tab_index(2)
                        .child(content),
                )
                .children(notices)
                .into_any_element(),
        );
        if drawer {
            let this = cx.entity().downgrade();
            // Both drawn after the page (the world's own overlays
            // included), under popovers (2) and tooltips (3): the scrim at
            // z-index 11 and the drawer at 12 over everything else.
            out.push(
                deferred(
                    div()
                        .id("drawer-scrim")
                        .absolute()
                        .inset_0()
                        .bg(colour::scrim())
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            cx.stop_propagation();
                            let _ = this.update(cx, |s, cx| s.set_drawer(false, window, cx));
                        }),
                )
                .with_priority(1)
                .into_any_element(),
            );
            let label = layout::sidebar_toggle_label(width, false, self.mode == Mode::Bench);
            out.push(
                deferred(
                    div()
                        .id("drawer")
                        .role(Role::Complementary)
                        .aria_label(label)
                        .track_focus(&self.narrow.region)
                        .tab_group()
                        .tab_index(0)
                        .occlude()
                        .absolute()
                        .top_0()
                        .left_0()
                        .bottom_0()
                        .w(px(layout::drawer_width(width)))
                        .shadow(shadow::panel())
                        .child(sidebar()),
                )
                .with_priority(1)
                .into_any_element(),
            );
        }
        out
    }

    /// Whether the window is narrow (980 px and below) at the last render.
    pub fn is_narrow(&self) -> bool {
        layout::narrow(self.narrow.width)
    }

    /// The window's width at the last render.
    pub fn width(&self) -> f32 {
        self.narrow.width
    }

    /// Whether the list's drawer shows.
    pub fn drawer_shown(&self) -> bool {
        self.narrow.drawer.shown(self.narrow.width)
    }

    /// Opens or closes the list's drawer (`setDrawerOpen`). Opening keeps
    /// what had focus and, unless focus is in the sidebar already (its
    /// button), puts it on the sidebar's button; closing gives focus back to
    /// what had it, or to the button.
    pub fn set_drawer(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.narrow.drawer.open == open {
            return;
        }
        self.narrow.drawer.open = open;
        self.log("drawer", serde_json::json!({ "open": open }));
        if open {
            self.narrow.opener = window.focused(cx);
        }
        self.sync_drawer(window, cx);
        if !open {
            let back = self.narrow.opener.take();
            let button = self.narrow.toggle.clone();
            match back {
                Some(handle)
                    if handle != button && !self.narrow.region.contains(&handle, window) =>
                {
                    window.focus(&handle, cx)
                }
                _ => window.focus(&button, cx),
            }
        }
        cx.notify();
    }

    /// Puts the drawer's layer and the sidebar in step with whether the
    /// drawer shows: its layer open while it shows (Escape closes it), the
    /// sidebar told to draw as a bar, a drawer or a column.
    fn sync_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.drawer_shown();
        if shown != self.narrow.shown {
            self.narrow.shown = shown;
            if shown {
                let this = cx.entity().downgrade();
                keys::open_layer(cx, Layer::Drawer, move |window, cx| {
                    let _ = this.update(cx, |s, cx| s.set_drawer(false, window, cx));
                });
                let inside = window
                    .focused(cx)
                    .is_some_and(|f| self.narrow.region.contains(&f, window));
                if !inside {
                    window.focus(&self.narrow.toggle, cx);
                }
            } else {
                keys::close_layer(cx, Layer::Drawer);
            }
        }
        // The sidebar is drawn again after this update (it may be the one
        // that asked).
        let sidebar = self.sidebar.clone();
        cx.defer(move |cx| sidebar.update(cx, |_, cx| cx.notify()));
    }

    /// The window's width changed: across 980 px the layout, the sidebar
    /// and the chat column are drawn again; the drawer's layer follows.
    fn resized(&mut self, width: f32, window: &mut Window, cx: &mut Context<Self>) {
        let was = layout::narrow(self.narrow.width);
        self.narrow.width = width;
        if was != layout::narrow(width) {
            self.log(
                "narrow",
                serde_json::json!({ "narrow": !was, "width": width }),
            );
            let chat = self.chat.clone();
            cx.defer(move |cx| chat.update(cx, |_, cx| cx.notify()));
            self.sync_drawer(window, cx);
        }
    }

    /// Tab and Shift+Tab while the drawer shows stay in it.
    fn focus_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let step = |window: &mut Window, cx: &mut App| {
            if forward {
                window.focus_next(cx)
            } else {
                window.focus_prev(cx)
            }
        };
        step(window, cx);
        if !self.drawer_shown() {
            return;
        }
        // Past its last stop the next one is outside: go on round until
        // focus is in the drawer again.
        for _ in 0..512 {
            let inside = window
                .focused(cx)
                .is_some_and(|f| self.narrow.region.contains(&f, window));
            if inside {
                return;
            }
            step(window, cx);
        }
        window.focus(&self.narrow.toggle, cx);
    }

    /// The sidebar button's focus handle.
    pub fn toggle_focus(&self) -> FocusHandle {
        self.narrow.toggle.clone()
    }
}

// start-place agent: the window opens at its start place at once, before
// the core's first lists (a recorded difference from the page's
// `startPlace`, `inventory/README.md`).
impl Shell {
    /// Opens a chat, bench or member start place before the lists: its
    /// header with no title yet, the transcript or comparison loading (its
    /// read sent now: the core reads it from the store, not from the
    /// lists), the composer live on that place's own draft, the pane's
    /// cover preparing. Other start places wait for the lists, as the
    /// page's do.
    fn open_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_start.is_some() {
            return;
        }
        let start = self.options.start.clone().unwrap_or(Place::Landing);
        // par-bench: the landing in Bench shows the new bench while the
        // lists load (`startPlace`), so what is typed there is the new
        // bench's.
        if start == Place::Landing && self.preferences.bench_mode {
            if self.moved_early || self.place == Place::NewBench {
                return;
            }
            self.opening_start = true;
            self.visit(Place::NewBench, Visit::Replace, false, window, cx);
            self.opening_start = false;
            self.chat.update(cx, |c, cx| c.focus_composer(window, cx));
            return;
        }
        if !matches!(
            start,
            Place::Chat { .. } | Place::Compare { .. } | Place::Member { .. }
        ) {
            // The interim new chat shown meanwhile is ready to type in, as
            // the page's is (never focus on nothing).
            if window.focused(cx).is_none() {
                self.chat.update(cx, |c, cx| c.focus_composer(window, cx));
            }
            return;
        }
        self.pending_start = Some(start.clone());
        self.log(
            "start place",
            serde_json::json!({ "place": crate::state::address::address(&start) }),
        );
        self.opening_start = true;
        self.visit(start, Visit::Replace, false, window, cx);
        self.opening_start = false;
        self.chat.update(cx, |c, cx| c.focus_composer(window, cx));
    }

    /// The start place as the first lists name it (`listedPlace`).
    fn listed_start(&self, target: &Place, cx: &App) -> Place {
        let lists = self.lists.read(cx);
        let chats: Vec<ListedChat> = lists
            .all()
            .iter()
            .map(|c| ListedChat {
                id: &c.summary.id.0,
                bench: c.summary.bench_id.as_deref(),
            })
            .collect();
        let benches: Vec<&str> = lists.benches().iter().map(|b| b.id.as_str()).collect();
        let address = crate::state::address::address(target);
        crate::state::address::listed_place(target, &address, &chats, &benches)
    }

    /// The lists name the start place opened before them: it goes on as it
    /// is (no second open, the draft, scroll and focus kept); what needed
    /// the lists is filled in (the comparison's round, the title).
    fn continue_start(&mut self, place: &Place, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(place, Place::Compare { .. } | Place::Member { .. }) {
            let place = place.clone();
            self.bench_pane.update(cx, |b, cx| b.show(place, cx));
        }
        self.tell_sidebar(window, cx);
        self.sync_title(window, cx);
        // pane agent's call: a comparison's world is its newest round's.
        self.sync_world(window, cx);
        self.save();
    }
}

// pickers: what the pickers ask of the shell.
impl Shell {
    /// Remembers a choice made in the composer with the window's
    /// preferences (`mcb-last-model`, `mcb-last-resolution`).
    pub fn remember_choice(&mut self, choice: super::pickers::catalog::Remember) {
        use super::pickers::catalog::Remember;
        match choice {
            Remember::Model(provider, model) => {
                self.preferences.last_model = Some((provider, model))
            }
            Remember::Resolution(resolution) => self.preferences.last_resolution = resolution,
        }
        self.save();
    }

    // pane agent: what the 3D pane reads of the shell, and the round a
    // bench comparison shows (the bench agent's place calls it).
    /// The 3D pane.
    pub fn world(&self) -> Entity<super::world::Pane> {
        self.world.clone()
    }

    /// The lists the core published.
    pub fn lists_entity(&self) -> Entity<Lists> {
        self.lists.clone()
    }

    /// Shows a notice (`showError`), as `Update::Error` does.
    pub fn notice(&mut self, text: String, cx: &mut Context<Self>) {
        self.notices.retain(|n| *n != text);
        self.notices.push(text.clone());
        self.announcer.update(cx, |a, cx| a.assertive(text, cx));
        cx.notify();
    }

    // par-chat (stage four's parity pass)
    /// New chat or New bench as the header, the sidebar, the missing
    /// screen, the palette and the `new` shortcut ask for it
    /// (`newConversation`, `showBenchView({ kind: 'new-bench' })`): a new
    /// chat asked for in Build with no model chosen is refused with the
    /// page's notice and the view stays where it is (from Bench the
    /// palette's New chat goes to Build's new chat unchecked, `goBuild`).
    ///
    /// A new chat asked for in Build also gets its row on the core at once
    /// (`createdChat`, `ui-seam-parity.md` item 1), so its world shows
    /// before its first message; on an untouched new chat it only gives
    /// the composer focus.
    pub fn new_here(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        if !(matches!(place, Place::NewChat { .. }) && self.mode() == Mode::Build) {
            return self.go(place, window, cx);
        }
        let chat = self.chat.read(cx);
        if !chat.pickers().read(cx).choice().chosen() {
            return self.notice(NO_MODEL_FOR_NEW_CHAT.to_string(), cx);
        }
        let untouched = matches!(self.place, Place::NewChat { .. })
            && chat.items().is_empty()
            && !chat.busy(cx)
            && !chat.creating();
        if untouched {
            self.chat.update(cx, |c, cx| c.focus_composer(window, cx));
            return self.chat.update(cx, |c, cx| c.create_row(cx));
        }
        self.go(place, window, cx);
        self.chat.update(cx, |c, cx| c.create_row(cx));
    }

    /// `Update::ChatCreated`: the new chat on screen takes its row (the same
    /// history entry; its world shows), or the route's text is a notice.
    fn chat_created(
        &mut self,
        result: Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(text) = &result {
            self.notice(text.clone(), cx);
        }
        let made = self
            .chat
            .update(cx, |c, cx| c.chat_created(result, window, cx));
        if let Some(id) = made
            && self.place == (Place::NewChat { id: None })
        {
            let place = crate::state::place::with_row(&self.place, &id);
            self.history.visit(place.clone(), Visit::Replace);
            self.place = place.clone();
            self.left.insert(Mode::Build, place);
            self.tell_sidebar(window, cx);
            self.sync_world(window, cx);
        }
    }

    /// The lists show the new chat's row (it got a message, perhaps from a
    /// send the window gave up on): it is settled, and the new chat on
    /// screen becomes that chat (`settleNewChat`).
    fn settle_new_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.chat.read(cx).new_row_id() else {
            return;
        };
        if self.lists.read(cx).chat(&id).is_none() {
            return;
        }
        self.chat.update(cx, |c, _| c.settle_row(&id));
        if self.place
            == (Place::NewChat {
                id: Some(id.clone()),
            })
        {
            self.chat.update(cx, |c, cx| c.adopt(id.clone(), cx));
            let place = crate::state::place::as_chat(&self.place);
            self.history.visit(place.clone(), Visit::Replace);
            self.place = place.clone();
            self.left.insert(Mode::Build, place);
            self.tell_sidebar(window, cx);
        }
    }
    // end of par-chat's block
}

// par-chat: `newConversation`'s refusal (`web/app.js` 4696).
/// New chat with no model chosen.
pub const NO_MODEL_FOR_NEW_CHAT: &str = "Choose a model before starting a chat";
