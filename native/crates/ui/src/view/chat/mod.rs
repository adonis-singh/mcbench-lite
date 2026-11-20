//! The chat pane: the transcript ([`transcript`]) and the composer
//! ([`composer`]) of the chat on screen, or of the new chat. The shell
//! (`view::shell`) puts it in the chat column under the pane header, tells
//! it which chat to show ([`ChatPane::show_chat`]) and hands it the core's
//! transcript and turn updates ([`ChatPane::apply`]). Its accessibility
//! nodes are the rows of `ACCESSIBILITY.md` from Transcript to Stop.
//!
//! | Part | Where |
//! |---|---|
//! | the turn's phases (`transcript.mjs`), the resync after a turn, joining a running turn | here, [`ChatPane::apply`], [`ChatPane::lists_changed`] |
//! | the transcript's rows, the working line, the empty chat, the read that failed | [`transcript`] |
//! | a user message's files, selections and tools (copy, edit and branch, branch arrows) | [`message`] |
//! | the composer box, send, stop, the held send, the editing line, type-ahead | [`composer`] |
//! | attachments: the picker, paste, reading, the tiles, the selection chips | [`attachments`] |
//! | the lightbox (a layer) | [`lightbox`] |
//! | the footer's pickers and context meter (the `pickers` agent's) | [`controls`] |

pub mod attachments;
// bench agent: the composer in Bench.
pub mod bench;
pub mod composer;
// bench agent: a library prompt in the composer.
pub mod prompt_parts;
// bench agent: the palette over the composer.
pub mod controls;
pub mod lightbox;
pub mod message;
pub mod palette;
pub mod transcript;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::*;

use super::markdown::Fonts;
use super::shell::announce::Announcer;
use super::shell::lists::Lists;
use super::text_leaf::HitMap;
use crate::state::branches::{self, Branch};
use crate::state::choice::Choice;
use crate::state::doc::Leaf;
use crate::state::draft::{self, Draft, Drafts};
use crate::state::rows::{self, Parsed, Row};
use crate::state::seam;
use crate::state::selection::Selection;
use crate::state::transcript::{self as items, Item, ReadState};
use crate::state::turn::{self, IDLE, Turn};
use crate::view::pickers::footer::FooterPickers;
use crate::{ActionLog, Command, Core, Step, Target, Update};

/// What the pane needs from the shell.
pub struct ChatOptions {
    /// The core.
    pub core: Arc<dyn Core>,
    /// Where user-visible actions are recorded, when given.
    pub log: Option<ActionLog>,
    /// Opens a link (the browser in the app; only the log in the dev binary).
    pub opener: super::Opener,
    /// The lists the core publishes (a chat's model).
    pub lists: Entity<Lists>,
    /// The window's live regions.
    pub announcer: Entity<Announcer>,
    // chat (stage four's chat agent)
    /// Shows a notice (`showError`): the shell's notices.
    pub notice: Notice,
    /// Picks files to attach.
    pub picker: super::Picker,
    /// The stored preferences: the pickers start from their last model and
    /// resolution.
    pub preferences: crate::Preferences,
    /// `Connect an AI to start building`: opens Settings on Connect an AI.
    pub connect: super::Press,
    /// The empty chat's `Bench` link: Bench, with the draft (`benchFromBuild`).
    pub to_bench: super::Press,
    /// Selections the new chat's draft starts with: the viewer's, once it
    /// makes them; the stand-in's (`--select`) until then.
    pub selections: Vec<crate::Selection>,
    // seen: the turn's end goes to the sidebar, which marks the chat seen.
    /// A turn of the open chat ended (`turnEnded`): the chat's id.
    pub turn_ended: TurnEnded,
    // bench agent: the new bench's `Build` link (`buildFromBench`), what
    // the line-up saves, and the comparison a new bench opens.
    /// `Or have one model build it in`'s `Build`: the new chat, with the draft.
    pub to_build: super::Press,
    /// Saves the new bench's line-up, Blind and resolution.
    pub keep_bench: crate::view::bench::KeepBench,
    /// A new bench was made: its comparison opens.
    pub bench_opened: crate::view::bench::Opened,
    /// The palette's `Edit`: the prompt opens in the library.
    pub edit_prompt: crate::view::bench::Opened,
    /// `Save message as prompt` saved one: the library opens on it.
    pub open_prompts: super::Press,
}

/// Shows a notice (`showError`).
pub type Notice = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// A turn of the open chat ended: the chat's id.
pub type TurnEnded = Rc<dyn Fn(String, &mut Window, &mut App)>;

/// A picture the lightbox or a thumbnail shows.
#[derive(Clone)]
pub enum Picture {
    /// A file (a stored picture, a shot).
    File(std::path::PathBuf),
    /// Bytes in memory (a file just attached or sent).
    Bytes(Arc<Image>),
}

/// A send handed to the core and not yet taken or refused: what goes back
/// if it is refused.
pub(crate) struct Pending {
    /// Where it went.
    pub target: Target,
    /// The transcript before it.
    pub before: Vec<Item>,
    /// The draft it was.
    pub draft: Draft,
    /// The draft key of the place it was written in.
    pub key: String,
}

