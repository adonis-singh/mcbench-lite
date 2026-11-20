//! A bench's comparison (`web/app.js` 6313-6428, `BenchResultRow`
//! 3004-3053; inventory `app.md` 7.8, `components.md` L.5,
//! `style-viewer-routes.md` 1.13): for each round of the details, in order,
//! its prompt (a press shows that round, or on the round on screen frames
//! every build), its versions' arrows and `Edit and branch`, then the list
//! `Round <n>` of one row per model in the bench's order: its state, its
//! letter or logo and name, its effort, tokens and time (none while
//! hidden), the vote, stop or mark, and `Open <name>'s chat`. A votable
//! round adds the tie row; a revealed bench keeps a blind tie as a mark.
//! After the rounds, a failed read says so with `Read again`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{BenchPane, Member};
use crate::state::bench::{self as rules, RoundState};
use crate::state::bench_details::Phase;
use crate::state::transcript::{FileSource, ShownFile};
use crate::view::controls::{Button, Kind, fonts};
use crate::view::icons::{self, Icon};
use crate::view::theme::{self, TextKind, colour, radius};
use crate::view::{Activate, ring};
use crate::{BenchRound, Content, Part, VotePick};

/// A round's text: its text parts joined by newlines.
pub fn round_text(content: &Content) -> String {
    match content {
        Content::Text(text) => text.clone(),
        Content::Parts(parts) => parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

/// A round's files (`content`'s attachments), as a message's.
pub fn round_files(content: &Content) -> Vec<ShownFile> {
    match content {
        Content::Text(_) => Vec::new(),
        Content::Parts(parts) => parts
            .iter()
            .filter_map(|p| match p {
                Part::Attachment { attachment } => Some(ShownFile {
                    name: attachment.name.clone(),
                    content_type: attachment.mime.clone(),
                    size: attachment.size,
                    source: FileSource::Stored(attachment.url.clone()),
                }),
                _ => None,
            })
            .collect(),
    }
}

/// What a row's state says (its status slot's name).
pub fn state_word(state: RoundState) -> &'static str {
    match state {
        RoundState::Done => "Done",
        RoundState::Running => "Working",
        RoundState::Failed => "Failed",
        RoundState::Stopped => "Stopped",
        RoundState::Queued => "Queued",
        RoundState::Held => "Waiting for a retry",
    }
}

impl BenchPane {
    /// Row `position` of round `n`'s focus stop for `part` (0 retry, 1 the
    /// vote or Stop, 2 open).
    fn row_focus(&mut self, n: usize, position: usize, part: u8, cx: &mut App) -> FocusHandle {
        self.row_focus
            .entry((n, position, part))
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// A round's files in its prompt (`renderFiles`, `.msg-attachments`):
    /// a picture's thumbnail and name (a button that opens the lightbox),
    /// any other file's icon and name (opens it), as a message's.
    fn round_files_strip(
        &mut self,
        n: usize,
        files: &[ShownFile],
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::view::chat::message::{file_picture, file_tile, picture_element};
        let mut strip = div().mt(px(8.)).flex().flex_wrap().gap(px(6.));
        for (index, file) in files.iter().enumerate() {
            let focus = self.row_focus(n, usize::MAX - 1, index as u8, cx);
            let picture = file_picture(file, &*self.options.core);
            let open = {
                let (file, open) = (file.clone(), self.options.open_file.clone());
                move |window: &mut Window, cx: &mut App| open(file.clone(), window, cx)
            };
            let open_key = open.clone();
            let open_a11y = open.clone();
            let tile: AnyElement = match (&picture, file.is_image()) {
                (Ok(picture), true) => picture_element(picture.clone())
                    .size(px(30.))
                    .rounded(px(5.))
                    .into_any_element(),
                (Err(sentence), true) => div()
                    .size(px(30.))
                    .rounded(px(5.))
                    .bg(colour::raised())
                    .id(("missing", index))
                    .aria_description(sentence.clone())
                    .into_any_element(),
                _ => file_tile(15.),
            };
            let name = match &picture {
                Err(sentence) => format!("{}, {sentence}", file.name),
                Ok(_) => file.name.clone(),
            };
            strip = strip.child(
                ring(
                    div()
                        .id(SharedString::from(format!("round-{n}-file-{index}")))
                        .role(if file.is_image() {
                            Role::Button
                        } else {
                            Role::Link
                        })
                        .aria_label(name)
                        .track_focus(&focus)
                        .key_context("Part")
                        .on_action(move |_: &Activate, window, cx| open_key(window, cx))
                        .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                            open_a11y(window, cx)
                        })
                        // The press goes on to the prompt's own, as the
                        // page's click bubbles to `showWholeRound`.
                        .on_click(move |_, window, cx| open(window, cx)),
                )
                .flex()
                .items_center()
                .gap(px(7.))
                .max_w(px(220.))
                .min_h(px(38.))
                .pl(px(4.))
                .pr(px(8.))
                .py(px(4.))
                .rounded(px(8.))
                .bg(colour::well())
                .border_color(colour::line())
                .text_size(px(11.))
                .text_color(colour::ash())
                .cursor_pointer()
                .hover(|s| s.text_color(colour::bone()))
                .child(tile)
                .child(div().min_w_0().truncate().child(file.name.clone())),
            );
        }
        strip.into_any_element()
    }

    /// When focus has moved to another row, scrolls that row into view
    /// with 12 px to spare (the rows' bounds are the last frame's: a move
    /// of focus does not move them).
    fn reveal_focused_row(&mut self, window: &mut Window) {
        let focused = self
            .row_focus
            .iter()
            .find(|(_, h)| h.is_focused(window))
            .map(|(&(n, position, _), _)| (n, position));
        if focused == self.revealed {
            return;
        }
        self.revealed = focused;
        let Some((n, position)) = focused else {
            return;
        };
        let Some(row) = self
            .row_bounds
            .borrow()
            .get(&n)
            .and_then(|rows| rows.get(position).copied())
        else {
            return;
        };
        let view = self.scroll.bounds();
        let spare = px(12.);
        let mut offset = self.scroll.offset();
        if row.bottom() + spare > view.bottom() {
            offset.y -= row.bottom() + spare - view.bottom();
        } else if row.top() - spare < view.top() {
            offset.y += view.top() - (row.top() - spare);
        } else {
            return;
        }
        offset.y = offset.y.min(px(0.));
        self.scroll.set_offset(offset);
    }

    pub(crate) fn render_compare(
        &mut self,
        bench: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let Some(listed) = self.listed(bench, cx).cloned() else {
            return div().into_any_element();
        };
        let members = self.members(&listed, cx);
        let held = self.details.get(bench).cloned();
        let shown = self.round_of(bench);
        let hidden = rules::hides_names(listed.blind, listed.revealed);
        let any_running = members.iter().any(|m| m.progress.running);
        let mut column = div()
            .id("bench-compare")
            .role(Role::Region)
            .aria_label(format!("{} comparison", listed.title))
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .px(px(16.))
            .py(px(18.))
            .flex()
            .flex_col()
            .gap(px(22.));
        self.reveal_focused_row(window);
        let data = held.as_ref().and_then(|d| d.data.clone());
        if let Some(data) = &data {
            for (index, round) in data.rounds.iter().enumerate() {
                let n = index + 1;
                let states: Vec<RoundState> = members
                    .iter()
                    .map(|m| rules::round_state(m.progress, n))
                    .collect();
                let votable = rules::votable(hidden, &states);
                let vote = rules::shown_vote(data, &round.id).cloned();
                column = column.child(self.round_section(
                    bench,
                    data,
                    round,
                    n,
                    n == shown,
                    &members,
                    &states,
                    votable,
                    vote,
                    hidden,
                    any_running,
                    window,
                    cx,
                ));
            }
        }
        if let Some(details) = &held
            && details.phase == Phase::Failed
        {
            let error = details.error.clone().unwrap_or_default();
            let this = cx.entity().downgrade();
            let again_bench = bench.to_string();
            column = column.child(
                theme::text(
                    div()
                        .id("bench-failed")
                        .role(Role::Alert)
                        .aria_label(format!("This bench could not be read: {error}"))
                        .flex()
                        .flex_wrap()
                        .gap(px(8.)),
                    TextKind::Error,
                    &fonts,
                )
                .child(format!("This bench could not be read: {error}"))
                .child(crate::view::chat::composer::text_link(
                    "bench-read-again",
                    "Read again",
                    move |_, cx| {
                        let bench = again_bench.clone();
                        let _ = this.update(cx, |p, cx| p.read(&bench, cx));
                    },
                    window,
                    cx,
                )),
            );
        }
        column.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn round_section(
        &mut self,
        bench: &str,
        data: &crate::BenchDetails,
        round: &BenchRound,
        n: usize,
        on_screen: bool,
        members: &[Member],
        states: &[RoundState],
        votable: bool,
        vote: Option<crate::Vote>,
        hidden: bool,
        any_running: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let text = round_text(&round.content);
        let files = round_files(&round.content);
        let editing = self.edited.as_deref() == Some(round.id.as_str());
        let this = cx.entity().downgrade();
        let press_bench = bench.to_string();
        let show = move |window: &mut Window, cx: &mut App| {
            let bench = press_bench.clone();
            let _ = this.update(cx, |p, cx| {
                if p.round_of(&bench) == n {
                    // The round on screen frames every build (the 3D pane's
                    // milestone); the press is recorded.
                    p.log(
                        "frame round",
                        serde_json::json!({ "bench": bench, "round": n }),
                    );
                    // pane agent: the 3D pane frames the whole round.
                    crate::view::world::frame_round(cx);
                } else {
                    p.show_round(&bench, n, cx);
                }
            });
            let _ = window;
        };
        let show_key = show.clone();
        // pane agent: a reader's press is the pointer's press (it showed the
        // round without framing it when the round was on show).
        let show_a11y = show.clone();
        let prompt_focus =
            crate::view::controls::focus_for(("bench-prompt", n), window, cx).tab_stop(true);
        let strip = (!files.is_empty()).then(|| self.round_files_strip(n, &files, window, cx));
        let prompt = theme::text(
            ring(
                div()
                    .id(("bench-prompt", n))
                    .role(Role::Button)
                    .aria_label(format!("Round {n}: {text}"))
                    .track_focus(&prompt_focus)
                    .key_context("Part")
                    .on_action(move |_: &Activate, window, cx| show_key(window, cx))
                    .on_click(move |_, window, cx| show(window, cx))
                    .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                        show_a11y(window, cx)
                    }),
            ),
            TextKind::Prose,
            &fonts,
        )
        .self_end()
        .max_w_full()
        .px(px(14.))
        .py(px(9.))
        .rounded(px(radius::USER))
        .bg(colour::raised())
        .border_color(if editing {
            colour::bone_28()
        } else {
            colour::line()
        })
        .hover(|s| s.border_color(colour::bevel_light()))
        .cursor_pointer()
        .child(text.clone())
        .children(strip);
        // Its tools (`.msg-tools`): the versions' arrows, and Edit and
        // branch; shown while the prompt is hovered or one of them has focus.
        let tool_focus: Vec<FocusHandle> = (0..3)
            .map(|part| self.row_focus(n, usize::MAX, part, cx))
            .collect();
        let tools_shown = tool_focus.iter().any(|h| h.is_focused(window));
        let block = SharedString::from(format!("user-block-{n}"));
        let mut tools = div()
            .self_end()
            .flex()
            .items_center()
            .gap(px(2.))
            .text_color(colour::dim())
            .when(!tools_shown, |d| {
                d.opacity(0.).group_hover(block.clone(), |s| s.opacity(1.))
            });
        let changing = self.branching;
        if round.branch.count > 1 {
            let previous = round.branch.previous_id.clone();
            let next = round.branch.next_id.clone();
            let arrow = |id: &'static str,
                         icon: Icon,
                         label: &'static str,
                         to: Option<String>,
                         focus: FocusHandle| {
                let this = cx.entity().downgrade();
                let bench = bench.to_string();
                Button::icon((id, n), icon, label)
                    .kind(Kind::Tool)
                    .with_icon(icon, 13.)
                    .focus(focus)
                    .tip(label)
                    .disabled(any_running || changing || to.is_none())
                    .on_press(move |_, cx| {
                        if let Some(to) = to.clone() {
                            let bench = bench.clone();
                            let _ = this.update(cx, |p, cx| p.branch(&bench, &to, cx));
                        }
                    })
            };
            tools = tools.child(
                div()
                    .id(("round-branch", n))
                    .role(Role::Group)
                    .aria_label("Round branch")
                    .mr(px(3.))
                    .flex()
                    .items_center()
                    .gap(px(1.))
                    .child(arrow(
                        "round-previous",
                        Icon::Back,
                        "Previous branch",
                        previous,
                        tool_focus[0].clone(),
                    ))
                    .child(
                        theme::text(div(), TextKind::ToolMeta, &fonts).child(format!(
                            "{} / {}",
                            round.branch.index + 1,
                            round.branch.count
                        )),
                    )
                    .child(arrow(
                        "round-next",
                        Icon::Forward,
                        "Next branch",
                        next,
                        tool_focus[1].clone(),
                    )),
            );
        }
        if !any_running {
            let edit = self.options.edit_round.clone();
            let (bench_id, round_id, edit_text, edit_files) =
                (bench.to_string(), round.id.clone(), text, files);
            tools = tools.child(
                Button::icon(("edit-round", n), Icon::Compose, "Edit and branch")
                    .kind(Kind::Tool)
                    .with_icon(Icon::Compose, 13.)
                    .focus(tool_focus[2].clone())
                    .tip("Edit and branch")
                    .disabled(changing)
                    .on_press(move |window, cx| {
                        edit(
                            bench_id.clone(),
                            round_id.clone(),
                            edit_text.clone(),
                            edit_files.clone(),
                            window,
                            cx,
                        )
                    }),
            );
        }
        let rows = self.result_rows(
            bench, data, round, n, members, states, votable, &vote, hidden, window, cx,
        );
        div()
            .id(("bench-round", n))
            .flex()
            .flex_col()
            .gap(px(10.))
            .when(on_screen, |d| d.opacity(1.))
            .child(
                div()
                    .group(block)
                    .self_end()
                    .max_w(relative(0.92))
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap(px(3.))
                    .child(prompt)
                    .child(tools),
            )
            .child(rows)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn result_rows(
        &mut self,
        bench: &str,
        data: &crate::BenchDetails,
        round: &BenchRound,
        n: usize,
        members: &[Member],
        states: &[RoundState],
        votable: bool,
        vote: &Option<crate::Vote>,
        hidden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let count = members.len()
            + usize::from(votable)
            + usize::from(!votable && !hidden && vote.as_ref().is_some_and(|v| v.pick.is_none()));
        let mut list = div()
            .on_children_prepainted({
                let bounds = self.row_bounds.clone();
                move |rows, _, _| {
                    bounds.borrow_mut().insert(n, rows);
                }
            })
            .id(("bench-results", n))
            .role(Role::List)
            .aria_label(format!("Round {n}"))
            .aria_size_of_set(count)
            .p(px(4.))
            .rounded(px(11.))
            .flex()
            .flex_col()
            .gap(px(1.));
        let now = crate::view::chat::now_ms();
        for (position, (member, state)) in members.iter().zip(states).enumerate() {
            let state = *state;
            let picked = vote.as_ref().and_then(|v| v.pick.as_deref()) == Some(member.id.as_str());
            let stats = rules::stats(data, &member.id, n - 1).unwrap_or_default();
            let tokens = (!hidden && state != RoundState::Running)
                .then(|| rules::tokens(&stats))
                .flatten();
            let time = if hidden {
                None
            } else if state == RoundState::Running {
                member
                    .running_since
                    .map(|since| crate::state::turn::format_elapsed(now - since))
            } else if matches!(
                state,
                RoundState::Done | RoundState::Failed | RoundState::Stopped
            ) {
                // `finished`: a time once the model has one, failed or
                // stopped too.
                stats
                    .ms
                    .map(|ms| crate::state::turn::format_elapsed(ms as i64))
            } else {
                None
            };
            // The row and its buttons name the model by its short name, or
            // `Build <letter>` while hidden (`nameOf`); the effort shown is
            // the member's, else the model's default (`caps.defaultEffort`).
            let shown = match member.letter {
                Some(_) => member.name.clone(),
                None => crate::state::models::model_label(&member.model).to_string(),
            };
            let effort = (!hidden)
                .then(|| {
                    member.effort.clone().or_else(|| {
                        let catalog = crate::view::pickers::catalog::Catalog::installed(cx);
                        // Asked once: a model whose facts cannot be read
                        // is not asked again at every frame.
                        let key = (member.provider.clone(), member.model.clone());
                        if self.facts_asked.insert(key) {
                            catalog.update(cx, |c, _| c.ask_facts(&member.provider, &member.model));
                        }
                        catalog
                            .read(cx)
                            .facts(&member.provider, &member.model)
                            .and_then(|f| f.default_effort.clone())
                    })
                })
                .flatten()
                .filter(|e| !e.is_empty());
            let mut name = format!("{shown}, {}", state_word(state));
            if picked {
                name.push_str(", your pick");
            }
            if let Some(effort) = &effort {
                name.push_str(&format!(", {effort}"));
            }
            if let Some((t, _)) = &tokens {
                name.push_str(&format!(", {t}"));
            }
            if let Some(t) = &time {
                name.push_str(&format!(", {t}"));
            }
            let key = format!("bench-row-{n}-{}", member.id);
            let focus =
                crate::view::controls::focus_for(ElementId::Name(key.clone().into()), window, cx)
                    .tab_stop(true);
            let this = cx.entity().downgrade();
            let focus_bench = bench.to_string();
            let framed = member.id.clone();
            let on_focus = move |_: &mut Window, cx: &mut App| {
                let bench = focus_bench.clone();
                let framed = framed.clone();
                let _ = this.update(cx, |p, cx| {
                    if p.round_of(&bench) == n {
                        p.log("frame build", serde_json::json!({ "round": n }));
                        // pane agent: the 3D pane frames this member's build.
                        crate::view::world::frame_member(framed, cx);
                    } else {
                        p.show_round(&bench, n, cx);
                    }
                });
            };
            let on_focus_key = on_focus.clone();
            // The status slot.
            let status: AnyElement = match state {
                RoundState::Done => div()
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(colour::done())
                    .child(icons::icon(Icon::Check, 14.))
                    .into_any_element(),
                RoundState::Running => div()
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().size(px(7.)).rounded_full().bg(colour::gold()))
                    .into_any_element(),
                RoundState::Failed | RoundState::Stopped => {
                    let title = if state == RoundState::Failed {
                        "Failed. Retry this round"
                    } else {
                        "Stopped. Retry this round"
                    };
                    let this = cx.entity().downgrade();
                    let (b, m) = (bench.to_string(), member.id.clone());
                    Button::icon(
                        ("bench-retry", position + n * 100),
                        Icon::Refresh,
                        format!("Retry {shown}"),
                    )
                    .kind(Kind::Status)
                    .with_icon(Icon::Refresh, 13.)
                    .focus(self.row_focus(n, position, 0, cx))
                    .tip(title)
                    .disabled(self.retrying.contains(&member.id))
                    .on_press(move |_, cx| {
                        let (b, m) = (b.clone(), m.clone());
                        let _ = this.update(cx, |p, cx| p.retry(&b, &m, cx));
                    })
                    .into_any_element()
                }
                RoundState::Queued | RoundState::Held => div()
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .border_1()
                            .border_color(colour::bone_28()),
                    )
                    .into_any_element(),
            };
            // The vote, stop or mark slot.
            let slot: Option<AnyElement> = if votable && state == RoundState::Done {
                let this = cx.entity().downgrade();
                let (b, r, m) = (bench.to_string(), round.id.clone(), member.id.clone());
                let (label, title) = if picked {
                    (
                        format!("Take back the pick of {shown}"),
                        "Your pick for this round. Click to take it back",
                    )
                } else {
                    (
                        format!("Pick {shown} as the best build"),
                        "Pick as the best build",
                    )
                };
                Some(
                    Button::new(
                        ("bench-vote", position + n * 100),
                        if picked { "Best" } else { "" },
                    )
                    .named(label)
                    .kind(Kind::Vote)
                    .focus(self.row_focus(n, position, 1, cx))
                    .with_icon(Icon::Trophy, 13.)
                    .icon_only_unless(picked)
                    .toggled(picked)
                    .pressed(picked)
                    .tip(title)
                    .on_press(move |_, cx| {
                        let pick = if picked {
                            VotePick::TakeBack
                        } else {
                            VotePick::Member(m.clone())
                        };
                        let (b, r) = (b.clone(), r.clone());
                        let _ = this.update(cx, |p, cx| p.vote(&b, &r, pick, cx));
                    })
                    .into_any_element(),
                )
            } else if state == RoundState::Running {
                let this = cx.entity().downgrade();
                let m = member.id.clone();
                Some(
                    Button::icon(
                        ("bench-stop", position + n * 100),
                        Icon::Stop,
                        format!("Stop {shown}"),
                    )
                    .kind(Kind::Stop)
                    .focus(self.row_focus(n, position, 1, cx))
                    .tip(format!("Stop {shown}"))
                    // A stop on its way waits for the list to say so.
                    .disabled(self.stopping.contains(&member.id))
                    .on_press(move |_, cx| {
                        let m = m.clone();
                        let _ = this.update(cx, |p, cx| p.stop(&m, cx));
                    })
                    .into_any_element(),
                )
            } else if picked {
                Some(
                    picked_mark(Icon::Trophy, "Best")
                        .id(("bench-mark", position + n * 100))
                        .role(Role::Image)
                        .aria_label(format!("Your blind pick for this round: {shown}"))
                        .into_any_element(),
                )
            } else {
                None
            };
            let go = self.options.go.clone();
            let place = crate::state::place::Place::Member {
                bench: bench.to_string(),
                member: member.id.clone(),
            };
            let open = Button::icon(
                ("bench-open", position + n * 100),
                Icon::Open,
                format!("Open {shown}'s chat"),
            )
            .kind(Kind::Open)
            .with_icon(Icon::Open, 14.)
            .focus(self.row_focus(n, position, 2, cx))
            .tip("Open chat")
            .on_press(move |window, cx| go(place.clone(), window, cx));
            let badge: AnyElement = match member.letter {
                // `.bench-letter`: 600 10.5 px, `--bone` on `--raised`
                // with a 1 px `--bevel-light` ring.
                Some(letter) => theme::text(div(), TextKind::Button, &fonts)
                    .size(px(16.))
                    .rounded(px(4.))
                    .bg(colour::raised())
                    .shadow(theme::shadow::bevel_ring())
                    .font_weight(FontWeight(600.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.5))
                    .child(letter.to_string())
                    .into_any_element(),
                None => icons::logo(
                    crate::state::vendor::vendor_icon(&member.provider, &member.model),
                    14.,
                ),
            };
            let row = theme::text(
                ring(
                    div()
                        .id(ElementId::Name(key.into()))
                        .role(Role::ListItem)
                        .aria_label(name)
                        .aria_position_in_set(position)
                        .aria_size_of_set(count)
                        .track_focus(&focus)
                        .key_context("Part")
                        .on_action(move |_: &Activate, window, cx| on_focus_key(window, cx))
                        .on_click(move |_, window, cx| on_focus(window, cx)),
                ),
                TextKind::Prose,
                &fonts,
            )
            // `padding: 5px 6px 5px 9px`, less the ring's 2 px border.
            .min_h(px(36.))
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(7.))
            .pr(px(4.))
            .py(px(3.))
            .rounded(px(8.))
            .text_size(px(13.))
            .text_color(colour::ash())
            .hover(|s| s.bg(colour::hover()))
            .cursor_pointer()
            .child(div().w(px(18.)).flex_none().child(status))
            // The grid's 14 px column; the 16 px letter overflows it, as there.
            .child(div().w(px(14.)).flex_none().child(badge))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id(("bench-model-name", position + n * 100))
                            .truncate()
                            .text_color(if picked {
                                colour::done()
                            } else if matches!(state, RoundState::Queued | RoundState::Held) {
                                colour::ash()
                            } else {
                                colour::bone()
                            })
                            .child(shown.clone())
                            // par-bench: the model's id as its `title` (none
                            // while names are hidden).
                            .map(|d| {
                                if hidden {
                                    d.into_any_element()
                                } else {
                                    crate::view::controls::cursor_tip(
                                        d,
                                        ("bench-model-tip", position + n * 100),
                                        member.model.clone(),
                                    )
                                    .into_any_element()
                                }
                            }),
                    )
                    .when_some(effort.clone(), |d, e| d.child(super::effort_mark(&e))),
            )
            .child(
                div()
                    .w(px(58.))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .text_size(px(12.))
                    .text_color(colour::dim())
                    // par-bench: `<in> in · <out> out` as the stat's `title`.
                    .children(tokens.map(|(t, title)| {
                        crate::view::controls::cursor_tip(
                            div().id(("bench-tokens", position + n * 100)).child(t),
                            ("bench-tokens-tip", position + n * 100),
                            title,
                        )
                    })),
            )
            .child(
                div()
                    .w(px(50.))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .text_size(px(12.))
                    .text_color(colour::dim())
                    .children(time),
            )
            .child(
                div()
                    .w(px(58.))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .children(slot),
            )
            .child(open);
            list = list.child(row);
        }
        // The tie row while the round can be voted on; after the reveal a
        // blind tie stays as a mark.
        let tie = vote.as_ref().is_some_and(|v| v.pick.is_none());
        if votable {
            let this = cx.entity().downgrade();
            let (b, r) = (bench.to_string(), round.id.clone());
            let (label, title) = if tie {
                (
                    "Take back the tie",
                    "Your call for this round. Click to take it back",
                )
            } else {
                ("Call this round a tie", "Call this round a tie")
            };
            list = list.child(
                theme::text(
                    div()
                        .id(("bench-tie", n))
                        .role(Role::ListItem)
                        // Its two texts, `Tie` and `no build is better`, as
                        // a reader reads the row; the call is the button's.
                        .aria_label("Tie no build is better")
                        .aria_position_in_set(members.len())
                        .aria_size_of_set(count),
                    TextKind::Prose,
                    &fonts,
                )
                .min_h(px(36.))
                .flex()
                .items_center()
                .gap(px(10.))
                .pl(px(9.))
                .pr(px(6.))
                .text_size(px(13.))
                .text_color(colour::ash())
                .child(div().w(px(44.)).flex_none())
                .child(
                    div().flex_1().flex().items_center().child("Tie").child(
                        div()
                            .ml(px(8.))
                            .text_size(px(12.))
                            .text_color(colour::dim())
                            .child("no build is better"),
                    ),
                )
                .child(
                    Button::new(("bench-tie-vote", n), if tie { "Tie" } else { "" })
                        .named(label)
                        .kind(Kind::Vote)
                        .focus(self.row_focus(n, members.len(), 1, cx))
                        .with_icon(Icon::Equal, 13.)
                        .icon_only_unless(tie)
                        .toggled(tie)
                        .pressed(tie)
                        .tip(title)
                        .on_press(move |_, cx| {
                            let pick = if tie {
                                VotePick::TakeBack
                            } else {
                                VotePick::Tie
                            };
                            let (b, r) = (b.clone(), r.clone());
                            let _ = this.update(cx, |p, cx| p.vote(&b, &r, pick, cx));
                        }),
                )
                // Under the trophies: the open column's room after it.
                .child(div().w(px(26.)).flex_none()),
            );
        } else if tie && !hidden {
            list = list.child(
                theme::text(
                    div()
                        .id(("bench-tie-mark", n))
                        .role(Role::ListItem)
                        .aria_label("Your blind call for this round: a tie")
                        .aria_position_in_set(members.len())
                        .aria_size_of_set(count),
                    TextKind::Prose,
                    &fonts,
                )
                .min_h(px(36.))
                .flex()
                .items_center()
                .gap(px(10.))
                .pl(px(9.))
                .pr(px(6.))
                .text_size(px(13.))
                .text_color(colour::ash())
                .child(div().w(px(44.)).flex_none())
                .child(
                    div().flex_1().flex().items_center().child("Tie").child(
                        div()
                            .ml(px(8.))
                            .text_size(px(12.))
                            .text_color(colour::dim())
                            .child("no build was better"),
                    ),
                )
                .child(picked_mark(Icon::Equal, "Tie"))
                .child(div().w(px(26.)).flex_none()),
            );
        }
        list.into_any_element()
    }
}

/// A vote that can no longer change (`.bench-vote.picked.bench-mark`): the
/// picked vote's look, not a button.
fn picked_mark(icon: Icon, word: &'static str) -> Div {
    div()
        .flex_none()
        .h(px(24.))
        .px(px(8.))
        .rounded(px(7.))
        .flex()
        .items_center()
        .gap(px(5.))
        .bg(colour::done_fill())
        .shadow(theme::shadow::done_ring())
        .text_size(px(12.))
        .font_weight(FontWeight(500.))
        .text_color(colour::done())
        .child(icons::icon(icon, 13.))
        .child(word)
}
