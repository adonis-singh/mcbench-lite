//! The composer's pickers and meters (`ACCESSIBILITY.md`, the pickers'
//! rows) checked in process on the gallery's pickers
//! (`mcbench_ui::dev::gallery_pickers`, over the stand-in's authored
//! catalogues): roles, names and states; the model picker's rail, search,
//! lists, favourites, failing and slow catalogues and keys; the effort and
//! resolution pickers; the meter and the chip; no AI connected.

mod common;

use common::*;
use gpui::accesskit::{Action, Role, Toggled};
use gpui::{Entity, TestAppContext, VisualTestContext};
use mcbench_ui::ActionLog;
use mcbench_ui::dev::gallery::Gallery;
use mcbench_ui::dev::gallery_pickers::PickerGallery;
use mcbench_ui::dev::standin::{CATALOGUE_FAILURE, MODEL, SLOW, STAR_FAILURE};
use mcbench_ui::state::popover::{EFFORT_EXIT, MODEL_EXIT, RESOLUTION_EXIT};

fn open<'a>(
    cx: &'a mut TestAppContext,
    test: &str,
) -> (Entity<PickerGallery>, &'a mut VisualTestContext, TempDir) {
    quiet_crashes();
    let dir = TempDir::new(test);
    let log = ActionLog::create(&dir.0.join("actions.jsonl"));
    cx.update(mcbench_ui::view::setup);
    let (gallery, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_a11y_for_test();
        Gallery::new(Some(log), cx)
    });
    vcx.run_until_parked();
    let _ = tree(vcx);
    let pickers = vcx
        .update(|_, cx| gallery.read(cx).pickers())
        .expect("the first frame made the pickers");
    vcx.run_until_parked();
    (pickers, vcx, dir)
}

fn logged(dir: &TempDir, action: &str) -> Vec<serde_json::Value> {
    log_lines(&dir.0.join("actions.jsonl"))
        .into_iter()
        .filter(|l| l["action"] == action)
        .collect()
}

fn trigger_name() -> String {
    format!("Chat model: {MODEL}")
}

fn open_model_picker(vcx: &mut VisualTestContext) {
    let tree = tree(vcx);
    click(vcx, named(&nodes(&tree), Role::Button, &trigger_name()));
}

fn options(vcx: &mut VisualTestContext) -> Vec<String> {
    let tree = tree(vcx);
    of_role(&nodes(&tree), Role::ListBoxOption)
        .iter()
        .filter(|n| {
            n.parent()
                .is_some_and(|p| p.label().as_deref() == Some("Chat model"))
        })
        .map(|n| n.label().unwrap_or_default())
        .collect()
}

#[gpui::test]
fn the_footer_pickers_have_their_roles_names_and_states(cx: &mut TestAppContext) {
    let (_, vcx, _dir) = open(cx, "pickers-roles");
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let model = named(&nodes, Role::Button, &trigger_name());
    assert_eq!(model.data().is_expanded(), Some(false));
    assert_eq!(model.description().as_deref(), Some(MODEL));
    assert!(
        model.data().has_popup().is_some(),
        "it says it opens a dialog"
    );
    let resolution = named(&nodes, Role::Button, "Resolution: Standard");
    assert_eq!(resolution.data().is_expanded(), Some(false));
    let effort = named(&nodes, Role::Button, "Reasoning effort: medium");
    assert_eq!(effort.data().is_expanded(), Some(false));
    // The usage is Claude Code's Opus: the meter and the chip show.
    // chat-fix-2: the percentage is the value alone, and the meter has no
    // description (a reader says each once).
    let meter = named(&nodes, Role::Meter, "Context \u{b7} 840k of 1M tokens");
    assert_eq!(meter.numeric_value(), Some(84.));
    assert_eq!(meter.data().min_numeric_value(), Some(0.));
    assert_eq!(meter.data().max_numeric_value(), Some(100.));
    assert_eq!(meter.description(), None);
    assert_eq!(meter.value(), None, "no value text beside the number");
    assert!(
        !meter.supports_action(gpui::accesskit::Action::SetValue, &|_| {
            accesskit_consumer::FilterResult::Include
        }),
        "a meter is not editable"
    );
    let chip = named(&nodes, Role::Label, "Near weekly limit");
    assert!(
        chip.description()
            .is_some_and(|d| d.starts_with("Resets at ")),
        "{:?}",
        chip.description()
    );
    let high = named(&nodes, Role::Meter, "Context \u{b7} 190k of 200k tokens");
    assert_eq!(high.numeric_value(), Some(95.));
    named(&nodes, Role::Label, "Near 5-hour limit");
    named(&nodes, Role::Label, "weekly Opus limit reached");
    let read_only = named(
        &nodes,
        Role::Button,
        "Resolution: High (2\u{d7}) (set when the bench was made)",
    );
    assert!(read_only.is_disabled());
    for tag in ["2\u{d7}", "5\u{d7}", "10\u{d7}"] {
        named(&nodes, Role::Label, tag);
    }
    assert!(!has(&nodes, Role::Label, "1\u{d7}"), "no tag at Standard");
    named(&nodes, Role::Button, "Choose chat model");
}