// par-chat (stage four's parity pass)
/// The new chat's row on the core (the page's `newChatRef`): made once,
/// when New chat asks for it, and the new chat's for as long as it is new
/// (however often the view leaves it and comes back), so no second row is
/// made; settled by the chat's first message or when the lists show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NewRow {
    /// None asked for.
    None,
    /// `Command::CreateChat` is on its way.
    Asked,
    /// The row's id.
    Made(String),
}

/// The chat pane.
pub struct ChatPane {
    pub(crate) options: Rc<ChatOptions>,
    /// The chat on screen; `None` is the new chat.
    pub(crate) open: Option<String>,
    /// A new chat's first message is on its way.
    pub(crate) creating: bool,
    pub(crate) items: Vec<Item>,
    /// The turn's phase (`state::turn`).
    pub(crate) phase: Turn,
    /// The transcript's read.
    pub(crate) read: ReadState,
    /// Each branchable message's place among its versions.
    pub(crate) branches: HashMap<String, Branch>,
    /// A branch switch waits for the core: the next `Transcript` or
    /// `TranscriptFailed` of this chat ends it (`Command::Branch`'s rule),
    /// and nothing else but leaving the chat.
    pub(crate) branching: bool,
    /// The row copied in the last 1.2 s, and which copy it was.
    pub(crate) copied: Option<(String, u64)>,
    pub(crate) copies: u64,
    pub(crate) open_folds: HashSet<String>,
    pub(crate) parsed: Parsed,
    pub(crate) rows: Vec<Row>,
    pub(crate) leaves: Vec<Vec<Leaf>>,
    pub(crate) row_focus: HashMap<String, FocusHandle>,
    pub(crate) current_row: Option<usize>,
    pub(crate) parts: HashMap<String, FocusHandle>,
    pub(crate) list: ListState,
    /// The list's items: the rows, then the working line while it shows.
    pub(crate) list_count: usize,
    pub(crate) transcript_focus: FocusHandle,
    pub(crate) selection: Option<Selection>,
    pub(crate) dragging: bool,
    pub(crate) hits: Rc<RefCell<HitMap>>,
    pub(crate) composer: Entity<TextareaState>,
    pub(crate) composer_empty: bool,
    /// The composer's draft but its text (the text is the composer's).
    pub(crate) draft: Draft,
    /// The drafts of the places not on screen.
    pub(crate) drafts: Drafts,
    /// Picks made so far, for each pick's number.
    pub(crate) picks: u64,
    /// The footer's pickers (the `pickers` agent's), which hold the
    /// message's choice a send reads.
    pub(crate) pickers: Entity<FooterPickers>,
    /// The draft key whose send is held until setup is ready.
    pub(crate) held: Option<String>,
    /// The setup steps, once the core published them.
    pub(crate) setup: Option<Vec<Step>>,
    /// The step that needed the person at the last update (settings agent,
    /// s4-needs): its line stays, with the working dots, while its work runs.
    pub(crate) needs_step: Option<String>,
    /// Whether an AI is connected, once the providers arrived.
    pub(crate) connected: Option<bool>,
    /// The placeholder's idea for this chat view.
    pub(crate) idea: &'static str,
    /// The chat column shows this pane (a chat or the new chat is the place).
    pub(crate) shown: bool,
    /// The lightbox's picture, and where focus goes when it closes.
    pub(crate) lightbox: Option<(Picture, Option<FocusHandle>)>,
    pub(crate) lightbox_focus: FocusHandle,
    pub(crate) send_focus: FocusHandle,
    pub(crate) stop_focus: FocusHandle,
    /// The send on its way.
    pub(crate) pending: Option<Pending>,
    /// Stop was asked while the send was on its way.
    pub(crate) stop_asked: bool,
    /// The next rebuild ends at the newest content (a send, an opened chat,
    /// a branch switch: the page's recorded height reset to 0).
    pub(crate) pin_next: bool,
    /// The turn in progress: the index of its first item, and whether it
    /// failed. Its answer is announced when it ends, if it has one.
    pub(crate) turn: Option<(usize, bool)>,
    /// The working line's clock, running only while a turn does.
    pub(crate) ticker: Option<Task<()>>,
    /// The chat whose turn the window reads, from its `Update::TurnJoined`
    /// on: its turn's events are applied, every other chat's are dropped.
    pub(crate) joined: Option<String>,
    /// The `Update::Transcript` after `TurnJoined` is the state at the
    /// turn's start: the turn's live items are rebuilt from it.
    pub(crate) snapshot_due: bool,
    /// The window is active: the working line's clock ticks.
    pub(crate) active: bool,
    /// Animations for looks move (`view::motion`): the working line's dots
    /// step.
    pub(crate) moving: bool,
    pub(crate) fonts: Fonts,
    /// Rows drawn (`render_row` calls), for the resize measurement.
    pub(crate) rows_drawn: usize,
    renders: usize,
    // bench agent: the Bench side (the new bench, a comparison), and the
    // library prompts the composer's message came from.
    pub(crate) bench: bench::BenchSide,
    pub(crate) inserted: Vec<crate::PromptLink>,
    /// The box the caret was last said to be in.
    pub(crate) field_said: Option<usize>,
    // start-place agent: the open chat's choice waits for its listed row.
    pickers_pending: bool,
    // narrow agent: the window is narrow (980 px and below): the
    // transcript's and the composer's paddings are smaller.
    pub(crate) narrow: bool,
    pub(crate) palette: Option<palette::PaletteState>,
    pub(crate) palette_sub: Option<Subscription>,
    // par-chat: where the composer's box was last painted, for the
    // palette's outside press (`composerBoxRef.current.contains`).
    pub(crate) composer_box: std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    // par-chat: each tool call's code box scroll (`markdown::RowEnv`).
    pub(crate) code_scrolls: HashMap<String, ScrollHandle>,
    // par-chat: the new chat's row (`newChatRef`), and a send from the new
    // chat that waits for it.
    pub(crate) new_row: NewRow,
    pub(crate) waiting_send: Option<crate::Outgoing>,
    _subscriptions: Vec<Subscription>,
}

