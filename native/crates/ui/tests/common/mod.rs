//! What the in-process accessibility tests share: the AccessKit tree read
//! through `accesskit_consumer` as an adapter reads it, lookups by role and
//! name, an assistive technology's actions, the action log, quiet crashes.
//! A test file uses it with `mod common;`.
#![allow(dead_code)]

use accesskit_consumer::{Node, Tree};
use gpui::VisualTestContext;
use gpui::accesskit::{Action, ActionRequest, Role};

/// The crash module for the test process, so a failure that could abort
/// ends quietly with a crash log instead of raising the macOS crash
/// reporter.
pub fn quiet_crashes() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let logs = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("mcbench-ui-crash-logs");
        mcbench_ui::crash::install(&logs, env!("CARGO_PKG_VERSION"), |_| false);
    });
}

/// A temporary folder removed at the end of the test.
pub struct TempDir(pub std::path::PathBuf);

impl TempDir {
    /// A fresh folder for `test`.
    pub fn new(test: &str) -> Self {
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("mcbench-ui-{}-{test}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temporary folder");
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The window's tree as the last frame built it.
pub fn tree(vcx: &mut VisualTestContext) -> Tree {
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    let update = vcx
        .update(|window, _| window.a11y_tree_update_for_test())
        .expect("a frame built the tree");
    Tree::new(update, true)
}

fn all<'a>(node: Node<'a>, out: &mut Vec<Node<'a>>) {
    out.push(node);
    for child in node.children() {
        all(child, out);
    }
}

/// Every node, in document order.
pub fn nodes(tree: &Tree) -> Vec<Node<'_>> {
    let mut out = Vec::new();
    all(tree.state().root(), &mut out);
    out
}

/// The one node with `role` named `label`.
pub fn named<'a>(nodes: &[Node<'a>], role: Role, label: &str) -> Node<'a> {
    let found: Vec<_> = nodes
        .iter()
        .filter(|n| n.role() == role && n.label().as_deref() == Some(label))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one {role:?} named {label:?}, found {}",
        found.len()
    );
    *found[0]
}

// sidebar agent: a list row by its title.
/// The one list row titled `title` (its name adds its status, its
/// resolution and its snippet after a comma).
pub fn row<'a>(nodes: &[Node<'a>], title: &str) -> Node<'a> {
    let found: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.role() == Role::ListItem
                && n.label()
                    .is_some_and(|l| l == title || l.starts_with(&format!("{title}, ")))
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one row titled {title:?}, found {}",
        found.len()
    );
    *found[0]
}

/// Whether a node with `role` named `label` is in the tree.
pub fn has(nodes: &[Node<'_>], role: Role, label: &str) -> bool {
    nodes
        .iter()
        .any(|n| n.role() == role && n.label().as_deref() == Some(label))
}

/// The nodes with `role`.
pub fn of_role<'a>(nodes: &[Node<'a>], role: Role) -> Vec<Node<'a>> {
    nodes.iter().copied().filter(|n| n.role() == role).collect()
}

/// The focused node's role and name.
pub fn focused(vcx: &mut VisualTestContext) -> (Role, Option<String>) {
    let tree = tree(vcx);
    let focus = tree.state().focus().expect("a focused node");
    (focus.role(), focus.label())
}

/// An assistive technology's `action` on `node`.
pub fn act(vcx: &mut VisualTestContext, node: Node, action: Action) {
    let request = ActionRequest {
        action,
        target_tree: gpui::accesskit::TreeId::ROOT,
        target_node: node.locate().0,
        data: None,
    };
    vcx.update(|window, cx| window.dispatch_a11y_action_for_test(request, cx));
    vcx.run_until_parked();
}

/// An assistive technology's press.
pub fn click(vcx: &mut VisualTestContext, node: Node) {
    act(vcx, node, Action::Click);
}

/// The action log's lines.
pub fn log_lines(path: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .expect("the action log")
        .lines()
        .map(|l| serde_json::from_str(l).expect("a JSON line"))
        .collect()
}

