//! Bench's list (inventory `app.md` 4.7, 4.8, 7.11; `components.md` L.4,
//! L.6; `web/app.js` 6253-6311 and 6620-6630): the `Leaderboard` row while
//! nothing narrows the list, then one row per bench in the core's order
//! (by search rank while searching, narrowed by the filters), each with its
//! fold (`Show models` / `Hide models`, an expandable button that opens no
//! bench), its title and snippet, its resolution tag, its dot (collapsed:
//! the first of running and unread among its models; else a placeholder
//! that keeps its place), and its archive button (disabled while a model
//! runs). Open, its models follow, each with its own dot and its name (its
//! letter while the bench hides names). A row opens its comparison; a
//! model's row opens that model's chat. Up and Down walk the benches and
//! the open benches' models without opening them.

use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::controls::fonts;
use super::super::icons::{self, Icon};
use super::super::theme::{self, TextKind, colour, radius};
use super::super::{Activate, MoveFirst, MoveLast, MoveNext, MovePrevious, OpenChat, ring};
use super::Sidebar;
use super::parts::{Shown, resolution_tag, row_icon, row_text};
use crate::state::place::Place;
use crate::state::sidebar::{self as rows, Dot, row_name};
use crate::state::{Step, filters, step};
use crate::{Command, Subject};

/// One line of the list: a bench, or one of an open bench's models.
#[derive(Clone, Debug, PartialEq)]
enum Line {
    Bench {
        id: String,
        title: String,
        snippet: Option<String>,
        resolution: i64,
        dot: Option<Dot>,
        arrival: Option<u32>,
        running: bool,
        open: bool,
        members: usize,
    },
    Member {
        bench: String,
        id: String,
        name: String,
        title: String,
        dot: Dot,
        arrival: Option<u32>,
        position: usize,
        count: usize,
    },
}

impl Line {
    fn key(&self) -> String {
        match self {
            Line::Bench { id, .. } => format!("bench:{id}"),
            Line::Member { id, .. } => format!("member:{id}"),
        }
    }
}

/// The list's state: which benches are open (`openBenches`), the current
/// line, and each line's and control's focus.
#[derive(Default)]
pub struct BenchList {
    /// The open benches, in the order they were opened (`openBenches`).
    pub open: Vec<String>,
    current: usize,
    focus: HashMap<String, FocusHandle>,
    caret: HashMap<String, FocusHandle>,
    archive: HashMap<String, FocusHandle>,
    leaderboard: Option<FocusHandle>,
    /// The current line's key as last drawn.
    current_key: Option<String>,
}

impl Sidebar {
    fn bench_lines(&mut self, shown: &filters::Filters, cx: &App) -> Vec<Line> {
        let lists = self.lists.read(cx);
        let all = lists.all();
        let open_member = match &self.place {
            Place::Member { member, .. } => Some(member.as_str()),
            _ => None,
        };
        let found: Option<Vec<String>> = self
            .find
            .found
            .as_ref()
            .map(|f| f.iter().map(|h| h.id.clone()).collect());
        let listed = filters::listed(
            lists.benches(),
            |b| b.id.as_str(),
            found.as_deref(),
            shown,
            |b| {
                let members: Vec<&mcbench_store::ConversationSummary> = rows::members(b, all)
                    .into_iter()
                    .map(|m| &m.summary)
                    .collect();
                filters::bench_facets(b, &members)
            },
        );
        let snippets: HashMap<&str, &str> = self
            .find
            .found
            .iter()
            .flatten()
            .filter_map(|h| h.snippet.as_deref().map(|s| (h.id.as_str(), s)))
            .filter(|(_, s)| !s.is_empty())
            .collect();
        let mut lines = Vec::new();
        for bench in listed {
            let members = rows::members(bench, all);
            let states: Vec<Dot> = members
                .iter()
                .map(|m| rows::member_dot(m, open_member))
                .collect();
            let open = self.benches.open.contains(&bench.id);
            let overall = rows::bench_dot(&states);
            let key = format!("bench:{}", bench.id);
            // Its history goes on while its dot is not shown.
            let arrival = self.dots.arrival(&key, overall.unwrap_or(Dot::Idle));
            let dot = overall.filter(|_| !open);
            let arrival = arrival.filter(|_| dot.is_some());
            lines.push(Line::Bench {
                id: bench.id.clone(),
                title: bench.title.clone(),
                snippet: snippets.get(bench.id.as_str()).map(|s| s.to_string()),
                resolution: bench.resolution.unwrap_or(1),
                dot,
                arrival,
                running: states.contains(&Dot::Running),
                open,
                members: members.len(),
            });
            for (position, (member, state)) in members.iter().zip(&states).enumerate() {
                let id = member.summary.id.0.clone();
                let arrival = self.dots.arrival(&id, *state);
                if open {
                    lines.push(Line::Member {
                        bench: bench.id.clone(),
                        name: rows::name_of(member, bench, all),
                        title: if rows::hides_names(bench) {
                            String::new()
                        } else {
                            member.summary.model.clone()
                        },
                        id,
                        dot: *state,
                        arrival,
                        position,
                        count: members.len(),
                    });
                }
            }
        }
        lines
    }

