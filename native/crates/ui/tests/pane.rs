//! The 3D pane (pane agent) in process, over the stand-in core and the fake
//! pane (`dev::pane`): the frame path, the cover, banner and toolbar, the
//! camera keys, a click on a block, the selection tool, Fill, Day/Night,
//! player mode and the exports, through the accessibility tree and the
//! pointer and keys as a person uses them.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use gpui::accesskit::{Action, Role};
use gpui::{Modifiers, MouseButton, VisualTestContext, point, px};
use mcbench_ui::dev::pane::{Behaviour, FakePane};
use mcbench_ui::dev::standin::{LiveFailures, StandIn};
use mcbench_ui::state::live;
use mcbench_ui::state::place::Place;
use mcbench_ui::{Core, Running, camera_keys};

const CHAT: &str = "chat-gate";

struct Open<'a> {
    shell: ShellUnderTest,
    core: Arc<StandIn>,
    pane: Arc<FakePane>,
    vcx: &'a mut VisualTestContext,
    saved: std::path::PathBuf,
}

/// The shell on the gate fixture with the fake pane, `CHAT` open.
fn open<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    behaviour: Behaviour,
    failures: LiveFailures,
) -> Open<'a> {
    use mcbench_ui::dev::fixtures;
    use mcbench_ui::view::shell::{Shell, ShellOptions};
    quiet_crashes();
    let dir = TempDir::new(test);
    let saved = dir.0.join("saved");
    std::fs::create_dir_all(&saved).expect("the save folder");
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let core = Arc::new(StandIn::new(
        fixtures::gate(&texts),
        texts.streamed.clone(),
        Duration::ZERO,
        None,
    ));
    let pane = FakePane::manual(behaviour);
    core.set_pane(pane.clone(), failures);
    let dyn_core: Arc<dyn Core> = core.clone();
    let log_path = dir.0.join("actions.jsonl");
    let log = mcbench_ui::ActionLog::create(&log_path);
    let saver_folder = saved.clone();
    cx.update(|cx| {
        mcbench_ui::view::setup(cx);
        mcbench_ui::view::world::set_saver(cx, mcbench_ui::view::world::fixed_saver(saver_folder));
    });
    let inbox = Arc::new(Inbox::default());
    let for_shell = dyn_core.clone();
    let (entity, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Shell::new(
            ShellOptions {
                core: for_shell,
                log: Some(log),
                opener: std::rc::Rc::new(|_, _| {}),
                metrics: std::rc::Rc::new(std::cell::RefCell::new(
                    mcbench_ui::view::metrics::Metrics::new(Instant::now()),
                )),
                start: Some(Place::Chat { id: CHAT.into() }),
                preferences: mcbench_ui::Preferences {
                    last_model: Some(("anthropic".into(), mcbench_ui::dev::standin::MODEL.into())),
                    ..mcbench_ui::Preferences::default()
                },
                picker: mcbench_ui::view::fixed_picker(Vec::new()),
                selections: Vec::new(),
            },
            window,
            cx,
        )
    });
    dyn_core.attach(inbox.clone());
    // Focus events (a blur releasing keys) need an active window.
    vcx.update(|window, _| window.activate_window());
    let shell = ShellUnderTest::new(entity, inbox, log_path, dir);
    pump(&shell, vcx);
    Open {
        shell,
        core,
        pane,
        vcx,
        saved,
    }
}

impl Open<'_> {
    /// Applies what arrives until `done` holds, within 5 s (the fake pane
    /// draws on its own thread).
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.pane.pump();
            pump(&self.shell, self.vcx);
            self.pane.pump();
            pump(&self.shell, self.vcx);
            if done(self) {
                return;
            }
            assert!(Instant::now() < deadline, "not within 5 s: {what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// The pane's work and the window's updates, twice over.
    fn settle(&mut self) {
        for _ in 0..3 {
            self.pane.pump();
            pump(&self.shell, self.vcx);
        }
    }

    fn has(&mut self, role: Role, label: &str) -> bool {
        let tree = tree(self.vcx);
        has(&nodes(&tree), role, label)
    }

    fn painted(&mut self) {
        self.until("a frame painted and the cover off", |o| {
            let tree = tree(o.vcx);
            let n = nodes(&tree);
            !n.iter()
                .any(|n| n.label().as_deref() == Some(live::PREPARING_CHAT))
                && has(&n, Role::Toolbar, live::TOOLBAR)
        });
    }

    fn press(&mut self, role: Role, label: &str) {
        let tree = tree(self.vcx);
        let n = nodes(&tree);
        let node = named(&n, role, label);
        act(self.vcx, node, Action::Click);
        self.settle();
    }

    fn focus_pane(&mut self) {
        let tree = tree(self.vcx);
        let n = nodes(&tree);
        let node = named(&n, Role::Canvas, live::PANE_NAME);
        act(self.vcx, node, Action::Focus);
        self.settle();
        assert_eq!(
            focused(self.vcx),
            (Role::Canvas, Some(live::PANE_NAME.to_string())),
            "the pane has focus"
        );
    }

    fn centre(&mut self) -> gpui::Point<gpui::Pixels> {
        let tree = tree(self.vcx);
        let n = nodes(&tree);
        let node = named(&n, Role::Canvas, live::PANE_NAME);
        let bounds = node.bounding_box().expect("the pane's bounds");
        centre_of(self.vcx, bounds)
    }

    fn polite(&mut self) -> Option<String> {
        let tree = tree(self.vcx);
        let n = nodes(&tree);
        of_role(&n, Role::Status)
            .into_iter()
            .find(|n| n.live() == gpui::accesskit::Live::Polite)
            .and_then(|n| n.label())
    }
}