/// The centre of a node's bounds (device pixels, as AccessKit reports
/// them) in the window's logical pixels.
pub fn centre_of(
    vcx: &mut VisualTestContext,
    bounds: gpui::accesskit::Rect,
) -> gpui::Point<gpui::Pixels> {
    let scale = f64::from(vcx.update(|window, _| window.scale_factor()));
    gpui::point(
        gpui::px(((bounds.x0 + bounds.x1) / 2. / scale) as f32),
        gpui::px(((bounds.y0 + bounds.y1) / 2. / scale) as f32),
    )
}

/// A pointer click at the centre of `node`'s bounds.
pub fn pointer_click(vcx: &mut VisualTestContext, node: Node) {
    let bounds = node.bounding_box().expect("bounds");
    let centre = centre_of(vcx, bounds);
    vcx.simulate_click(centre, gpui::Modifiers::none());
    vcx.run_until_parked();
}

/// A window for the core that keeps updates until the test applies them.
#[derive(Default)]
pub struct Inbox(pub std::sync::Mutex<Vec<mcbench_ui::Update>>);

impl mcbench_ui::Window for Inbox {
    fn publish(&self, update: mcbench_ui::Update) {
        self.0.lock().expect("lock").push(update);
    }
}

/// The whole window (the shell) over the stand-in core.
pub struct ShellUnderTest {
    /// The shell.
    pub entity: gpui::Entity<mcbench_ui::view::shell::Shell>,
    /// What the core published and the test has not applied yet.
    pub inbox: std::sync::Arc<Inbox>,
    /// The action log.
    pub log: std::path::PathBuf,
    _dir: TempDir,
}

impl ShellUnderTest {
    /// A shell made by a test of its own (with its own core).
    #[allow(dead_code)]
    pub fn new(
        entity: gpui::Entity<mcbench_ui::view::shell::Shell>,
        inbox: std::sync::Arc<Inbox>,
        log: std::path::PathBuf,
        dir: TempDir,
    ) -> Self {
        ShellUnderTest {
            entity,
            inbox,
            log,
            _dir: dir,
        }
    }
}

/// Which chats the stand-in serves.
pub enum Fixture {
    /// Three chats with every block (`fixtures::gate`).
    Gate,
    /// One chat of 400 messages (`fixtures::long`).
    Long,
}

/// The shell on a fixture, its first list applied, opening `start` (the
/// landing when none).
pub fn open_shell<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    fixture: Fixture,
    start: Option<mcbench_ui::state::place::Place>,
) -> (ShellUnderTest, &'a mut VisualTestContext) {
    use mcbench_ui::dev::{fixtures, standin::StandIn};
    use mcbench_ui::view::shell::{Shell, ShellOptions};
    quiet_crashes();
    let dir = TempDir::new(test);
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let chats = match fixture {
        Fixture::Gate => fixtures::gate(&texts),
        Fixture::Long => fixtures::long(),
    };
    let core: std::sync::Arc<dyn mcbench_ui::Core> = std::sync::Arc::new(StandIn::new(
        chats,
        texts.streamed.clone(),
        std::time::Duration::ZERO,
        None,
    ));
    let log_path = dir.0.join("actions.jsonl");
    let log = mcbench_ui::ActionLog::create(&log_path);
    cx.update(mcbench_ui::view::setup);
    let inbox = std::sync::Arc::new(Inbox::default());
    let core_for_shell = core.clone();
    let (entity, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Shell::new(
            ShellOptions {
                core: core_for_shell,
                log: Some(log),
                opener: std::rc::Rc::new(|_, _| {}),
                metrics: std::rc::Rc::new(std::cell::RefCell::new(
                    mcbench_ui::view::metrics::Metrics::new(std::time::Instant::now()),
                )),
                start,
                // chat: the stand-in's model is the last one used, so a
                // new chat sends with it.
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
    core.attach(inbox.clone());
    let shell = ShellUnderTest {
        entity,
        inbox,
        log: log_path,
        _dir: dir,
    };
    pump(&shell, vcx);
    (shell, vcx)
}

/// Applies every update the core published, until there are none.
pub fn pump(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    loop {
        let updates: Vec<mcbench_ui::Update> =
            std::mem::take(&mut *shell.inbox.0.lock().expect("lock"));
        if updates.is_empty() {
            break;
        }
        for update in updates {
            vcx.update(|window, cx| shell.entity.update(cx, |s, cx| s.apply(update, window, cx)));
        }
        vcx.run_until_parked();
    }
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
}

/// A pointer drag from `from` to `to` (logical pixels), in `steps` moves.
pub fn drag(
    vcx: &mut VisualTestContext,
    from: gpui::Point<gpui::Pixels>,
    to: gpui::Point<gpui::Pixels>,
    steps: usize,
) {
    vcx.simulate_mouse_down(from, gpui::MouseButton::Left, gpui::Modifiers::none());
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let at = gpui::point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        vcx.simulate_mouse_move(at, Some(gpui::MouseButton::Left), gpui::Modifiers::none());
    }
    vcx.simulate_mouse_up(to, gpui::MouseButton::Left, gpui::Modifiers::none());
    vcx.run_until_parked();
}

// sidebar agent: the shell over a stand-in the test can drive.
/// The shell on `fixture`, with the stand-in itself (its list states can
/// be stepped: `StandIn::set_turn`, `fail_archive_reads`).
pub fn open_standin<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    fixture: mcbench_ui::dev::fixtures::Fixture,
    start: Option<mcbench_ui::state::place::Place>,
) -> (
    ShellUnderTest,
    std::sync::Arc<mcbench_ui::dev::standin::StandIn>,
    &'a mut VisualTestContext,
) {
    // chat: the stand-in's model is the last one used, so a new chat sends
    // with it.
    let preferences = mcbench_ui::Preferences {
        last_model: Some(("anthropic".into(), mcbench_ui::dev::standin::MODEL.into())),
        ..mcbench_ui::Preferences::default()
    };
    open_standin_with(cx, test, fixture, start, preferences)
}

/// [`open_standin`] with the window's stored preferences.
pub fn open_standin_with<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    fixture: mcbench_ui::dev::fixtures::Fixture,
    start: Option<mcbench_ui::state::place::Place>,
    preferences: mcbench_ui::Preferences,
) -> (
    ShellUnderTest,
    std::sync::Arc<mcbench_ui::dev::standin::StandIn>,
    &'a mut VisualTestContext,
) {
    open_standin_selecting(cx, test, fixture, start, preferences, Vec::new())
}

