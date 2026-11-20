//! The Share panel (share agent, `ui-seam-share.md`): the stand-in keeps
//! the endings of `ReadShare`, `Publish` and `Unshare` as agreed with the
//! core's owner.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Inbox, TempDir};
use common::{ShellUnderTest, act, focused, has, named, nodes, of_role, pump, tree};
use mcbench_ui::dev::fixtures;
use mcbench_ui::dev::standin::{self, SharePlay, StandIn};
use mcbench_ui::{Command, Core, PublishStep, ShareRead, Subject, Update};

fn gate(dir: &TempDir) -> (StandIn, Arc<Inbox>) {
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let core = StandIn::new(
        fixtures::gate(&texts),
        texts.streamed.clone(),
        Duration::ZERO,
        None,
    );
    let inbox = Arc::new(Inbox::default());
    core.attach(inbox.clone());
    take(&inbox);
    (core, inbox)
}

fn take(inbox: &Inbox) -> Vec<Update> {
    std::mem::take(&mut *inbox.0.lock().expect("lock"))
}

fn chat(id: &str) -> Subject {
    Subject::Chat(id.into())
}

fn steps(updates: &[Update]) -> Vec<PublishStep> {
    updates
        .iter()
        .map(|u| match u {
            Update::PublishStep { step, .. } => step.clone(),
            other => panic!("not a step: {other:?}"),
        })
        .collect()
}

/// Updates are not `PartialEq`: compared by what they say.
fn same(got: Vec<Update>, want: &[Update]) {
    assert_eq!(format!("{got:?}"), format!("{want:?}"));
}

const REPO: &str = "mcbench-builds/gallery";

#[test]
fn the_stand_in_keeps_the_share_commands_endings() {
    let dir = TempDir::new("share-endings");
    let (core, inbox) = gate(&dir);
    // A read: exactly one ShareRead, with the repository and the url.
    core.send(Command::ReadShare {
        subject: chat("chat-lamp"),
    });
    let lamp = "https://mcbench-builds.github.io/gallery/b/5f2a9c01b7de/".to_string();
    same(
        take(&inbox),
        &[Update::ShareRead {
            subject: chat("chat-lamp"),
            read: Ok(ShareRead {
                repo: Some(REPO.into()),
                url: Some(lamp.clone()),
            }),
        }],
    );
    // A publish: Exporting at once, one step a tick, ending in Live.
    core.send(Command::Publish {
        subject: chat("chat-gate"),
    });
    assert_eq!(steps(&take(&inbox)), [PublishStep::Exporting]);
    core.send(Command::Publish {
        subject: chat("chat-gate"),
    });
    assert_eq!(
        steps(&take(&inbox)),
        [PublishStep::Failed {
            error: standin::PUBLISHING.into()
        }],
        "a second publish is refused alone"
    );
    core.share_tick();
    core.share_tick();
    assert_eq!(
        steps(&take(&inbox)),
        [PublishStep::Publishing, PublishStep::Waiting]
    );
    core.share_tick();
    let live = steps(&take(&inbox));
    let [PublishStep::Live { url }] = &live[..] else {
        panic!("one Live: {live:?}");
    };
    // Settings' list has it.
    core.send(Command::ReadShares);
    let [Update::Shares(Ok(published))] = &take(&inbox)[..] else {
        panic!("the list");
    };
    let made = published
        .builds
        .iter()
        .find(|b| b.conversation_id.as_deref() == Some("chat-gate"))
        .expect("the new build is listed");
    assert_eq!(&made.url, url);
    // An unshare ends in ShareChanged with no url.
    core.send(Command::Unshare {
        subject: chat("chat-gate"),
    });
    same(
        take(&inbox),
        &[Update::ShareChanged {
            subject: chat("chat-gate"),
            url: None,
        }],
    );
    // One that fails: the error, then the url it still has.
    core.share_play(SharePlay::UnshareFails);
    core.send(Command::Unshare {
        subject: chat("chat-lamp"),
    });
    same(
        take(&inbox),
        &[
            Update::Error(standin::SITE_FAILED.into()),
            Update::ShareChanged {
                subject: chat("chat-lamp"),
                url: Some(lamp.clone()),
            },
        ],
    );
    // Settings' removal of a build of a chat that exists: ShareChanged
    // between ShareRemoved and Shares.
    core.send(Command::RemoveShare {
        share_id: "5f2a9c01b7de".into(),
    });
    let updates = take(&inbox);
    assert!(
        matches!(
            &updates[..],
            [Update::ShareRemoved { result: Ok(()), .. }, Update::ShareChanged { subject, url: None }, Update::Shares(Ok(_))]
                if *subject == chat("chat-lamp")
        ),
        "{updates:?}"
    );
}

