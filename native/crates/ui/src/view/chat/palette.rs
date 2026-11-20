//! The palette over the composer (bench agent; `PromptPalette`,
//! `web/app.js` 1758-1828 and 5587-5728; inventory `components.md` I.5,
//! `app.md` 6.5): opened by a `/query` typed at the caret (the text after it
//! filters) or by the prompts button (`Insert a saved prompt`, with its own
//! field `Search prompts`). Its list holds the sections `Favorites`,
//! `Recent`, `All` (labelled only when there is more than one), each row a
//! prompt's title with its `Added by` mark and file count, `Edit <title>`
//! and its star; the preview on the right shows the highlighted prompt. Up
//! and Down move round; Enter or Tab inserts; Escape closes it and only it.
//! `Save message as prompt` saves the composer as a new prompt. A library
//! that failed to load says `Prompts failed to load.` with `Try again`.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ChatPane;
use crate::LibraryPrompt;
use crate::state::prompt_sync::Phase;
use crate::state::prompts;
use crate::view::bench::library::Library;
use crate::view::controls::fonts;
use crate::view::icons::{self, Icon};
use crate::view::theme::{self, TextKind, colour, shadow};

/// The palette's state while open.
pub struct PaletteState {
    /// The text it filters by.
    pub query: String,
    /// The highlighted row in the flattened sections.
    pub index: usize,
    /// The `/query` that opened it (its byte range), none from the button.
    pub slash: Option<std::ops::Range<usize>>,
    /// Opened by the button: its own search field.
    pub search: Option<Entity<InputState>>,
}

/// The `/query` just before a collapsed caret, at the text's start or after
/// whitespace, with no whitespace or `/` in the query (`slashAtCaret`): its
/// range (from the slash) and the query.
pub fn slash_at(text: &str, caret: usize) -> Option<(std::ops::Range<usize>, String)> {
    let before = text.get(..caret)?;
    let slash = before.rfind('/')?;
    let query = &before[slash + 1..];
    if query.chars().any(char::is_whitespace) {
        return None;
    }
    let lead = before[..slash].chars().next_back();
    if lead.is_some_and(|c| !c.is_whitespace()) {
        return None;
    }
    Some((slash..caret, query.to_string()))
}

impl ChatPane {
    fn library(&self, cx: &App) -> Option<Entity<Library>> {
        Library::installed(cx)
    }

    /// The composer changed: a `/query` at the caret opens or updates the
    /// palette; moving off it closes a palette a slash opened.
    pub(crate) fn palette_follow(&mut self, cx: &mut Context<Self>) {
        let (text, at) = {
            let c = self.composer.read(cx);
            (c.value().to_string(), c.selected_range())
        };
        let found = (at.start == at.end)
            .then(|| slash_at(&text, at.end))
            .flatten();
        match (&mut self.palette, found) {
            (Some(p), Some((range, query))) if p.search.is_none() => {
                p.query = query;
                p.slash = Some(range);
                p.index = 0;
            }
            (None, Some((range, query))) => {
                self.palette = Some(PaletteState {
                    query,
                    index: 0,
                    slash: Some(range),
                    search: None,
                })
            }
            (Some(p), None) if p.slash.is_some() => self.palette = None,
            _ => {}
        }
        cx.notify();
    }