// par-shell: the shell started with the viewer's selections in its draft
// (the dev binary's `--select`).
/// [`open_standin_with`] with `selections` in the first draft.
pub fn open_standin_selecting<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    fixture: mcbench_ui::dev::fixtures::Fixture,
    start: Option<mcbench_ui::state::place::Place>,
    preferences: mcbench_ui::Preferences,
    selections: Vec<mcbench_ui::Selection>,
) -> (
    ShellUnderTest,
    std::sync::Arc<mcbench_ui::dev::standin::StandIn>,
    &'a mut VisualTestContext,
) {
    use mcbench_ui::dev::{fixtures, standin::StandIn};
    use mcbench_ui::view::shell::{Shell, ShellOptions};
    quiet_crashes();
    let dir = TempDir::new(test);
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let standin = std::sync::Arc::new(StandIn::new(
        fixture,
        texts.streamed.clone(),
        std::time::Duration::ZERO,
        None,
    ));
    let core: std::sync::Arc<dyn mcbench_ui::Core> = standin.clone();
    let log_path = dir.0.join("actions.jsonl");
    let log = mcbench_ui::ActionLog::create(&log_path);
    cx.update(mcbench_ui::view::setup);
    let inbox = std::sync::Arc::new(Inbox::default());
    let core_for_shell = core.clone();
    let (entity, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Shell::new(
            ShellOptions {
                core: core_for_shell,
                log: Some(log),
                opener: std::rc::Rc::new(|_, _| {}),
                metrics: std::rc::Rc::new(std::cell::RefCell::new(
                    mcbench_ui::view::metrics::Metrics::new(std::time::Instant::now()),
                )),
                start,
                preferences,
                picker: mcbench_ui::view::fixed_picker(Vec::new()),
                selections,
            },
            window,
            cx,
        )
    });
    core.attach(inbox.clone());
    let shell = ShellUnderTest {
        entity,
        inbox,
        log: log_path,
        _dir: dir,
    };
    pump(&shell, vcx);
    (shell, standin, vcx)
}