#[test]
fn every_failure_ends_a_publish_in_one_failed() {
    let dir = TempDir::new("share-failures");
    // The play, the ticks to its end, the steps before its `Failed`.
    for (play, at, made, error) in [
        (
            SharePlay::FailsExporting,
            0,
            1,
            standin::SHARE_EXPORT_FAILED,
        ),
        (SharePlay::FailsPublishing, 2, 2, standin::PUSH_FAILED),
        (SharePlay::FailsWaiting, 3, 3, standin::PAGES_LATE),
    ] {
        let (core, inbox) = gate(&dir);
        core.share_play(play);
        core.send(Command::Publish {
            subject: chat("chat-gate"),
        });
        for _ in 0..at {
            core.share_tick();
        }
        let got = steps(&take(&inbox));
        let (last, before) = got.split_last().expect("steps");
        assert_eq!(before.len(), made, "{play:?}: {got:?}");
        assert!(
            matches!(last, PublishStep::Failed { error: e } if e.starts_with(error)),
            "{play:?}: {got:?}"
        );
        core.share_tick();
        assert!(take(&inbox).is_empty(), "{play:?}: nothing after its end");
    }
    // The real core's answers until sharing is ported.
    let (core, inbox) = gate(&dir);
    core.share_play(SharePlay::NotJoined);
    core.send(Command::ReadShare {
        subject: chat("chat-lamp"),
    });
    core.send(Command::Publish {
        subject: chat("chat-lamp"),
    });
    core.send(Command::Unshare {
        subject: chat("chat-lamp"),
    });
    same(
        take(&inbox),
        &[
            Update::ShareRead {
                subject: chat("chat-lamp"),
                read: Ok(ShareRead {
                    repo: Some(REPO.into()),
                    url: None,
                }),
            },
            Update::PublishStep {
                subject: chat("chat-lamp"),
                step: PublishStep::Failed {
                    error: standin::SHARING_NOT_JOINED.into(),
                },
            },
            Update::Error(standin::SHARING_NOT_JOINED.into()),
            Update::ShareChanged {
                subject: chat("chat-lamp"),
                url: None,
            },
        ],
    );
}

// The panel in process, over the stand-in and the fake pane.

use gpui::VisualTestContext;
use gpui::accesskit::{Action, Role};
use mcbench_ui::dev::pane::{Behaviour, FakePane};
use mcbench_ui::dev::standin::LiveFailures;
use mcbench_ui::state::place::Place;
use mcbench_ui::view::share as panel;

struct Open<'a> {
    shell: ShellUnderTest,
    core: Arc<StandIn>,
    pane: Arc<FakePane>,
    vcx: &'a mut VisualTestContext,
    opened: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