impl ChatPane {
    /// The pane, on the new chat until the shell shows a chat.
    pub fn new(options: ChatOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let idea = crate::state::ideas::idea_now();
        let composer = cx.new(|cx| {
            // `.composer-input`: 46 px to 220 px at 20 px lines (2 to 10 rows
            // with its padding), then it scrolls.
            TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .submit_on_enter(true)
                .placeholder(idea)
        });
        let composer_for_fields = composer.clone();
        let subscription = cx.subscribe_in(
            &composer,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    // bench agent: a `/query` at the caret.
                    this.palette_follow(cx);
                    let empty = this.composer.read(cx).value().trim().is_empty();
                    if empty != this.composer_empty {
                        this.composer_empty = empty;
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { shift: false, .. } if this.palette.is_none() => {
                    this.submit(window, cx)
                }
                _ => {}
            },
        );
        let theme = cx.global::<gpui_kit::component::Theme>();
        let fonts = Fonts {
            ui: theme.font_family.clone(),
            mono: theme.mono_font_family.clone(),
        };
        let list = ListState::new(0, ListAlignment::Bottom, px(600.)).keep_neighbours();
        let pane = cx.entity().downgrade();
        list.set_scroll_handler(move |event, window, cx| {
            // Keep the current message among the rows laid out, so the key
            // that moves it always finds it (the list keeps a focused row).
            let visible = event.visible_range.clone();
            let _ = pane.update(cx, |this, cx| {
                if let Some(current) = this.current_row {
                    let focused = this
                        .rows
                        .get(current)
                        .and_then(|r| this.row_focus.get(&r.key))
                        .is_some_and(|f| f.contains_focused(window, cx));
                    if !focused
                        && !visible.contains(&current)
                        && !visible.is_empty()
                        && visible.start < this.rows.len()
                    {
                        this.current_row = Some(visible.start);
                        cx.notify();
                    }
                }
            });
        });
        let type_ahead = composer::type_ahead(cx.entity().downgrade(), cx);
        // chat-fix (W6): the working line's dots move and its clock ticks
        // only while the window is active (`view::motion`).
        let activation = cx.observe_window_activation(window, |this: &mut Self, window, cx| {
            this.moving = crate::view::motion::moving(window, cx);
            this.active = window.is_window_active();
            cx.notify();
        });
        let pickers = cx.new(|cx| {
            FooterPickers::new(
                crate::view::pickers::catalog::Catalog::installed(cx),
                options.preferences.clone(),
                window,
                cx,
            )
        });
        let selections = options.selections.clone();
        let bench_side = bench::BenchSide::new(
            &options.preferences,
            cx.entity().downgrade(),
            options.keep_bench.clone(),
            options.bench_opened.clone(),
            window,
            cx,
        );
        let pane = ChatPane {
            options: Rc::new(options),
            open: None,
            creating: false,
            items: Vec::new(),
            phase: IDLE,
            read: ReadState::Shown,
            branches: HashMap::new(),
            branching: false,
            copied: None,
            copies: 0,
            open_folds: HashSet::new(),
            parsed: Parsed::default(),
            rows: Vec::new(),
            leaves: Vec::new(),
            row_focus: HashMap::new(),
            current_row: None,
            parts: HashMap::new(),
            list,
            list_count: 0,
            transcript_focus: cx.focus_handle(),
            selection: None,
            dragging: false,
            hits: Rc::new(RefCell::new(HitMap::default())),
            composer,
            composer_empty: true,
            draft: Draft {
                selections,
                ..draft::empty()
            },
            drafts: Drafts::default(),
            picks: 0,
            pickers,
            held: None,
            setup: None,
            needs_step: None,
            connected: None,
            idea,
            shown: true,
            lightbox: None,
            lightbox_focus: cx.focus_handle().tab_stop(true),
            send_focus: cx.focus_handle().tab_stop(true),
            stop_focus: cx.focus_handle().tab_stop(true),
            pending: None,
            stop_asked: false,
            pin_next: false,
            turn: None,
            ticker: None,
            joined: None,
            snapshot_due: false,
            fonts,
            rows_drawn: 0,
            renders: 0,
            active: window.is_window_active(),
            moving: crate::view::motion::moving(window, cx),
            bench: bench_side,
            inserted: Vec::new(),
            field_said: None,
            pickers_pending: false,
            narrow: false,
            palette: None,
            palette_sub: None,
            composer_box: Default::default(),
            code_scrolls: HashMap::new(),
            new_row: NewRow::None,
            waiting_send: None,
            // bench agent: entering a prompt's box says its name.
            _subscriptions: vec![
                subscription,
                type_ahead,
                activation,
                cx.observe(&composer_for_fields, |this: &mut Self, _, cx| {
                    this.say_field(cx)
                }),
            ],
        };
        // Escape with no layer open cancels an edit (`cancelEdit`).
        let this = cx.entity().downgrade();
        super::keys::on_escape(cx, move |window, cx| {
            let _ = this.update(cx, |pane, cx| {
                if pane.draft.edit.is_some() {
                    pane.cancel_edit(window, cx);
                }
            });
        });
        pane
    }

