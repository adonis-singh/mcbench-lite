//! The shortcuts and layers wired (`view::keys`), in process on the
//! gallery: a bind runs its action, a modal layer holds binds back (search
//! still runs), Escape closes only the topmost layer, a popover's Escape
//! closes the popover and goes no further, and a bind is the app's key even
//! when held back.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::*;
use gpui::accesskit::Role;
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::dev::gallery::Gallery;
use mcbench_ui::state::keymap::BindAction;
use mcbench_ui::state::layers::Layer;
use mcbench_ui::view::keys;

type Ran = Rc<RefCell<Vec<String>>>;

fn open(cx: &mut TestAppContext) -> (&mut VisualTestContext, Ran) {
    quiet_crashes();
    cx.update(mcbench_ui::view::setup);
    let ran: Ran = Rc::default();
    for action in BindAction::ALL {
        let ran = ran.clone();
        cx.update(|cx| {
            keys::on_bind(cx, action, move |_, _| {
                ran.borrow_mut().push(action.id().into())
            })
        });
    }
    let (_, vcx) = cx.add_window_view(|window, cx| {
        window.activate_a11y_for_test();
        Gallery::new(None, cx)
    });
    vcx.run_until_parked();
    (vcx, ran)
}

const MOD: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

#[gpui::test]
fn a_bind_runs_and_a_modal_layer_holds_binds_back_but_not_search(cx: &mut TestAppContext) {
    let (vcx, ran) = open(cx);
    vcx.simulate_keystrokes(&format!(
        "{MOD}-1 {MOD}-2 {MOD}-k {MOD}-shift-o {MOD}-b alt-s"
    ));
    assert_eq!(
        *ran.borrow(),
        ["build", "bench", "search", "new", "sidebar", "select"]
    );
    ran.borrow_mut().clear();
    vcx.update(|_, cx| keys::open_layer(cx, Layer::Settings, |_, _| {}));
    vcx.simulate_keystrokes(&format!("{MOD}-1 {MOD}-b {MOD}-k"));
    assert_eq!(*ran.borrow(), ["search"], "only search runs under Settings");
    // The drawer: the sidebar bind closes it.
    ran.borrow_mut().clear();
    vcx.update(|_, cx| {
        keys::close_layer(cx, Layer::Settings);
        keys::open_layer(cx, Layer::Drawer, |_, _| {});
    });
    vcx.simulate_keystrokes(&format!("{MOD}-b {MOD}-1"));
    assert_eq!(*ran.borrow(), ["sidebar"]);
}

#[gpui::test]
fn escape_closes_only_the_topmost_layer(cx: &mut TestAppContext) {
    let (vcx, _) = open(cx);
    // Focus somewhere, as the app always has (the composer at start).
    let tree0 = tree(vcx);
    act(
        vcx,
        named(&nodes(&tree0), Role::Button, "Raised"),
        gpui::accesskit::Action::Focus,
    );
    let closed: Ran = Rc::default();
    for layer in [Layer::Settings, Layer::Command, Layer::Share] {
        let closed = closed.clone();
        vcx.update(move |_, cx| {
            keys::open_layer(cx, layer, move |_, cx| {
                closed.borrow_mut().push(layer.id().into());
                keys::close_layer(cx, layer);
            })
        });
    }
    vcx.simulate_keystrokes("escape");
    assert_eq!(*closed.borrow(), ["command"]);
    vcx.simulate_keystrokes("escape");
    assert_eq!(*closed.borrow(), ["command", "settings"]);
    vcx.simulate_keystrokes("escape escape");
    assert_eq!(
        *closed.borrow(),
        ["command", "settings", "share"],
        "then nothing is left"
    );
    // With no layer open, Escape cancels an edit.
    let cancelled = Rc::new(RefCell::new(0));
    let c = cancelled.clone();
    vcx.update(move |_, cx| keys::on_escape(cx, move |_, _| *c.borrow_mut() += 1));
    vcx.simulate_keystrokes("escape");
    assert_eq!(*cancelled.borrow(), 1);
}

#[gpui::test]
fn a_popovers_escape_goes_no_further(cx: &mut TestAppContext) {
    let (vcx, _) = open(cx);
    let closed: Ran = Rc::default();
    let c = closed.clone();
    vcx.update(move |_, cx| {
        keys::open_layer(cx, Layer::Share, move |_, _| {
            c.borrow_mut().push("share".into())
        })
    });
    let tree1 = tree(vcx);
    click(vcx, named(&nodes(&tree1), Role::Button, "Effort: medium"));
    vcx.simulate_keystrokes("escape");
    vcx.executor().advance_clock(Duration::from_millis(110));
    vcx.run_until_parked();
    let tree2 = tree(vcx);
    assert!(
        !has(&nodes(&tree2), Role::Menu, "Effort"),
        "the popover closed"
    );
    assert!(closed.borrow().is_empty(), "the layer under it did not");
    vcx.simulate_keystrokes("escape");
    assert_eq!(*closed.borrow(), ["share"]);
}
