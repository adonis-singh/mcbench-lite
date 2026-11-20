//! The accessibility contract (`native/crates/ui/ACCESSIBILITY.md`) checked
//! in process: the chat screen over the stand-in core and the `gate`
//! fixture, in GPUI's test platform (no window, nothing on any display), its
//! AccessKit tree read through `accesskit_consumer` as the platform adapters
//! read it. Runs on every host, the Mac included.

use std::sync::{Arc, Mutex};

use accesskit_consumer::{Node, Tree};
use gpui::accesskit::{Action, ActionRequest, Role};
use gpui::{Entity, TestAppContext, VisualTestContext};
use mcbench_ui::dev::fixtures;
use mcbench_ui::dev::standin::StandIn;
use mcbench_ui::view::shell::{Shell, ShellOptions};
use mcbench_ui::{ActionLog, Core, Update, Window};

/// A window for the core that keeps updates until the test applies them.
#[derive(Default)]
struct Inbox(Mutex<Vec<Update>>);

impl Window for Inbox {
    fn publish(&self, update: Update) {
        self.0.lock().expect("lock").push(update);
    }
}

struct Screen {
    entity: Entity<Shell>,
    inbox: Arc<Inbox>,
    log: std::path::PathBuf,
    _dir: TempDir,
}

/// A temporary folder removed at the end of the test.
struct TempDir(std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The crash module for the test process, so a failure that could abort
/// (a panic while panicking) ends quietly with a crash log instead of
/// raising the macOS crash reporter. A failed assertion ends the whole test
/// binary with status 101 and its message on stderr.
fn quiet_crashes() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let logs = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("mcbench-ui-crash-logs");
        mcbench_ui::crash::install(&logs, env!("CARGO_PKG_VERSION"), |_| false);
    });
}

fn open<'a>(cx: &'a mut TestAppContext, test: &str) -> (Screen, &'a mut VisualTestContext) {
    open_fixture(cx, test, false)
}

/// The screen on the gate fixture, or with `long` on the 400-message one.
fn open_fixture<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
    long: bool,
) -> (Screen, &'a mut VisualTestContext) {
    open_fixture_with(cx, test, long, None)
}

// narrow agent: a test that needs the wide window's 360 px chat column in
// a window under 860 px names its chat column's width.
fn open_fixture_with<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
    long: bool,
    chat_width: Option<f32>,
) -> (Screen, &'a mut VisualTestContext) {
    quiet_crashes();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("mcbench-ui-contract-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    let picture = dir.join("lamp.png");
    std::fs::write(&picture, fixtures::PICTURE).expect("the picture");
    let texts = fixtures::Texts::new(&picture);
    let core: Arc<dyn Core> = Arc::new(StandIn::new(
        if long {
            fixtures::long()
        } else {
            fixtures::gate(&texts)
        },
        texts.streamed.clone(),
        std::time::Duration::ZERO,
        None,
    ));
    let log_path = dir.join("actions.jsonl");
    let log = ActionLog::create(&log_path);
    cx.update(|cx| {
        mcbench_ui::view::setup(cx);
    });
    let inbox = Arc::new(Inbox::default());
    let core_for_screen = core.clone();
    let (entity, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Shell::new(
            ShellOptions {
                core: core_for_screen,
                log: Some(log),
                opener: std::rc::Rc::new(|_, _| {}),
                metrics: std::rc::Rc::new(std::cell::RefCell::new(
                    mcbench_ui::view::metrics::Metrics::new(std::time::Instant::now()),
                )),
                start: None,
                // chat: the stand-in's model is the last one used, so a
                // new chat sends with it.
                preferences: mcbench_ui::Preferences {
                    last_model: Some(("anthropic".into(), mcbench_ui::dev::standin::MODEL.into())),
                    chat_width,
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
    let screen = Screen {
        entity,
        inbox,
        log: log_path,
        _dir: TempDir(dir),
    };
    pump(&screen, vcx);
    (screen, vcx)
}

/// Applies every update the core published, until there are none.
fn pump(screen: &Screen, vcx: &mut VisualTestContext) {
    loop {
        let updates: Vec<Update> = std::mem::take(&mut *screen.inbox.0.lock().expect("lock"));
        if updates.is_empty() {
            break;
        }
        for update in updates {
            vcx.update(|window, cx| {
                screen
                    .entity
                    .update(cx, |s, cx| s.apply(update, window, cx))
            });
        }
        vcx.run_until_parked();
    }
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
}

fn tree(vcx: &mut VisualTestContext) -> Tree {
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

fn nodes(tree: &Tree) -> Vec<Node<'_>> {
    let mut out = Vec::new();
    all(tree.state().root(), &mut out);
    out
}

// sidebar agent: a list row by its title (its name adds its status, its
// resolution and its snippet after a comma).
fn row<'a>(nodes: &[Node<'a>], title: &str) -> Node<'a> {
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

fn named<'a>(nodes: &[Node<'a>], role: Role, label: &str) -> Node<'a> {
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

/// The content of a row's top-level block `index`: the block's stop (a
/// group named by its kind) holds it.
fn block<'a>(row: Node<'a>, index: usize) -> Node<'a> {
    let stop = row
        .children()
        .filter(|c| c.role() == Role::Group)
        .nth(index)
        .expect("the block's stop");
    stop.children().next().expect("the block")
}

/// The text of the live region with `role` (`Status` or `Alert`): its name,
/// which is its value too; none when it has none.
fn live(vcx: &mut VisualTestContext, role: Role) -> Option<String> {
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    // pane agent: the 3D pane's cover, banner and notes are status nodes
    // too (today's `role="status"`); the window's live region is the one
    // with its live property.
    let found: Vec<_> = of_role(&nodes, role)
        .into_iter()
        .filter(|n| n.live() != gpui::accesskit::Live::Off)
        .collect();
    assert_eq!(found.len(), 1, "one {role:?} region");
    let label = found[0].label();
    assert_eq!(
        label.as_deref(),
        found[0].value().as_deref(),
        "name and value agree"
    );
    label
}

fn status(vcx: &mut VisualTestContext) -> Option<String> {
    live(vcx, Role::Status)
}

fn of_role<'a>(nodes: &[Node<'a>], role: Role) -> Vec<Node<'a>> {
    nodes.iter().copied().filter(|n| n.role() == role).collect()
}

fn text(node: Node) -> String {
    assert!(
        node.supports_text_ranges(),
        "{:?} {:?} has no text ranges",
        node.role(),
        node.label()
    );
    node.document_range().text()
}