#[gpui::test]
fn the_model_picker_opens_on_the_chosen_models_provider_with_focus_in_its_search(
    cx: &mut TestAppContext,
) {
    let (_, vcx, _dir) = open(cx, "pickers-open");
    open_model_picker(vcx);
    let tree1 = tree(vcx);
    let nodes1 = nodes(&tree1);
    let ns = &nodes1;
    named(ns, Role::Dialog, "Chat model");
    assert_eq!(
        named(ns, Role::Button, &trigger_name())
            .data()
            .is_expanded(),
        Some(true)
    );
    let rail = named(ns, Role::TabList, "Providers");
    let tabs: Vec<String> = of_role(ns, Role::Tab)
        .iter()
        .filter(|t| t.parent().is_some_and(|p| p.id() == rail.id()))
        .map(|t| t.label().unwrap_or_default())
        .collect();
    assert_eq!(
        tabs,
        [
            "Favorites",
            "OpenRouter",
            "OpenAI \u{2014} not configured",
            "ChatGPT",
            "Anthropic",
            "Groq",
            "Google",
            "Claude Code harness: its own agent loop with mcbench tools attached",
            "Codex CLI harness: its own agent loop with mcbench tools attached \u{2014} CLI not signed in",
        ],
        "QwenCloud is turned off in Settings and Cursor is not in the native app"
    );
    assert!(named(ns, Role::Tab, "OpenAI \u{2014} not configured").is_disabled());
    assert!(
        named(
            ns,
            Role::Tab,
            "Claude Code harness: its own agent loop with mcbench tools attached"
        )
        .is_selected()
            == Some(true)
    );
    named(ns, Role::Button, "Set up providers");
    named(ns, Role::Button, "Compare several in Bench");
    assert_eq!(
        focused(vcx),
        (Role::TextInput, Some("Search models".into()))
    );
    assert_eq!(options(vcx), ["Claude Opus 5.5", "Claude Haiku 5.5"]);
    let tree2 = tree(vcx);
    let nodes2 = nodes(&tree2);
    let ns = &nodes2;
    let opus = named(ns, Role::ListBoxOption, "Claude Opus 5.5");
    assert_eq!(opus.is_selected(), Some(true));
    assert_eq!(opus.data().position_in_set(), Some(0));
    assert_eq!(opus.data().size_of_set(), Some(2));
    let star = named(ns, Role::Button, "Favorite Claude Opus 5.5");
    assert_eq!(star.toggled(), Some(Toggled::False));
    assert_eq!(star.description().as_deref(), Some("Add favorite"));
}