    pub(crate) fn log(&self, action: &str, fields: serde_json::Value) {
        if let Some(log) = &self.options.log {
            log.record(action, fields);
        }
    }

    /// Shows a notice (`showError`).
    pub(crate) fn notice(&self, text: String, window: &mut Window, cx: &mut App) {
        (self.options.notice)(text, window, cx);
    }

    pub(crate) fn model(&self, cx: &App) -> String {
        self.open
            .as_ref()
            .and_then(|id| self.options.lists.read(cx).chat(id))
            .map(|c| c.model.clone())
            .unwrap_or_else(|| "Assistant".to_string())
    }

    /// The chat on screen, none for the new chat.
    pub fn open_chat(&self) -> Option<&str> {
        self.open.as_deref()
    }

    /// A new chat's first message is on its way.
    pub fn creating(&self) -> bool {
        self.creating
    }

    // par-chat (stage four's parity pass): the new chat's row.
    /// The new chat's row (`createdChat`): asked of the core once, with the
    /// picker's model, unless one is asked for or made already.
    pub fn create_row(&mut self, cx: &mut Context<Self>) {
        if self.new_row != NewRow::None {
            return;
        }
        let choice = self.pickers.read(cx).choice().clone();
        self.new_row = NewRow::Asked;
        self.log(
            "create chat",
            serde_json::json!({ "provider": choice.provider, "model": choice.model }),
        );
        self.options.core.send(Command::CreateChat {
            provider: choice.provider,
            model: choice.model,
        });
    }

    /// The transcript list's scroll as last laid out: the offset from the
    /// top and the largest offset (for the 48 px follow rule's tests).
    pub fn transcript_scroll(&self) -> (f32, f32) {
        (
            -f32::from(self.list.scroll_px_offset_for_scrollbar().y),
            f32::from(self.list.max_offset_for_scrollbar().y),
        )
    }

    /// Whether a copied message's icon shows its check (`copied`, 1,200 ms).
    pub fn shows_copied(&self) -> bool {
        self.copied.is_some()
    }

    /// The new chat's row, once made.
    pub fn new_row_id(&self) -> Option<String> {
        match &self.new_row {
            NewRow::Made(id) => Some(id.clone()),
            _ => None,
        }
    }

    /// `Update::ChatCreated`: the row is the new chat's (a send waiting for
    /// it goes now, to it), or the kept request is cleared so the next New
    /// chat or send tries again (a waiting send is refused as the page's
    /// `send` fails when `createdChat()` does). Returns the id when the new
    /// chat took it. An answer to a request no longer kept changes nothing.
    pub fn chat_created(
        &mut self,
        result: Result<String, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        if self.new_row != NewRow::Asked {
            return None;
        }
        match result {
            Ok(id) => {
                self.new_row = NewRow::Made(id.clone());
                if let Some(mut outgoing) = self.waiting_send.take() {
                    let Target::NewChat { provider, model } = outgoing.target.clone() else {
                        panic!("a send waiting for the new chat's row goes to the new chat")
                    };
                    outgoing.target = Target::Chat {
                        id: id.clone(),
                        provider,
                        model,
                    };
                    if let Some(pending) = self.pending.as_mut() {
                        pending.target = outgoing.target.clone();
                    }
                    self.options.core.send(Command::Send(outgoing));
                }
                Some(id)
            }
            Err(_) => {
                self.new_row = NewRow::None;
                if let Some(outgoing) = self.waiting_send.take() {
                    self.refused(&outgoing.target, window, cx);
                }
                None
            }
        }
    }

    /// The new chat's row reached the lists (its first message): it is a
    /// chat like any other, and the next new chat gets its own row
    /// (`settleNewChat`).
    pub fn settle_row(&mut self, id: &str) {
        if self.new_row == NewRow::Made(id.to_string()) {
            self.new_row = NewRow::None;
        }
    }
    // end of par-chat's block

    /// The current message: the transcript's Tab stop.
    pub fn current_row(&self) -> Option<usize> {
        self.current_row
    }

    // chat-fix-2: for the tests of rows kept out of view.
    /// How many rows the transcript has.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The turn's phase.
    pub fn phase(&self) -> &Turn {
        &self.phase
    }

    /// The transcript's items as shown (the stored messages and the live
    /// turn's), for tests.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The composer's draft, its text from the composer.
    pub fn current_draft(&self, cx: &App) -> Draft {
        Draft {
            text: self.composer.read(cx).value().to_string(),
            ..self.draft.clone()
        }
    }