#[gpui::test]
fn a_chats_world_is_covered_until_its_frame_is_painted_then_shows_its_toolbar(
    cx: &mut gpui::TestAppContext,
) {
    let mut o = open(
        cx,
        "pane-first",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    assert!(o.pane.counts().drawn >= 1);
    let tree = tree(o.vcx);
    let n = nodes(&tree);
    let canvas = named(&n, Role::Canvas, live::PANE_NAME);
    let description = canvas.description().expect("the pane is described");
    assert!(description.ends_with("blocks, playing"), "{description}");
    assert!(has(&n, Role::Button, "Pause"));
    assert!(has(&n, Role::Button, live::RESTART));
    let step = named(&n, Role::Button, live::STEP);
    assert!(step.is_disabled(), "Step is disabled while playing");
    assert!(has(
        &n,
        Role::Label,
        &format!("{}: 0 ticks", live::TICKS_TITLE)
    ));
    o.pane.stop();
}

#[gpui::test]
fn pause_step_and_play_end_in_the_state_the_core_publishes(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-toolbar",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, "Pause");
    o.until("Play shows", |o| o.has(Role::Button, "Play"));
    o.press(Role::Button, live::STEP);
    o.until("one tick", |o| {
        o.has(Role::Label, &format!("{}: 1 ticks", live::TICKS_TITLE))
    });
    o.press(Role::Button, "Play");
    o.until("Pause shows", |o| o.has(Role::Button, "Pause"));
    o.pane.stop();
}

#[gpui::test]
fn the_camera_keys_are_held_exactly_and_released_on_key_up(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-keys",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.focus_pane();
    // A W typed with the pane's focus is the camera's, not the composer's.
    o.vcx.simulate_keystrokes("w");
    o.until("held", |o| o.pane.held() == camera_keys::FORWARD);
    o.vcx.simulate_event(gpui::KeyUpEvent {
        keystroke: gpui::Keystroke::parse("w").unwrap(),
    });
    o.until("released", |o| o.pane.held() == 0);
    // Focus leaving the pane releases what it held.
    o.vcx.simulate_keystrokes("d");
    o.until("held", |o| o.pane.held() == camera_keys::RIGHT);
    o.vcx.simulate_keystrokes("tab");
    o.settle();
    let held = o
        .shell
        .entity
        .update(o.vcx, |s, cx| s.world().read(cx).held());
    let now = focused(o.vcx);
    assert_eq!(held, 0, "the window let go after Tab to {now:?}");
    o.until("released on blur", |o| o.pane.held() == 0);
    o.focus_pane();
    o.vcx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("w").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    o.until("held", |o| o.pane.held() == camera_keys::FORWARD);
    o.vcx.simulate_event(gpui::KeyUpEvent {
        keystroke: gpui::Keystroke::parse("w").unwrap(),
    });
    o.until("released", |o| o.pane.held() == 0);
    // Shift+W is not the camera's.
    o.vcx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("shift-w").unwrap(),
        is_held: false,
        prefer_character_input: false,
    });
    o.settle();
    assert_eq!(o.pane.held(), 0);
    o.pane.stop();
}

#[gpui::test]
fn a_click_on_a_block_is_sent_marked_and_announced(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-click",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    let at = o.centre();
    o.vcx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    o.vcx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    o.until("the click is announced", |o| {
        o.polite()
            .is_some_and(|t| t.contains(" at ") && t.contains(", "))
    });
    let said = o.polite().expect("announced");
    assert!(
        said.ends_with("takes no input") || said.ends_with("toggled") || said.ends_with("used"),
        "{said}"
    );
    assert!(o.pane.overlays_now().marked.is_some());
    o.pane.stop();
}

#[gpui::test]
fn enter_uses_the_block_at_the_centre(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-enter",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.focus_pane();
    o.vcx.simulate_keystrokes("enter");
    o.until("the centre's block is announced", |o| {
        o.polite().is_some_and(|t| t.contains(" at "))
    });
    o.pane.stop();
}

#[gpui::test]
fn a_selection_drag_lands_in_the_composer_as_a_chip(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-select",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::SELECT);
    let selecting = o
        .shell
        .entity
        .update(o.vcx, |s, cx| s.world().read(cx).selecting());
    assert!(selecting);
    let c = o.centre();
    let from = point(c.x - px(20.), c.y + px(40.));
    let to = point(c.x + px(20.), c.y + px(60.));
    o.vcx
        .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    o.until("the first block picked", |o| {
        o.pane.overlays_now().boxes.iter().any(|b| b.fill == 0.16)
    });
    o.vcx
        .simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    o.settle();
    o.vcx
        .simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    o.until("the chip in the composer", |o| {
        let tree = tree(o.vcx);
        nodes(&tree).iter().any(|n| {
            n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with("Remove "))
        })
    });
    // Its box is drawn over the world.
    o.until("the draft box drawn", |o| {
        o.pane.overlays_now().boxes.iter().any(|b| b.fill == 0.08)
    });
    // Escape stops the tool and goes no further.
    o.focus_pane();
    o.vcx.simulate_keystrokes("escape");
    o.settle();
    let selecting = o
        .shell
        .entity
        .update(o.vcx, |s, cx| s.world().read(cx).selecting());
    assert!(!selecting);
    o.pane.stop();
}

#[gpui::test]
fn fill_is_a_layer_escape_closes(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-fill",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::FILL);
    assert!(o.has(Role::Button, live::BACK));
    o.vcx.simulate_keystrokes("escape");
    o.settle();
    assert!(o.has(Role::Button, live::FILL));
    o.pane.stop();
}

#[gpui::test]
fn day_night_switches_the_pane_and_is_kept(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-night",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::DAY_NIGHT);
    o.until("night", |o| {
        o.pane.time_now() == mcbench_ui::TimeOfDay::Night
    });
    let kept = o.shell.entity.update(o.vcx, |s, _| s.preferences().night);
    assert!(kept);
    let tree = tree(o.vcx);
    let n = nodes(&tree);
    let switch = named(&n, Role::Button, live::DAY_NIGHT);
    assert_eq!(switch.toggled(), Some(gpui::accesskit::Toggled::True));
    o.pane.stop();
}

#[gpui::test]
fn no_adapter_covers_the_pane_with_the_renderers_sentence(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-no-adapter",
        Behaviour::default(),
        LiveFailures {
            no_adapter: true,
            ..LiveFailures::default()
        },
    );
    o.until("the sentence", |o| {
        o.has(Role::Status, mcbench_ui::dev::standin::NO_ADAPTER)
    });
    o.pane.stop();
}

#[gpui::test]
fn a_first_placement_that_failed_says_so_with_try_again(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-place-fails",
        Behaviour::default(),
        LiveFailures {
            place: true,
            ..LiveFailures::default()
        },
    );
    o.until("the banner", |o| {
        o.has(Role::Status, mcbench_ui::dev::standin::PLACE_FAILED)
    });
    o.core.set_live_failures(LiveFailures::default());
    o.press(Role::Button, "Try again");
    o.painted();
    o.pane.stop();
}

#[gpui::test]
fn a_second_lost_device_covers_the_pane(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-lost",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.pane.lose_device();
    o.pane.lose_device();
    o.until("the sentence", |o| {
        o.has(Role::Status, mcbench_ui::dev::pane::DEVICE_LOST)
    });
    o.pane.stop();
}