/// The gate fixture's texts, for [`open_standin`].
pub fn gate_fixture() -> mcbench_ui::dev::fixtures::Fixture {
    let texts = mcbench_ui::dev::fixtures::Texts::new(std::path::Path::new("lamp.png"));
    mcbench_ui::dev::fixtures::gate(&texts)
}
// chat (stage four's chat agent)
/// The shell on the gate fixture as [`open_shell`], with the stand-in set up
/// by `configure` (its script, files kept under the test's folder, sends it
/// refuses, turns it runs on a join) and `picks` for `Attach file`.
pub fn open_shell_with<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    start: Option<mcbench_ui::state::place::Place>,
    picks: Vec<std::path::PathBuf>,
    configure: impl FnOnce(&mcbench_ui::dev::standin::StandIn, &std::path::Path),
) -> (ShellUnderTest, &'a mut VisualTestContext) {
    let preferences = mcbench_ui::Preferences {
        last_model: Some(("anthropic".into(), mcbench_ui::dev::standin::MODEL.into())),
        ..mcbench_ui::Preferences::default()
    };
    open_shell_preferring(cx, test, start, picks, preferences, configure)
}

/// [`open_shell_with`] with these stored preferences.
pub fn open_shell_preferring<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    start: Option<mcbench_ui::state::place::Place>,
    picks: Vec<std::path::PathBuf>,
    preferences: mcbench_ui::Preferences,
    configure: impl FnOnce(&mcbench_ui::dev::standin::StandIn, &std::path::Path),
) -> (ShellUnderTest, &'a mut VisualTestContext) {
    use mcbench_ui::dev::{fixtures, standin::StandIn};
    use mcbench_ui::view::shell::{Shell, ShellOptions};
    quiet_crashes();
    let dir = TempDir::new(test);
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let stand_in = StandIn::new(
        fixtures::gate(&texts),
        texts.streamed.clone(),
        std::time::Duration::ZERO,
        None,
    );
    stand_in.keep_files(&dir.0.join("files"), &picture);
    configure(&stand_in, &dir.0);
    let core: std::sync::Arc<dyn mcbench_ui::Core> = std::sync::Arc::new(stand_in);
    let log_path = dir.0.join("actions.jsonl");
    let log = mcbench_ui::ActionLog::create(&log_path);
    cx.update(mcbench_ui::view::setup);
    let inbox = std::sync::Arc::new(Inbox::default());
    let core_for_shell = core.clone();
    let (entity, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Shell::new(
            ShellOptions {
                core: core_for_shell,
                log: Some(log),
                opener: std::rc::Rc::new(|_, _| {}),
                metrics: std::rc::Rc::new(std::cell::RefCell::new(
                    mcbench_ui::view::metrics::Metrics::new(std::time::Instant::now()),
                )),
                start,
                preferences,
                picker: mcbench_ui::view::fixed_picker(picks),
                selections: Vec::new(),
            },
            window,
            cx,
        )
    });
    core.attach(inbox.clone());
    let shell = ShellUnderTest {
        entity,
        inbox,
        log: log_path,
        _dir: dir,
    };
    pump(&shell, vcx);
    (shell, vcx)
}

// bench agent: the shell over the `bench` fixture, its library and votes,
// with plays.
/// The shell on `fixtures::bench` with its library and votes
/// (`StandIn::bench_fixture`) and `plays`, opened at `start`.
#[allow(dead_code)]
pub fn open_bench<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    start: Option<mcbench_ui::state::place::Place>,
    plays: &[mcbench_ui::dev::standin::BenchPlay],
) -> (
    ShellUnderTest,
    std::sync::Arc<mcbench_ui::dev::standin::StandIn>,
    &'a mut VisualTestContext,
) {
    open_bench_at(
        cx,
        test,
        start,
        plays,
        mcbench_ui::state::layout::SIDEBAR_WIDTH,
    )
}

