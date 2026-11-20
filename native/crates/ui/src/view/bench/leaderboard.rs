//! The leaderboard (`web/app.js` 6874-6894; inventory `app.md` 7.10,
//! `style-viewer-routes.md` 1.10 `.leaderboard-table`): nothing until it
//! is read; with no entry `No blind votes yet` and its line; else the table
//! `Models ranked by blind votes` with `#`, `Model`, `Rating`, `Picked`,
//! `Benches`, one row per entry in the core's order. A vote cast or the
//! votes cleared publish it again, so it follows while open. Up and Down
//! move between its rows; each row reads its cells.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::BenchPane;
use crate::state::{Step, step};
use crate::view::controls::fonts;
use crate::view::icons;
use crate::view::theme::{self, TextKind, colour};
use crate::view::{MoveFirst, MoveLast, MoveNext, MovePrevious, ring};

/// The columns' widths after the model's (`20px minmax(0, 1fr) 56px 64px
/// 60px`).
const RANK: f32 = 20.;
const RATING: f32 = 56.;
const PICKED: f32 = 64.;
const BENCHES: f32 = 60.;

impl BenchPane {
    fn step_leaderboard(&mut self, to: Step, window: &mut Window, cx: &mut Context<Self>) {
        let at = self
            .leaderboard_rows
            .iter()
            .position(|h| h.is_focused(window));
        if let Some(next) = step(at, self.leaderboard_rows.len(), to) {
            window.focus(&self.leaderboard_rows[next], cx);
        }
    }

    pub(crate) fn render_leaderboard(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let entries = match &self.leaderboard {
            Some(Ok(board)) => board.entries.clone(),
            _ => {
                return div()
                    .id("leaderboard")
                    .role(Role::Region)
                    .aria_label("Leaderboard")
                    .size_full()
                    .into_any_element();
            }
        };
        if entries.is_empty() {
            return div()
                .id("leaderboard")
                .role(Role::Region)
                .aria_label("No blind votes yet")
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(
                    theme::text(div(), TextKind::EmptyHeading, &fonts).child("No blind votes yet"),
                )
                .child(
                    theme::text(div(), TextKind::EmptySub, &fonts)
                        .child("Vote on a blind bench's rounds and the models are ranked here."),
                )
                .into_any_element();
        }
        while self.leaderboard_rows.len() < entries.len() {
            self.leaderboard_rows.push(cx.focus_handle().tab_stop(true));
        }
        self.leaderboard_rows.truncate(entries.len());
        let cell = |w: Option<f32>, text: String| {
            let d = div().flex_none().text_right();
            match w {
                Some(w) => d.w(px(w)),
                None => d.flex_1().min_w_0().text_left(),
            }
            .child(text)
        };
        let head = theme::text(
            div()
                .id("leaderboard-head")
                .role(Role::Row)
                .aria_label("#, Model, Rating, Picked, Benches"),
            TextKind::Prose,
            &fonts,
        )
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(14.))
        .text_size(px(12.))
        .text_color(colour::dim())
        .child(cell(Some(RANK), "#".into()))
        .child(cell(None, "Model".into()))
        .child(cell(Some(RATING), "Rating".into()))
        .child(cell(Some(PICKED), "Picked".into()))
        .child(cell(Some(BENCHES), "Benches".into()));
        let count = entries.len();
        let mut table = div()
            .id("leaderboard-table")
            .role(Role::Table)
            .aria_label("Models ranked by blind votes")
            .aria_row_count(count + 1)
            .key_context("ChatList")
            .on_action(cx.listener(|this, _: &MovePrevious, window, cx| {
                this.step_leaderboard(Step::Previous, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveNext, window, cx| {
                this.step_leaderboard(Step::Next, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveFirst, window, cx| {
                this.step_leaderboard(Step::First, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLast, window, cx| {
                this.step_leaderboard(Step::Last, window, cx)
            }))
            .w(relative(1.))
            .max_w(px(760.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(head);
        for (index, entry) in entries.iter().enumerate() {
            let label = crate::state::models::model_label(&entry.model).to_string();
            let model = match &entry.effort {
                Some(effort) if !effort.is_empty() => format!("{label} {effort}"),
                _ => label.clone(),
            };
            let (picked, picked_title) = crate::state::bench::picked(entry.wins, entry.votes);
            let rating = format!("{}", entry.rating.round() as i64);
            let name = format!(
                "{}, {model}, rating {rating}, picked {picked}, {} benches",
                index + 1,
                entry.benches
            );
            table = table.child(
                theme::text(
                    ring(
                        div()
                            .id(("leaderboard-row", index))
                            .role(Role::Row)
                            .aria_label(name)
                            .aria_row_index(index + 1)
                            .aria_description(picked_title.clone())
                            .track_focus(&self.leaderboard_rows[index]),
                    ),
                    TextKind::Prose,
                    &fonts,
                )
                .min_h(px(36.))
                .px(px(10.))
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap(px(14.))
                .text_size(px(13.))
                .text_color(colour::ash())
                .hover(|s| s.bg(colour::hover()))
                .child(cell(Some(RANK), (index + 1).to_string()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .text_color(colour::bone())
                        .child(icons::logo(
                            crate::state::vendor::vendor_icon(&entry.provider, &entry.model),
                            14.,
                        ))
                        .child(div().truncate().child(label.clone()))
                        .when_some(entry.effort.as_deref().filter(|e| !e.is_empty()), |d, e| {
                            d.child(super::effort_mark(e))
                        }),
                )
                .child(cell(Some(RATING), rating))
                // par-bench: the count's `title`, as a browser shows it.
                .child(div().w(px(PICKED)).flex_none().flex().justify_end().child(
                    crate::view::controls::cursor_tip(
                        div().id(("leaderboard-picked", index)).child(picked),
                        ("leaderboard-picked-tip", index),
                        picked_title.clone(),
                    ),
                ))
                .child(cell(Some(BENCHES), entry.benches.to_string())),
            );
        }
        div()
            .id("leaderboard")
            .role(Role::Region)
            .aria_label("Leaderboard")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.leaderboard_scroll)
            .flex()
            .justify_center()
            .px(px(16.))
            .py(px(24.))
            .child(table)
            .into_any_element()
    }
}