    fn move_bench(&mut self, to: Step, lines: usize, window: &mut Window, cx: &mut Context<Self>) {
        let keys = self.bench_keys(cx);
        if let Some(index) = step(Some(self.benches.current), lines.min(keys.len()), to) {
            self.benches.current = index;
            if let Some(handle) = self.benches.focus.get(&keys[index]) {
                window.focus(handle, cx);
            }
            cx.notify();
        }
    }

    fn bench_keys(&mut self, cx: &App) -> Vec<String> {
        let offer = self.offer(cx).0;
        let shown = self.shown_filters(&offer);
        self.bench_lines(&shown, cx).iter().map(Line::key).collect()
    }

    fn open_line(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let offer = self.offer(cx).0;
        let shown = self.shown_filters(&offer);
        let lines = self.bench_lines(&shown, cx);
        match lines.iter().find(|l| l.key() == key) {
            Some(Line::Bench { id, .. }) => {
                let place = Place::Compare { bench: id.clone() };
                self.go(place, window, cx)
            }
            Some(Line::Member { bench, id, .. }) => {
                let place = Place::Member {
                    bench: bench.clone(),
                    member: id.clone(),
                };
                self.go(place, window, cx)
            }
            None => {}
        }
    }

    /// Folds a bench open or closed without opening it.
    fn fold_bench(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.benches.open.iter().position(|o| o == id) {
            Some(at) => {
                self.benches.open.remove(at);
            }
            None => self.benches.open.push(id.to_string()),
        }
        self.keep_folds(cx);
        self.log(
            "toggled",
            serde_json::json!({ "bench": id, "open": self.benches.open.iter().any(|o| o == id) }),
        );
        cx.notify();
    }

    // bench agent: a new bench is folded open (`runBench`).
    /// Folds a bench open (a bench just made), keeping the folds.
    pub fn open_bench_fold(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.benches.open.iter().any(|o| o == id) {
            self.benches.open.push(id.to_string());
            self.keep_folds(cx);
            cx.notify();
        }
    }

    fn archive_bench(&mut self, id: String, cx: &mut Context<Self>) {
        self.log("archive", serde_json::json!({ "bench": id }));
        self.send(Command::Archive(Subject::Bench(id)));
        self.read_archive_again(cx);
    }