#[gpui::test]
fn a_stopped_world_says_why_and_takes_only_restart(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-stopped",
        Behaviour::default(),
        LiveFailures {
            stop_after: Some(1),
            ..LiveFailures::default()
        },
    );
    o.painted();
    o.core.live_ticks(1);
    o.until("the banner", |o| {
        o.has(Role::Status, mcbench_ui::dev::standin::STOPPED)
    });
    let state = o
        .shell
        .entity
        .update(o.vcx, |s, cx| s.world().read(cx).state().cloned())
        .expect("a state");
    assert!(matches!(state.running, Some(Running::Stopped(_))));
    let tree = tree(o.vcx);
    let n = nodes(&tree);
    assert!(named(&n, Role::Button, "Play").is_disabled());
    assert!(!named(&n, Role::Button, live::RESTART).is_disabled());
    o.pane.stop();
}

#[gpui::test]
fn the_software_floor_is_said_once_in_the_bar(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-floor",
        Behaviour {
            software: true,
            ..Behaviour::default()
        },
        LiveFailures::default(),
    );
    o.painted();
    assert!(o.has(Role::Label, mcbench_ui::dev::pane::FLOOR));
    o.pane.stop();
}

#[gpui::test]
fn player_mode_stands_beside_the_build_and_sends_its_keys(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-player",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::PLAYER);
    o.until("standing", |o| {
        o.shell.entity.update(o.vcx, |s, cx| {
            s.world().read(cx).state().map(|s| s.player) == Some(mcbench_ui::PlayerState::Standing)
        })
    });
    assert!(o.has(Role::Status, live::HINT_FREE));
    let at = o.centre();
    o.vcx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    o.vcx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    o.settle();
    assert!(o.has(Role::Status, live::HINT_TAKEN));
    assert!(
        o.vcx.update(|window, _| window.pointer_locked()),
        "a click takes the pointer"
    );
    // The locked pointer's motion turns the look, 0.15° a pixel.
    let look = |o: &mut Open| {
        o.shell
            .entity
            .update(o.vcx, |s, cx| s.world().read(cx).player_look())
            .expect("player mode")
    };
    let (yaw, pitch) = look(&mut o);
    o.vcx.simulate_event(gpui::MouseMotionEvent {
        delta: point(px(100.), px(-20.)),
        modifiers: Modifiers::none(),
    });
    o.settle();
    let (yaw2, pitch2) = look(&mut o);
    assert!((yaw2 - yaw - 15.0).abs() < 0.01, "{yaw} -> {yaw2}");
    assert!(
        (pitch2 - (pitch - 3.0).clamp(-90.0, 90.0)).abs() < 0.01,
        "{pitch} -> {pitch2}"
    );
    // The window losing focus gives the pointer back.
    o.vcx.deactivate_window();
    o.settle();
    assert!(!o.vcx.update(|window, _| window.pointer_locked()));
    assert!(o.has(Role::Status, live::HINT_FREE));
    o.vcx.update(|window, _| window.activate_window());
    o.settle();
    let at = o.centre();
    o.vcx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    o.vcx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    o.settle();
    assert!(o.vcx.update(|window, _| window.pointer_locked()));
    // Escape frees the pointer first.
    o.focus_pane();
    o.vcx.simulate_keystrokes("escape");
    o.settle();
    let focus = focused(o.vcx);
    assert!(
        !o.vcx.update(|window, _| window.pointer_locked()),
        "Escape gives the pointer back"
    );
    assert!(
        o.has(Role::Status, live::HINT_FREE),
        "focus {focus:?}, log {:?}",
        log_lines(&o.shell.log)
            .iter()
            .filter(|l| l["action"] == "pointer")
            .collect::<Vec<_>>()
    );
    // Perspective leaves player mode.
    o.press(Role::Button, live::PERSPECTIVE);
    o.until("left", |o| {
        o.shell.entity.update(o.vcx, |s, cx| {
            s.world().read(cx).state().map(|s| s.player) == Some(mcbench_ui::PlayerState::None)
        })
    });
    o.pane.stop();
}

#[gpui::test]
fn a_schematic_is_saved_where_the_saver_says_and_announced(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-export",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::DOWNLOAD);
    o.until("saved", |o| {
        o.polite()
            .is_some_and(|t| t.starts_with("Saved ") && t.ends_with(".schem"))
    });
    let files: Vec<_> = std::fs::read_dir(&o.saved)
        .expect("the save folder")
        .map(|e| e.expect("an entry").file_name())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    o.pane.stop();
}

#[gpui::test]
fn an_idle_pane_draws_nothing_more(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-idle",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    // The world is placed playing; the stand-in's clock does not run in
    // the tests, so nothing changes.
    let before = o.pane.counts().drawn;
    o.settle();
    assert_eq!(o.pane.counts().drawn, before);
    o.pane.stop();
}

#[gpui::test]
fn the_pane_is_told_its_physical_size(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-size",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    let size = o.pane.size_now();
    let scale = o.vcx.update(|window, _| window.scale_factor());
    assert!(size.width > 0 && size.height > 0);
    assert_eq!(size.scale, scale);
    o.pane.stop();
}

#[gpui::test]
fn with_no_renderer_joined_the_cover_says_so_and_offers_nothing(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-not-joined",
        Behaviour::default(),
        LiveFailures {
            not_joined: true,
            ..LiveFailures::default()
        },
    );
    o.until("the sentence", |o| {
        o.has(Role::Status, mcbench_ui::dev::standin::NOT_JOINED)
    });
    let tree = tree(o.vcx);
    let n = nodes(&tree);
    assert!(
        !has(&n, Role::Button, "Try again"),
        "no Try again that could never work"
    );
    assert!(!has(&n, Role::Toolbar, live::TOOLBAR), "no toolbar");
    assert!(!has(&n, Role::Button, "Pause") && !has(&n, Role::Button, "Play"));
    assert!(!has(&n, Role::Button, live::SELECT));
    o.pane.stop();
}

#[gpui::test]
fn a_click_answered_for_another_view_is_not_announced(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-other-click",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    let before = o.polite();
    let update = mcbench_ui::Update::Clicked {
        view: mcbench_ui::ViewKey("elsewhere".into()),
        block: [1, 2, 3],
        name: String::new(),
        input: None,
    };
    o.vcx.update(|window, cx| {
        o.shell
            .entity
            .update(cx, |s, cx| s.apply(update, window, cx))
    });
    o.settle();
    assert_eq!(o.polite(), before);
    o.pane.stop();
}

#[gpui::test]
fn a_restart_keeps_the_picture_with_no_cover(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-restart",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::RESTART);
    let tree = tree(o.vcx);
    let n = nodes(&tree);
    assert!(!has(&n, Role::Status, live::PREPARING_CHAT));
    assert!(has(&n, Role::Toolbar, live::TOOLBAR));
    o.pane.stop();
}

