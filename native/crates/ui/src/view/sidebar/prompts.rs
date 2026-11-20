//! Bench's prompts section (`PromptsSection` and `PromptList`, inventory
//! `components.md` I.6 and I.7, `app.md` 4.8), as far as this milestone
//! goes: the library's prompt titles under a fold (`Prompts`), the search
//! button that grows into its field (title, text and tags, case folded;
//! Escape or a press outside ends it), and one row per prompt that opens
//! the `prompt` place. Shown once the library has loaded; when it could not
//! be read, `Prompts failed to load.` with `Try again`. The prompt editor,
//! New prompt, the tag filter, stars, delete and reordering are the prompt
//! library's milestone.

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::controls::fonts;
use super::super::icons::{self, Icon};
use super::super::theme::{self, TextKind, colour, radius, shadow};
use super::super::{Activate, MoveFirst, MoveLast, MoveNext, MovePrevious, OpenChat, ring};
use super::parts::{side_empty, side_note, side_title_marked};
use super::{PromptFindEscape, Sidebar};
use crate::LibraryPrompt;
use crate::state::place::Place;
use crate::state::{Step, step};

/// The library as the sidebar holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Library {
    /// Not read yet.
    Unloaded,
    /// Read.
    Loaded(Vec<LibraryPrompt>),
    /// It could not be read.
    Failed(String),
}

/// The section's state.
pub struct Prompts {
    /// The library.
    pub library: Library,
    /// Folded (`mcb-prompts-open` = `closed`); unfolded by default.
    pub folded: bool,
    searching: bool,
    input: Option<Entity<InputState>>,
    query: String,
    /// The prompt last opened (`promptsActiveId`).
    pub active: Option<String>,
    current: usize,
    fold: Option<FocusHandle>,
    search: Option<FocusHandle>,
    retry: Option<FocusHandle>,
    rows: HashMap<String, FocusHandle>,
    _subscription: Option<Subscription>,
    // bench agent: the tag filter and a drag.
    /// The tags chosen in the filter menu (a prompt shows with any of them).
    pub tags: Vec<String>,
    tag_menu: Option<Entity<crate::view::controls::popover::PopoverState>>,
    /// A press on a row that may become a drag (`PromptList`'s).
    drag: Option<crate::state::prompts::PromptDrag>,
    // par-bench: the list's scroll (a drag at an edge scrolls it; it
    // keeps the rows' bounds a drag starts from), and each row's slide
    // (from, to, since) while the others give way.
    scroll: ScrollHandle,
    slides: HashMap<usize, (f32, f32, std::time::Instant)>,
    /// A drag just ended: the click that ends it opens nothing.
    dragged: bool,
}

impl Default for Prompts {
    fn default() -> Self {
        Prompts {
            library: Library::Unloaded,
            folded: false,
            searching: false,
            input: None,
            query: String::new(),
            active: None,
            current: 0,
            fold: None,
            search: None,
            retry: None,
            rows: HashMap::new(),
            _subscription: None,
            tags: Vec::new(),
            tag_menu: None,
            drag: None,
            dragged: false,
            scroll: ScrollHandle::new(),
            slides: HashMap::new(),
        }
    }
}

impl Prompts {
    /// The library's prompts, none before it loaded.
    pub fn library(&self) -> &[LibraryPrompt] {
        match &self.library {
            Library::Loaded(list) => list,
            _ => &[],
        }
    }

    /// The prompts the search and the tag filter show (`PromptList`'s
    /// filter: the text over title, text and tags; any chosen tag).
    fn shown(&self) -> Vec<&LibraryPrompt> {
        let tags: Vec<String> = self.shown_tags();
        self.library()
            .iter()
            .filter(|p| crate::state::prompts::list_shows(p, &self.query, &tags))
            .collect()
    }

    /// The chosen tags some prompt still has (`shownTags`).
    fn shown_tags(&self) -> Vec<String> {
        let all = crate::state::prompts::all_tags(self.library());
        self.tags
            .iter()
            .filter(|t| all.contains(t))
            .cloned()
            .collect()
    }