    /// The prompts button: the palette with its search, or closed.
    pub(crate) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.as_ref().is_some_and(|p| p.search.is_some()) {
            self.palette = None;
            self.composer.update(cx, |c, cx| c.focus(window, cx));
            return cx.notify();
        }
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search prompts"));
        let sub = cx.subscribe_in(
            &search,
            window,
            |this: &mut Self, s, e: &InputEvent, _, cx| {
                if let InputEvent::Change = e
                    && let Some(p) = this.palette.as_mut()
                {
                    p.query = s.read(cx).value().to_string();
                    p.index = 0;
                    cx.notify();
                }
            },
        );
        self.palette_sub = Some(sub);
        search.update(cx, |s, cx| s.focus(window, cx));
        self.palette = Some(PaletteState {
            query: String::new(),
            index: 0,
            slash: None,
            search: Some(search),
        });
        if let Some(library) = self.library(cx) {
            library.update(cx, |l, cx| l.load(cx));
        }
        self.log("palette", serde_json::json!({ "open": true }));
        cx.notify();
    }

    /// Closes the palette, the composer keeping the focus.
    pub(crate) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = None;
        self.palette_sub = None;
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
    }

    /// The prompts the palette lists, flattened in section order.
    fn palette_rows(&self, cx: &App) -> (Vec<(&'static str, Vec<usize>)>, Vec<LibraryPrompt>) {
        let Some(library) = self.library(cx) else {
            return (Vec::new(), Vec::new());
        };
        let list = library.read(cx).list().to_vec();
        let query = self.palette.as_ref().map_or("", |p| p.query.as_str());
        (prompts::prompt_sections(&list, query), list)
    }

    /// The palette's keys, before any other composer key: Up, Down, Enter
    /// (no Shift), Tab, Escape. True when the key was its own.
    pub(crate) fn palette_key(
        &mut self,
        key: &str,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.palette.is_none() {
            return false;
        }
        let (sections, list) = self.palette_rows(cx);
        let flat: Vec<usize> = sections.iter().flat_map(|(_, s)| s.clone()).collect();
        let palette = self.palette.as_mut().expect("open");
        match key {
            "down" if !flat.is_empty() => {
                palette.index = (palette.index + 1) % flat.len();
                cx.notify();
                true
            }
            "up" if !flat.is_empty() => {
                palette.index = (palette.index + flat.len() - 1) % flat.len();
                cx.notify();
                true
            }
            "enter" | "tab" if !(key == "enter" && shift) => {
                let Some(&at) = flat.get(palette.index.min(flat.len().saturating_sub(1))) else {
                    return key == "enter";
                };
                let prompt = list[at].clone();
                self.insert_from_palette(&prompt, window, cx);
                true
            }
            "escape" => {
                self.close_palette(window, cx);
                true
            }
            _ => false,
        }
    }

    /// Inserts a prompt in place of the `/query` that summoned it, or at
    /// the caret; its `usedAt` is now; the palette closes.
    pub(crate) fn insert_from_palette(
        &mut self,
        prompt: &LibraryPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let slash = self.palette.as_ref().and_then(|p| p.slash.clone());
        self.palette = None;
        self.palette_sub = None;
        if let Some(range) = slash {
            self.composer
                .update(cx, |c, cx| c.set_selected_range(range, cx));
        }
        self.log(
            "inserted prompt",
            serde_json::json!({ "prompt": prompt.id }),
        );
        self.put_prompt(prompt, false, window, cx);
        if let Some(library) = self.library(cx) {
            let id = prompt.id.clone();
            library.update(cx, |l, cx| l.used(&id, cx));
        }
    }

    fn save_message_as_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().to_string();
        let files: Vec<crate::PromptAttachment> = self
            .draft
            .attachments
            .iter()
            .map(|f| {
                use base64::Engine as _;
                crate::PromptAttachment {
                    name: f.name.clone(),
                    content_type: f.content_type.clone(),
                    size: f.size,
                    data: format!(
                        "data:{};base64,{}",
                        f.content_type,
                        base64::engine::general_purpose::STANDARD.encode(&f.data)
                    ),
                }
            })
            .collect();
        self.palette = None;
        if let Some(library) = self.library(cx) {
            let saved = library.update(cx, |l, cx| l.save_draft(&text, files, cx));
            if saved.is_some() {
                (self.options.open_prompts)(window, cx);
            }
        }
        cx.notify();
    }

    /// The palette's element (`.prompt-palette`), over the composer box.
    pub(crate) fn palette_element(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let palette = self.palette.as_ref()?;
        let fonts = fonts(cx);
        let library = self.library(cx)?;
        let failed = library.read(cx).phase() == Phase::Failed;
        let empty_library = library.read(cx).list().is_empty();
        let (sections, list) = self.palette_rows(cx);
        let flat: Vec<usize> = sections.iter().flat_map(|(_, s)| s.clone()).collect();
        let index = palette.index.min(flat.len().saturating_sub(1));
        let search = palette.search.clone();
        let count = flat.len();
        let mut rows = div()
            .id("palette-list")
            .role(Role::ListBox)
            .aria_label("Saved prompts")
            .aria_size_of_set(count)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .p(px(6.));
        // `.pp-search`: the prompts button's search over the list.
        let search_bar = search.as_ref().map(|search| {
            div()
                .flex_none()
                .h(px(42.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_color(colour::dim())
                .shadow(shadow::line_below())
                .child(icons::icon(Icon::Search, 14.))
                .child(
                    div()
                        .flex_1()
                        .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                            let s = &e.keystroke;
                            if this.palette_key(&s.key, s.modifiers.shift, window, cx) {
                                cx.stop_propagation();
                            }
                        }))
                        .child(
                            Input::new(search)
                                .aria_label("Search prompts")
                                .appearance(false)
                                .text_size(px(13.)),
                        ),
                )
        });
        if flat.is_empty() {
            let text = if failed {
                "Prompts failed to load."
            } else if empty_library {
                "No saved prompts yet."
            } else {
                "No prompts match."
            };
            let mut line = theme::text(
                div()
                    .id("palette-empty")
                    .role(if failed { Role::Alert } else { Role::Status })
                    .aria_label(text)
                    .flex()
                    .gap(px(6.))
                    .px(px(8.))
                    .py(px(8.)),
                TextKind::EmptySub,
                &fonts,
            )
            .child(text);
            if failed {
                let library = library.clone();
                line = line.child(super::composer::text_link(
                    "palette-retry",
                    "Try again",
                    move |_, cx| library.update(cx, |l, cx| l.load(cx)),
                    window,
                    cx,
                ));
            }
            rows = rows.child(line);
        }
        let labelled = sections.len() > 1;
        let mut position = 0;
        for (label, members) in &sections {
            if labelled && !label.is_empty() {
                rows = rows.child(
                    theme::text(
                        div().px(px(8.)).pt(px(8.)).pb(px(4.)),
                        TextKind::SideTitle,
                        &fonts,
                    )
                    .text_size(px(11.5))
                    .font_weight(FontWeight(500.))
                    .text_color(colour::dim())
                    .child(*label),
                );
            }
            for &at in members {
                let prompt = &list[at];
                let active = position == index;
                let title = prompts::title_or_untitled(&prompt.title).to_string();
                let this = cx.entity().downgrade();
                let pick = prompt.clone();
                let hover_at = position;
                let hover_this = this.clone();
                let edit_this = this.clone();
                let edit_id = prompt.id.clone();
                let star_id = prompt.id.clone();
                let star_library = library.clone();
                let mut name = title.clone();
                if prompt.favorite {
                    name.push_str(", favorite");
                }
                if !prompt.attachments.is_empty() {
                    name.push_str(&format!(", {} files", prompt.attachments.len()));
                }
                rows = rows.child(
                    theme::text(
                        div()
                            .id(("palette-row", position))
                            .role(Role::ListBoxOption)
                            .aria_label(name)
                            .aria_selected(active)
                            .aria_position_in_set(position)
                            .aria_size_of_set(count)
                            .on_hover(move |hovered, _, cx| {
                                if *hovered {
                                    let _ = hover_this.update(cx, |c, cx| {
                                        if let Some(p) = c.palette.as_mut() {
                                            p.index = hover_at;
                                            cx.notify();
                                        }
                                    });
                                }
                            })
                            .on_mouse_down(MouseButton::Left, |_, window, _| {
                                window.prevent_default()
                            })
                            .on_click(move |_, window, cx| {
                                let _ = this
                                    .update(cx, |c, cx| c.insert_from_palette(&pick, window, cx));
                            }),
                        TextKind::MenuItem,
                        &fonts,
                    )
                    .flex_none()
                    .h(px(32.))
                    .pl(px(9.))
                    .pr(px(4.))
                    .rounded(px(7.))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(colour::ash())
                    .when(active, |d| {
                        d.bg(colour::raised()).text_color(colour::bone())
                    })
                    .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                    .when(prompt.added_by.is_some(), |d| {
                        d.child(
                            div()
                                .text_color(colour::ash())
                                .child(icons::icon(Icon::Sparkles, 12.)),
                        )
                    })
                    .when(!prompt.attachments.is_empty(), |d| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(3.))
                                .text_size(px(11.5))
                                .text_color(colour::dim())
                                .child(icons::icon(Icon::File, 12.))
                                .child(prompt.attachments.len().to_string()),
                        )
                    })
                    .child(row_action(
                        ("palette-edit", position),
                        // `Edit ${prompt.title || 'prompt'}`.
                        if prompt.title.is_empty() {
                            "Edit prompt".to_string()
                        } else {
                            format!("Edit {}", prompt.title)
                        },
                        "Edit prompt",
                        None,
                        active,
                        icons::icon(Icon::Edit, 13.).into_any_element(),
                        std::rc::Rc::new(move |window, cx| {
                            let id = edit_id.clone();
                            let _ = edit_this.update(cx, |c, cx| {
                                c.palette = None;
                                (c.options.edit_prompt)(id, window, cx);
                                cx.notify();
                            });
                        }),
                        crate::view::controls::focus_for(("palette-edit", position), window, cx),
                    ))
                    .child(
                        row_action(
                            ("palette-star", position),
                            format!("Favorite {title}"),
                            if prompt.favorite {
                                "Unfavorite"
                            } else {
                                "Favorite"
                            },
                            Some(prompt.favorite),
                            active || prompt.favorite,
                            if prompt.favorite {
                                icons::icon_filled(Icon::Star, 13.).into_any_element()
                            } else {
                                icons::icon(Icon::Star, 13.).into_any_element()
                            },
                            std::rc::Rc::new(move |_, cx| {
                                let id = star_id.clone();
                                star_library.update(cx, |l, cx| l.toggle_favorite(&id, cx));
                            }),
                            crate::view::controls::focus_for(
                                ("palette-star", position),
                                window,
                                cx,
                            ),
                        )
                        .when(prompt.favorite, |d| d.text_color(colour::star())),
                    ),
                );
                position += 1;
            }
        }
        // `.pp-foot`: Save message as prompt, under the list.
        let foot = (!self.composer_empty).then(|| {
            let this = cx.entity().downgrade();
            let key = this.clone();
            let focus = crate::view::controls::focus_for("palette-save", window, cx);
            div()
                .flex_none()
                .px(px(12.))
                .py(px(8.))
                .flex()
                .items_center()
                .shadow(shadow::line_above())
                .child(
                    crate::view::ring(
                        div()
                            .id("palette-save")
                            .role(Role::Button)
                            .aria_label("Save message as prompt")
                            .track_focus(&focus)
                            .key_context("Part")
                            .on_action(move |_: &crate::view::Activate, window, cx| {
                                let _ =
                                    key.update(cx, |c, cx| c.save_message_as_prompt(window, cx));
                            })
                            .on_mouse_down(MouseButton::Left, |_, window, _| {
                                window.prevent_default()
                            })
                            .on_click(move |_, window, cx| {
                                let _ =
                                    this.update(cx, |c, cx| c.save_message_as_prompt(window, cx));
                            }),
                    )
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .font_weight(FontWeight(500.))
                    .text_color(colour::ash())
                    .hover(|s| s.text_color(colour::bone()))
                    .child("Save message as prompt"),
                )
        });
        // `.pp-preview`: the highlighted prompt.
        let preview =
            flat.get(index).map(|&at| {
                let p = &list[at];
                let mut tags = div().flex().flex_wrap().gap(px(5.));
                for tag in &p.tags {
                    let c = colour::tag(prompts::tag_color(tag));
                    tags = tags.child(
                        div()
                            .px(px(7.))
                            .py(px(2.))
                            .rounded(px(5.))
                            .bg(c.opacity(0.14))
                            .text_color(c)
                            .text_size(px(11.))
                            .child(tag.clone()),
                    );
                }
                theme::text(
                    div()
                        .id("palette-preview")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_y_scroll()
                        .px(px(18.))
                        .py(px(16.))
                        .bg(colour::cave())
                        .flex()
                        .flex_col()
                        .gap(px(10.)),
                    TextKind::Prose,
                    &fonts,
                )
                .child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight(600.))
                        .text_color(colour::bone())
                        .child(prompts::title_or_untitled(&p.title).to_string()),
                )
                .when_some(p.added_by.as_ref(), |d, by| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .text_size(px(12.))
                            .text_color(colour::ash())
                            .child(icons::icon(Icon::Sparkles, 13.))
                            .child(prompts::added_by_label(&by.model)),
                    )
                })
                .when(!p.tags.is_empty(), |d| d.child(tags))
                .child(
                    div()
                        .text_size(px(12.5))
                        .line_height(px(12.5 * 1.55))
                        .text_color(colour::ash())
                        .child(marked_text(prompts::text_marks(&p.text))),
                )
                .when(!p.attachments.is_empty(), |d| {
                    d.child(div().flex().flex_wrap().gap(px(6.)).children(
                        p.attachments.iter().map(|a| {
                            div()
                                .max_w_full()
                                .px(px(8.))
                                .py(px(4.))
                                .rounded(px(6.))
                                .bg(colour::raised())
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_size(px(11.5))
                                .text_color(colour::ash())
                                .child(icons::icon(Icon::File, 14.))
                                .child(a.name.clone())
                        }),
                    ))
                })
            });
        // narrow agent: at 720 px and below one column without the preview,
        // 300 px high (`(max-width: 720px)`, S:2046-2049).
        let two = crate::state::layout::palette_preview(f32::from(window.viewport_size().width));
        Some(
            div()
                .id("prompt-palette")
                .role(Role::Dialog)
                .aria_label("Prompts")
                .occlude()
                .h(px(if two { 320. } else { 300. }))
                .flex()
                .rounded(px(12.))
                .overflow_hidden()
                .bg(colour::well())
                .shadow(shadow::panel())
                // par-bench: a press in the composer's own box keeps it open
                // (`composerBoxRef.current.contains(evt.target)`, web/app.js
                // 5607-5612); any other press outside closes it.
                .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if this.in_composer_box(event.position) {
                        return;
                    }
                    this.palette = None;
                    cx.notify();
                }))
                // The list's column is 1 of the grid's 2.15 parts.
                .child(
                    div()
                        .map(|d| {
                            if two {
                                d.w(relative(1. / 2.15)).flex_none()
                            } else {
                                d.flex_1().min_w_0()
                            }
                        })
                        .h_full()
                        .flex()
                        .flex_col()
                        .children(search_bar)
                        .child(rows)
                        .children(foot),
                )
                .children(preview.filter(|_| two))
                .into_any_element(),
        )
    }
}