    /// The footer's pickers.
    pub fn pickers(&self) -> Entity<FooterPickers> {
        self.pickers.clone()
    }

    /// The parked drafts.
    pub fn drafts(&self) -> &Drafts {
        &self.drafts
    }

    /// The composer's state, for tests that drive it as a platform's input
    /// method does.
    pub fn composer(&self) -> Entity<TextareaState> {
        self.composer.clone()
    }

    /// How many times the pane has rendered.
    pub fn render_count(&self) -> usize {
        self.renders
    }

    /// How many transcript rows have been drawn.
    pub fn rows_drawn(&self) -> usize {
        self.rows_drawn
    }

    /// The draft key of the place on screen (`chat:new`, `chat:<id>`).
    pub fn draft_key(&self) -> String {
        // bench agent: a Bench surface's draft.
        if let Some(key) = self.bench_draft_key() {
            return key;
        }
        match &self.open {
            Some(id) => crate::state::place::chat_draft(id),
            None => crate::state::place::NEW_CHAT_DRAFT.to_string(),
        }
    }

    /// The list row's running turn of the chat on screen.
    pub(crate) fn row_running(&self, cx: &App) -> Option<i64> {
        let id = self.open.as_deref()?;
        self.options
            .lists
            .read(cx)
            .all()
            .iter()
            .find(|c| c.summary.id.0 == id)
            .and_then(|c| c.running_since)
    }

    /// A bench model's chat: it reads, never edits.
    pub(crate) fn read_only(&self, cx: &App) -> bool {
        self.open
            .as_ref()
            .and_then(|id| self.options.lists.read(cx).chat(id))
            .is_some_and(|c| c.bench_id.is_some())
    }

    /// Whether a turn runs on the chat on screen (`turnRunning`).
    pub fn busy(&self, cx: &App) -> bool {
        turn::turn_running(&self.phase, self.row_running(cx))
    }

    /// The working line's start, when it shows: the turn's start as the
    /// core said it, or the list row's.
    pub(crate) fn ticker_since(&self, cx: &App) -> Option<i64> {
        if !self.busy(cx) {
            return None;
        }
        match &self.phase {
            Turn::Reading {
                since: Some(since), ..
            } => Some(*since),
            _ => self.row_running(cx),
        }
    }

    /// The working line's text now.
    pub(crate) fn working_line(&self, cx: &App) -> Option<String> {
        let since = self.ticker_since(cx)?;
        let status = match &self.phase {
            Turn::Detached { .. } => Some(turn::DETACHED_STATUS.to_string()),
            Turn::Reading { status, .. } => status.clone(),
            _ => None,
        };
        Some(turn::working_line(status.as_deref(), now_ms() - since))
    }

    /// Takes a transcript or turn update for the chat on screen.
    pub fn apply(&mut self, update: Update, window: &mut Window, cx: &mut Context<Self>) {
        let ours = |chat: &str, this: &Self| this.open.as_deref() == Some(chat);
        match update {
            Update::Transcript {
                chat,
                messages,
                tree,
                ..
            } => {
                if ours(&chat, self) {
                    self.pickers.update(cx, |p, cx| p.transcript(&messages, cx));
                    self.items = items::items_from_messages(&seam::from_stored(&messages));
                    // The snapshot a join sends: the turn's items start
                    // after it (the answer of the window's own send is
                    // announced from there).
                    if std::mem::take(&mut self.snapshot_due) && self.turn.is_some() {
                        self.turn = Some((self.items.len(), false));
                    }
                    self.branches = branches::branch_navigation(&tree);
                    self.branching = false;
                    self.read = ReadState::Shown;
                    self.rebuild(cx);
                }
            }
            Update::TranscriptFailed { chat, error } => {
                if ours(&chat, self) {
                    self.read = ReadState::Failed(error);
                    self.branching = false;
                    self.rebuild(cx);
                }
            }
            // The send was taken, or the join answered: the turn's view is
            // reset and rebuilt from the snapshot that follows (the chat's
            // transcript at the turn's start), then the events so far, then
            // the live ones. It holds also for a turn the window's own send
            // began and joins again.
            Update::TurnJoined { chat, since } => {
                if ours(&chat, self) {
                    self.joined = Some(chat.clone());
                    self.snapshot_due = true;
                    let status = match &self.phase {
                        Turn::Reading { status, .. } => status.clone(),
                        _ => None,
                    };
                    self.phase = turn::turn_to(
                        &self.phase,
                        Turn::Reading {
                            status,
                            since: Some(since),
                        },
                    );
                    self.pending = None;
                    self.creating = false;
                    if std::mem::take(&mut self.stop_asked) {
                        self.options.core.send(Command::Stop { chat });
                    }
                    self.start_ticker(cx);
                    self.rebuild(cx);
                }
            }
            // Joined too late: the turn ended; the chat is read again.
            Update::TurnNotRunning { chat } => {
                if ours(&chat, self) {
                    self.end_turn(chat, true, window, cx);
                }
            }
            Update::Turn { chat, event } => {
                // The core publishes every running turn's events; the window
                // applies only those of the turn it joined (its own send's
                // included, which the core answers with `TurnJoined`).
                if self.joined.as_deref() != Some(chat.as_str()) || !ours(&chat, self) {
                    return;
                }
                match &event {
                    mcbench_agent::TurnEvent::HarnessStatus(status) => {
                        let since = match &self.phase {
                            Turn::Reading { since, .. } => *since,
                            _ => None,
                        };
                        self.phase = turn::turn_to(
                            &self.phase,
                            Turn::Reading {
                                status: status.clone(),
                                since,
                            },
                        );
                        cx.notify();
                        return;
                    }
                    // The core follows the end with the chat's transcript
                    // and the list.
                    mcbench_agent::TurnEvent::Done => {
                        return self.end_turn(chat, false, window, cx);
                    }
                    mcbench_agent::TurnEvent::Error { message, .. } => {
                        if let Some(turn) = self.turn.as_mut() {
                            turn.1 = true;
                        }
                        let message = message.clone();
                        self.options
                            .announcer
                            .update(cx, |a, cx| a.assertive(message, cx));
                        items::apply_event(&mut self.items, &event);
                        return self.end_turn(chat, false, window, cx);
                    }
                    _ => {}
                }
                items::apply_event(&mut self.items, &event);
                self.rebuild(cx);
            }
            other => panic!("the chat pane takes transcripts and turns, not {other:?}"),
        }
        self.sync_placeholder(window, cx);
        cx.notify();
    }

