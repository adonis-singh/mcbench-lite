//! A focused synthetic node (a link inside text, named focused through
//! `A11ySubtreeBuilder::set_focus`, vendor patch 1) stays the tree's focus
//! while its cached view is reused (vendor patch 9) because a sibling
//! redraws every frame: a running chat's pulse in the sidebar, the 3D pane.

use gpui::accesskit::Role;
use gpui::prelude::*;
use gpui::{Context, Entity, FocusHandle, TestAppContext, Window, div, px};

/// A paragraph whose focusable box (no node of its own) is a link: the
/// paragraph names its synthetic link node as focused while the box has
/// keyboard focus, as `view::text_leaf` does.
struct Paragraph {
    focus: FocusHandle,
    renders: usize,
}

impl Render for Paragraph {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders += 1;
        let focused = self.focus.is_focused(window);
        div()
            .id("paragraph")
            .role(Role::Paragraph)
            .aria_label("Read the wiki page on lamps")
            .a11y_synthetic_children(move |builder| {
                let link = builder.synthetic_node_id(("link", 0));
                let mut node = gpui::accesskit::Node::new(Role::Link);
                node.set_label("the wiki page on lamps");
                builder.push_child(link, node);
                if focused {
                    builder.set_focus(link);
                }
            })
            .size(px(200.))
            .child(div().id("link-box").track_focus(&self.focus).size(px(40.)))
    }
}

/// A sibling that draws again whenever it is notified.
struct Ticker(usize);

impl Render for Ticker {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.0 += 1;
        div().size(px(10.)).child(format!("{}", self.0))
    }
}

struct Root {
    paragraph: Entity<Paragraph>,
    ticker: Entity<Ticker>,
}

impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(
                self.paragraph
                    .clone()
                    .cached(gpui::StyleRefinement::default().size_full()),
            )
            .child(self.ticker.clone())
    }
}

fn focused_role(vcx: &mut gpui::VisualTestContext) -> (Role, Option<String>) {
    let update = vcx
        .update(|window, _| window.a11y_tree_update_for_test())
        .expect("a frame built the tree");
    let tree = accesskit_consumer::Tree::new(update, true);
    let focus = tree.state().focus().expect("a focused node");
    (focus.role(), focus.label())
}

#[gpui::test]
fn a_focused_link_in_a_reused_cached_view_stays_focused(cx: &mut TestAppContext) {
    let (root, vcx) = cx.add_window_view(|window, cx| {
        window.activate_a11y_for_test();
        Root {
            paragraph: cx.new(|cx| Paragraph {
                focus: cx.focus_handle(),
                renders: 0,
            }),
            ticker: cx.new(|_| Ticker(0)),
        }
    });
    vcx.run_until_parked();
    let paragraph = vcx.update(|_, cx| root.read(cx).paragraph.clone());
    let ticker = vcx.update(|_, cx| root.read(cx).ticker.clone());
    let focus = vcx.update(|_, cx| paragraph.read(cx).focus.clone());
    vcx.update(|window, cx| window.focus(&focus, cx));
    vcx.run_until_parked();
    assert_eq!(
        focused_role(vcx),
        (Role::Link, Some("the wiki page on lamps".into()))
    );
    let drawn = vcx.update(|_, cx| paragraph.read(cx).renders);
    for frame in 0..10 {
        vcx.update(|_, cx| ticker.update(cx, |_, cx| cx.notify()));
        vcx.run_until_parked();
        assert_eq!(
            vcx.update(|_, cx| paragraph.read(cx).renders),
            drawn,
            "frame {frame}: the paragraph's cached view was reused"
        );
        assert_eq!(
            focused_role(vcx),
            (Role::Link, Some("the wiki page on lamps".into())),
            "frame {frame}: the link is still the focused node"
        );
    }
}