fn actions(screen: &Screen) -> Vec<serde_json::Value> {
    std::fs::read_to_string(&screen.log)
        .expect("the action log")
        .lines()
        .map(|l| serde_json::from_str(l).expect("a JSON line"))
        .collect()
}

fn click(vcx: &mut VisualTestContext, node: Node) {
    let request = ActionRequest {
        action: Action::Click,
        target_tree: gpui::accesskit::TreeId::ROOT,
        target_node: node.locate().0,
        data: None,
    };
    vcx.update(|window, cx| window.dispatch_a11y_action_for_test(request, cx));
    vcx.run_until_parked();
}

#[gpui::test]
fn window_list_and_controls(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "window");
    let tree = tree(vcx);
    let nodes = nodes(&tree);

    // The app opens ready to type: the composer holds focus, and the tree says so.
    let focus = tree.state().focus().expect("a focused node");
    assert_eq!(focus.role(), Role::MultilineTextInput);
    assert_eq!(focus.label().as_deref(), Some("Message"));

    let root = tree.state().root();
    assert_eq!(root.role(), Role::Window);
    assert_eq!(
        root.label().as_deref(),
        Some("Redstone lamp with a lever \u{b7} mcbench-lite")
    );

    let list = named(&nodes, Role::List, "Chats");
    let rows: Vec<_> = list.children().collect();
    assert_eq!(rows.len(), 3);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.role(), Role::ListItem);
        // No selection state: selecting would be a click and open the chat.
        assert_eq!(row.is_selected(), None);
        // AccessKit counts positions from 0; the adapters add 1.
        assert_eq!(row.position_in_set(), Some(index));
        assert_eq!(row.size_of_set(), Some(3));
        assert!(row.supports_action(Action::Click, &|_| {
            accesskit_consumer::FilterResult::Include
        }));
        assert!(row.is_focusable(&|_| accesskit_consumer::FilterResult::Include));
        assert!(row.has_bounds(), "a chat row has bounds");
    }
    assert_eq!(
        rows[0].label().as_deref(),
        Some("Redstone lamp with a lever, open")
    );
    assert_eq!(
        rows[0].aria_current(),
        Some(gpui::accesskit::AriaCurrent::Page)
    );
    assert_eq!(rows[1].aria_current(), None);

    named(&nodes, Role::Button, "New chat");
    let send = named(&nodes, Role::Button, "Send message");
    assert!(
        send.is_disabled(),
        "Send is disabled while the composer is empty"
    );
    assert!(
        of_role(&nodes, Role::Button)
            .iter()
            .all(|b| b.label().as_deref() != Some("Stop generating"))
    );
    let composer = named(&nodes, Role::MultilineTextInput, "Message");
    assert!(composer.supports_text_ranges());

    // The set's size is on the containers as well as on each item.
    assert_eq!(list.size_of_set(), Some(3));
    let transcript = named(&nodes, Role::Region, "Conversation");
    assert_eq!(transcript.size_of_set(), Some(9));
    assert!(!transcript.is_busy());
    assert!(transcript.supports_action(Action::ScrollDown, &|_| {
        accesskit_consumer::FilterResult::Include
    }));

    // Opening another chat through its row's click action.
    click(vcx, rows[1]);
    pump(&screen, vcx);
    let logged = actions(&screen);
    assert!(
        logged
            .iter()
            .any(|a| a["action"] == "opened chat" && a["chat"] == "chat-gate")
    );
}