#[gpui::test]
fn a_search_reads_every_provider_names_a_failure_waits_for_the_slow_one_and_stays(
    cx: &mut TestAppContext,
) {
    let (_, vcx, _dir) = open(cx, "pickers-search");
    open_model_picker(vcx);
    vcx.simulate_input("gemini");
    vcx.run_until_parked();
    let found = options(vcx);
    assert!(
        found.contains(&"Gemini 3 Pro, OpenRouter".to_string()),
        "{found:?}"
    );
    let tree1 = tree(vcx);
    named(
        &nodes(&tree1),
        Role::Label,
        &format!("Groq: {CATALOGUE_FAILURE}"),
    );
    // Google's catalogue is slow: its models come when it answers.
    vcx.executor().advance_clock(SLOW);
    vcx.run_until_parked();
    let later = options(vcx);
    assert!(
        later.contains(&"Gemini 3 Pro, Google".to_string()),
        "{later:?}"
    );
    // Escape closes only the picker, focus goes back to its trigger, and
    // the search's text stays for the next opening.
    vcx.simulate_keystrokes("escape");
    vcx.executor().advance_clock(MODEL_EXIT);
    vcx.run_until_parked();
    let tree2 = tree(vcx);
    assert!(!has(&nodes(&tree2), Role::Dialog, "Chat model"));
    assert_eq!(focused(vcx), (Role::Button, Some(trigger_name())));
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    let tree3 = tree(vcx);
    let search = of_role(&nodes(&tree3), Role::TextInput)
        .into_iter()
        .find(|n| n.label().as_deref() == Some("Search models"))
        .expect("the search");
    assert_eq!(search.value().as_deref(), Some("gemini"));
}

#[gpui::test]
fn the_tabs_show_their_states(cx: &mut TestAppContext) {
    let (_, vcx, _dir) = open(cx, "pickers-tabs");
    open_model_picker(vcx);
    let tree1 = tree(vcx);
    click(vcx, named(&nodes(&tree1), Role::Tab, "OpenRouter"));
    // Without a search: the favourites, then the four vendors' models with
    // tools (the hidden Llama and the ones without tools never show).
    assert_eq!(
        options(vcx),
        [
            "Claude Opus 4.5",
            "GPT-5.5",
            "Grok 4 Fast",
            "Gemini 3 Pro",
            "GPT-5.5 Mini",
            "gpt-oss-120b",
            "Claude Sonnet 4.5",
            "Claude Haiku 4.5",
            "Grok 4",
            "Gemini 2.5 Flash",
        ]
    );
    let tree2 = tree(vcx);
    click(vcx, named(&nodes(&tree2), Role::Tab, "Groq"));
    let tree3 = tree(vcx);
    named(&nodes(&tree3), Role::Label, CATALOGUE_FAILURE);
    assert!(options(vcx).is_empty());
    let tree4 = tree(vcx);
    click(vcx, named(&nodes(&tree4), Role::Tab, "ChatGPT"));
    // Its saved order puts Codex first.
    assert_eq!(options(vcx), ["GPT-5.5 Codex", "GPT-5.5", "GPT-5.5 Mini"]);
    let tree5 = tree(vcx);
    click(vcx, named(&nodes(&tree5), Role::Tab, "Favorites"));
    // The favourites, provider by provider.
    assert_eq!(options(vcx), ["Claude Opus 5.5", "Claude Opus 4.5"]);
    // A disabled provider takes no press.
    let tree6 = tree(vcx);
    click(
        vcx,
        named(&nodes(&tree6), Role::Tab, "OpenAI \u{2014} not configured"),
    );
    let tree7 = tree(vcx);
    assert_eq!(
        named(&nodes(&tree7), Role::Tab, "Favorites").is_selected(),
        Some(true)
    );
}

#[gpui::test]
fn the_slow_tab_says_it_is_loading_until_it_answers(cx: &mut TestAppContext) {
    let (_, vcx, _dir) = open(cx, "pickers-slow");
    open_model_picker(vcx);
    let tree1 = tree(vcx);
    click(vcx, named(&nodes(&tree1), Role::Tab, "Google"));
    let tree2 = tree(vcx);
    assert!(has(&nodes(&tree2), Role::Label, "Loading models..."));
    vcx.executor().advance_clock(SLOW);
    vcx.run_until_parked();
    assert_eq!(options(vcx), ["Gemini 3 Pro", "Gemini 2.5 Flash"]);
    let tree3 = tree(vcx);
    assert!(!has(&nodes(&tree3), Role::Label, "Loading models..."));
}