/// [`open_bench`] with the sidebar `sidebar_width` wide.
#[allow(dead_code)]
pub fn open_bench_at<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    start: Option<mcbench_ui::state::place::Place>,
    plays: &[mcbench_ui::dev::standin::BenchPlay],
    sidebar_width: f32,
) -> (
    ShellUnderTest,
    std::sync::Arc<mcbench_ui::dev::standin::StandIn>,
    &'a mut VisualTestContext,
) {
    use mcbench_ui::dev::{fixtures, standin::StandIn};
    use mcbench_ui::view::shell::{Shell, ShellOptions};
    quiet_crashes();
    let dir = TempDir::new(test);
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let standin = std::sync::Arc::new(StandIn::new(
        fixtures::bench(&texts),
        texts.streamed.clone(),
        std::time::Duration::ZERO,
        None,
    ));
    standin.bench_fixture();
    for play in plays {
        standin.bench_play(*play);
    }
    let core: std::sync::Arc<dyn mcbench_ui::Core> = standin.clone();
    let log_path = dir.0.join("actions.jsonl");
    let log = mcbench_ui::ActionLog::create(&log_path);
    cx.update(mcbench_ui::view::setup);
    let inbox = std::sync::Arc::new(Inbox::default());
    let core_for_shell = core.clone();
    let preferences = mcbench_ui::Preferences {
        last_model: Some(("anthropic".into(), mcbench_ui::dev::standin::MODEL.into())),
        bench_mode: true,
        sidebar_width,
        ..mcbench_ui::Preferences::default()
    };
    let (entity, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Shell::new(
            ShellOptions {
                core: core_for_shell,
                log: Some(log),
                opener: std::rc::Rc::new(|_, _| {}),
                metrics: std::rc::Rc::new(std::cell::RefCell::new(
                    mcbench_ui::view::metrics::Metrics::new(std::time::Instant::now()),
                )),
                start,
                preferences,
                picker: mcbench_ui::view::fixed_picker(Vec::new()),
                selections: Vec::new(),
            },
            window,
            cx,
        )
    });
    core.attach(inbox.clone());
    let shell = ShellUnderTest {
        entity,
        inbox,
        log: log_path,
        _dir: dir,
    };
    pump(&shell, vcx);
    (shell, standin, vcx)
}

// start-place agent: the shell with the core's first lists held back (the
// real core's cold start sends them after its setup and the pane).
/// The shell over the gate fixture (`bench`: the bench fixture in Bench)
/// with `start`, the stand-in holding its first lists until
/// `StandIn::release_lists`; `late` false sends them first, as before.
#[allow(dead_code)]
pub fn open_with_lists<'a>(
    cx: &'a mut gpui::TestAppContext,
    test: &str,
    bench: bool,
    start: Option<mcbench_ui::state::place::Place>,
    late: bool,
) -> (
    ShellUnderTest,
    std::sync::Arc<mcbench_ui::dev::standin::StandIn>,
    &'a mut VisualTestContext,
) {
    use mcbench_ui::dev::{fixtures, standin::StandIn};
    use mcbench_ui::view::shell::{Shell, ShellOptions};
    quiet_crashes();
    let dir = TempDir::new(test);
    let picture = dir.0.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let fixture = if bench {
        fixtures::bench(&texts)
    } else {
        fixtures::gate(&texts)
    };
    let standin = std::sync::Arc::new(StandIn::new(
        fixture,
        texts.streamed.clone(),
        std::time::Duration::ZERO,
        None,
    ));
    if bench {
        standin.bench_fixture();
    }
    if late {
        standin.hold_first_lists();
    }
    let core: std::sync::Arc<dyn mcbench_ui::Core> = standin.clone();
    let log_path = dir.0.join("actions.jsonl");
    let log = mcbench_ui::ActionLog::create(&log_path);
    cx.update(mcbench_ui::view::setup);
    let inbox = std::sync::Arc::new(Inbox::default());
    let core_for_shell = core.clone();
    let preferences = mcbench_ui::Preferences {
        last_model: Some(("anthropic".into(), mcbench_ui::dev::standin::MODEL.into())),
        bench_mode: bench,
        ..mcbench_ui::Preferences::default()
    };
    let (entity, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Shell::new(
            ShellOptions {
                core: core_for_shell,
                log: Some(log),
                opener: std::rc::Rc::new(|_, _| {}),
                metrics: std::rc::Rc::new(std::cell::RefCell::new(
                    mcbench_ui::view::metrics::Metrics::new(std::time::Instant::now()),
                )),
                start,
                preferences,
                picker: mcbench_ui::view::fixed_picker(Vec::new()),
                selections: Vec::new(),
            },
            window,
            cx,
        )
    });
    core.attach(inbox.clone());
    let shell = ShellUnderTest {
        entity,
        inbox,
        log: log_path,
        _dir: dir,
    };
    pump(&shell, vcx);
    (shell, standin, vcx)
}