#[gpui::test]
fn messages_and_markdown(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "messages");
    // The transcript starts at its end; Home brings the first rows in.
    vcx.update(|window, cx| {
        screen.entity.update(cx, |s, cx| {
            s.chat().update(cx, |c, cx| c.reveal_row(0, window, cx))
        });
    });
    pump(&screen, vcx);
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let articles = of_role(&nodes, Role::Article);
    assert!(!articles.is_empty());
    let count = articles[0].size_of_set().expect("a set size");
    for article in &articles {
        assert_eq!(article.size_of_set(), Some(count));
        assert!(article.position_in_set().is_some());
        let label = article.label().unwrap_or_default();
        assert!(
            label.starts_with("You") || label.starts_with("claude-opus-5-5"),
            "{label}"
        );
        assert_eq!(
            article.description(),
            None,
            "no description: the name reads"
        );
        assert!(article.has_bounds());
    }
    let first = articles
        .iter()
        .find(|a| a.position_in_set() == Some(0))
        .expect("the first message");
    assert_eq!(
        first.label().as_deref(),
        Some("You, Build a redstone lamp that turns on with a lever, and explain what you placed."),
        "{:?}",
        articles
            .iter()
            .map(|a| (a.position_in_set(), a.label()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        text(block(*first, 0)),
        "Build a redstone lamp that turns on with a lever, and explain what you placed."
    );
    // The answer's name: its prose as text, its code block and table by
    // kind and size.
    let answer = articles
        .iter()
        .find(|a| a.position_in_set() == Some(2))
        .expect("the answer")
        .label()
        .unwrap_or_default();
    assert!(
        answer.starts_with("claude-opus-5-5, A lamp on a lever\nThe lamp is built"),
        "{answer}"
    );
    assert!(answer.contains("\ncode, rust, 5 lines\n"), "{answer}");
    assert!(answer.contains("\ntable, 4 rows, 3 columns\n"), "{answer}");
    assert!(
        !answer.contains("world.fill") && !answer.contains("redstone_lamp, 1"),
        "{answer}"
    );

    let heading = named(&nodes, Role::Heading, "A lamp on a lever");
    // AccessKit counts levels and positions from 0; every adapter adds 1.
    assert_eq!(heading.level(), Some(0));
    assert_eq!(text(heading), "A lamp on a lever");

    let paragraph = of_role(&nodes, Role::Paragraph)
        .into_iter()
        .find(|p| p.supports_text_ranges() && text(*p).starts_with("The lamp is built"))
        .expect("the first paragraph");
    let link = paragraph
        .children()
        .find(|c| c.role() == Role::Link)
        .expect("a link in the paragraph");
    assert_eq!(link.label().as_deref(), Some("the wiki page on lamps"));
    assert_eq!(link.url(), Some("https://minecraft.wiki/w/Redstone_Lamp"));
    assert!(text(paragraph).contains("the wiki page on lamps has the details"));

    let lists = of_role(&nodes, Role::List);
    assert!(
        lists.iter().any(|l| l
            .children()
            .any(|i| i.children().any(|c| c.role() == Role::List))),
        "a nested list"
    );
    let code = of_role(&nodes, Role::Code)
        .into_iter()
        .find(|c| c.label().as_deref() == Some("Code, rust"))
        .expect("the rust code block");
    assert!(text(code).starts_with("fn build(world: &mut World) {\n"));
    named(&nodes, Role::Button, "Copy code");
    let quote = of_role(&nodes, Role::Blockquote);
    assert_eq!(quote.len(), 1);

    let table = of_role(&nodes, Role::Table)
        .into_iter()
        .next()
        .expect("a table");
    assert_eq!(table.data().row_count(), Some(4));
    assert_eq!(table.data().column_count(), Some(3));
    let headers = of_role(&nodes, Role::ColumnHeader);
    assert_eq!(
        headers.iter().map(|h| text(*h)).collect::<Vec<_>>(),
        ["Block", "Count", "Light"]
    );
    let cells = of_role(&nodes, Role::Cell);
    assert!(cells.iter().any(|c| text(*c) == "redstone_lamp"
        && c.data().row_index() == Some(2)
        && c.data().column_index() == Some(0)));

    // Every node has bounds a screen reader can draw, but the composer's
    // runs: gpui-base gives a multi-line editor's runs no geometry
    // (`input/base/a11y_text.rs`), listed as open in ACCESSIBILITY.md.
    for node in &nodes {
        // sidebar agent: the search fields' runs likewise (single-line
        // inputs of the same editor).
        let composer_run = node.role() == Role::TextRun
            && node.parent().is_some_and(|p| {
                p.role() == Role::MultilineTextInput || p.role() == Role::TextInput
            });
        if node.role() != Role::Window && !composer_run {
            assert!(
                node.has_bounds(),
                "{:?} {:?} {:?} under {:?} {:?} has no bounds",
                node.role(),
                node.label(),
                node.value(),
                node.parent().map(|p| p.role()),
                node.parent().and_then(|p| p.label()),
            );
        }
    }
}

#[gpui::test]
fn folds_tool_rows_and_activation(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "folds");
    vcx.update(|window, cx| {
        screen.entity.update(cx, |s, cx| {
            s.chat().update(cx, |c, cx| c.reveal_row(1, window, cx))
        });
    });
    pump(&screen, vcx);
    let fold = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        let fold = named(&nodes, Role::Button, "Worked \u{b7} 2 steps");
        assert_eq!(fold.data().is_expanded(), Some(false));
        fold.locate().0
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Click,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: fold,
                data: None,
            },
            cx,
        )
    });
    pump(&screen, vcx);
    let tree2 = tree(vcx);
    let nodes2 = nodes(&tree2);
    assert_eq!(
        named(&nodes2, Role::Button, "Worked \u{b7} 2 steps")
            .data()
            .is_expanded(),
        Some(true)
    );
    let build = named(&nodes2, Role::Button, "Built: a lamp with a lever");
    assert_eq!(build.data().is_expanded(), Some(false));
    named(&nodes2, Role::Button, "Ran command: ls -la /work");
    named(&nodes2, Role::Button, "Reasoning");
    click(vcx, build);
    pump(&screen, vcx);
    let tree3 = tree(vcx);
    let nodes3 = nodes(&tree3);
    assert_eq!(
        named(&nodes3, Role::Button, "Built: a lamp with a lever")
            .data()
            .is_expanded(),
        Some(true)
    );
    let details = named(&nodes3, Role::Group, "Built: a lamp with a lever, code");
    let code = details.children().next().expect("the code");
    assert!(text(code).starts_with("world.fill("));
    let logged = actions(&screen);
    assert!(
        logged
            .iter()
            .any(|a| a["action"] == "toggled" && a["key"] == "call-build" && a["open"] == true)
    );
}

#[gpui::test]
fn tab_walks_list_transcript_composer_send(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "tab");
    let mut walk = Vec::new();
    // pane agent: two more stops, the 3D pane and its Fill button.
    // share agent: and the header's Share (the core shows the chat's world
    // with its blocks).
    for _ in 0..19 {
        vcx.simulate_keystrokes("tab");
        pump(&screen, vcx);
        let tree = tree(vcx);
        let focus = tree
            .state()
            .focus()
            .map(|n| (n.role(), n.label().unwrap_or_default()))
            .expect("a focused node");
        walk.push(focus);
    }
    eprintln!("{walk:?}");

    // bench agent: the composer's prompts button follows Attach file;
    // pane agent: the pane's two stops come before the sidebar.
    let message = walk[17].1.clone();
    assert_eq!(
        walk[17].0,
        Role::Article,
        "the transcript's current message"
    );
    // The sidebar top to bottom (the mode switcher, New chat, the toggle,
    // the search and its filter button, the current chat row and its
    // archive button, the archived section's toggle, Settings), the
    // transcript's current message, the composer, Attach file (chat agent:
    // the composer's footer); Send is no stop while it is disabled, so Tab
    // wraps.
    let expected = [
        (Role::Button, "Attach file"),
        (Role::Button, "Insert a saved prompt"),
        // The footer's pickers, in the page's order (the meter is no stop).
        (Role::Button, "Resolution: Standard"),
        (Role::Button, "Chat model: claude-opus-5-5"),
        (Role::Button, "Reasoning effort: medium"),
        // pane agent: the 3D pane after the chat column (covered: no world
        // in this test, so its bar has Fill alone).
        (Role::Canvas, "Live Minecraft world"),
        (Role::Button, "Fill the window"),
        (Role::Button, "mcbuild"),
        (Role::Button, "New chat"),
        (Role::Button, "Collapse sidebar"),
        (Role::TextInput, "Search chats"),
        (Role::Button, "Filter"),
        (Role::ListItem, "Redstone lamp with a lever, open"),
        (Role::Button, "Archive build"),
        (Role::Button, "Show archived"),
        (Role::Button, "Settings"),
        // share agent: the pane header's Share.
        (Role::Button, "Share"),
        (Role::Article, message.as_str()),
        (Role::MultilineTextInput, "Message"),
    ];
    let walked: Vec<(Role, &str)> = walk.iter().map(|(r, l)| (*r, l.as_str())).collect();
    assert_eq!(walked, expected);
}

/// A parent that draws the screen as a cached view beside a counter, so a
/// notify of the parent repaints with the screen's frame reused.
struct Cached {
    screen: Entity<Shell>,
    renders: usize,
}

impl gpui::Render for Cached {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{ParentElement as _, Styled as _};
        self.renders += 1;
        gpui::div()
            .size_full()
            .child(gpui::div().child(format!("{}", self.renders)))
            .child(
                self.screen
                    .clone()
                    .cached(gpui::StyleRefinement::default().size_full()),
            )
    }
}