#[gpui::test]
fn arrows_and_enter_choose_a_model_which_resets_the_effort_and_is_remembered(
    cx: &mut TestAppContext,
) {
    let (pickers, vcx, dir) = open(cx, "pickers-keys");
    open_model_picker(vcx);
    vcx.simulate_keystrokes("down");
    assert_eq!(
        focused(vcx),
        (Role::ListBoxOption, Some("Claude Opus 5.5".into()))
    );
    vcx.simulate_keystrokes("down");
    assert_eq!(
        focused(vcx),
        (Role::ListBoxOption, Some("Claude Haiku 5.5".into()))
    );
    vcx.simulate_keystrokes("up up");
    assert_eq!(
        focused(vcx),
        (Role::TextInput, Some("Search models".into())),
        "Up from the first model goes back to the search"
    );
    vcx.simulate_keystrokes("down down enter");
    vcx.executor().advance_clock(MODEL_EXIT);
    vcx.run_until_parked();
    let choice = vcx.update(|_, cx| pickers.read(cx).footer().read(cx).choice().clone());
    assert_eq!(
        (
            choice.provider.as_str(),
            choice.model.as_str(),
            choice.effort
        ),
        ("claude-code", "claude-haiku-5-5", None)
    );
    let tree1 = tree(vcx);
    named(&nodes(&tree1), Role::Button, "Chat model: claude-haiku-5-5");
    assert_eq!(
        focused(vcx),
        (Role::Button, Some("Chat model: claude-haiku-5-5".into()))
    );
    let remembered = logged(&dir, "remembered");
    assert_eq!(
        remembered.last().expect("remembered")["model"],
        serde_json::json!(["claude-code", "claude-haiku-5-5"])
    );
    // Typing on a model goes on in the search.
    open_model_picker_named(vcx, "Chat model: claude-haiku-5-5");
    vcx.simulate_keystrokes("down");
    vcx.simulate_keystrokes("o");
    assert_eq!(
        focused(vcx),
        (Role::TextInput, Some("Search models".into()))
    );
    let tree2 = tree(vcx);
    let search = of_role(&nodes(&tree2), Role::TextInput)
        .into_iter()
        .find(|n| n.label().as_deref() == Some("Search models"))
        .expect("the search");
    assert_eq!(search.value().as_deref(), Some("o"));
}

fn open_model_picker_named(vcx: &mut VisualTestContext, name: &str) {
    let tree = tree(vcx);
    click(vcx, named(&nodes(&tree), Role::Button, name));
}