fn open_at<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    start: Place,
    plays: &[SharePlay],
) -> Open<'a> {
    use mcbench_ui::view::shell::{Shell, ShellOptions};
    common::quiet_crashes();
    let dir = TempDir::new(test);
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let core = Arc::new(StandIn::new(
        fixtures::gate(&texts),
        texts.streamed.clone(),
        Duration::ZERO,
        None,
    ));
    for play in plays {
        core.share_play(*play);
    }
    let pane = FakePane::manual(Behaviour::default());
    core.set_pane(pane.clone(), LiveFailures::default());
    let dyn_core: Arc<dyn Core> = core.clone();
    let log_path = dir.0.join("actions.jsonl");
    let log = mcbench_ui::ActionLog::create(&log_path);
    cx.update(mcbench_ui::view::setup);
    let inbox = Arc::new(Inbox::default());
    let for_shell = dyn_core.clone();
    let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = opened.clone();
    let (entity, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Shell::new(
            ShellOptions {
                core: for_shell,
                log: Some(log),
                opener: std::rc::Rc::new(move |url, _| seen.borrow_mut().push(url.to_string())),
                metrics: std::rc::Rc::new(std::cell::RefCell::new(
                    mcbench_ui::view::metrics::Metrics::new(std::time::Instant::now()),
                )),
                start: Some(start),
                preferences: mcbench_ui::Preferences {
                    last_model: Some(("anthropic".into(), standin::MODEL.into())),
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
    vcx.update(|window, _| window.activate_window());
    let shell = ShellUnderTest::new(entity, inbox, log_path, dir);
    pump(&shell, vcx);
    let mut o = Open {
        shell,
        core,
        pane,
        vcx,
        opened,
    };
    o.until("the Share button", |o| o.has(Role::Button, panel::SHARE));
    o
}

impl Open<'_> {
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            self.pane.pump();
            pump(&self.shell, self.vcx);
            if done(self) {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "not within 5 s: {what}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

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

    fn press(&mut self, role: Role, label: &str) {
        let tree = tree(self.vcx);
        let node = named(&nodes(&tree), role, label);
        act(self.vcx, node, Action::Click);
        self.settle();
    }

    fn tick(&mut self) {
        self.core.share_tick();
        self.settle();
    }

    fn announced(&mut self, live: gpui::accesskit::Live) -> Option<String> {
        let tree = tree(self.vcx);
        of_role(&nodes(&tree), Role::Status)
            .into_iter()
            .chain(of_role(&nodes(&tree), Role::Alert))
            .find(|n| n.live() == live)
            .and_then(|n| n.label())
    }
}

#[gpui::test]
fn a_chat_publishes_step_by_step_to_its_link_copy_and_post(cx: &mut gpui::TestAppContext) {
    let mut o = open_at(
        cx,
        "share-publish",
        Place::Chat {
            id: "chat-gate".into(),
        },
        &[],
    );
    // Enter on the button opens the panel, focus inside it.
    let tree0 = tree(o.vcx);
    let button = named(&nodes(&tree0), Role::Button, panel::SHARE);
    assert_eq!(button.data().is_expanded(), Some(false));
    act(o.vcx, button, Action::Focus);
    o.vcx.simulate_keystrokes("enter");
    o.settle();
    assert!(o.has(Role::Dialog, "Share this build"));
    let offer = panel::offer(false, "mcbench-builds/gallery");
    assert!(o.has(Role::Label, &offer), "{offer}");
    assert_eq!(focused(o.vcx), (Role::Button, Some("Publish".into())));
    o.vcx.simulate_keystrokes("enter");
    o.settle();
    assert!(o.has(Role::Status, "Preparing the build…"));
    assert_eq!(
        o.announced(gpui::accesskit::Live::Polite).as_deref(),
        Some("Preparing the build…")
    );
    o.tick();
    assert!(o.has(Role::Status, "Uploading…"));
    o.tick();
    assert!(o.has(Role::Status, "Waiting for GitHub Pages…"));
    o.tick();
    // Published: the link, Copy link, Post on X, Publish again, Remove.
    let tree1 = tree(o.vcx);
    let n = nodes(&tree1);
    let link = n
        .iter()
        .find(|n| n.label().as_deref() == Some("Shared page link"))
        .expect("the link field");
    let url = link.value().expect("the link's address").to_string();
    assert!(
        url.starts_with("https://mcbench-builds.github.io/gallery/b/"),
        "{url}"
    );
    assert!(has(
        &n,
        Role::Label,
        "The page shows the build as it was when published."
    ));
    assert!(has(&n, Role::Button, "Publish again"));
    assert!(has(&n, Role::Button, "Remove"));
    o.press(Role::Button, "Copy link");
    assert!(o.has(Role::Button, "Copied"));
    o.press(Role::Button, "Post on X");
    assert_eq!(
        o.opened.borrow().last().cloned(),
        Some(mcbench_ui::state::share::post_url(&url))
    );
    // Settings' published list shows the build the panel made.
    o.core.send(Command::ReadShares);
    o.settle();
    o.vcx.update(|window, cx| {
        o.shell.entity.update(cx, |s, cx| {
            s.open_settings_on(
                Some(mcbench_ui::state::settings::Pane::General),
                None,
                window,
                cx,
            )
        })
    });
    o.settle();
    let tree2 = tree(o.vcx);
    assert!(
        nodes(&tree2)
            .iter()
            .any(|n| n.label().is_some_and(|l| l.contains("Open")))
            && has(&nodes(&tree2), Role::Button, "Copy link"),
        "the list holds the new build"
    );
    o.pane.stop();
}

#[gpui::test]
fn escape_closes_the_panel_and_gives_focus_back_to_share(cx: &mut gpui::TestAppContext) {
    let mut o = open_at(
        cx,
        "share-escape",
        Place::Chat {
            id: "chat-gate".into(),
        },
        &[],
    );
    o.press(Role::Button, panel::SHARE);
    assert!(o.has(Role::Dialog, "Share this build"));
    let tree0 = tree(o.vcx);
    assert_eq!(
        named(&nodes(&tree0), Role::Button, panel::SHARE)
            .data()
            .is_expanded(),
        Some(true)
    );
    o.vcx.simulate_keystrokes("escape");
    o.settle();
    assert!(!o.has(Role::Dialog, "Share this build"));
    assert_eq!(focused(o.vcx), (Role::Button, Some(panel::SHARE.into())));
    o.pane.stop();
}

#[gpui::test]
fn a_shared_chat_is_removed_after_its_confirmation_and_leaves_settings_list(
    cx: &mut gpui::TestAppContext,
) {
    let mut o = open_at(
        cx,
        "share-remove",
        Place::Chat {
            id: "chat-lamp".into(),
        },
        &[],
    );
    o.press(Role::Button, panel::SHARE);
    assert!(o.has(Role::Button, "Remove"));
    o.press(Role::Button, "Remove");
    assert!(o.has(Role::Button, "Remove the page"), "asks first");
    o.press(Role::Button, "Remove the page");
    let offer = panel::offer(false, "mcbench-builds/gallery");
    assert!(o.has(Role::Label, &offer));
    o.core.send(Command::ReadShares);
    let updates = std::mem::take(&mut *o.shell.inbox.0.lock().expect("lock"));
    let shown = updates.iter().any(|u| {
        matches!(u, Update::Shares(Ok(p)) if p.builds.iter().any(|b| b.conversation_id.as_deref() == Some("chat-lamp")))
    });
    assert!(!shown, "Settings' list lost it");
    o.pane.stop();
}

#[gpui::test]
fn a_removal_in_settings_moves_the_panels_share_to_not_shared(cx: &mut gpui::TestAppContext) {
    let mut o = open_at(
        cx,
        "share-elsewhere",
        Place::Chat {
            id: "chat-lamp".into(),
        },
        &[],
    );
    o.press(Role::Button, panel::SHARE);
    assert!(o.has(Role::Button, "Remove"));
    o.vcx.simulate_keystrokes("escape");
    o.settle();
    o.core.send(Command::RemoveShare {
        share_id: "5f2a9c01b7de".into(),
    });
    o.settle();
    o.press(Role::Button, panel::SHARE);
    assert!(o.has(Role::Button, "Publish"), "not shared any more");
    o.pane.stop();
}

#[gpui::test]
fn each_failure_says_why_with_try_again(cx: &mut gpui::TestAppContext) {
    let mut o = open_at(
        cx,
        "share-fails",
        Place::Chat {
            id: "chat-gate".into(),
        },
        &[SharePlay::FailsPublishing],
    );
    o.press(Role::Button, panel::SHARE);
    o.press(Role::Button, "Publish");
    o.tick();
    o.tick();
    assert!(o.has(Role::Alert, standin::PUSH_FAILED));
    assert_eq!(
        o.announced(gpui::accesskit::Live::Assertive).as_deref(),
        Some(standin::PUSH_FAILED)
    );
    o.press(Role::Button, "Try again");
    assert!(o.has(Role::Status, "Preparing the build…"));
    o.pane.stop();
}

#[gpui::test]
fn until_sharing_joins_a_publish_fails_with_the_cores_sentence(cx: &mut gpui::TestAppContext) {
    let mut o = open_at(
        cx,
        "share-not-joined",
        Place::Chat {
            id: "chat-lamp".into(),
        },
        &[SharePlay::NotJoined],
    );
    o.press(Role::Button, panel::SHARE);
    o.press(Role::Button, "Publish");
    assert!(o.has(Role::Alert, standin::SHARING_NOT_JOINED));
    assert!(o.has(Role::Button, "Try again"));
    o.pane.stop();
}

#[gpui::test]
fn a_blind_bench_says_its_names_are_hidden_and_its_panel_comes_back_with_its_place(
    cx: &mut gpui::TestAppContext,
) {
    let mut o = open_at(
        cx,
        "share-bench",
        Place::Compare {
            bench: "bench-towers".into(),
        },
        &[],
    );
    o.press(Role::Button, panel::SHARE);
    assert!(o.has(Role::Dialog, "Share this bench"));
    assert!(o.has(Role::Label, panel::BLIND_NOTE));
    // The fixture's bench is shared already: its link.
    assert!(o.has(Role::Button, "Publish again"));
    // Another place hides it; coming back shows it again (`shareFor`).
    o.vcx.update(|window, cx| {
        o.shell.entity.update(cx, |s, cx| {
            s.go(
                Place::Chat {
                    id: "chat-gate".into(),
                },
                window,
                cx,
            )
        })
    });
    o.settle();
    assert!(!o.has(Role::Dialog, "Share this bench"));
    o.vcx.update(|window, cx| {
        o.shell.entity.update(cx, |s, cx| {
            s.go(
                Place::Compare {
                    bench: "bench-towers".into(),
                },
                window,
                cx,
            )
        })
    });
    o.settle();
    assert!(o.has(Role::Dialog, "Share this bench"));
    o.pane.stop();
}