#[gpui::test]
fn a_cached_view_keeps_its_accessibility_subtree(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "cached");
    let entity = screen.entity.clone();
    let window = vcx.update(|window, cx| {
        window.replace_root(cx, |_, _| Cached {
            screen: entity,
            renders: 0,
        })
    });
    // Nothing in the screen changes while it is cached: no caret blinks
    // (focus on a chat row, not in the composer; never on nothing).
    let castle = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        row(&nodes, "Castle gate").locate().0
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Focus,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: castle,
                data: None,
            },
            cx,
        )
    });
    vcx.run_until_parked();
    let ids = |vcx: &mut VisualTestContext| {
        let update = vcx
            .update(|window, _| window.a11y_tree_update_for_test())
            .expect("a tree");
        let mut ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        ids.sort_unstable();
        ids
    };
    let before = ids(vcx);
    let screen_renders = vcx.update(|_, cx| screen.entity.read(cx).render_count());
    let parent_renders = vcx.update(|_, cx| window.read(cx).renders);
    assert!(before.len() > 40, "the screen's nodes are in the tree");
    // The parent re-renders; the screen is not notified, so its cached frame is reused.
    vcx.update(|_, cx| window.update(cx, |_, cx| cx.notify()));
    vcx.run_until_parked();
    let renders = vcx.update(|_, cx| window.read(cx).renders);
    assert_eq!(renders, parent_renders + 1, "the parent drew again");
    assert_eq!(
        vcx.update(|_, cx| screen.entity.read(cx).render_count()),
        screen_renders,
        "the screen's frame was reused, not rendered"
    );
    let after = ids(vcx);
    assert_eq!(
        before, after,
        "a reused cached view keeps its accessibility nodes"
    );
    // Its actions still work: open another chat by its row's click.
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let row = row(&nodes, "Wheat farm");
    click(vcx, row);
    pump(&screen, vcx);
    assert!(
        actions(&screen)
            .iter()
            .any(|a| a["action"] == "opened chat" && a["chat"] == "chat-farm")
    );
}

#[gpui::test]
fn an_error_turn_is_announced_and_can_be_dismissed(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "error");
    let row = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        row(&nodes, "Wheat farm").locate().0
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Click,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: row,
                data: None,
            },
            cx,
        )
    });
    pump(&screen, vcx);
    // Clicking the row focused it: focus the composer as a screen reader
    // would (its Focus action), then type and send.
    let composer = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        named(&nodes, Role::MultilineTextInput, "Message")
            .locate()
            .0
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Focus,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: composer,
                data: None,
            },
            cx,
        )
    });
    pump(&screen, vcx);
    // Opening a chat says its title once, and nothing of its answers.
    assert_eq!(status(vcx).as_deref(), Some("Wheat farm"));
    vcx.simulate_keystrokes("h i enter");
    pump(&screen, vcx);
    // The failed turn, which ends without an answer, announces its error and
    // nothing politely (not the chat's earlier answer).
    assert_eq!(status(vcx), None);
    let after_send = tree(vcx);
    let sent_nodes = nodes(&after_send);
    let alert = of_role(&sent_nodes, Role::Alert)
        .into_iter()
        .next()
        .expect("the alert");
    assert_eq!(
        alert.label().as_deref(),
        Some(mcbench_ui::dev::standin::FAILURE)
    );
    assert_eq!(alert.live(), gpui::accesskit::Live::Assertive);
    let error = named(
        &sent_nodes,
        Role::Article,
        &format!("Error, {}", mcbench_ui::dev::standin::FAILURE),
    );
    assert_eq!(text(block(error, 0)), mcbench_ui::dev::standin::FAILURE);
    let dismiss = named(&sent_nodes, Role::Button, "Dismiss error");
    assert!(dismiss.has_bounds());
    click(vcx, dismiss);
    pump(&screen, vcx);
    let after_dismiss = tree(vcx);
    let focus = after_dismiss.state().focus().expect("a focused node");
    assert_eq!(
        (focus.role(), focus.label().as_deref()),
        (Role::MultilineTextInput, Some("Message")),
        "after Dismiss error the composer has focus"
    );
    let dismissed_nodes = nodes(&after_dismiss);
    assert!(
        of_role(&dismissed_nodes, Role::Article)
            .iter()
            .all(|a| !a.label().unwrap_or_default().starts_with("Error"))
    );
    assert!(
        actions(&screen)
            .iter()
            .any(|a| a["action"] == "dismissed error")
    );
    assert!(
        actions(&screen)
            .iter()
            .any(|a| a["action"] == "sent" && a["text"] == "hi")
    );
}

#[gpui::test]
fn send_by_its_press_action_leaves_focus_in_the_composer(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "send-focus");
    vcx.simulate_keystrokes("h i");
    pump(&screen, vcx);
    let send = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        named(&nodes, Role::Button, "Send message").locate().0
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Click,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: send,
                data: None,
            },
            cx,
        )
    });
    pump(&screen, vcx);
    assert!(
        actions(&screen)
            .iter()
            .any(|a| a["action"] == "sent" && a["text"] == "hi")
    );
    let tree = tree(vcx);
    let focus = tree.state().focus().expect("a focused node");
    assert_eq!(
        focus.role(),
        Role::MultilineTextInput,
        "focus is back in the composer"
    );
}

#[gpui::test]
fn moving_over_the_chat_list_opens_nothing(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "list-walk");
    let castle = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        row(&nodes, "Castle gate").locate().0
    };
    // A screen reader moving to a row focuses it; only a click opens.
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Focus,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: castle,
                data: None,
            },
            cx,
        )
    });
    pump(&screen, vcx);
    let opened: Vec<_> = actions(&screen)
        .into_iter()
        .filter(|a| a["action"] == "opened chat")
        .collect();
    assert_eq!(opened.len(), 1, "only the chat opened at start: {opened:?}");
    let tree = tree(vcx);
    let focus = tree.state().focus().expect("a focused node");
    assert_eq!(focus.label().as_deref(), Some("Castle gate"));
}

#[gpui::test]
fn folds_take_expand_and_collapse(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "expand");
    vcx.update(|window, cx| {
        screen.entity.update(cx, |s, cx| {
            s.chat().update(cx, |c, cx| c.reveal_row(1, window, cx))
        });
    });
    pump(&screen, vcx);
    let act = |vcx: &mut VisualTestContext, name: &str, action: Action| {
        let id = {
            let tree = tree(vcx);
            let nodes = nodes(&tree);
            named(&nodes, Role::Button, name).locate().0
        };
        vcx.update(|window, cx| {
            window.dispatch_a11y_action_for_test(
                ActionRequest {
                    action,
                    target_tree: gpui::accesskit::TreeId::ROOT,
                    target_node: id,
                    data: None,
                },
                cx,
            )
        });
        pump(&screen, vcx);
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        named(&nodes, Role::Button, name).data().is_expanded()
    };
    let fold = "Worked \u{b7} 2 steps";
    assert_eq!(
        act(vcx, fold, Action::Collapse),
        Some(false),
        "Collapse on a closed fold keeps it closed"
    );
    assert_eq!(act(vcx, fold, Action::Expand), Some(true));
    assert_eq!(
        act(vcx, fold, Action::Expand),
        Some(true),
        "Expand on an open fold keeps it open"
    );
    let row = "Built: a lamp with a lever";
    assert_eq!(act(vcx, row, Action::Expand), Some(true));
    assert_eq!(act(vcx, row, Action::Collapse), Some(false));
    assert_eq!(act(vcx, row, Action::Click), Some(true));
    assert_eq!(act(vcx, fold, Action::Collapse), Some(false));
}