/// A prompt's text with its fill-ins marked (`.pp-text mark`: `--star` on
/// a 16% tint of it), its line breaks kept.
fn marked_text(parts: Vec<(String, bool)>) -> StyledText {
    let mut text = String::new();
    let mut highlights = Vec::new();
    for (part, mark) in parts {
        let start = text.len();
        text.push_str(&part);
        if mark {
            highlights.push((
                start..text.len(),
                HighlightStyle {
                    color: Some(colour::star()),
                    background_color: Some(colour::star().opacity(0.16)),
                    ..Default::default()
                },
            ));
        }
    }
    StyledText::new(text).with_highlights(highlights)
}

#[allow(clippy::too_many_arguments)]
/// A palette row's 24 px action (`.pp-row` edit and star): shown on the
/// highlighted row (and, for the star, on a favourite) or on its own focus;
/// a press never inserts the row's prompt.
fn row_action(
    id: (&'static str, usize),
    name: String,
    tip: &'static str,
    toggled: Option<bool>,
    shown: bool,
    body: AnyElement,
    press: crate::view::Press,
    focus: FocusHandle,
) -> Stateful<Div> {
    let key = press.clone();
    let mut d = crate::view::ring(
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(name)
            .aria_description(tip)
            .track_focus(&focus)
            .key_context("Part")
            .on_action(move |_: &crate::view::Activate, window, cx| key(window, cx))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                press(window, cx)
            }),
    )
    .flex_none()
    .size(px(24.))
    .rounded(px(6.))
    .flex()
    .items_center()
    .justify_center()
    .cursor_pointer()
    .text_color(colour::dim())
    .hover(|s| s.bg(colour::well()).text_color(colour::bone()))
    .when(!shown, |d| d.opacity(0.).focus_visible(|s| s.opacity(1.)))
    .child(body);
    if let Some(on) = toggled {
        d = d.aria_toggled(if on {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        });
    }
    d
}

#[cfg(test)]
mod tests {
    use super::slash_at;

    #[test]
    fn a_slash_at_the_caret_after_a_space_or_at_the_start_opens_the_palette() {
        assert_eq!(slash_at("/cas", 4), Some((0..4, "cas".into())));
        assert_eq!(slash_at("a /", 3), Some((2..3, String::new())));
        assert_eq!(slash_at("a/b", 3), None, "inside a word");
        assert_eq!(slash_at("/a b", 4), None, "a space ends the query");
        assert_eq!(slash_at("/a/b", 4), None);
    }
}