    /// The turn ended (`finishTurn`): idle with its start seen to end, its
    /// answer announced, the sidebar told (it marks the chat seen if the
    /// window is visible, `turnEnded`); the stored transcript replaces
    /// the live items: the core sends it after `Done` or `Error`, and
    /// `read_again` asks for it (`Command::Open`) when the turn ended
    /// unread (not running when joined, or a detached turn the list stopped
    /// showing).
    fn end_turn(
        &mut self,
        chat: String,
        read_again: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.joined = None;
        self.snapshot_due = false;
        let ended = match &self.phase {
            Turn::Reading { since, .. } => *since,
            Turn::Detached { started_at } => *started_at,
            _ => None,
        };
        self.phase = turn::turn_to(&self.phase, Turn::Idle { ended });
        self.ticker = None;
        // The natural current message after a turn is its last one (unless
        // a message has focus).
        let focused = self.rows.iter().any(|row| {
            self.row_focus
                .get(&row.key)
                .is_some_and(|h| h.contains_focused(window, cx))
        });
        if !focused {
            self.current_row = None;
        }
        // This turn's answer as it reads, without its markdown; nothing for
        // a failed turn or a turn with no answer.
        if let Some((start, false)) = self.turn.take() {
            let answer = self.items[start.min(self.items.len())..]
                .iter()
                .rev()
                .find_map(|i| match i {
                    Item::Assistant { text, .. } => Some(text.clone()),
                    _ => None,
                });
            if let Some(answer) = answer {
                let spoken = crate::state::doc::spoken_blocks(&crate::state::doc::parse(&answer));
                self.options
                    .announcer
                    .update(cx, |a, cx| a.polite(spoken, cx));
            }
        }
        self.turn = None;
        self.rebuild(cx);
        (self.options.turn_ended)(chat.clone(), window, cx);
        if read_again {
            self.options.core.send(Command::Open { chat });
        }
        self.sync_placeholder(window, cx);
        cx.notify();
    }