#[gpui::test]
fn the_fatal_screen_reads_its_message_and_focuses_restart(cx: &mut TestAppContext) {
    quiet_crashes();
    cx.update(mcbench_ui::view::setup);
    let message = "The data folder /nowhere could not be made: permission denied".to_string();
    // As on a real platform: the window draws, then accessibility comes on
    // (a reader asks), and the tree's focus is Quit from that frame.
    let (_, vcx) = cx.add_window_view(move |window, cx| {
        mcbench_ui::view::fatal::FatalScreen::new(message, window, cx)
    });
    vcx.run_until_parked();
    // The platform's next frame: Quit takes focus after the first one.
    vcx.update(|window, _| window.refresh());
    vcx.run_until_parked();
    vcx.update(|window, _| window.activate_a11y_for_test());
    vcx.run_until_parked();
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    // The message is in the alert's name: every reader speaks it.
    let alert = named(
        &nodes,
        Role::Alert,
        "mcbench-lite stopped on an error: The data folder /nowhere could not be made: permission denied",
    );
    let paragraph = alert
        .children()
        .find(|n| n.role() == Role::Paragraph)
        .expect("the message");
    assert_eq!(
        text(paragraph),
        "The data folder /nowhere could not be made: permission denied"
    );
    let quit = named(&nodes, Role::Button, "Restart");
    let focus = tree.state().focus().expect("a focused node");
    assert_eq!(focus.id(), quit.id(), "Restart has focus");
}

#[gpui::test]
fn an_inactive_window_stops_the_caret_blink(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "inactive");
    let renders =
        |vcx: &mut VisualTestContext| vcx.update(|_, cx| screen.entity.read(cx).render_count());
    let tick = |vcx: &mut VisualTestContext, ms: u64| {
        vcx.executor()
            .advance_clock(std::time::Duration::from_millis(ms));
        vcx.run_until_parked();
    };
    // The composer has focus in the active window: its caret blinks, and
    // every blink renders the screen.
    // The test platform opens the window inactive.
    vcx.update(|window, _| window.activate_window());
    tick(vcx, 100);
    assert!(vcx.update(|window, _| window.is_window_active()));
    let before = renders(vcx);
    tick(vcx, 1_100);
    assert!(
        renders(vcx) > before,
        "the caret blinks while the window is active"
    );
    vcx.deactivate_window();
    tick(vcx, 100);
    let inactive = renders(vcx);
    tick(vcx, 3_000);
    assert_eq!(
        renders(vcx),
        inactive,
        "nothing renders in an inactive window"
    );
}

#[gpui::test]
fn right_and_left_walk_the_current_message_by_block(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "blocks");
    vcx.update(|window, cx| {
        screen.entity.update(cx, |s, cx| {
            s.chat().update(cx, |c, cx| c.reveal_row(0, window, cx))
        });
    });
    pump(&screen, vcx);
    let focus = |vcx: &mut VisualTestContext| {
        let tree = tree(vcx);
        tree.state()
            .focus()
            .map(|n| {
                (
                    n.role(),
                    n.label().unwrap_or_default(),
                    n.description().unwrap_or_default(),
                )
            })
            .expect("a focused node")
    };
    let first = focus(vcx);
    assert_eq!(first.0, Role::Article);
    assert!(first.1.starts_with("You, "), "{}", first.1);
    vcx.simulate_keystrokes("right");
    let block = focus(vcx);
    assert_eq!(
        block,
        (
            Role::Group,
            "paragraph, Build a redstone lamp that turns on with a lever, and explain what you placed."
                .to_string(),
            String::new()
        )
    );
    // The user's message has one block: Right stays on it, Left goes back.
    vcx.simulate_keystrokes("right");
    assert_eq!(focus(vcx), block);
    vcx.simulate_keystrokes("left");
    assert_eq!(focus(vcx).0, Role::Article);
    // The answer (the third row): Right visits each of its blocks in order,
    // each read on its own; Down from a block moves to the next message.
    vcx.simulate_keystrokes("down down");
    pump(&screen, vcx);
    let message = focus(vcx);
    let mut kinds = Vec::new();
    for _ in 0..12 {
        vcx.simulate_keystrokes("right");
        let now = focus(vcx);
        if now.0 != Role::Group || kinds.last() == Some(&now) {
            break;
        }
        kinds.push(now);
    }
    eprintln!("{message:?} {kinds:?}");
    assert!(kinds.len() >= 3, "{kinds:?}");
    assert!(
        kinds.iter().any(|k| k.1.starts_with("heading level")),
        "{kinds:?}"
    );
    // Each named by its kind and how it reads, none described.
    assert!(
        kinds.iter().all(|k| k.2.is_empty() && k.1.contains(", ")),
        "{kinds:?}"
    );
    let names: Vec<&str> = kinds.iter().map(|k| k.1.as_str()).collect();
    assert!(
        names.contains(&"heading level 1, A lamp on a lever"),
        "{names:?}"
    );
    assert!(names.contains(&"code, rust, 5 lines"), "{names:?}");
    assert!(names.contains(&"table, 4 rows, 3 columns"), "{names:?}");
    vcx.simulate_keystrokes("down");
    assert_eq!(focus(vcx).0, Role::Article);
}

#[gpui::test]
fn the_same_answer_twice_is_announced_twice(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "twice");
    assert_eq!(status(vcx), None);
    let answered = |vcx: &mut VisualTestContext| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            pump(&screen, vcx);
            if let Some(now) = status(vcx) {
                return now;
            }
            assert!(std::time::Instant::now() < deadline, "no answer announced");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    };
    vcx.simulate_keystrokes("h i enter");
    let first = answered(vcx);
    // The stand-in answers at once; its events wait in the inbox until the
    // next pump, so this frame is the turn's start.
    vcx.simulate_keystrokes("h i enter");
    assert_eq!(
        status(vcx),
        None,
        "a turn's start leaves the status region without text"
    );
    let second = answered(vcx);
    assert_eq!(first, second, "the same answer, announced again");
    // Plain text, as the answer reads: no markdown marks.
    assert!(!first.contains("**") && !first.contains("# "), "{first:?}");
}