#[gpui::test]
fn a_star_shows_at_once_and_one_that_cannot_be_saved_goes_back_and_says_so(
    cx: &mut TestAppContext,
) {
    let (pickers, vcx, dir) = open(cx, "pickers-star");
    open_model_picker(vcx);
    let tree1 = tree(vcx);
    click(
        vcx,
        named(&nodes(&tree1), Role::Button, "Favorite Claude Haiku 5.5"),
    );
    let tree2 = tree(vcx);
    let star = named(&nodes(&tree2), Role::Button, "Favorite Claude Haiku 5.5");
    assert_eq!(star.toggled(), Some(Toggled::True));
    assert_eq!(star.description().as_deref(), Some("Remove favorite"));
    assert!(
        has(&nodes(&tree2), Role::Dialog, "Chat model"),
        "the picker stays open"
    );
    let kept = vcx.update(|_, cx| pickers.read(cx).catalog().read(cx).favorites());
    assert!(kept.contains(&("claude-code".to_string(), "claude-haiku-5-5".to_string())));
    // The stand-in cannot star Grok 4 Fast: the star goes back, and a notice
    // says so.
    let tree3 = tree(vcx);
    click(vcx, named(&nodes(&tree3), Role::Tab, "OpenRouter"));
    let tree4 = tree(vcx);
    click(
        vcx,
        named(&nodes(&tree4), Role::Button, "Favorite Grok 4 Fast"),
    );
    vcx.run_until_parked();
    let tree5 = tree(vcx);
    assert_eq!(
        named(&nodes(&tree5), Role::Button, "Favorite Grok 4 Fast").toggled(),
        Some(Toggled::False)
    );
    let notices: Vec<String> = logged(&dir, "notice")
        .iter()
        .map(|n| n["text"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(notices, [STAR_FAILURE]);
}

#[gpui::test]
fn the_rails_set_up_and_the_footer_leave_the_picker(cx: &mut TestAppContext) {
    let (_, vcx, dir) = open(cx, "pickers-leave");
    open_model_picker(vcx);
    let tree1 = tree(vcx);
    click(vcx, named(&nodes(&tree1), Role::Button, "Set up providers"));
    assert_eq!(logged(&dir, "opened").len(), 1, "Settings opens");
    vcx.executor().advance_clock(MODEL_EXIT);
    vcx.run_until_parked();
    let tree2 = tree(vcx);
    assert!(!has(&nodes(&tree2), Role::Dialog, "Chat model"));
    open_model_picker(vcx);
    let tree3 = tree(vcx);
    click(
        vcx,
        named(&nodes(&tree3), Role::Button, "Compare several in Bench"),
    );
    assert_eq!(logged(&dir, "bench").len(), 1);
}

#[gpui::test]
fn the_effort_picker_lists_the_models_efforts_and_sends_the_chosen_one(cx: &mut TestAppContext) {
    let (pickers, vcx, _dir) = open(cx, "pickers-effort");
    let tree1 = tree(vcx);
    click(
        vcx,
        named(&nodes(&tree1), Role::Button, "Reasoning effort: medium"),
    );
    let tree2 = tree(vcx);
    let nodes2 = nodes(&tree2);
    named(&nodes2, Role::ListBox, "Reasoning effort");
    let efforts: Vec<String> = of_role(&nodes2, Role::ListBoxOption)
        .iter()
        .filter(|n| {
            n.parent()
                .is_some_and(|p| p.label().as_deref() == Some("Reasoning effort"))
        })
        .map(|n| n.label().unwrap_or_default())
        .collect();
    assert_eq!(efforts, ["low", "medium", "high"]);
    assert_eq!(
        named(&nodes2, Role::ListBoxOption, "medium").is_selected(),
        Some(true)
    );
    assert_eq!(focused(vcx), (Role::ListBoxOption, Some("medium".into())));
    vcx.simulate_keystrokes("down");
    assert_eq!(focused(vcx), (Role::ListBoxOption, Some("high".into())));
    vcx.simulate_keystrokes("down");
    assert_eq!(
        focused(vcx),
        (Role::ListBoxOption, Some("low".into())),
        "it wraps"
    );
    vcx.simulate_keystrokes("up enter");
    vcx.executor().advance_clock(EFFORT_EXIT);
    vcx.run_until_parked();
    let tree3 = tree(vcx);
    named(&nodes(&tree3), Role::Button, "Reasoning effort: high");
    let (effort, sent) = vcx.update(|_, cx| {
        let footer = pickers.read(cx).footer();
        let footer = footer.read(cx);
        (footer.choice().effort.clone(), footer.effort_to_send(cx))
    });
    assert_eq!(effort.as_deref(), Some("high"));
    assert_eq!(sent.as_deref(), Some("high"));
}

#[gpui::test]
fn the_resolution_slider_steps_through_the_four_stops_and_stays_open(cx: &mut TestAppContext) {
    let (pickers, vcx, dir) = open(cx, "pickers-resolution");
    let tree1 = tree(vcx);
    click(
        vcx,
        named(&nodes(&tree1), Role::Button, "Resolution: Standard"),
    );
    let tree2 = tree(vcx);
    named(&nodes(&tree2), Role::Dialog, "Resolution");
    let slider = named(&nodes(&tree2), Role::Slider, "Resolution");
    assert_eq!(slider.numeric_value(), Some(0.));
    assert_eq!(slider.value().as_deref(), Some("Standard"));
    assert_eq!(
        slider.description().as_deref(),
        Some("The model chooses the size.")
    );
    assert_eq!(focused(vcx), (Role::Slider, Some("Resolution".into())));
    vcx.simulate_keystrokes("right");
    let tree3 = tree(vcx);
    let slider = named(&nodes(&tree3), Role::Slider, "Resolution");
    assert_eq!(slider.value().as_deref(), Some("High (2\u{d7})"));
    named(&nodes(&tree3), Role::Button, "Resolution: High (2\u{d7})");
    vcx.simulate_keystrokes("end");
    let tree4 = tree(vcx);
    assert_eq!(
        named(&nodes(&tree4), Role::Slider, "Resolution").numeric_value(),
        Some(3.)
    );
    // A reader's decrement, and a tick.
    act(
        vcx,
        named(&nodes(&tree4), Role::Slider, "Resolution"),
        Action::Decrement,
    );
    let tree5 = tree(vcx);
    assert_eq!(
        named(&nodes(&tree5), Role::Slider, "Resolution")
            .value()
            .as_deref(),
        Some("Very high (5\u{d7})")
    );
    click(vcx, named(&nodes(&tree5), Role::Button, "Standard"));
    let tree6 = tree(vcx);
    assert!(
        has(&nodes(&tree6), Role::Dialog, "Resolution"),
        "it stays open"
    );
    let resolution = vcx.update(|_, cx| pickers.read(cx).footer().read(cx).choice().resolution);
    assert_eq!(resolution, mcbench_ui::Resolution::Standard);
    let remembered: Vec<serde_json::Value> = logged(&dir, "remembered")
        .iter()
        .map(|l| l["resolution"].clone())
        .collect();
    assert_eq!(remembered, [2, 10, 5, 1].map(|n| serde_json::json!(n)));
    vcx.simulate_keystrokes("escape");
    vcx.executor().advance_clock(RESOLUTION_EXIT);
    vcx.run_until_parked();
    assert_eq!(
        focused(vcx),
        (Role::Button, Some("Resolution: Standard".into()))
    );
}

#[gpui::test]
fn with_no_ai_connected_the_picker_offers_connect_an_ai(cx: &mut TestAppContext) {
    let (_, vcx, dir) = open(cx, "pickers-no-ai");
    let tree1 = tree(vcx);
    click(
        vcx,
        named(&nodes(&tree1), Role::Button, "Choose chat model"),
    );
    let tree2 = tree(vcx);
    named(&nodes(&tree2), Role::Label, "No AI connected yet");
    assert!(
        !of_role(&nodes(&tree2), Role::TextInput)
            .iter()
            .any(|n| n.label().as_deref() == Some("Search models")),
        "no search"
    );
    click(vcx, named(&nodes(&tree2), Role::Button, "Connect an AI"));
    assert_eq!(logged(&dir, "opened").len(), 1);
}

#[gpui::test]
fn a_catalogues_error_wraps_inside_the_list_column_and_can_be_copied(cx: &mut TestAppContext) {
    // The error once ran left over the provider rail on one line. It wraps
    // inside the list column, right of the rail, and is selectable text.
    let (_, vcx, _dir) = open(cx, "pickers-error-wraps");
    open_model_picker(vcx);
    vcx.simulate_input("gemini");
    vcx.run_until_parked();
    let error = format!("Groq: {CATALOGUE_FAILURE}");
    let tree1 = tree(vcx);
    let n1 = nodes(&tree1);
    let note = named(&n1, Role::Label, &error);
    let rail = named(&n1, Role::TabList, "Providers")
        .bounding_box()
        .expect("the rail's bounds");
    let dialog = named(&n1, Role::Dialog, "Chat model")
        .bounding_box()
        .expect("the dialog's bounds");
    let text = note.bounding_box().expect("the note's bounds");
    assert!(
        text.x0 >= rail.x1 && text.x1 <= dialog.x1,
        "inside the list column: {text:?}, rail {rail:?}, dialog {dialog:?}"
    );
    let scale = f64::from(vcx.update(|window, _| window.scale_factor()));
    assert!(
        (text.y1 - text.y0) / scale > 2. * 12.5 * 1.2,
        "wrapped onto more than one line: {text:?}"
    );
    // A press on it, then select all and copy.
    pointer_click(vcx, note);
    let (all, copy) = if cfg!(target_os = "macos") {
        ("cmd-a", "cmd-c")
    } else {
        ("ctrl-a", "ctrl-c")
    };
    vcx.simulate_keystrokes(all);
    vcx.simulate_keystrokes(copy);
    vcx.run_until_parked();
    let copied = vcx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default();
    assert_eq!(copied, error);
}

// par-shell (parity pass, `components.md` D.1 and D.3): the pickers'
// `title` tooltips through the shared tooltip, and the rail's divider.

/// The tooltip shown after hovering `node`'s centre for 600 ms.
fn hover_tip(vcx: &mut VisualTestContext, bounds: gpui::accesskit::Rect) -> Vec<String> {
    let centre = centre_of(vcx, bounds);
    vcx.simulate_mouse_move(centre, None, gpui::Modifiers::none());
    let _ = tree(vcx);
    vcx.executor()
        .advance_clock(std::time::Duration::from_millis(600));
    vcx.run_until_parked();
    let tree = tree(vcx);
    let tips = of_role(&nodes(&tree), Role::Tooltip)
        .iter()
        .filter_map(|t| t.label())
        .collect();
    vcx.simulate_mouse_move(
        gpui::point(gpui::px(1.), gpui::px(1.)),
        None,
        gpui::Modifiers::none(),
    );
    vcx.run_until_parked();
    tips
}

#[gpui::test]
fn the_pickers_titles_show_on_hover_and_the_harness_divider_is_a_separator(
    cx: &mut TestAppContext,
) {
    let (_, vcx, _dir) = open(cx, "pickers-tips");
    let bounds = |vcx: &mut VisualTestContext, role: Role, name: &str| {
        let tree = tree(vcx);
        named(&nodes(&tree), role, name)
            .bounding_box()
            .expect("bounds")
    };
    let trigger = bounds(vcx, Role::Button, &trigger_name());
    assert_eq!(
        hover_tip(vcx, trigger),
        [MODEL],
        "the trigger's full model id"
    );
    let resolution = bounds(vcx, Role::Button, "Resolution: Standard");
    assert_eq!(hover_tip(vcx, resolution), ["Resolution: Standard"]);
    open_model_picker(vcx);
    // A row: its model's id; its star: what a press does.
    let (row, star, star_name) = {
        let tree = tree(vcx);
        let all = nodes(&tree);
        let option = of_role(&all, Role::ListBoxOption)[0];
        let star = option
            .children()
            .find(|c| c.role() == Role::Button)
            .expect("the row's star");
        (
            option.bounding_box().expect("bounds"),
            star.bounding_box().expect("bounds"),
            star.description().expect("the star's title"),
        )
    };
    let row_left = gpui::accesskit::Rect {
        x1: row.x0 + 60.,
        ..row
    };
    let row_tips = hover_tip(vcx, row_left);
    assert_eq!(row_tips.len(), 1, "{row_tips:?}");
    assert!(!row_tips[0].contains(' '), "a model id: {row_tips:?}");
    assert_eq!(hover_tip(vcx, star), [star_name], "one tip over the star");
    // Every rail tab's title, and the divider before the harnesses.
    let tabs: Vec<(String, gpui::accesskit::Rect)> = {
        let tree = tree(vcx);
        of_role(&nodes(&tree), Role::Tab)
            .into_iter()
            .filter(|t| {
                t.parent()
                    .is_some_and(|p| p.label().as_deref() == Some("Providers"))
            })
            .filter_map(|t| Some((t.label()?, t.bounding_box()?)))
            .collect()
    };
    assert!(tabs.len() > 2);
    for (name, b) in tabs {
        assert_eq!(
            hover_tip(vcx, b),
            std::slice::from_ref(&name),
            "the tab {name:?}"
        );
    }
    let tree = tree(vcx);
    let dividers: Vec<_> = of_role(&nodes(&tree), Role::Splitter)
        .into_iter()
        .filter(|d| d.label().as_deref() == Some("Harnesses"))
        .collect();
    assert_eq!(dividers.len(), 1, "one Harnesses separator");
}