#[gpui::test]
fn before_setup_a_selection_says_the_data_is_not_ready(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-not-ready",
        Behaviour::default(),
        LiveFailures {
            data_not_ready: true,
            ..LiveFailures::default()
        },
    );
    o.painted();
    o.press(Role::Button, live::SELECT);
    let c = o.centre();
    let from = point(c.x - px(20.), c.y + px(40.));
    o.vcx
        .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    o.until("the first block picked", |o| {
        o.pane.overlays_now().boxes.iter().any(|b| b.fill == 0.16)
    });
    o.vcx
        .simulate_mouse_up(from, MouseButton::Left, Modifiers::none());
    o.until("the notice", |o| {
        let tree = tree(o.vcx);
        nodes(&tree).iter().any(|n| {
            n.label()
                .is_some_and(|l| l.contains(mcbench_ui::dev::standin::DATA_NOT_READY))
        })
    });
    o.pane.stop();
}

#[gpui::test]
fn the_player_looks_where_the_core_stood_it(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-player-look",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::PLAYER);
    o.until("the look from the eye's first frame", |o| {
        let look = o
            .shell
            .entity
            .update(o.vcx, |s, cx| s.world().read(cx).player_look());
        let camera = o.pane.camera_now();
        camera.mode == mcbench_ui::CameraMode::Player
            && look.is_some_and(|(yaw, pitch)| {
                let (y, p) = live::start_look(camera.rotation, camera.angle);
                (yaw - y).abs() < 0.01 && (pitch - p).abs() < 0.01
            })
    });
    o.pane.stop();
}

#[gpui::test]
fn hovering_a_selection_chip_lights_its_box(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-chip-hover",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::SELECT);
    let c = o.centre();
    let from = point(c.x - px(20.), c.y + px(40.));
    o.vcx
        .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    o.until("the first block picked", |o| {
        o.pane.overlays_now().boxes.iter().any(|b| b.fill == 0.16)
    });
    o.vcx
        .simulate_mouse_up(from, MouseButton::Left, Modifiers::none());
    o.until("the draft box drawn", |o| {
        o.pane.overlays_now().boxes.iter().any(|b| b.fill == 0.08)
    });
    let tree = tree(o.vcx);
    let n = nodes(&tree);
    let chip = n
        .iter()
        .find(|n| {
            n.role() == Role::Group && n.description().is_some_and(|d| d.ends_with(" blocks"))
        })
        .copied()
        .expect("the chip");
    let at = centre_of(o.vcx, chip.bounding_box().expect("its bounds"));
    o.vcx.simulate_mouse_move(at, None, Modifiers::none());
    o.until("the box lit", |o| {
        o.pane.overlays_now().boxes.iter().any(|b| b.fill == 0.22)
    });
    o.vcx
        .simulate_mouse_move(point(px(5.), px(5.)), None, Modifiers::none());
    o.until("the box unlit", |o| {
        !o.pane.overlays_now().boxes.iter().any(|b| b.fill == 0.22)
    });
    o.pane.stop();
}

/// Draft selections over the chat's model world (`Y_OFFSET` below it):
/// A on the stone bricks, B on the oak door (a block the stand-in has no
/// icon for), C in the air above. Put in the draft as the selection tool
/// puts one, the pane's boxes and markers drawn after.
fn draft_three(o: &mut Open) {
    let selection = |letter: &str, min, max, blocks, block: Option<&str>| mcbench_ui::Selection {
        letter: letter.into(),
        label: format!("{letter} box"),
        min,
        max,
        blocks,
        block: block.map(str::to_string),
    };
    let three = vec![
        selection(
            "A",
            [4, 0, 4],
            [10, 4, 12],
            315,
            Some("minecraft:stone_bricks"),
        ),
        selection("B", [14, 0, 14], [14, 1, 14], 2, Some("minecraft:oak_door")),
        selection("C", [0, 6, 0], [1, 6, 1], 4, None),
    ];
    let shell = o.shell.entity.clone();
    o.vcx.update(|_, cx| {
        shell.update(cx, |s, cx| {
            for selection in three {
                s.add_selection(selection, cx);
            }
        })
    });
    o.settle();
}

fn drawn(o: &mut Open, selector: &'static str) -> bool {
    o.vcx.debug_bounds(selector).is_some()
}