    /// The lists changed: a running turn not read is joined (`turnToJoin`),
    /// a detached one that the list stopped showing ends (`turnOver`).
    pub fn lists_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.open.clone() else {
            return;
        };
        // start-place agent: the chat opened before the lists now has its
        // row: the pickers take its own choice.
        if self.pickers_pending
            && let Some(place) = self.own_choice(&chat, cx)
        {
            self.pickers_pending = false;
            self.pickers
                .update(cx, |p, cx| p.show_place(Some(place), cx));
        }
        let running = self.row_running(cx);
        if turn::turn_over(&self.phase, running) {
            return self.end_turn(chat, true, window, cx);
        }
        if let Some(since) = turn::turn_to_join(&self.phase, running) {
            self.phase = turn::turn_to(
                &self.phase,
                Turn::Reading {
                    status: None,
                    since: Some(since),
                },
            );
            self.log("joined", serde_json::json!({ "chat": chat }));
            self.options.core.send(Command::JoinTurn { chat });
            self.start_ticker(cx);
        }
        self.rebuild(cx);
        cx.notify();
    }

    /// The working line's clock: a redraw a second while a turn runs, and
    /// none after (an untouched window draws nothing).
    pub(crate) fn start_ticker(&mut self, cx: &mut Context<Self>) {
        if self.ticker.is_some() {
            return;
        }
        self.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let going = this
                    .update(cx, |this, cx| {
                        let going = this.busy(cx);
                        // An inactive window's clock stops; activation
                        // redraws it at the time then.
                        if going && this.active {
                            cx.notify();
                        } else {
                            this.ticker = None;
                        }
                        going
                    })
                    .unwrap_or(false);
                if !going {
                    break;
                }
            }
        }));
    }

    /// Recomputes the rows from the items and the open folds, tells the
    /// list which rows changed, and keeps the newest content in view when
    /// it was (5.8: within 48 px of the bottom of the content before).
    pub(crate) fn rebuild(&mut self, cx: &mut Context<Self>) {
        let busy = self.busy(cx);
        let read_only = self.read_only(cx);
        let entries = items::group_items(&self.items, busy);
        let keys = items::entry_keys(&entries);
        let copied = self.copied.as_ref().map(|(key, _)| key.clone());
        let context = rows::Context {
            branches: &self.branches,
            busy,
            changing: self.branching || self.draft.edit.is_some(),
            edited: self.draft.edit.as_ref().map(|e| e.of.as_str()),
            copied: copied.as_deref(),
            read_only,
            several: self.items.len() > 1,
        };
        let rows = rows::rows(
            &entries,
            &keys,
            &self.open_folds,
            &mut self.parsed,
            &context,
        );
        let keep: HashSet<String> = self
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Assistant { text, .. } | Item::Reasoning { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        self.parsed.retain_texts(&keep);
        let same = self
            .rows
            .iter()
            .zip(&rows)
            .take_while(|(a, b)| a == b)
            .count();
        for row in &rows {
            self.row_focus
                .entry(row.key.clone())
                .or_insert_with(|| cx.focus_handle());
        }
        let ticker = self.ticker_since(cx).is_some();
        let mut handles: Vec<Option<FocusHandle>> = rows[same..]
            .iter()
            .map(|r| self.row_focus.get(&r.key).cloned())
            .collect();
        if ticker {
            handles.push(None);
        }
        // Pinned: the view was within 48 px of the content's end as it was
        // last laid out, or the next update was asked to jump there.
        let offset = -f32::from(self.list.scroll_px_offset_for_scrollbar().y);
        let max = f32::from(self.list.max_offset_for_scrollbar().y);
        let pinned = std::mem::take(&mut self.pin_next) || offset >= max - 48.;
        self.list_count = rows.len() + usize::from(ticker);
        self.list
            .splice_focusable(same..self.list.item_count(), handles);
        if pinned {
            self.list.scroll_to_end();
        }
        self.leaves = rows.iter().map(Row::leaves).collect();
        self.rows = rows;
        if let Some(selection) = self.selection
            && selection.ordered().1.row >= self.rows.len()
        {
            self.selection = None;
        }
        self.current_row = match self.current_row {
            Some(i) if i < self.rows.len() => Some(i),
            _ => self.rows.len().checked_sub(1),
        };
    }

    /// `chat`'s own choice, from its listed row (none before the lists
    /// name it).
    fn own_choice(&self, chat: &str, cx: &App) -> Option<(String, Choice)> {
        let lists = self.options.lists.read(cx);
        let row = lists.chat(chat)?;
        let own = Choice::of_row(
            &row.provider,
            &row.model,
            row.effort.as_deref(),
            row.resolution as u32,
        );
        Some((chat.to_string(), own))
    }

    /// Shows chat `chat`, or the new chat for none: the composer's draft is
    /// parked and the place's own taken out, the transcript empties, and for
    /// a chat the core is asked for it (`Command::Open`).
    pub fn show_chat(&mut self, chat: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let leaving = self.draft_key();
        let draft = self.current_draft(cx);
        if !draft::draft_empty(&draft) {
            self.drafts.park(&leaving, draft);
        }
        // bench agent: back from a Bench surface.
        self.bench.surface = bench::Surface::Chat;
        self.open = chat.clone();
        // The pickers follow the place: a chat's own choice is its row's.
        // start-place agent: a start place opened before the lists has no
        // row yet; its choice follows when the lists name it.
        let place = chat.as_ref().and_then(|id| self.own_choice(id, cx));
        self.pickers_pending = chat.is_some() && place.is_none();
        self.pickers.update(cx, |p, cx| p.show_place(place, cx));
        let arriving = self.draft_key();
        let taken = self.drafts.take(&arriving);
        self.put_draft(taken, window, cx);
        self.creating = false;
        self.items.clear();
        self.phase = IDLE;
        self.pending = None;
        self.stop_asked = false;
        self.ticker = None;
        self.turn = None;
        self.joined = None;
        self.snapshot_due = false;
        self.branches.clear();
        self.branching = false;
        self.selection = None;
        self.open_folds.clear();
        self.current_row = None;
        self.read = if chat.is_some() {
            ReadState::Loading
        } else {
            ReadState::Shown
        };
        self.idea = crate::state::ideas::idea_now();
        self.list.reset(0);
        self.list_count = 0;
        self.rows.clear();
        self.leaves.clear();
        self.pin_next = true;
        self.sync_placeholder(window, cx);
        if let Some(chat) = chat {
            self.options.core.send(Command::Open { chat });
            self.lists_changed(window, cx);
        }
        cx.notify();
    }

    /// Puts `draft` in the composer: its text, files, selections and edit.
    pub(crate) fn put_draft(&mut self, draft: Draft, window: &mut Window, cx: &mut Context<Self>) {
        let text = draft.text.clone();
        self.draft = Draft {
            text: String::new(),
            ..draft
        };
        self.composer_empty = text.trim().is_empty();
        self.composer
            .update(cx, |c, cx| c.set_value(text, window, cx));
    }

    /// The composer takes focus with the caret at the end (`caretAtEnd`:
    /// after an edit begins or is cancelled).
    pub(crate) fn caret_at_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |c, cx| {
            let line = c.value().matches('\n').count() as u32;
            c.set_cursor_position(
                gpui_kit::component::input::Position {
                    line,
                    character: u32::MAX,
                },
                window,
                cx,
            );
        });
    }

    /// The new chat's first message made its row: the same transcript, now
    /// that chat's.
    pub fn adopt(&mut self, chat: String, cx: &mut Context<Self>) {
        self.open = Some(chat);
        cx.notify();
    }

    /// The core refused a send before its turn began: the transcript is as
    /// it was, the message goes back where it was written (unless something
    /// new was written there since), the turn is over.
    pub fn refused(&mut self, target: &Target, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.pending.take_if(|p| p.target == *target) else {
            return;
        };
        if pending.key == self.draft_key() {
            let now = self.current_draft(cx);
            let back = draft::sent_back(&pending.draft, &now);
            self.put_draft(back, window, cx);
        } else {
            self.drafts.put_back(&pending.key, &pending.draft);
        }
        self.items = pending.before;
        self.phase = IDLE;
        self.stop_asked = false;
        self.turn = None;
        self.joined = None;
        self.creating = false;
        self.rebuild(cx);
        self.sync_placeholder(window, cx);
        cx.notify();
    }

    /// The setup steps changed: a held send goes once setup is ready, if
    /// its place is on screen (`held`, 2.4).
    pub fn set_setup(&mut self, steps: Vec<Step>, window: &mut Window, cx: &mut Context<Self>) {
        let ready = crate::state::setup::setup_status(&steps).ready;
        self.needs_step = crate::state::setup::remembered_after(&steps, self.needs_step.as_deref());
        self.setup = Some(steps);
        if ready
            && let Some(key) = self.held.take()
            && key == self.draft_key()
        {
            self.submit(window, cx);
        }
        cx.notify();
    }

    /// The providers arrived: whether an AI is connected (for the notice of
    /// a send without a model).
    pub fn set_providers(&mut self, providers: &[crate::Provider], cx: &mut Context<Self>) {
        self.connected = Some(providers.iter().any(|p| p.connected && p.enabled));
        cx.notify();
    }

    // par-chat: `noAI` holds only after a read that succeeded.
    /// The providers' read failed: whether an AI is connected is unknown.
    pub fn providers_failed(&mut self, cx: &mut Context<Self>) {
        self.connected = None;
        cx.notify();
    }

    /// Whether the chat column shows this pane (the shell's place is a chat
    /// or the new chat): type-ahead goes to the composer only then.
    pub fn set_shown(&mut self, shown: bool) {
        self.shown = shown;
    }

    /// Puts focus in the composer.
    pub fn focus_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |c, cx| c.focus(window, cx));
    }
}

