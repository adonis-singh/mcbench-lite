//! Bench's pane header controls (`web/app.js` 6666-6726; inventory `app.md`
//! 7.11 and 14): a member's breadcrumb back to its comparison; on a
//! comparison of more than one round the round switch (`Round shown in the
//! world`, one pressed button per round); `Reveal models` while names are
//! hidden (a confirm: `Reveal for good`); on a member `Fork to Build` (a
//! confirm, `Fork and reveal models`, while hidden); on the prompt library
//! its breadcrumb and `Run bench`.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::BenchPane;
use crate::state::bench as rules;
use crate::state::place::Place;
use crate::view::controls::{Button, ConfirmButton, Kind, fonts};
use crate::view::icons::Icon;
use crate::view::shell::Shell;
use crate::view::theme::{self, TextKind, colour, shadow};

/// The title for a member and the prompt library: a breadcrumb.
pub fn crumbs(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let fonts = fonts(cx);
    // `.crumb-sep`: `margin: 0 7px`, `--dim-text`, weight 400.
    let sep = || {
        div()
            .text_color(colour::dim())
            .font_weight(FontWeight::NORMAL)
            .mx(px(7.))
            .child("/")
    };
    match shell.place().clone() {
        Place::Member { bench, member } => {
            let lists = shell.lists().read(cx);
            let listed = lists.benches().iter().find(|b| b.id == bench)?.clone();
            let row = lists.all().iter().find(|c| c.summary.id.0 == member)?;
            let name = crate::state::sidebar::name_of(row, &listed, lists.all());
            let this = cx.entity().downgrade();
            let back = Place::Compare {
                bench: bench.clone(),
            };
            Some(
                theme::text(
                    div().flex_1().min_w_0().flex().items_center().truncate(),
                    TextKind::PaneTitle,
                    &fonts,
                )
                .child(
                    Button::new("crumb-bench", listed.title.clone())
                        .kind(Kind::Crumb)
                        .on_press(move |window, cx| {
                            let place = back.clone();
                            let _ = this.update(cx, |s, cx| s.go(place, window, cx));
                        }),
                )
                .child(sep())
                .child(div().truncate().child(name))
                .into_any_element(),
            )
        }
        Place::Prompt => {
            let title = shell.library().read(cx).shown_title();
            Some(
                theme::text(
                    div().flex_1().min_w_0().flex().items_center().truncate(),
                    TextKind::PaneTitle,
                    &fonts,
                )
                .child(theme::text(div(), TextKind::Crumb, &fonts).child("Prompts"))
                .when_some(title, |d, title| {
                    d.child(sep()).child(div().truncate().child(title))
                })
                .into_any_element(),
            )
        }
        _ => None,
    }
}

/// The controls at the header's right for Bench's places.
pub fn actions(shell: &Shell, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    let mut out = Vec::new();
    let pane = shell.bench_pane();
    match shell.place().clone() {
        Place::Compare { bench } => {
            let Some(listed) = shell
                .lists()
                .read(cx)
                .benches()
                .iter()
                .find(|b| b.id == bench)
                .cloned()
            else {
                return out;
            };
            if listed.rounds > 1 {
                let shown = pane.read(cx).round_of(&bench);
                let mut group = div()
                    .id("round-switch")
                    .role(Role::Group)
                    .aria_label("Round shown in the world")
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(2.))
                    .p(px(2.))
                    .rounded(px(8.))
                    .bg(colour::well())
                    .shadow(shadow::line_ring());
                for n in 1..=listed.rounds {
                    let pane = pane.clone();
                    let bench = bench.clone();
                    group = group.child(
                        Button::new(("round-switch", n), n.to_string())
                            .named(format!("Round {n}"))
                            .kind(Kind::Segment)
                            .toggled(n == shown)
                            .pressed(n == shown)
                            .tip(format!("Round {n}"))
                            .on_press(move |_, cx| {
                                let bench = bench.clone();
                                pane.update(cx, |p, cx| p.show_round(&bench, n, cx));
                            }),
                    );
                }
                out.push(group.into_any_element());
            }
            if rules::hides_names(listed.blind, listed.revealed) {
                out.push(reveal(&pane, &bench));
            }
        }
        Place::Member { bench, member } => {
            let lists = shell.lists().read(cx);
            let Some(listed) = lists.benches().iter().find(|b| b.id == bench).cloned() else {
                return out;
            };
            let hidden = rules::hides_names(listed.blind, listed.revealed);
            if hidden {
                out.push(reveal(&pane, &bench));
            }
            let Some(row) = pane
                .read(cx)
                .members(&listed, cx)
                .into_iter()
                .find(|m| m.id == member)
            else {
                return out;
            };
            let disabled = pane.read(cx).forking || rules::fork_round(row.progress) < 1;
            let fork_pane = pane.clone();
            let fork = move |_: &mut Window, cx: &mut App| {
                let row = row.clone();
                fork_pane.update(cx, |p, cx| p.fork(&row, cx));
            };
            if hidden {
                out.push(
                    ConfirmButton::new(
                        "fork",
                        format!("fork:{member}"),
                        Icon::Fork,
                        "Fork to Build",
                        "Fork and reveal models",
                        fork,
                    )
                    .title("Build shows the model, so forking reveals this bench")
                    .disabled(disabled)
                    .into_any_element(),
                );
            } else {
                out.push(
                    Button::new("fork", "Fork to Build")
                        .kind(Kind::Quiet)
                        .with_icon(Icon::Fork, 14.)
                        .tip("Continue this model's build as a new chat in Build")
                        .disabled(disabled)
                        .on_press(fork)
                        .into_any_element(),
                );
            }
        }
        Place::Prompt => {
            let library = shell.library();
            let shown = library.read(cx).shown();
            if let Some(prompt) = shown {
                let disabled = prompt.text.trim().is_empty();
                let this = cx.entity().downgrade();
                out.push(
                    Button::new("run-bench", "Run bench")
                        .kind(Kind::Header)
                        .disabled(disabled)
                        .on_press(move |window, cx| {
                            let prompt = prompt.clone();
                            let _ = this.update(cx, |s, cx| s.bench_prompt(prompt, window, cx));
                        })
                        .into_any_element(),
                );
            }
        }
        _ => {}
    }
    out
}

fn reveal(pane: &Entity<BenchPane>, bench: &str) -> AnyElement {
    let pane = pane.clone();
    let id = bench.to_string();
    ConfirmButton::new(
        "reveal",
        format!("reveal:{bench}"),
        Icon::Eye,
        "Reveal models",
        "Reveal for good",
        move |_, cx| pane.update(cx, |p, _| p.reveal(&id)),
    )
    .title("Show which model made each build. Voting ends here: the votes cast stay as they are.")
    .into_any_element()
}