#[gpui::test]
fn a_selections_icon_is_the_blocks_or_an_empty_square(cx: &mut gpui::TestAppContext) {
    use mcbench_ui::view::block_icons::{self, Known};
    let mut o = open(
        cx,
        "pane-block-icons",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    draft_three(&mut o);
    o.until("the chips' squares", |o| {
        drawn(o, "selection-A-icon")
            && drawn(o, "selection-B-empty")
            && drawn(o, "selection-C-empty")
    });
    // The core answered stone_bricks with its PNG: decoded at 16 px for the
    // chip, 80 px texel by texel for the marker.
    let (a, b) = o.vcx.update(|_, cx| {
        (
            block_icons::known("stone_bricks", cx),
            block_icons::known("oak_door", cx),
        )
    });
    match a {
        Some(Known::Shown(icon)) => {
            assert_eq!(
                icon.chip.size(0),
                gpui::size(gpui::DevicePixels(16), gpui::DevicePixels(16))
            );
            assert_eq!(
                icon.marker.size(0),
                gpui::size(gpui::DevicePixels(80), gpui::DevicePixels(80))
            );
        }
        _ => panic!("stone_bricks has its icon"),
    }
    match b {
        Some(Known::Refused(why)) => assert_eq!(why, mcbench_ui::dev::standin::UNKNOWN_BLOCK),
        _ => panic!("oak_door is refused with the core's sentence"),
    }
    // Over the world: A's icon, B's and C's empty squares.
    o.until("the pane's markers", |o| {
        drawn(o, "live-icon-0-icon")
            && drawn(o, "live-icon-1-empty")
            && drawn(o, "live-icon-2-empty")
    });
    assert!(!drawn(&mut o, "selection-A-empty") && !drawn(&mut o, "live-icon-0-empty"));
    o.pane.stop();
}

#[gpui::test]
fn before_setup_a_selections_icon_is_an_empty_square(cx: &mut gpui::TestAppContext) {
    use mcbench_ui::view::block_icons::{self, Known};
    let mut o = open(
        cx,
        "pane-block-icons-not-ready",
        Behaviour::default(),
        LiveFailures {
            data_not_ready: true,
            ..LiveFailures::default()
        },
    );
    o.painted();
    draft_three(&mut o);
    o.until("the core's answer", |o| {
        o.vcx.update(|_, cx| {
            matches!(
                block_icons::known("stone_bricks", cx),
                Some(Known::Refused(_))
            )
        })
    });
    let why = o
        .vcx
        .update(|_, cx| match block_icons::known("stone_bricks", cx) {
            Some(Known::Refused(why)) => why,
            _ => unreachable!(),
        });
    assert_eq!(why, mcbench_ui::dev::standin::DATA_NOT_READY);
    o.until("the empty squares", |o| {
        drawn(o, "selection-A-empty") && drawn(o, "live-icon-0-empty")
    });
    o.pane.stop();
}

/// GPUI's quit (`App::shutdown`, which every platform's quit runs) stops
/// the core once, with the fake pane's handle dropped after it.
#[gpui::test]
fn the_quit_stops_the_core_once(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-quit",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    let core: Arc<dyn Core> = o.core.clone();
    o.vcx
        .update(|_, cx| mcbench_ui::stop_core_on_quit(core, cx));
    assert_eq!(o.core.shutdowns(), 0);
    o.vcx.cx.update(|cx| cx.shutdown());
    assert_eq!(o.core.shutdowns(), 1, "stopped by the quit");
    // A second quit finds no handler left.
    o.vcx.cx.update(|cx| cx.shutdown());
    assert_eq!(o.core.shutdowns(), 1, "stopped once");
    o.pane.stop();
}

/// The same with the fake pane's handle dropped before the quit.
#[gpui::test]
fn the_quit_stops_the_core_once_after_the_pane_ended(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-quit-ended",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    let core: Arc<dyn Core> = o.core.clone();
    o.vcx
        .update(|_, cx| mcbench_ui::stop_core_on_quit(core, cx));
    o.pane.stop();
    o.settle();
    o.vcx.cx.update(|cx| cx.shutdown());
    assert_eq!(o.core.shutdowns(), 1);
}

/// Escape in player mode filling the window, as the page orders it
/// (inventory style-viewer-routes.md:1388): with the pointer taken, it
/// frees the pointer and goes no further; with the pointer free (or the
/// lock refused), it does as in map mode and closes the topmost layer,
/// the window filled, and player mode stays.
#[gpui::test]
fn escape_in_player_mode_frees_the_pointer_then_leaves_fill(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-player-escape",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::FILL);
    o.press(Role::Button, live::PLAYER);
    o.until("standing", |o| {
        o.shell.entity.update(o.vcx, |s, cx| {
            s.world().read(cx).state().map(|s| s.player) == Some(mcbench_ui::PlayerState::Standing)
        })
    });
    let at = o.centre();
    o.vcx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    o.vcx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    o.settle();
    assert!(o.vcx.update(|window, _| window.pointer_locked()), "taken");
    let in_player = |o: &mut Open| {
        o.shell
            .entity
            .update(o.vcx, |s, cx| s.world().read(cx).player_look())
            .is_some()
    };
    // Taken: Escape frees it, the window stays filled.
    o.vcx.simulate_keystrokes("escape");
    o.settle();
    assert!(!o.vcx.update(|window, _| window.pointer_locked()), "freed");
    assert!(o.has(Role::Button, live::BACK), "still filling the window");
    assert!(in_player(&mut o));
    // Free: Escape closes the filled window; player mode stays.
    o.vcx.simulate_keystrokes("escape");
    o.settle();
    assert!(o.has(Role::Button, live::FILL), "back beside the chat");
    assert!(in_player(&mut o), "still in player mode");
    o.pane.stop();
}

/// Player mode standing, the pane focused and W held: the stand-in holds
/// the forward bit, as the core holds the last `PlayerInput`'s keys.
fn holding_w<'a>(cx: &'a mut gpui::TestAppContext, test: &str) -> Open<'a> {
    let mut o = open(cx, test, Behaviour::default(), LiveFailures::default());
    o.painted();
    o.press(Role::Button, live::PLAYER);
    o.until("standing", |o| {
        o.shell.entity.update(o.vcx, |s, cx| {
            s.world().read(cx).state().map(|s| s.player) == Some(mcbench_ui::PlayerState::Standing)
        })
    });
    o.focus_pane();
    o.vcx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("w").expect("w"),
        is_held: false,
        prefer_character_input: false,
    });
    // The input goes out on the next paint (tests have no frame loop).
    o.vcx.update(|window, cx| window.simulate_next_frame(cx));
    o.until("W held in the core", |o| o.core.held_keys() == 1);
    o
}

/// The keys released in the core: the key's up sends on the next paint,
/// every other path at once.
fn released(o: &mut Open, path: &str, paint: bool) {
    if paint {
        o.vcx.update(|window, cx| window.simulate_next_frame(cx));
    }
    o.until(path, |o| o.core.held_keys() == 0);
    let last = log_lines(&o.shell.log)
        .into_iter()
        .rev()
        .find(|l| l["action"] == "player input")
        .expect("a player input line");
    assert_eq!(last["keys"], 0, "{path}: {last}");
}

#[gpui::test]
fn the_key_up_sends_the_keys_released(cx: &mut gpui::TestAppContext) {
    let mut o = holding_w(cx, "pane-release-up");
    o.vcx.simulate_event(gpui::KeyUpEvent {
        keystroke: gpui::Keystroke::parse("w").expect("w"),
    });
    released(&mut o, "the key's up", true);
    o.pane.stop();
}

#[gpui::test]
fn blur_sends_the_keys_released(cx: &mut gpui::TestAppContext) {
    let mut o = holding_w(cx, "pane-release-blur");
    // Tab moves focus on from the pane.
    o.vcx.simulate_keystrokes("tab");
    o.settle();
    assert_ne!(
        focused(o.vcx),
        (Role::Canvas, Some(live::PANE_NAME.to_string()))
    );
    released(&mut o, "blur", false);
    o.pane.stop();
}

#[gpui::test]
fn the_window_deactivated_sends_the_keys_released(cx: &mut gpui::TestAppContext) {
    let mut o = holding_w(cx, "pane-release-deactivate");
    o.vcx.deactivate_window();
    released(&mut o, "the window deactivated", false);
    o.pane.stop();
}

#[gpui::test]
fn the_pointer_freed_sends_the_keys_released(cx: &mut gpui::TestAppContext) {
    let mut o = holding_w(cx, "pane-release-lock");
    let at = o.centre();
    o.vcx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    o.vcx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    o.settle();
    assert!(o.vcx.update(|window, _| window.pointer_locked()));
    assert_eq!(o.core.held_keys(), 1, "the click keeps W held");
    o.vcx.simulate_keystrokes("escape");
    o.settle();
    assert!(!o.vcx.update(|window, _| window.pointer_locked()));
    released(&mut o, "the pointer freed", false);
    o.pane.stop();
}