/// Now, epoch ms.
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after 1970")
        .as_millis() as i64
}

impl Render for ChatPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        // narrow agent: the rows' padding follows the window's width.
        let narrow = crate::state::layout::narrow(f32::from(window.viewport_size().width));
        if narrow != self.narrow {
            self.narrow = narrow;
            self.list.remeasure();
        }
        // A row that took focus by any route (a screen reader's Focus action
        // on a row laid out off screen, say) becomes the current message and
        // is scrolled into view this frame.
        let focused = self.rows.iter().position(|row| {
            self.row_focus
                .get(&row.key)
                .is_some_and(|handle| handle.contains_focused(window, cx))
        });
        match focused {
            Some(focused) if self.current_row != Some(focused) => {
                self.current_row = Some(focused);
                self.list.scroll_to_reveal_item(focused);
            }
            Some(_) => {}
            // The transcript is always a Tab stop: the current message is a
            // row the list lays out. A chat just opened starts at its last
            // message (the list shows its end); a current message the list
            // no longer lays out (a turn, or the pointer, scrolled it away)
            // gives way to the top row on screen.
            None if !self.rows.is_empty() => match self.current_row {
                None => self.current_row = Some(self.rows.len() - 1),
                Some(current)
                    if current >= self.rows.len() || {
                        let viewport = self.list.viewport_bounds();
                        !self.list.bounds_for_item(current).is_some_and(|row| {
                            row.top() < viewport.bottom() && row.bottom() > viewport.top()
                        })
                    } =>
                {
                    let top = self.list.logical_scroll_top().item_ix;
                    self.current_row = Some(top.min(self.rows.len() - 1));
                }
                Some(_) => {}
            },
            None => {}
        }
        self.hits.borrow_mut().clear();
        // bench agent: a Bench surface in the transcript's place; a bench
        // model's chat has no composer.
        let transcript = match self.bench.surface.clone() {
            bench::Surface::Chat => self.transcript(window, cx),
            bench::Surface::NewBench => self.new_bench_body(window, cx),
            bench::Surface::Compare(_) => match &self.bench.body {
                Some(body) => div()
                    .flex_1()
                    .min_h_0()
                    .child(body.clone())
                    .into_any_element(),
                None => div().flex_1().into_any_element(),
            },
        };
        let composer = if self.read_only(cx) && self.bench.surface == bench::Surface::Chat {
            div().into_any_element()
        } else {
            self.composer_block(window, cx)
        };
        let lightbox = self.lightbox_layer(window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .child(transcript)
            .child(composer)
            .children(lightbox)
    }
}