    /// Rows can be dragged and moved only with no text and no tag narrowing
    /// the list.
    fn reorderable(&self) -> bool {
        self.query.trim().is_empty() && self.shown_tags().is_empty()
    }
}

/// A prompt's title as shown.
fn title(p: &LibraryPrompt) -> String {
    if p.title.is_empty() {
        "Untitled prompt".into()
    } else {
        p.title.clone()
    }
}

impl Sidebar {
    /// The library from the core.
    pub fn prompts_read(
        &mut self,
        library: Result<Vec<LibraryPrompt>, String>,
        cx: &mut Context<Self>,
    ) {
        self.prompts.library = match library {
            Ok(list) => Library::Loaded(list),
            Err(error) => Library::Failed(error),
        };
        cx.notify();
    }

    fn ensure_prompt_find(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = &self.prompts.input {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx));
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this: &mut Self, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.prompts.query = input.read(cx).value().to_string();
                    cx.notify();
                }
            },
        );
        self.prompts.input = Some(input.clone());
        self.prompts._subscription = Some(subscription);
        input
    }

    /// The search button: unfolds the section, opens the field, focuses it.
    fn start_prompt_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.prompts.folded {
            self.prompts.folded = false;
            self.keep_folds(cx);
        }
        self.prompts.searching = true;
        let input = self.ensure_prompt_find(window, cx);
        input.update(cx, |i, cx| i.focus(window, cx));
        cx.notify();
    }

    /// Ends the search: the text goes.
    fn end_prompt_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.prompts.searching {
            return;
        }
        self.prompts.searching = false;
        self.prompts.query.clear();
        if let Some(input) = self.prompts.input.clone() {
            input.update(cx, |i, cx| i.set_value("", window, cx));
        }
        cx.notify();
    }

    fn open_prompt(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.log("opened prompt", serde_json::json!({ "prompt": id }));
        self.prompts.active = Some(id);
        self.go(Place::Prompt, window, cx);
        cx.notify();
    }

    fn move_prompt(&mut self, to: Step, window: &mut Window, cx: &mut Context<Self>) {
        let ids: Vec<String> = self.prompts.shown().iter().map(|p| p.id.clone()).collect();
        if let Some(index) = step(Some(self.prompts.current), ids.len(), to) {
            self.prompts.current = index;
            if let Some(handle) = self.prompts.rows.get(&ids[index]) {
                window.focus(handle, cx);
            }
            cx.notify();
        }
    }

    /// The section (Bench mode).
    pub(crate) fn prompts_section(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let fonts = fonts(cx);
        if let Library::Failed(_) = &self.prompts.library {
            let retry = self
                .prompts
                .retry
                .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            let again = cx.listener(|this, _, _, cx| {
                this.prompts.library = Library::Unloaded;
                this.send(crate::Command::ReadPrompts);
                cx.notify();
            });
            let again_key = cx.listener(|this, _: &Activate, _, cx| {
                this.prompts.library = Library::Unloaded;
                this.send(crate::Command::ReadPrompts);
                cx.notify();
            });
            return Some(
                side_empty("Prompts failed to load. ", cx)
                    .id("prompts-failed")
                    .role(Role::Alert)
                    .aria_label("Prompts failed to load.")
                    .flex()
                    .child(
                        ring(
                            div()
                                .id("prompts-retry")
                                .role(Role::Button)
                                .aria_label("Try again")
                                .track_focus(&retry)
                                .key_context("Part")
                                .on_click(again)
                                .on_action(again_key),
                        )
                        .text_color(colour::ash())
                        .underline()
                        .hover(|s| s.text_color(colour::bone()))
                        .cursor_pointer()
                        .child("Try again"),
                    )
                    .into_any_element(),
            );
        }
        let Library::Loaded(_) = &self.prompts.library else {
            return None;
        };
        let fold = self
            .prompts
            .fold
            .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let search = self
            .prompts
            .search
            .get_or_insert_with(|| cx.focus_handle())
            .clone()
            .tab_stop(!self.prompts.searching);
        let this = cx.entity().downgrade();
        let marks: Vec<Hsla> = self
            .prompts
            .shown_tags()
            .iter()
            .map(|t| colour::tag(crate::state::prompts::tag_color(t)))
            .collect();
        let header = side_title_marked(
            "prompts-fold",
            &fold,
            "Prompts",
            !self.prompts.folded,
            Rc::new(move |_, cx| {
                let _ = this.update(cx, |s, cx| {
                    s.prompts.folded = !s.prompts.folded;
                    s.keep_folds(cx);
                    cx.notify();
                });
            }),
            &marks,
            cx,
        );
        let searching = self.prompts.searching;
        let field = searching.then(|| {
            let input = self.ensure_prompt_find(window, cx);
            theme::text(
                div()
                    .key_context("PromptFind")
                    .on_action(cx.listener(|this, _: &PromptFindEscape, window, cx| {
                        this.end_prompt_search(window, cx)
                    }))
                    .flex_1()
                    .min_w_0(),
                TextKind::SideSearch,
                &fonts,
            )
            .text_size(px(12.5))
            .child(
                Input::new(&input)
                    .aria_label("Search prompts")
                    .appearance(false)
                    .text_size(px(12.5)),
            )
        });
        let start = cx.listener(|this, _, window, cx| this.start_prompt_search(window, cx));
        let start_key =
            cx.listener(|this, _: &Activate, window, cx| this.start_prompt_search(window, cx));
        let search_area = div()
            .id("prompts-search")
            .h(px(28.))
            .min_w(px(28.))
            .flex()
            .items_center()
            .rounded(px(radius::ROW))
            .when(searching, |d| {
                d.flex_1()
                    .bg(colour::raised())
                    .shadow(shadow::line_ring())
                    .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                        // bench agent: a press in the open tag menu is the
                        // search area's own.
                        let menu_open = this
                            .prompts
                            .tag_menu
                            .as_ref()
                            .is_some_and(|m| m.read(cx).is_open());
                        if !menu_open {
                            this.end_prompt_search(window, cx)
                        }
                    }))
            })
            .child(
                ring(
                    div()
                        .id("prompts-search-button")
                        .role(Role::Button)
                        .aria_label("Search prompts")
                        .track_focus(&search)
                        .key_context("Part")
                        .when(!searching, |d| d.on_click(start).on_action(start_key)),
                )
                .flex_none()
                .size(px(28.))
                .rounded(px(radius::ROW))
                .flex()
                .items_center()
                .justify_center()
                .text_color(if searching {
                    colour::dim()
                } else {
                    colour::ash()
                })
                .when(!searching, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.text_color(colour::bone()).bg(colour::raised()))
                })
                .child(icons::icon(Icon::Search, 14.)),
            )
            .children(field)
            // bench agent: the tag filter, in the search bar.
            .children(searching.then(|| self.tag_filter(window, cx)).flatten());
        let mut section = div().flex_none().flex().flex_col().min_h_0().child(
            div()
                .min_h(px(34.))
                .pt(px(6.))
                .px(px(4.))
                .flex()
                .items_center()
                .gap(px(2.))
                .child(header)
                .when(!searching, |d| d.child(div().flex_1()))
                .child(search_area)
                // bench agent: New prompt.
                .child(self.new_prompt_button(cx)),
        );
        if self.prompts.folded {
            return Some(section.into_any_element());
        }
        let shown: Vec<LibraryPrompt> = self.prompts.shown().into_iter().cloned().collect();
        if shown.is_empty() {
            if !self.prompts.library().is_empty() {
                section = section.child(side_note("prompts-empty", "No prompts match.", cx));
            }
            return Some(section.into_any_element());
        }
        let on_prompt = self.place == Place::Prompt;
        let active = on_prompt.then(|| {
            self.prompts
                .active
                .clone()
                .filter(|id| shown.iter().any(|p| &p.id == id))
                .unwrap_or_else(|| shown[0].id.clone())
        });
        if let Some(at) = shown.iter().position(|p| {
            self.prompts
                .rows
                .get(&p.id)
                .is_some_and(|h| h.is_focused(window))
        }) {
            self.prompts.current = at;
        }
        self.prompts.current = self.prompts.current.min(shown.len() - 1);
        let count = shown.len();
        let mut list = div()
            .id("prompts")
            // It clips its rows: one scrolled out is not showing (W2).
            .a11y_synthetic_children(|builder| {
                builder.parent_node().set_clips_children();
            })
            .role(Role::List)
            .aria_label("Prompts")
            .aria_size_of_set(count)
            .key_context("ChatList")
            .on_action(cx.listener(|this, _: &MovePrevious, window, cx| {
                this.move_prompt(Step::Previous, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveNext, window, cx| {
                this.move_prompt(Step::Next, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveFirst, window, cx| {
                this.move_prompt(Step::First, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLast, window, cx| {
                this.move_prompt(Step::Last, window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenChat, window, cx| {
                let shown: Vec<String> =
                    this.prompts.shown().iter().map(|p| p.id.clone()).collect();
                if let Some(id) = shown.get(this.prompts.current).cloned() {
                    this.open_prompt(id, window, cx)
                }
            }))
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.prompts.scroll)
            .flex()
            .flex_col()
            .gap(px(2.))
            .p(px(7.));
        for (index, prompt) in shown.iter().enumerate() {
            let focus = self
                .prompts
                .rows
                .entry(prompt.id.clone())
                .or_insert_with(|| cx.focus_handle())
                .clone()
                .tab_stop(index == self.prompts.current);
            let is_active = active.as_deref() == Some(prompt.id.as_str());
            let id = prompt.id.clone();
            let row = theme::text(
                ring(
                    div()
                        .id(ElementId::Name(format!("prompt-{}", prompt.id).into()))
                        // bench agent: its tools show on its hover.
                        .group(format!("prompt-row-{index}"))
                        .role(Role::ListItem)
                        .aria_label(title(prompt))
                        .a11y_synthetic_children(move |builder| {
                            if is_active {
                                builder
                                    .parent_node()
                                    .set_aria_current(accesskit::AriaCurrent::Page);
                            }
                        })
                        .aria_position_in_set(index)
                        .aria_size_of_set(count)
                        .track_focus(&focus)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if std::mem::take(&mut this.prompts.dragged) {
                                return;
                            }
                            this.open_prompt(id.clone(), window, cx)
                        })),
                ),
                TextKind::SidebarRow,
                &fonts,
            )
            .min_h(px(34.))
            .flex()
            .items_center()
            .gap(px(7.))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(radius::ROW))
            .when(is_active, |d| {
                d.bg(colour::raised()).text_color(colour::bone())
            })
            .hover(|s| s.bg(colour::hover()).text_color(colour::bone()))
            .cursor_pointer()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    // bench agent: the tests measure the title.
                    .debug_selector(|| format!("prompt-title-{index}"))
                    .child(title(prompt)),
            )
            // bench agent: the row's marks and its star and delete
            // (`.pe-item-actions`, gap 1 px).
            .children(self.prompt_row_tools(index, prompt, is_active, window, cx));
            list = list.child(self.reorder_row(index, count, row, window, cx));
        }
        // bench agent: a drag follows the pointer over the list and ends on
        // its release, wherever it is.
        let list = list
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                this.prompt_drag_moved(f32::from(e.position.x), f32::from(e.position.y), cx)
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.prompt_drag_ended(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.prompt_drag_ended(cx)),
            );
        Some(section.child(list).into_any_element())
    }
}