#[gpui::test]
fn the_window_hidden_sends_the_keys_released(cx: &mut gpui::TestAppContext) {
    let mut o = holding_w(cx, "pane-release-hidden");
    o.vcx
        .simulate_visibility_change(gpui::WindowVisibility::Hidden);
    released(&mut o, "the window hidden", false);
    o.pane.stop();
}

#[gpui::test]
fn leaving_player_mode_sends_the_keys_released(cx: &mut gpui::TestAppContext) {
    let mut o = holding_w(cx, "pane-release-leave");
    o.press(Role::Button, live::PERSPECTIVE);
    released(&mut o, "leaving player mode", false);
    o.until("left", |o| {
        o.shell.entity.update(o.vcx, |s, cx| {
            s.world().read(cx).state().map(|s| s.player) == Some(mcbench_ui::PlayerState::None)
        })
    });
    o.pane.stop();
}

#[gpui::test]
fn a_modal_layer_opening_sends_the_keys_released(cx: &mut gpui::TestAppContext) {
    let mut o = holding_w(cx, "pane-release-layer");
    o.vcx.update(|_, cx| {
        mcbench_ui::view::keys::open_layer(
            cx,
            mcbench_ui::state::layers::Layer::Settings,
            |_, _| {},
        )
    });
    released(&mut o, "a modal layer opening", false);
    o.pane.stop();
}

// narrow agent: the pane in player mode and filling the window while the
// window crosses 980 px both ways.
#[gpui::test]
fn player_mode_filling_the_window_holds_across_980(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-narrow-player",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::FILL);
    o.press(Role::Button, live::PLAYER);
    o.until("standing", |o| {
        o.shell.entity.update(o.vcx, |s, cx| {
            s.world().read(cx).state().map(|s| s.player) == Some(mcbench_ui::PlayerState::Standing)
        })
    });
    for width in [800., 1280., 640., 979.] {
        o.vcx
            .simulate_resize(gpui::size(gpui::px(width), gpui::px(820.)));
        o.settle();
        let (full, player) = o.shell.entity.update(o.vcx, |s, cx| {
            let w = s.world().read(cx);
            (w.full(), w.player_look().is_some())
        });
        assert!(full && player, "{width}: full {full}, player {player}");
        assert!(o.has(Role::Button, live::BACK), "{width}");
    }
    // Escape leaves the filled window; player mode stays, the world back in
    // its row of the narrow window.
    o.vcx.simulate_keystrokes("escape");
    o.settle();
    assert!(o.has(Role::Button, live::FILL));
    o.pane.stop();
}

/// Alt+S (the `select` bind) turns the selection tool on and off in the
/// shown chat's viewer, as the page's `setSelectToggle` does.
#[gpui::test]
fn the_select_bind_turns_the_selection_tool_on_and_off(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-select-bind",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    let select = |o: &mut Open| -> Vec<serde_json::Value> {
        log_lines(&o.shell.log)
            .into_iter()
            .filter(|l| l["action"] == "select")
            .map(|l| l["on"].clone())
            .collect()
    };
    o.vcx.simulate_keystrokes("alt-s");
    o.settle();
    assert_eq!(select(&mut o), [true]);
    o.vcx.simulate_keystrokes("alt-s");
    o.settle();
    assert_eq!(select(&mut o), [true, false]);
    o.pane.stop();
}

/// The frame fills the pane's box at every width: the size told to the
/// pane is the box's own, with no border taking room (the page's focus
/// ring is an outline), wide, at 979, 800 and 640 px and wide again.
#[gpui::test]
fn the_frame_fills_the_pane_box_at_every_width(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-fills-box",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    for width in [1280., 979., 800., 640., 1280.] {
        o.vcx
            .simulate_resize(gpui::size(gpui::px(width), gpui::px(820.)));
        o.settle();
        let tree = tree(o.vcx);
        let n = nodes(&tree);
        let b = named(&n, Role::Canvas, live::PANE_NAME)
            .bounding_box()
            .expect("the pane's bounds");
        let told = o.pane.size_now();
        assert_eq!(
            (told.width, told.height),
            (b.width().round() as u32, b.height().round() as u32),
            "{width}: the size told against the box"
        );
    }
    o.pane.stop();
}

fn standing(o: &mut Open) {
    o.press(Role::Button, live::PLAYER);
    o.until("standing", |o| {
        o.shell.entity.update(o.vcx, |s, cx| {
            s.world().read(cx).state().map(|s| s.player) == Some(mcbench_ui::PlayerState::Standing)
        })
    });
}

/// The world lets the player go (a fresh placement that failed): player
/// mode ends with the page's note (`web/bluemap-bridge.js` 203).
#[gpui::test]
fn the_player_let_go_ends_player_mode_with_its_note(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-player-gone",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    standing(&mut o);
    o.core.set_live_failures(LiveFailures {
        place: true,
        ..LiveFailures::default()
    });
    o.press(Role::Button, live::RESTART);
    o.until("the note", |o| o.has(Role::Status, live::PLAYER_GONE));
    let player = o
        .shell
        .entity
        .update(o.vcx, |s, cx| s.world().read(cx).player_look());
    assert!(player.is_none(), "player mode ended");
    o.pane.stop();
}

/// Player mode asked of a world that is not running says so
/// (`Player mode needs a running world.`), and the note goes after
/// 5,000 ms.
#[gpui::test]
fn player_mode_needs_a_running_world(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-player-needs",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.core.set_live_failures(LiveFailures {
        place: true,
        ..LiveFailures::default()
    });
    o.press(Role::Button, live::RESTART);
    o.settle();
    o.press(Role::Button, live::PLAYER);
    assert!(o.has(Role::Status, live::NEEDS_RUNNING));
    o.vcx
        .executor()
        .advance_clock(Duration::from_millis(live::NOTE_MS - 100));
    o.settle();
    assert!(o.has(Role::Status, live::NEEDS_RUNNING), "still shown");
    o.vcx.executor().advance_clock(Duration::from_millis(200));
    o.settle();
    assert!(!o.has(Role::Status, live::NEEDS_RUNNING), "gone after 5 s");
    o.pane.stop();
}

/// In spectator, Shift goes down as long as it is held (BlueMap's free
/// flight: `ShiftLeft` with PageDown).
#[gpui::test]
fn spectators_shift_goes_down(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-spectator-shift",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::SPECTATOR);
    o.focus_pane();
    o.vcx.simulate_event(gpui::ModifiersChangedEvent {
        modifiers: Modifiers::shift(),
        capslock: Default::default(),
    });
    o.until("down held", |o| o.pane.held() == camera_keys::DOWN);
    o.vcx.simulate_event(gpui::ModifiersChangedEvent {
        modifiers: Modifiers::none(),
        capslock: Default::default(),
    });
    o.until("released", |o| o.pane.held() == 0);
    o.pane.stop();
}