    /// The `Leaderboard` row, the empty line and the list.
    pub(crate) fn bench_list(
        &mut self,
        shown: &filters::Filters,
        narrowed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let fonts = fonts(cx);
        let lines = self.bench_lines(shown, cx);
        let keys: Vec<String> = lines.iter().map(Line::key).collect();
        self.benches.current_key = None;
        let active_key = match &self.place {
            Place::Compare { bench } => Some(format!("bench:{bench}")),
            Place::Member { member, .. } => Some(format!("member:{member}")),
            _ => None,
        };
        let walking = keys.iter().position(|k| {
            self.benches
                .focus
                .get(k)
                .is_some_and(|h| h.is_focused(window))
        });
        match (
            walking,
            active_key
                .as_ref()
                .and_then(|a| keys.iter().position(|k| k == a)),
        ) {
            (Some(at), _) | (None, Some(at)) => self.benches.current = at,
            _ => {}
        }
        self.benches.current = self.benches.current.min(keys.len().saturating_sub(1));
        let mut out = Vec::new();
        if !narrowed {
            let focus = self
                .benches
                .leaderboard
                .get_or_insert_with(|| cx.focus_handle().tab_stop(true))
                .clone();
            let active = self.place == Place::Leaderboard;
            let go = cx.listener(|this, _, window, cx| this.go(Place::Leaderboard, window, cx));
            let go_key = cx
                .listener(|this, _: &Activate, window, cx| this.go(Place::Leaderboard, window, cx));
            out.push(
                theme::text(
                    ring(
                        div()
                            .id("leaderboard-row")
                            .role(Role::Button)
                            .aria_label("Leaderboard")
                            .a11y_synthetic_children(move |builder| {
                                if active {
                                    builder
                                        .parent_node()
                                        .set_aria_current(accesskit::AriaCurrent::Page);
                                }
                            })
                            .track_focus(&focus)
                            .key_context("Part")
                            .on_click(go)
                            .on_action(go_key),
                    ),
                    TextKind::SidebarRow,
                    &fonts,
                )
                .flex_none()
                .min_h(px(34.))
                .flex()
                .items_center()
                .gap(px(9.))
                .px(px(9.))
                .py(px(5.))
                .rounded(px(radius::ROW))
                .when(active, |d| {
                    d.bg(colour::raised()).text_color(colour::bone())
                })
                .hover(|s| s.bg(colour::hover()).text_color(colour::bone()))
                .cursor_pointer()
                .child(icons::icon(Icon::Trophy, 14.))
                .child(div().flex_1().truncate().child("Leaderboard"))
                .into_any_element(),
            );
        }
        let benches = lines
            .iter()
            .filter(|l| matches!(l, Line::Bench { .. }))
            .count();
        if narrowed && benches == 0 {
            out.push(self.no_rows("benches-empty", cx));
        }
        if benches == 0 {
            return out;
        }
        let count = lines.len();
        let mut list = div()
            .id("benches")
            // It clips its rows: one scrolled out is not showing (W2).
            .a11y_synthetic_children(|builder| {
                builder.parent_node().set_clips_children();
            })
            .role(Role::List)
            .aria_label("Benches")
            .aria_size_of_set(benches)
            .key_context("ChatList")
            .on_action(cx.listener(move |this, _: &MovePrevious, window, cx| {
                this.move_bench(Step::Previous, count, window, cx)
            }))
            .on_action(cx.listener(move |this, _: &MoveNext, window, cx| {
                this.move_bench(Step::Next, count, window, cx)
            }))
            .on_action(cx.listener(move |this, _: &MoveFirst, window, cx| {
                this.move_bench(Step::First, count, window, cx)
            }))
            .on_action(cx.listener(move |this, _: &MoveLast, window, cx| {
                this.move_bench(Step::Last, count, window, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenChat, window, cx| {
                let keys = this.bench_keys(cx);
                let key = keys
                    .iter()
                    .find(|k| {
                        this.benches
                            .focus
                            .get(*k)
                            .is_some_and(|h| h.is_focused(window))
                    })
                    .or(keys.get(this.benches.current))
                    .cloned();
                if let Some(key) = key {
                    this.open_line(&key, window, cx)
                }
            }))
            .flex_initial()
            .max_h(relative(0.58))
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(1.))
            .py(px(3.));
        let mut bench_index = 0;
        let mut i = 0;
        while i < lines.len() {
            let Line::Bench {
                id,
                title,
                snippet,
                resolution,
                dot,
                arrival,
                running,
                open,
                members,
            } = lines[i].clone()
            else {
                unreachable!("a bench comes before its models")
            };
            let key = lines[i].key();
            let current = i == self.benches.current;
            if current {
                self.benches.current_key = Some(key.clone());
            }
            let focus = self
                .benches
                .focus
                .entry(key.clone())
                .or_insert_with(|| cx.focus_handle())
                .clone()
                .tab_stop(current);
            let caret = self
                .benches
                .caret
                .entry(id.clone())
                .or_insert_with(|| cx.focus_handle())
                .clone()
                .tab_stop(current);
            let archive = self
                .benches
                .archive
                .entry(id.clone())
                .or_insert_with(|| cx.focus_handle())
                .clone()
                .tab_stop(current);
            let active = active_key.as_deref() == Some(key.as_str());
            let mut name = row_name(&title, active, dot);
            if let Some((_, tag)) = crate::state::resolution::tag(resolution) {
                name = format!("{name}, {tag}");
            }
            if let Some(snippet) = &snippet {
                name = format!("{name}, {}", super::parts::plain(snippet));
            }
            let group = format!("bench-row-{bench_index}");
            let click_key = key.clone();
            let fold_id = id.clone();
            let this = cx.entity().downgrade();
            let fold_this = this.clone();
            let archive_id = id.clone();
            let shown_dot = match dot {
                Some(d) => super::dot::dot(&key, d, arrival, 7., self.pulses.clone(), self.moving),
                None => {
                    super::dot::dot(&key, Dot::Idle, None, 7., self.pulses.clone(), self.moving)
                }
            };
            let fold = super::parts::caret(
                ("bench-caret", bench_index),
                &caret,
                open,
                Rc::new(move |_, cx| {
                    let id = fold_id.clone();
                    let _ = fold_this.update(cx, |s, cx| s.fold_bench(&id, cx));
                }),
            );
            let mut item = ring(
                div()
                    .id(ElementId::Name(key.clone().into()))
                    .group(group.clone())
                    .role(Role::ListItem)
                    .aria_label(name)
                    .a11y_synthetic_children(move |builder| {
                        if active {
                            builder
                                .parent_node()
                                .set_aria_current(accesskit::AriaCurrent::Page);
                        }
                    })
                    .aria_position_in_set(bench_index)
                    .aria_size_of_set(benches)
                    .track_focus(&focus)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_line(&click_key, window, cx)
                    })),
            )
            .flex()
            .flex_col();
            let row = theme::text(div(), TextKind::SidebarRow, &fonts)
                .min_h(px(34.))
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(2.))
                .pr(px(9.))
                .py(px(5.))
                .rounded(px(radius::ROW))
                .when(active, |d| {
                    d.bg(colour::raised()).text_color(colour::bone())
                })
                .hover(|s| s.bg(colour::hover()).text_color(colour::bone()))
                .cursor_pointer()
                .child(fold)
                .child(row_text(&title, snippet.as_deref(), cx))
                .children(resolution_tag(resolution, cx))
                .child(shown_dot)
                .child(div().flex_none().my(px(-5.)).mr(px(-7.)).child(row_icon(
                    ("archive-bench", bench_index),
                    &group,
                    &archive,
                    Icon::Archive,
                    "Archive bench",
                    "Archive bench (it can be restored for 7 days)",
                    running.then_some("Stop the running models before archiving"),
                    false,
                    Shown::OnHover,
                    Rc::new(move |_, cx| {
                        let id = archive_id.clone();
                        let _ = this.update(cx, |s, cx| s.archive_bench(id, cx));
                    }),
                )));
            item = item.child(row);
            i += 1;
            if open {
                let mut models = div()
                    .id(("bench-models", bench_index))
                    .role(Role::List)
                    .aria_label(format!("{title} models"))
                    .aria_size_of_set(members)
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .mt(px(1.))
                    .mb(px(3.))
                    .ml(px(17.));
                while let Some(Line::Member {
                    bench,
                    id,
                    name,
                    title,
                    dot,
                    arrival,
                    position,
                    count,
                }) = lines.get(i).cloned()
                {
                    let key = lines[i].key();
                    let current = i == self.benches.current;
                    if current {
                        self.benches.current_key = Some(key.clone());
                    }
                    let focus = self
                        .benches
                        .focus
                        .entry(key.clone())
                        .or_insert_with(|| cx.focus_handle())
                        .clone()
                        .tab_stop(current);
                    let active = active_key.as_deref() == Some(key.as_str());
                    let click = Place::Member {
                        bench: bench.clone(),
                        member: id.clone(),
                    };
                    let label = row_name(&name, active, (dot != Dot::Idle).then_some(dot));
                    let member = ring(
                        div()
                            .id(ElementId::Name(key.clone().into()))
                            .role(Role::ListItem)
                            .aria_label(label)
                            .when(!title.is_empty(), |d| d.aria_description(title.clone()))
                            .a11y_synthetic_children(move |builder| {
                                if active {
                                    builder
                                        .parent_node()
                                        .set_aria_current(accesskit::AriaCurrent::Page);
                                }
                            })
                            .aria_position_in_set(position)
                            .aria_size_of_set(count)
                            .track_focus(&focus)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.go(click.clone(), window, cx)
                            })),
                    );
                    models = models.child(
                        theme::text(member, TextKind::BenchMember, &fonts)
                            .min_h(px(30.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .px(px(7.))
                            .py(px(3.))
                            .rounded(px(radius::ROW))
                            .when(active, |d| {
                                d.bg(colour::raised()).text_color(colour::bone())
                            })
                            .hover(|s| s.bg(colour::hover()).text_color(colour::bone()))
                            .cursor_pointer()
                            .child(super::dot::dot(
                                &id,
                                dot,
                                arrival,
                                7.,
                                self.pulses.clone(),
                                self.moving,
                            ))
                            // par-bench: the model's id as the name's `title`
                            // (none while the bench hides names).
                            .child({
                                let shown = div()
                                    .id(ElementId::Name(format!("{key}-name").into()))
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(name);
                                if title.is_empty() {
                                    shown.into_any_element()
                                } else {
                                    crate::view::controls::cursor_tip(
                                        shown,
                                        ElementId::Name(format!("{key}-tip").into()),
                                        title.clone(),
                                    )
                                    .into_any_element()
                                }
                            }),
                    );
                    i += 1;
                }
                item = item.child(models);
            }
            list = list.child(item);
            bench_index += 1;
        }
        out.push(list.into_any_element());
        out
    }
}

// chat-fix: where focus goes when the focused element goes (W1).
impl Sidebar {
    /// A Bench line or one of its controls has focus.
    pub(crate) fn benches_hold_focus(&self, window: &Window) -> bool {
        self.benches
            .focus
            .values()
            .chain(self.benches.caret.values())
            .chain(self.benches.archive.values())
            .any(|h| h.is_focused(window))
    }

    /// The current Bench line's focus, else the leaderboard row's.
    pub(crate) fn bench_current_focus(&self) -> Option<FocusHandle> {
        self.benches
            .current_key
            .as_ref()
            .and_then(|key| self.benches.focus.get(key))
            .or(self.benches.leaderboard.as_ref())
            .cloned()
    }
}