#[gpui::test]
fn the_window_name_changes_with_the_title(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "title");
    let castle = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        assert_eq!(
            tree.state().root().label().as_deref(),
            Some("Redstone lamp with a lever \u{b7} mcbench-lite")
        );
        row(&nodes, "Castle gate").locate().0
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Click,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: castle,
                data: None,
            },
            cx,
        )
    });
    // One frame: the platform title and the window's accessible name are
    // the same in the tree that frame built.
    vcx.run_until_parked();
    let title = vcx.window_title();
    let name = tree(vcx).state().root().label();
    assert_eq!(title.as_deref(), Some("Castle gate \u{b7} mcbench-lite"));
    assert_eq!(name, title);
    pump(&screen, vcx);
}

#[gpui::test]
fn an_input_method_commit_leaves_a_caret_after_the_text(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler as _;
    let (screen, vcx) = open(cx, "ime");
    let composer = vcx.update(|_, cx| screen.entity.read(cx).chat().read(cx).composer());
    let selection = |vcx: &mut VisualTestContext| {
        let tree = tree(vcx);
        let focus = tree.state().focus().expect("a focused node");
        assert_eq!(focus.role(), Role::MultilineTextInput);
        let anchor = focus.text_selection_anchor().expect("an anchor");
        let caret = focus.text_selection_focus().expect("a focus");
        (
            anchor.to_global_utf16_index(),
            caret.to_global_utf16_index(),
        )
    };
    // As macOS does for 日本語: the marked text with a caret at its end,
    // then the commit (insertText with no replacement range).
    vcx.update(|window, cx| {
        composer.update(cx, |c, cx| {
            c.replace_and_mark_text_in_range(None, "日本語", Some(3..3), window, cx)
        })
    });
    vcx.run_until_parked();
    assert_eq!(selection(vcx), (3, 3), "the caret after the marked text");
    vcx.update(|window, cx| {
        composer.update(cx, |c, cx| {
            c.replace_text_in_range(None, "日本語", window, cx)
        })
    });
    vcx.run_until_parked();
    assert_eq!(selection(vcx), (3, 3), "the caret after the committed text");
    assert_eq!(
        vcx.update(|_, cx| composer.read(cx).value().to_string()),
        "日本語"
    );
    // The same with the marked range named as the replacement range, as
    // some input methods commit.
    vcx.update(|window, cx| {
        composer.update(cx, |c, cx| {
            c.replace_and_mark_text_in_range(None, "かな", Some(2..2), window, cx)
        })
    });
    vcx.run_until_parked();
    assert_eq!(selection(vcx), (5, 5));
    vcx.update(|window, cx| {
        composer.update(cx, |c, cx| {
            c.replace_text_in_range(Some(3..5), "仮名", window, cx)
        })
    });
    vcx.run_until_parked();
    assert_eq!(selection(vcx), (5, 5), "the caret after the committed text");
}

#[gpui::test]
fn a_screen_filling_message_keeps_its_neighbours_in_the_tree(cx: &mut TestAppContext) {
    // narrow agent: 1000 wide with the chat column dragged to 360 px (was a
    // 700 px window and the default 360): at 980 and below the window is
    // narrow, the world over the transcript.
    let (screen, vcx) = open_fixture_with(cx, "neighbours", true, Some(360.));
    // A window so short that a five-line message (every fifth from the
    // fifth) fills the transcript.
    vcx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(280.)));
    vcx.run_until_parked();
    // Down from the first message, as a reader moves (the list measures
    // rows as it reaches them), stopping at the top, the middle and the end.
    let mut at = 0usize;
    for index in [4usize, 199, 399] {
        while at <= index {
            vcx.update(|window, cx| {
                screen.entity.update(cx, |s, cx| {
                    s.chat().update(cx, |c, cx| c.reveal_row(at, window, cx))
                });
            });
            vcx.update(|window, _| window.refresh());
            vcx.run_until_parked();
            at += 1;
        }
        pump(&screen, vcx);
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        let mut articles: Vec<(usize, f64)> = of_role(&nodes, Role::Article)
            .iter()
            .map(|a| {
                (
                    a.position_in_set().expect("a position"),
                    a.bounding_box().expect("bounds").y0,
                )
            })
            .collect();
        articles.sort_by_key(|(position, _)| *position);
        let positions: Vec<usize> = articles.iter().map(|(p, _)| *p).collect();
        for neighbour in [index.checked_sub(1), Some(index), Some(index + 1)]
            .into_iter()
            .flatten()
            .filter(|n| *n < 400)
        {
            assert!(
                positions.contains(&neighbour),
                "message {neighbour} is in the tree with {index} current: {positions:?}"
            );
        }
        // In reading order, at their real places: each below the one before.
        assert!(articles.windows(2).all(|w| w[0].1 < w[1].1), "{articles:?}");
        let focus = tree.state().focus().expect("a focused node");
        assert_eq!(focus.position_in_set(), Some(index));
        // The current message fills the transcript: its neighbours are laid
        // out outside it.
        let region = of_role(&nodes, Role::Region)[0]
            .bounding_box()
            .expect("bounds");
        let current = focus.bounding_box().expect("bounds");
        assert!(
            current.y0 <= region.y0 && current.y1 >= region.y1,
            "message {index} at {current:?} fills the transcript at {region:?}"
        );
    }
}