/// The camera rows (BlueMap's bar and zoom buttons): Flat, Spectator and
/// Perspective switch the camera; the zoom buttons move it in and out;
/// Reset frames the build again; a double click on the world fills the
/// window and a second leaves it.
#[gpui::test]
fn the_camera_rows_switch_zoom_reset_and_a_double_click_fills(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-camera-rows",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    for (label, mode) in [
        (live::FLAT, mcbench_ui::CameraMode::Flat),
        (live::SPECTATOR, mcbench_ui::CameraMode::Spectator),
        (live::PERSPECTIVE, mcbench_ui::CameraMode::Perspective),
    ] {
        o.press(Role::Button, label);
        o.until(label, |o| o.pane.camera_now().mode == mode);
    }
    o.press(Role::Button, live::FILL);
    let far = o.pane.camera_now().distance;
    o.press(Role::Button, live::ZOOM_IN);
    o.until("closer", |o| o.pane.camera_now().distance < far);
    let near = o.pane.camera_now().distance;
    o.press(Role::Button, live::ZOOM_OUT);
    o.until("farther", |o| o.pane.camera_now().distance > near);
    o.press(Role::Button, live::RESET);
    o.until("reset", |o| {
        (o.pane.camera_now().rotation - 0.6).abs() < 1e-9
    });
    let full = |o: &mut Open| {
        o.shell
            .entity
            .update(o.vcx, |s, cx| s.world().read(cx).full())
    };
    assert!(full(&mut o));
    let at = o.centre();
    o.vcx.simulate_click(at, Modifiers::none());
    o.vcx.simulate_event(gpui::MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    o.vcx.simulate_event(gpui::MouseUpEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::none(),
        click_count: 2,
    });
    o.settle();
    assert!(!full(&mut o), "a double click leaves the filled window");
    o.pane.stop();
}

/// The world exported as a zip where the saver says, and announced.
#[gpui::test]
fn a_world_is_exported_as_a_zip_and_announced(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-export-zip",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    o.press(Role::Button, live::EXPORT);
    o.until("saved", |o| {
        o.polite()
            .is_some_and(|t| t.starts_with("Saved ") && t.ends_with(".zip"))
    });
    let files: Vec<_> = std::fs::read_dir(&o.saved)
        .expect("the save folder")
        .map(|e| e.expect("an entry").file_name())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    o.pane.stop();
}

/// In player mode too a double click fills the window
/// (`web/bluemap-bridge.js` 951-955: any mode).
#[gpui::test]
fn a_double_click_in_player_mode_fills_the_window(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-player-double",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    standing(&mut o);
    let at = o.centre();
    o.vcx.simulate_event(gpui::MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    o.settle();
    assert!(
        o.shell
            .entity
            .update(o.vcx, |s, cx| s.world().read(cx).full())
    );
    o.pane.stop();
}

/// Beside the chat at the default split the pane is narrower than
/// BlueMap's 575.98 px, where BlueMap moves Day/Night, the view switch and
/// Reset into its menu, which is not carried: they stay on a second row of
/// the bar, each reached by Tab from the pane.
#[gpui::test]
fn a_narrow_pane_keeps_every_world_control_within_tab_reach(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-narrow-bar",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.vcx
        .simulate_resize(gpui::size(gpui::px(1280.), gpui::px(820.)));
    o.painted();
    let width = {
        let tree = tree(o.vcx);
        let n = nodes(&tree);
        let b = named(&n, Role::Canvas, live::PANE_NAME)
            .bounding_box()
            .expect("the pane's bounds");
        let scale = f64::from(o.vcx.update(|window, _| window.scale_factor()));
        b.width() / scale
    };
    assert!(
        width < 575.98,
        "the pane is narrow beside the chat: {width}"
    );
    let wanted = [
        live::DAY_NIGHT,
        live::PERSPECTIVE,
        live::FLAT,
        live::PLAYER,
        live::SPECTATOR,
        live::RESET,
    ];
    for name in wanted {
        assert!(o.has(Role::Button, name), "{name} on the bar");
    }
    o.focus_pane();
    let mut reached = Vec::new();
    for _ in 0..40 {
        o.vcx.simulate_keystrokes("tab");
        o.settle();
        if let (Role::Button, Some(name)) = focused(o.vcx) {
            reached.push(name);
        }
    }
    for name in wanted {
        assert!(
            reached.iter().any(|r| r == name),
            "{name} by Tab: {reached:?}"
        );
    }
    o.pane.stop();
}

/// A fly-to in player mode moves the player (`teleportTo`): a flying
/// respawn where the bridge puts it; outside player mode the camera flies.
#[gpui::test]
fn a_fly_to_in_player_mode_stands_the_player_before_the_box(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-teleport",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    let teleports = |o: &mut Open| {
        log_lines(&o.shell.log)
            .into_iter()
            .filter(|l| l["action"] == "teleport")
            .count()
    };
    o.shell
        .entity
        .update(o.vcx, |s, cx| s.world().update(cx, |p, _| p.focus_round()));
    o.settle();
    assert_eq!(
        teleports(&mut o),
        0,
        "no player: the camera flies, no teleport"
    );
    standing(&mut o);
    o.shell
        .entity
        .update(o.vcx, |s, cx| s.world().update(cx, |p, _| p.focus_round()));
    o.settle();
    let line = log_lines(&o.shell.log)
        .into_iter()
        .rfind(|l| l["action"] == "teleport")
        .expect("a teleport line");
    let at: Vec<f64> = serde_json::from_value(line["at"].clone()).expect("at");
    o.until("stood there", |o| {
        o.shell.entity.update(o.vcx, |s, cx| {
            s.world().read(cx).state().map(|s| s.player) == Some(mcbench_ui::PlayerState::Standing)
        })
    });
    o.until("the pane's eye at the feet plus the eye height", |o| {
        let eye = o.pane.camera_now().eye;
        (eye[0] - at[0]).abs() < 1e-6 && (eye[2] - at[2]).abs() < 1e-6 && eye[1] > at[1]
    });
    o.pane.stop();
}