// bench agent: the list's row tools (`PromptList`: the added-by mark, a dot
// per tag, the star, the delete that asks first) and New prompt, over the
// window's library (`view::bench::library`).
impl Sidebar {
    /// The row's marks and actions after its title (`.pe-item`): the
    /// compact added-by mark, one 6 px dot per tag, then the 23 px star and
    /// delete (`.pe-item-actions`), shown on the row's hover, on the open
    /// prompt, on their own keyboard focus, the star always for a favourite
    /// (in `--star`), the delete always while armed (then the word `Delete`
    /// in `--err-text` on its tint).
    fn prompt_row_tools(
        &self,
        index: usize,
        prompt: &LibraryPrompt,
        active: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(library) = crate::view::bench::library::Library::installed(cx) else {
            return Vec::new();
        };
        let group: SharedString = format!("prompt-row-{index}").into();
        let mut out = Vec::new();
        if let Some(by) = &prompt.added_by {
            out.push(
                div()
                    .id(("prompt-added", index))
                    .role(Role::Image)
                    .aria_label(crate::state::prompts::added_by_label(&by.model))
                    .flex_none()
                    .text_color(colour::ash())
                    .child(icons::icon(Icon::Sparkles, 12.))
                    .into_any_element(),
            );
        }
        if !prompt.tags.is_empty() {
            out.push(
                div()
                    .flex_none()
                    .flex()
                    .gap(px(3.))
                    .children(prompt.tags.iter().map(|t| {
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .bg(colour::tag(crate::state::prompts::tag_color(t)))
                    }))
                    .into_any_element(),
            );
        }
        let action = |id: (&'static str, usize),
                      name: &'static str,
                      shown: bool,
                      body: AnyElement,
                      hover_text: Hsla,
                      press: crate::view::Press,
                      window: &mut Window,
                      cx: &mut App| {
            let focus = crate::view::controls::focus_for(id, window, cx);
            let key = press.clone();
            ring(
                div()
                    .id(id)
                    .role(Role::Button)
                    .aria_label(name)
                    .track_focus(&focus)
                    .key_context("Part")
                    .on_action(move |_: &Activate, window, cx| key(window, cx))
                    // A press on a tool is not a press on the row: no drag.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        press(window, cx)
                    }),
            )
            .flex_none()
            .min_w(px(23.))
            .h(px(23.))
            .rounded(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(colour::dim())
            .hover(move |s| s.bg(colour::hover()).text_color(hover_text))
            .when(!shown, |d| {
                d.opacity(0.)
                    .group_hover(group.clone(), |s| s.opacity(1.))
                    .focus_visible(|s| s.opacity(1.))
            })
            .child(body)
        };
        let mut actions = Vec::new();
        let (star_lib, star_id) = (library.clone(), prompt.id.clone());
        let favorite = prompt.favorite;
        actions.push(
            action(
                ("prompt-star", index),
                if favorite {
                    "Unfavorite prompt"
                } else {
                    "Favorite prompt"
                },
                active || favorite,
                div()
                    .child(if favorite {
                        icons::icon_filled(Icon::Star, 13.).into_any_element()
                    } else {
                        icons::icon(Icon::Star, 13.).into_any_element()
                    })
                    .into_any_element(),
                colour::star(),
                std::rc::Rc::new(move |_, cx| {
                    star_lib.update(cx, |l, cx| l.toggle_favorite(&star_id, cx))
                }),
                window,
                cx,
            )
            .when(favorite, |d| d.text_color(colour::star()))
            // par-bench: its `title`, by the shared tip.
            .map(|d| {
                crate::view::controls::tip(
                    d,
                    ("prompt-star-tip", index),
                    if favorite {
                        "Unfavorite prompt"
                    } else {
                        "Favorite prompt"
                    },
                )
                .into_any_element()
            }),
        );
        let armed = library.read(cx).delete_armed(&prompt.id);
        let (del_lib, del_id) = (library, prompt.id.clone());
        let body = if armed {
            div()
                .text_size(px(11.5))
                .font_weight(FontWeight(500.))
                .child("Delete")
                .into_any_element()
        } else {
            div().child(icons::icon(Icon::X, 13.)).into_any_element()
        };
        actions.push(
            action(
                ("prompt-delete", index),
                if armed {
                    "Confirm delete"
                } else {
                    "Delete prompt"
                },
                active || armed,
                body,
                colour::err(),
                std::rc::Rc::new(move |_, cx| {
                    del_lib.update(cx, |l, cx| l.press_delete(&del_id, cx))
                }),
                window,
                cx,
            )
            .when(armed, |d| {
                d.px(px(7.))
                    .text_color(colour::err())
                    .bg(colour::armed_fill())
            })
            // par-bench: its `title`, by the shared tip.
            .map(|d| {
                crate::view::controls::tip(
                    d,
                    ("prompt-delete-tip", index),
                    if armed {
                        "Click again to delete"
                    } else {
                        "Delete prompt"
                    },
                )
                .into_any_element()
            }),
        );
        out.push(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(1.))
                .children(actions)
                .into_any_element(),
        );
        out
    }

    fn new_prompt_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().downgrade();
        crate::view::controls::Button::icon("prompt-new", Icon::Plus, "New prompt")
            .tip("New prompt")
            .on_press(move |window, cx| {
                let Some(library) = crate::view::bench::library::Library::installed(cx) else {
                    return;
                };
                let id = library.update(cx, |l, cx| l.create(cx));
                let _ = this.update(cx, |s, cx| s.open_prompt(id, window, cx));
            })
            .into_any_element()
    }
}