#[gpui::test]
fn the_same_error_twice_is_announced_twice(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "error-twice");
    let farm = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        row(&nodes, "Wheat farm").locate().0
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Click,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: farm,
                data: None,
            },
            cx,
        )
    });
    pump(&screen, vcx);
    let composer = vcx.update(|_, cx| screen.entity.read(cx).chat().read(cx).composer());
    vcx.update(|window, cx| composer.update(cx, |c, cx| c.focus(window, cx)));
    assert_eq!(live(vcx, Role::Alert), None);
    // As in the app: Send, and the stand-in's error (published while
    // sending) applied before the next frame. Each frame's tree is read, as
    // an adapter sees the trees one after another.
    let frame = |vcx: &mut VisualTestContext| {
        vcx.update(|window, cx| window.draw(cx).clear(cx));
        live(vcx, Role::Alert)
    };
    let mut seen = Vec::new();
    for _ in 0..3 {
        vcx.update(|window, cx| {
            composer.update(cx, |c, cx| c.set_value("hi", window, cx));
            screen
                .entity
                .update(cx, |s, cx| s.chat().update(cx, |c, cx| c.send(window, cx)));
            // No frame in between: the app's update loop applies the
            // stand-in's error (published while sending) next.
            let updates: Vec<Update> = std::mem::take(&mut *screen.inbox.0.lock().expect("lock"));
            assert!(!updates.is_empty(), "the stand-in's error is waiting");
            for update in updates {
                screen
                    .entity
                    .update(cx, |s, cx| s.apply(update, window, cx));
            }
        });
        // The frame the test platform draws when the update ends, then two
        // more.
        seen.push(live(vcx, Role::Alert));
        for _ in 0..2 {
            seen.push(frame(vcx));
        }
        assert_eq!(status(vcx), None);
    }
    // Each failure is a change between two frames: the alert shows the
    // error three times, each after a frame without it.
    let shown: Vec<Option<&str>> = seen.iter().map(|t| t.as_deref()).collect();
    let changes_to_error = shown
        .windows(2)
        .filter(|w| w[0].is_none() && w[1] == Some(mcbench_ui::dev::standin::FAILURE))
        .count()
        + usize::from(shown[0] == Some(mcbench_ui::dev::standin::FAILURE));
    assert_eq!(changes_to_error, 3, "{shown:?}");
}

#[gpui::test]
fn focus_moved_by_a_screen_reader_keeps_the_next_row_and_scrolls(cx: &mut TestAppContext) {
    // narrow agent: 1000 wide with the chat column dragged to 360 px (was a
    // 700 px window and the default 360): at 980 and below the window is
    // narrow, the world over the transcript.
    let (screen, vcx) = open_fixture_with(cx, "focus-walk", true, Some(360.));
    vcx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(240.)));
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        screen.entity.update(cx, |s, cx| {
            s.chat().update(cx, |c, cx| c.reveal_row(0, window, cx))
        });
    });
    for _ in 0..3 {
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
    }
    let article_at = |nodes: &[Node], position: usize| {
        nodes
            .iter()
            .find(|n| n.role() == Role::Article && n.position_in_set() == Some(position))
            .map(|n| (n.locate().0, n.bounding_box().expect("bounds")))
    };
    {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        let region = of_role(&nodes, Role::Region)[0]
            .bounding_box()
            .expect("bounds");
        let (_, first) = article_at(&nodes, 0).expect("message 0");
        // It runs past the transcript's end: no other message is on screen.
        assert!(
            first.y1 >= region.y1,
            "message 0 at {first:?} fills the transcript at {region:?}"
        );
    }
    // A screen reader moves focus to the next message by the Focus action,
    // with no scroll key, five times.
    for target in 1..=5usize {
        let (id, _) = {
            let tree = tree(vcx);
            let nodes = nodes(&tree);
            article_at(&nodes, target)
                .unwrap_or_else(|| panic!("message {target}, the next one, is in the tree"))
        };
        vcx.update(|window, cx| {
            window.dispatch_a11y_action_for_test(
                ActionRequest {
                    action: Action::Focus,
                    target_tree: gpui::accesskit::TreeId::ROOT,
                    target_node: id,
                    data: None,
                },
                cx,
            )
        });
        for _ in 0..3 {
            vcx.update(|window, _| window.refresh());
            vcx.run_until_parked();
        }
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        let focus = tree.state().focus().expect("a focused node");
        assert_eq!(focus.position_in_set(), Some(target));
        assert!(
            article_at(&nodes, target + 1).is_some(),
            "message {} follows the focused {target} in the tree",
            target + 1
        );
        let region = of_role(&nodes, Role::Region)[0]
            .bounding_box()
            .expect("bounds");
        let shown = focus.bounding_box().expect("bounds");
        assert!(
            shown.y0 < region.y1 && shown.y1 > region.y0,
            "the focused message {target} at {shown:?} is scrolled into the transcript at {region:?}"
        );
    }
}

/// Sends `turns` messages in the open chat, each waiting for the stand-in's
/// streamed answer (the status region's announcement).
fn stream_turns(screen: &Screen, vcx: &mut VisualTestContext, turns: usize) {
    let composer = vcx.update(|_, cx| screen.entity.read(cx).chat().read(cx).composer());
    for _ in 0..turns {
        vcx.update(|window, cx| composer.update(cx, |c, cx| c.focus(window, cx)));
        vcx.simulate_keystrokes("h i enter");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            pump(screen, vcx);
            if status(vcx).is_some() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "no answer");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

/// Shift+Tab from the composer lands on a message (the transcript's Tab
/// stop); returns its position.
fn shift_tab_from_composer(screen: &Screen, vcx: &mut VisualTestContext) -> usize {
    let composer = vcx.update(|_, cx| screen.entity.read(cx).chat().read(cx).composer());
    vcx.update(|window, cx| composer.update(cx, |c, cx| c.focus(window, cx)));
    for _ in 0..2 {
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
    }
    vcx.simulate_keystrokes("shift-tab");
    vcx.run_until_parked();
    // chat: a user message's tools (Copy message, Edit and branch) are its
    // parts after it in the Tab order; Shift+Tab walks back over them.
    for _ in 0..4 {
        let tree = tree(vcx);
        let focus = tree.state().focus().expect("a focused node");
        if focus.role() == Role::Article || focus.role() != Role::Button {
            break;
        }
        vcx.simulate_keystrokes("shift-tab");
        vcx.run_until_parked();
    }
    let tree = tree(vcx);
    let focus = tree.state().focus().expect("a focused node");
    assert_eq!(
        focus.role(),
        Role::Article,
        "Shift+Tab from the composer reaches a message, not {:?} {:?}",
        focus.role(),
        focus.label()
    );
    focus.position_in_set().expect("a position")
}

fn row_count(screen: &Screen, vcx: &mut VisualTestContext) -> usize {
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let _ = screen;
    of_role(&nodes, Role::Article)[0]
        .size_of_set()
        .expect("a set size")
}

#[gpui::test]
fn the_transcript_stays_a_tab_stop_after_turns_scrolling_and_reopening(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "tab-stop");
    for turns in [3usize, 27] {
        stream_turns(&screen, vcx, turns);
        let rows = row_count(&screen, vcx);
        assert_eq!(
            shift_tab_from_composer(&screen, vcx),
            rows - 1,
            "after the turns the current message is the last of {rows}"
        );
    }
    // Scrolled far up with the pointer's wheel.
    let centre = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        let region = of_role(&nodes, Role::Region)[0]
            .bounding_box()
            .expect("bounds");
        let scale = vcx.update(|window, _| window.scale_factor()) as f64;
        gpui::point(
            gpui::px(((region.x0 + region.x1) / 2.0 / scale) as f32),
            gpui::px(((region.y0 + region.y1) / 2.0 / scale) as f32),
        )
    };
    for _ in 0..60 {
        vcx.simulate_event(gpui::ScrollEvent {
            position: centre,
            delta: gpui::ScrollDelta::Lines(gpui::point(0., 10.)),
            ..Default::default()
        });
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
    }
    let reached = shift_tab_from_composer(&screen, vcx);
    let rows = row_count(&screen, vcx);
    assert!(
        reached + 5 < rows,
        "a message on screen after scrolling up: {reached} of {rows}"
    );
    // Another chat and back.
    for chat in ["Castle gate", "Redstone lamp with a lever"] {
        let row = {
            let tree = tree(vcx);
            let nodes = nodes(&tree);
            row(&nodes, chat).locate().0
        };
        vcx.update(|window, cx| {
            window.dispatch_a11y_action_for_test(
                ActionRequest {
                    action: Action::Click,
                    target_tree: gpui::accesskit::TreeId::ROOT,
                    target_node: row,
                    data: None,
                },
                cx,
            )
        });
        pump(&screen, vcx);
        let rows = row_count(&screen, vcx);
        assert_eq!(shift_tab_from_composer(&screen, vcx), rows - 1, "{chat}");
    }
}