/// The block's label (BlueMap's popup) stands at the block's top face and
/// closes on a key on the world: its box at once, the label after its
/// 300 ms fade.
#[gpui::test]
fn the_block_label_stands_at_the_block_and_closes_on_a_key(cx: &mut gpui::TestAppContext) {
    let mut o = open(
        cx,
        "pane-popup",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    let at = o.centre();
    o.vcx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    o.vcx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    o.until("marked", |o| o.pane.overlays_now().marked.is_some());
    let block = o.pane.overlays_now().marked.expect("marked");
    let top = [
        block[0] as f64 + 0.5,
        block[1] as f64 + 1.0,
        block[2] as f64 + 0.5,
    ];
    assert!(
        o.pane.overlays_now().anchors.contains(&top),
        "the label's anchor at the block's top face"
    );
    o.until("the label", |o| {
        let tree = tree(o.vcx);
        nodes(&tree)
            .iter()
            .any(|n| n.label().is_some_and(|l| l.starts_with("Block: x")))
    });
    o.focus_pane();
    o.vcx.simulate_keystrokes("left");
    o.settle();
    assert!(
        o.pane.overlays_now().marked.is_none(),
        "the box goes at once"
    );
    o.vcx
        .executor()
        .advance_clock(Duration::from_millis(live::POPUP_FADE_MS + 50));
    o.settle();
    let tree = tree(o.vcx);
    assert!(
        !nodes(&tree)
            .iter()
            .any(|n| n.label().is_some_and(|l| l.starts_with("Block: x"))),
        "the label gone after its fade"
    );
    o.pane.stop();
}

/// A letter typed on the world with the block's label open closes the
/// label (its box at once) and goes into the composer (type-ahead).
#[gpui::test]
fn a_letter_on_the_world_closes_the_block_label_and_reaches_the_composer(
    cx: &mut gpui::TestAppContext,
) {
    let mut o = open(
        cx,
        "pane-popup-type",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    let at = o.centre();
    o.vcx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    o.vcx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    o.until("marked", |o| o.pane.overlays_now().marked.is_some());
    o.focus_pane();
    o.vcx.simulate_keystrokes("x");
    o.settle();
    assert!(
        o.pane.overlays_now().marked.is_none(),
        "the box goes at once"
    );
    let focus = focused(o.vcx);
    assert_eq!(
        focus.0,
        Role::MultilineTextInput,
        "the composer took the key: {focus:?}"
    );
    o.pane.stop();
}

// -- locked look ---------------------------------------------------------------
//
// Spectator under the pointer lock (inventory style-viewer-routes.md 2.12,
// FreeFlightControls.js 146-165): a click on the world takes the pointer,
// its motion turns the view as `CameraInput::LockedLook` in physical pixels.

fn pointer_locked(o: &mut Open) -> bool {
    o.vcx.update(|window, _| window.pointer_locked())
}

fn click_world(o: &mut Open) {
    let at = o.centre();
    o.vcx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    o.vcx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    o.settle();
}

fn locked_motion(o: &mut Open, dx: f32, dy: f32) {
    o.vcx.simulate_event(gpui::MouseMotionEvent {
        delta: point(px(dx), px(dy)),
        modifiers: Modifiers::none(),
    });
    o.settle();
}

fn into_mode(o: &mut Open, label: &str, mode: mcbench_ui::CameraMode) {
    o.press(Role::Button, label);
    o.until(label, |o| o.pane.camera_now().mode == mode);
}

#[gpui::test]
fn locked_look_a_spectator_click_takes_the_pointer_and_its_motion_turns_the_view(
    cx: &mut gpui::TestAppContext,
) {
    use mcbench_ui::CameraMode;
    let mut o = open(
        cx,
        "pane-locked-look",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    into_mode(&mut o, live::SPECTATOR, CameraMode::Spectator);
    click_world(&mut o);
    assert!(
        pointer_locked(&mut o),
        "a click in spectator takes the pointer"
    );
    // The motion reaches the pane in physical pixels: the stand-in turns
    // 0.005 a pixel.
    let scale = o.vcx.update(|window, _| window.scale_factor()) as f64;
    let before = o.pane.camera_now();
    locked_motion(&mut o, 10., 4.);
    o.until("the view turned", |o| {
        let now = o.pane.camera_now();
        (now.rotation - before.rotation - 10. * scale * 0.005).abs() < 1e-6
            && (now.angle - before.angle - 4. * scale * 0.005).abs() < 1e-6
    });
    // Escape gives the pointer back; motion then turns nothing.
    o.focus_pane();
    o.vcx.simulate_keystrokes("escape");
    o.settle();
    assert!(!pointer_locked(&mut o), "Escape gives the pointer back");
    let held = o.pane.camera_now().rotation;
    locked_motion(&mut o, 10., 0.);
    assert_eq!(
        o.pane.camera_now().rotation,
        held,
        "free motion turns nothing"
    );
    // Leaving spectator gives it back.
    click_world(&mut o);
    assert!(pointer_locked(&mut o));
    into_mode(&mut o, live::PERSPECTIVE, CameraMode::Perspective);
    assert!(
        !pointer_locked(&mut o),
        "leaving spectator gives the pointer back"
    );
    // The window losing focus gives it back.
    into_mode(&mut o, live::SPECTATOR, CameraMode::Spectator);
    click_world(&mut o);
    assert!(pointer_locked(&mut o));
    o.vcx.deactivate_window();
    o.settle();
    assert!(!pointer_locked(&mut o), "a blur gives the pointer back");
    o.pane.stop();
}

#[gpui::test]
fn locked_look_a_click_in_perspective_or_flat_takes_no_pointer(cx: &mut gpui::TestAppContext) {
    use mcbench_ui::CameraMode;
    let mut o = open(
        cx,
        "pane-locked-look-map",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.painted();
    for (label, mode) in [
        (live::PERSPECTIVE, CameraMode::Perspective),
        (live::FLAT, CameraMode::Flat),
    ] {
        into_mode(&mut o, label, mode);
        click_world(&mut o);
        assert!(!pointer_locked(&mut o), "{label}: no lock");
        let before = o.pane.camera_now();
        locked_motion(&mut o, 30., 12.);
        let now = o.pane.camera_now();
        assert_eq!(
            (now.rotation, now.angle),
            (before.rotation, before.angle),
            "{label}: motion turns nothing"
        );
    }
    o.pane.stop();
}

#[gpui::test]
fn locked_look_a_refused_lock_shows_the_note(cx: &mut gpui::TestAppContext) {
    use mcbench_ui::CameraMode;
    let mut o = open(
        cx,
        "pane-locked-look-refused",
        Behaviour::default(),
        LiveFailures::default(),
    );
    o.vcx.update(|_, cx| {
        mcbench_ui::view::world::set_locker(
            cx,
            mcbench_ui::view::world::refusing_locker("no pointer here"),
        )
    });
    o.painted();
    into_mode(&mut o, live::SPECTATOR, CameraMode::Spectator);
    click_world(&mut o);
    assert!(!pointer_locked(&mut o), "the pointer stays free");
    assert!(o.has(
        Role::Status,
        "The pointer could not be taken: no pointer here"
    ));
    o.pane.stop();
}
