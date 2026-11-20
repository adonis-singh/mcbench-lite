//! The Connect an AI pane (`ConnectPane`, `web/app.js:1153-1177`;
//! `settings-panel.mjs` `CONNECT_WAYS`; inventory `components.md` E.9):
//! each way in, what it needs, and a button per provider pane. Cursor's
//! way is not in the app (inventory README).

use gpui_kit::*;

use super::parts::{pane_head, pane_title};
use super::{SettingsLayer, logo};
use crate::state::settings::{self as panel, Draft, Pane};
use crate::view::controls::fonts;
use crate::view::icons::{self, Icon};
use crate::view::theme::{self, TextKind, colour, shadow};
use crate::view::{Activate, ring};

impl SettingsLayer {
    pub(super) fn render_connect(
        &mut self,
        draft: &Draft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fonts = fonts(cx);
        let entries = draft.entries();
        let ways = panel::connect_ways(&entries).unwrap_or_else(|e| panic!("{e}"));
        let mut column = div().flex().flex_col().gap(px(26.)).child(pane_head(vec![
            div()
                .text_color(colour::ash())
                .child(icons::icon(Icon::Plug, 20.))
                .into_any_element(),
            pane_title("Connect an AI", cx),
        ]));
        column = column.child(
            theme::text(
                super::parts::text_label(
                    "connect-intro",
                    "mcbench builds with a model you connect. Choose one way in; you can add others later.",
                ),
                TextKind::SettingsValue,
                &fonts,
            )
            .mt(px(-12.))
            .text_size(px(13.))
            .line_height(relative(1.5))
            .text_color(colour::ash()),
        );
        let mut list = div().flex().flex_col().gap(px(12.));
        for (way, panes) in ways {
            let mut buttons = div().flex().flex_wrap().gap(px(8.));
            for p in panes {
                let id = p.id.clone();
                let state = panel::connect_state(p).to_string();
                let connected = panel::usable(p);
                let starts = panel::connect_starts(p);
                let method = p.sign_in.as_ref().and_then(|o| o.methods.first().cloned());
                let this = cx.entity().downgrade();
                let press = std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                    let (id, method) = (id.clone(), method.clone());
                    let _ = this.update(cx, |s, cx| {
                        if starts {
                            // `onConnect`: the pane opens and starts its
                            // sign-in (or install).
                            s.start_with = Some((id.clone(), method));
                        }
                        s.nav_current = Some(id.clone());
                        s.show_pane(Pane::Provider(id), window, cx);
                    });
                });
                let (click, key, a11y) = (press.clone(), press.clone(), press);
                let focus = crate::view::controls::focus_for(
                    SharedString::from(format!("connect-{}", p.id)),
                    _window,
                    cx,
                )
                .tab_stop(true);
                buttons = buttons.child(
                    theme::text(
                        ring(
                            div()
                                .id(SharedString::from(format!("connect-{}", p.id)))
                                .role(Role::Button)
                                .aria_label(format!("{}, {state}", p.label))
                                .track_focus(&focus)
                                .key_context("Part")
                                .on_click(move |_, window, cx| click(window, cx))
                                .on_a11y_action(accesskit::Action::Click, move |_, window, cx| {
                                    a11y(window, cx)
                                })
                                .on_action(move |_: &Activate, window, cx| key(window, cx)),
                        )
                        .min_h(px(32.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .rounded(px(8.))
                        .bg(colour::raised())
                        .shadow(shadow::control_face())
                        .cursor_pointer()
                        .hover(|s| s.bg(colour::hover())),
                        TextKind::Button,
                        &fonts,
                    )
                    .text_size(px(12.5))
                    .child(logo(&p.id, 16.))
                    .child(p.label.clone())
                    .child(
                        div()
                            .font_weight(FontWeight(400.))
                            .text_color(if connected {
                                colour::ash()
                            } else {
                                colour::dim()
                            })
                            .child(state),
                    )
                    .child(
                        div()
                            .text_color(colour::dim())
                            .child(icons::icon(Icon::Forward, 13.)),
                    ),
                );
            }
            list = list.child(
                div()
                    .id(way.key)
                    .role(Role::Group)
                    .aria_label(way.title)
                    .px(px(16.))
                    .py(px(14.))
                    .rounded(px(10.))
                    .bg(colour::well())
                    .shadow(theme::settings::inset_line())
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        theme::text(
                            div()
                                .id("way-title")
                                .role(Role::Heading)
                                .aria_label(way.title)
                                .aria_level(4),
                            TextKind::SettingsRowTitle,
                            &fonts,
                        )
                        .text_size(px(13.5))
                        .font_weight(FontWeight(600.))
                        .child(way.title),
                    )
                    .child(
                        theme::text(
                            super::parts::text_label("way-need", way.need),
                            TextKind::SettingsValue,
                            &fonts,
                        )
                        .text_size(px(12.5))
                        .line_height(relative(1.5))
                        .text_color(colour::ash()),
                    )
                    .child(buttons.mt(px(4.))),
            );
        }
        column.child(list).into_any_element()
    }
}