// bench agent: reordering by drag and by keys, and the tag filter
// (`PromptList`'s drag, `PromptsSection`'s tag menu).
impl Sidebar {
    /// A row made draggable and movable by Alt+Up and Alt+Down while the
    /// list is not narrowed: the dragged row is painted over the others
    /// (deferred) under the pointer, the rows it passes slide out of its
    /// way over 150 ms, and a dropped row settles into its gap.
    fn reorder_row(
        &mut self,
        index: usize,
        count: usize,
        row: Stateful<Div>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if !self.prompts.reorderable() {
            return row.into_any_element();
        }
        let drag = self.prompts.drag.clone().filter(|d| d.started);
        let dragged = drag.as_ref().is_some_and(|d| d.from == index);
        let offset = match &drag {
            None => {
                self.prompts.slides.remove(&index);
                0.
            }
            // The lifted row follows the pointer at once; dropped, it
            // settles as the others slide.
            Some(d) if dragged && !d.dropping => {
                let at = d.offset(index);
                self.prompts
                    .slides
                    .insert(index, (at, at, std::time::Instant::now()));
                at
            }
            Some(d) => self.slide(index, d.offset(index), window),
        };
        let row = row
            .relative()
            .top(px(offset))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                    if this.prompts.drag.as_ref().is_some_and(|d| d.dropping) {
                        return;
                    }
                    this.prompts.dragged = false;
                    this.prompts.drag = Some(crate::state::prompts::PromptDrag::press(
                        index,
                        f32::from(e.position.x),
                        f32::from(e.position.y),
                    ));
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(move |this, e: &KeyDownEvent, window, cx| {
                let k = &e.keystroke;
                if !k.modifiers.alt {
                    return;
                }
                let to = match k.key.as_str() {
                    "up" if index > 0 => index - 1,
                    "down" if index + 1 < count => index + 1,
                    _ => return,
                };
                cx.stop_propagation();
                this.move_prompt_row(index, to, true, window, cx);
            }))
            .when(dragged, |d| {
                d.bg(colour::raised())
                    .text_color(colour::bone())
                    .shadow(theme::settings::dragged())
                    .cursor(CursorStyle::ClosedHand)
            });
        if dragged {
            deferred(row).with_priority(1).into_any_element()
        } else {
            row.into_any_element()
        }
    }

    /// Where row `index` is drawn on its way to `target`: a new target
    /// starts a 150 ms slide from where it is (`transition: transform
    /// 150ms cubic-bezier(0.2, 0.7, 0.2, 1)`).
    fn slide(&mut self, index: usize, target: f32, window: &mut Window) -> f32 {
        let now = std::time::Instant::now();
        let span = crate::state::prompts::SETTLE.as_secs_f32();
        let shown = |(from, to, since): (f32, f32, std::time::Instant), at: std::time::Instant| {
            let t = (at - since).as_secs_f32() / span;
            from + (to - from) * crate::state::prompts::slide_ease(t)
        };
        let slide = match self.prompts.slides.get(&index).copied() {
            Some(slide) if slide.1 == target => slide,
            Some(slide) => (shown(slide, now), target, now),
            None => (0., target, now),
        };
        self.prompts.slides.insert(index, slide);
        if (now - slide.2).as_secs_f32() < span {
            window.request_animation_frame();
        }
        shown(slide, now)
    }

    /// The pointer moved over the list: a press past 4 px becomes a drag
    /// (from the rows as drawn); near an edge the list scrolls 8 px; the
    /// row follows.
    fn prompt_drag_moved(&mut self, x: f32, y: f32, cx: &mut Context<Self>) {
        let Some(mut drag) = self.prompts.drag.clone() else {
            return;
        };
        if drag.dropping {
            return;
        }
        let scroll = &self.prompts.scroll;
        if !drag.started {
            if !drag.past_slop(x, y) {
                return;
            }
            let rows: Vec<Bounds<Pixels>> = (0..scroll.children_count())
                .map(|i| scroll.bounds_for_item(i).expect("a row's bounds"))
                .collect();
            drag.start(
                rows.iter().map(|b| f32::from(b.origin.y)).collect(),
                rows.iter().map(|b| f32::from(b.size.height)).collect(),
                -f32::from(scroll.offset().y),
            );
            self.prompts.dragged = true;
        }
        let list = scroll.bounds();
        let step =
            crate::state::prompts::edge_scroll(y, f32::from(list.top()), f32::from(list.bottom()));
        if step != 0. {
            let most = f32::from(scroll.max_offset().y);
            let top = (-f32::from(scroll.offset().y) + step).clamp(0., most);
            scroll.set_offset(point(scroll.offset().x, px(-top)));
        }
        drag.follow(y, -f32::from(scroll.offset().y));
        self.prompts.drag = Some(drag);
        cx.notify();
    }

    /// The release: a press that never moved is a click; a drag settles
    /// into its gap and, 150 ms on, the order is committed.
    fn prompt_drag_ended(&mut self, cx: &mut Context<Self>) {
        let Some(mut drag) = self.prompts.drag.take() else {
            return;
        };
        if drag.dropping {
            self.prompts.drag = Some(drag);
            return;
        }
        if !drag.drop() {
            return;
        }
        self.prompts.dragged = true;
        let (from, to) = (drag.from, drag.to);
        self.prompts.drag = Some(drag);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(crate::state::prompts::SETTLE)
                .await;
            let _ = this.update(cx, |this, cx| {
                if to != from
                    && let Some(library) = crate::view::bench::library::Library::installed(cx)
                {
                    this.log(
                        "reordered prompt",
                        serde_json::json!({ "from": from, "to": to }),
                    );
                    library.update(cx, |l, cx| l.reorder(from, to, cx));
                }
                this.prompts.drag = None;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Moves row `from` to `to` (`reorderPrompts`); by keys, focus stays on
    /// the moved prompt and its new place is said.
    fn move_prompt_row(
        &mut self,
        from: usize,
        to: usize,
        by_keys: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(library) = crate::view::bench::library::Library::installed(cx) else {
            return;
        };
        let moved = self.prompts.library().get(from).cloned();
        library.update(cx, |l, cx| l.reorder(from, to, cx));
        self.log(
            "reordered prompt",
            serde_json::json!({ "from": from, "to": to }),
        );
        if let Some(moved) = moved
            && by_keys
        {
            self.prompts.current = to;
            if let Some(handle) = self.prompts.rows.get(&moved.id).cloned() {
                window.focus(&handle, cx);
            }
            let count = self.prompts.library().len();
            let line = format!("{} moved to {} of {count}", title(&moved), to + 1);
            if let Some(a) = self.announcer.clone() {
                a.update(cx, |a, cx| a.polite(line, cx));
            }
        }
        cx.notify();
    }

    /// The filter button and its menu (`Filter by tag`): one checkbox item
    /// a tag, in alphabetical order, with its dot; a choice toggles the tag
    /// and the menu stays open.
    fn tag_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        use crate::view::controls::popover::{self, Placement, PopoverState};
        let tags = crate::state::prompts::all_tags(self.prompts.library());
        if tags.is_empty() {
            return None;
        }
        let state = self
            .prompts
            .tag_menu
            .get_or_insert_with(|| {
                cx.new(|cx| PopoverState::new(crate::state::popover::MENU_EXIT, cx))
            })
            .clone();
        let open = state.read(cx).is_open();
        let chosen = self.prompts.shown_tags();
        let toggler = state.clone();
        let trigger = popover::trigger(
            &state,
            crate::view::controls::Button::icon("prompt-tag-filter", Icon::Filter, "Filter by tag")
                .kind(crate::view::controls::Kind::Filter)
                .with_icon(Icon::Filter, 14.)
                .expanded(open)
                .pressed(!chosen.is_empty())
                .on_press(move |window, cx| toggler.update(cx, |s, cx| s.toggle(window, cx))),
            cx,
        );
        let count = tags.len();
        let mut menu = div()
            .id("prompt-tag-menu")
            .role(Role::Menu)
            .aria_label("Filter by tag")
            .aria_size_of_set(count)
            .key_context("Menu")
            .w(px(190.))
            .max_h(px(260.))
            .overflow_y_scroll()
            .p(px(5.))
            .rounded(px(10.))
            .border_1()
            .border_color(colour::line())
            .bg(colour::well())
            .shadow(shadow::menu())
            .flex()
            .flex_col();
        for (i, tag) in tags.iter().enumerate() {
            let on = chosen.contains(tag);
            let focus = state.update(cx, |s, cx| s.item(i, cx));
            let this = cx.entity().downgrade();
            let t = tag.clone();
            let press = std::rc::Rc::new(move |_: &mut Window, cx: &mut App| {
                let t = t.clone();
                let _ = this.update(cx, |s, cx| {
                    if let Some(at) = s.prompts.tags.iter().position(|x| *x == t) {
                        s.prompts.tags.remove(at);
                    } else {
                        s.prompts.tags.push(t);
                    }
                    cx.notify();
                });
            });
            let key = press.clone();
            menu = menu.child(
                ring(
                    div()
                        .id(("prompt-tag", i))
                        .role(Role::MenuItemCheckBox)
                        .aria_label(tag.clone())
                        .aria_toggled(if on {
                            accesskit::Toggled::True
                        } else {
                            accesskit::Toggled::False
                        })
                        .aria_position_in_set(i)
                        .aria_size_of_set(count)
                        .track_focus(&focus)
                        .key_context("Part")
                        .on_action(move |_: &Activate, window, cx| key(window, cx))
                        .on_click(move |_, window, cx| press(window, cx)),
                )
                // `padding: 5px 8px`, less the ring's 2 px border.
                .min_h(px(30.))
                .px(px(6.))
                .py(px(3.))
                .rounded(px(7.))
                .flex()
                .items_center()
                .gap(px(9.))
                .text_size(px(12.5))
                .text_color(if on { colour::bone() } else { colour::ash() })
                .cursor_pointer()
                .hover(|s| s.bg(colour::raised()).text_color(colour::bone()))
                .child(
                    div()
                        .flex_none()
                        .size(px(8.))
                        .rounded_full()
                        .bg(colour::tag(crate::state::prompts::tag_color(tag))),
                )
                .child(div().flex_1().min_w_0().truncate().child(tag.clone()))
                .when(on, |d| d.child(icons::icon(Icon::Check, 13.))),
            );
        }
        let _ = window;
        let menu = menu
            .on_action({
                let s = state.clone();
                move |_: &MoveNext, window, cx| {
                    s.update(cx, |p, cx| p.step(count, Step::Next, window, cx))
                }
            })
            .on_action({
                let s = state.clone();
                move |_: &MovePrevious, window, cx| {
                    s.update(cx, |p, cx| p.step(count, Step::Previous, window, cx))
                }
            })
            .into_any_element();
        Some(
            div()
                .relative()
                .flex_none()
                .child(trigger)
                .children(popover::panel(&state, Placement::Below, menu, cx))
                .into_any_element(),
        )
    }
}