#[gpui::test]
fn opening_a_chat_says_its_title_once(cx: &mut TestAppContext) {
    let (screen, vcx) = open(cx, "open-announce");
    // At launch nothing is announced: the window name says the chat.
    assert_eq!(status(vcx), None);
    let castle = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        row(&nodes, "Castle gate").locate().0
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Click,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: castle,
                data: None,
            },
            cx,
        )
    });
    pump(&screen, vcx);
    assert_eq!(status(vcx).as_deref(), Some("Castle gate"));
    // Enter on a chat row opens it too, and says so.
    let lamp = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        row(&nodes, "Redstone lamp with a lever").locate().0
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Focus,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: lamp,
                data: None,
            },
            cx,
        )
    });
    vcx.run_until_parked();
    vcx.simulate_keystrokes("enter");
    pump(&screen, vcx);
    assert_eq!(status(vcx).as_deref(), Some("Redstone lamp with a lever"));
}

#[gpui::test]
fn the_open_chat_is_current_and_open_is_in_its_name(cx: &mut TestAppContext) {
    // The open chat and the list's focused row are different things: the
    // open one is current and says `open` in its name (no description);
    // walking moves the focused row and opens nothing, so `open` stays.
    let (screen, vcx) = open(cx, "open-name");
    let second = {
        let tree = tree(vcx);
        let nodes = nodes(&tree);
        let rows: Vec<_> = named(&nodes, Role::List, "Chats").children().collect();
        let names: Vec<_> = rows.iter().map(|r| r.label().unwrap_or_default()).collect();
        assert_eq!(names[0], "Redstone lamp with a lever, open");
        assert!(
            names[1..]
                .iter()
                .all(|n| !n.split(", ").any(|part| part == "open")),
            "{names:?}"
        );
        for row in &rows {
            assert_eq!(
                row.description(),
                None,
                "no description on {:?}",
                row.label()
            );
            assert_eq!(row.is_selected(), None);
        }
        rows[1].locate().0
    };
    vcx.update(|window, cx| {
        window.dispatch_a11y_action_for_test(
            ActionRequest {
                action: Action::Focus,
                target_tree: gpui::accesskit::TreeId::ROOT,
                target_node: second,
                data: None,
            },
            cx,
        )
    });
    pump(&screen, vcx);
    let tree = tree(vcx);
    let focus = tree.state().focus().expect("a focused node");
    assert_eq!(focus.locate().0, second, "the walked row has focus");
    assert_eq!(
        focus.aria_current(),
        None,
        "the focused row is not the open chat"
    );
    let nodes = nodes(&tree);
    let current: Vec<_> = of_role(&nodes, Role::ListItem)
        .into_iter()
        .filter(|n| n.aria_current().is_some())
        .map(|n| n.label().unwrap_or_default())
        .collect();
    assert_eq!(current, ["Redstone lamp with a lever, open"]);
}

#[gpui::test]
fn rows_scrolled_out_of_the_transcript_are_clipped_out_and_take_no_pointer(
    cx: &mut TestAppContext,
) {
    // W2: the transcript clips its rows; the neighbours kept outside the
    // viewport (vendor patch 31) are clipped out (AT-SPI: not showing; UIA:
    // offscreen) and a point outside the transcript hits none of them.
    // narrow agent: 1000 wide with the chat column dragged to 360 px (was a
    // 700 px window and the default 360): at 980 and below the window is
    // narrow, the world over the transcript.
    let (screen, vcx) = open_fixture_with(cx, "clipped", true, Some(360.));
    vcx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(280.)));
    vcx.run_until_parked();
    for at in 0..=199 {
        vcx.update(|window, cx| {
            screen.entity.update(cx, |s, cx| {
                s.chat().update(cx, |c, cx| c.reveal_row(at, window, cx))
            });
        });
        vcx.update(|window, _| window.refresh());
        vcx.run_until_parked();
    }
    pump(&screen, vcx);
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let region = of_role(&nodes, Role::Region)[0];
    assert!(region.clips_children());
    let clip = region.bounding_box().expect("bounds");
    let articles = of_role(&nodes, Role::Article);
    let (out, shown): (Vec<Node>, Vec<Node>) = articles.iter().partition(|a| {
        let b = a.bounding_box().expect("bounds");
        b.y1 <= clip.y0 || b.y0 >= clip.y1
    });
    assert!(
        !out.is_empty(),
        "a kept neighbour lies outside the transcript"
    );
    assert!(!shown.is_empty());
    assert!(
        out.iter().all(|a| a.is_clipped_out()),
        "outside: clipped out"
    );
    assert!(
        shown.iter().all(|a| !a.is_clipped_out()),
        "on screen: shown"
    );
    // A point above the transcript (in the pane's header) hits no row.
    let above = out
        .iter()
        .map(|a| a.bounding_box().expect("bounds"))
        .find(|b| b.y1 <= clip.y0)
        .expect("a row above");
    let point = gpui::accesskit::Point::new((above.x0 + above.x1) / 2., (above.y0 + above.y1) / 2.);
    let hit = tree
        .state()
        .root()
        .node_at_point(point, &|_| accesskit_consumer::FilterResult::Include);
    assert!(
        hit.is_none_or(|n| n.role() != Role::Article),
        "the pointer over the header hits a hidden row: {:?}",
        hit.map(|n| n.label())
    );
}
