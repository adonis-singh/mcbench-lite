//! Bench's places and the prompt library in the window, over the stand-in's
//! `bench` fixture (bench agent): the comparison's rounds and rows, a vote
//! cast and announced, the round switch, Reveal, a member's read-only chat
//! and its fork, the new bench from two models to its comparison, the
//! leaderboard, the palette over the composer, the editor's edits reaching
//! the core after 300 ms, and an idle comparison drawing nothing.

mod common;

use std::time::Duration;

use common::*;
use gpui::accesskit::{Role, Toggled};
use gpui::{TestAppContext, VisualTestContext};
use mcbench_ui::dev::standin::BenchPlay;
use mcbench_ui::state::place::Place;

fn settle(shell: &ShellUnderTest, vcx: &mut VisualTestContext) {
    pump(shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(400));
    pump(shell, vcx);
}

fn labels(vcx: &mut VisualTestContext, role: Role) -> Vec<String> {
    let tree = tree(vcx);
    of_role(&nodes(&tree), role)
        .iter()
        .filter_map(|n| n.label())
        .collect()
}

fn press(shell: &ShellUnderTest, vcx: &mut VisualTestContext, role: Role, label: &str) {
    let tree = tree(vcx);
    click(vcx, named(&nodes(&tree), role, label));
    settle(shell, vcx);
}

fn castle() -> Option<Place> {
    Some(Place::Compare {
        bench: "bench-castle".into(),
    })
}

#[gpui::test]
fn a_comparison_lists_each_round_and_one_row_per_model_in_the_bench_s_order(
    cx: &mut TestAppContext,
) {
    let (shell, _core, vcx) = open_bench(cx, "bench-compare", castle(), &[]);
    settle(&shell, vcx);
    let lists = labels(vcx, Role::List);
    for n in 1..=3 {
        assert!(lists.contains(&format!("Round {n}")), "{lists:?}");
    }
    let rows = labels(vcx, Role::ListItem);
    // Blind: letters, in the order the letters run.
    assert!(rows.iter().any(|r| r.starts_with("Build A, ")), "{rows:?}");
    assert!(
        rows.iter().any(|r| r.contains("Working")),
        "the running model"
    );
    assert!(rows.iter().any(|r| r.contains("Failed")));
    assert!(rows.iter().any(|r| r.contains("Stopped")));
    // The hidden names hide time and tokens too.
    assert!(!rows.iter().any(|r| r.contains(" tok")));
    // The round switch, Reveal.
    let buttons = labels(vcx, Role::Button);
    assert!(buttons.contains(&"Round 1".to_string()));
    assert!(buttons.contains(&"Reveal models".to_string()));
    assert!(buttons.iter().any(|b| b.starts_with("Retry Build")));
    assert!(buttons.iter().any(|b| b.starts_with("Stop Build")));
}

#[gpui::test]
fn a_vote_reached_by_tab_is_scrolled_into_view(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-vote-tab", castle(), &[]);
    // Short enough that the comparison, opened at its last round, hides
    // the first rounds' votes.
    vcx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(420.)));
    settle(&shell, vcx);
    // Scrolled to the end, as a person reading the last round has.
    let tree0 = tree(vcx);
    let region = named(
        &nodes(&tree0),
        Role::Region,
        "Castle with a moat comparison",
    );
    let centre = centre_of(vcx, region.bounding_box().expect("bounds"));
    vcx.simulate_event(gpui::ScrollEvent {
        position: centre,
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-5000.))),
        ..Default::default()
    });
    settle(&shell, vcx);
    let mut reached = None;
    for _ in 0..120 {
        vcx.simulate_keystrokes("tab");
        settle(&shell, vcx);
        let (role, label) = focused(vcx);
        if role == Role::Button
            && label
                .as_deref()
                .is_some_and(|l| l.starts_with("Pick Build"))
        {
            reached = label;
            break;
        }
    }
    let label = reached.expect("Tab reaches a vote");
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    let all = nodes(&tree1);
    let region = named(&all, Role::Region, "Castle with a moat comparison")
        .bounding_box()
        .expect("bounds");
    let vote = tree1
        .state()
        .focus()
        .expect("focus")
        .bounding_box()
        .expect("bounds");
    assert!(
        vote.y0 >= region.y0 && vote.y1 <= region.y1,
        "{label} at {vote:?} outside {region:?}"
    );
}

#[gpui::test]
fn a_vote_is_cast_shown_pressed_announced_and_taken_back(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-vote", castle(), &[]);
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    let pick = of_role(&nodes(&tree1), Role::Button)
        .into_iter()
        .find(|n| {
            n.label()
                .is_some_and(|l| l.starts_with("Pick Build") && l.ends_with("as the best build"))
        })
        .expect("a vote button on round 2");
    let label = pick.label().expect("named");
    let name = label
        .trim_start_matches("Pick ")
        .trim_end_matches(" as the best build")
        .to_string();
    click(vcx, pick);
    settle(&shell, vcx);
    let tree2 = tree(vcx);
    let taken = named(
        &nodes(&tree2),
        Role::Button,
        &format!("Take back the pick of {name}"),
    );
    assert_eq!(taken.toggled(), Some(Toggled::True));
    // Said without the person reading it again.
    let status = labels(vcx, Role::Status);
    assert!(
        status
            .iter()
            .any(|s| s == &format!("Your pick for this round: {name}")),
        "{status:?}"
    );
    press(
        &shell,
        vcx,
        Role::Button,
        &format!("Take back the pick of {name}"),
    );
    assert!(
        labels(vcx, Role::Status)
            .iter()
            .any(|s| s == "Vote taken back")
    );
}

#[gpui::test]
fn a_refused_vote_says_so_and_changes_nothing(cx: &mut TestAppContext) {
    let (shell, _core, vcx) =
        open_bench(cx, "bench-vote-refused", castle(), &[BenchPlay::VoteFails]);
    settle(&shell, vcx);
    let picks = |vcx: &mut VisualTestContext| {
        labels(vcx, Role::Button)
            .iter()
            .filter(|b| b.starts_with("Take back the pick"))
            .count()
    };
    let before = picks(vcx);
    let tree1 = tree(vcx);
    let pick = of_role(&nodes(&tree1), Role::Button)
        .into_iter()
        .find(|n| n.label().is_some_and(|l| l.starts_with("Pick Build")))
        .expect("a vote button");
    click(vcx, pick);
    settle(&shell, vcx);
    assert!(
        labels(vcx, Role::Article)
            .iter()
            .any(|a| a.contains(mcbench_ui::dev::standin::VOTE_REFUSED))
    );
    assert_eq!(picks(vcx), before, "the votes as they were");
}

#[gpui::test]
fn the_round_switch_shows_another_round_and_reveal_shows_the_names(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-switch", castle(), &[]);
    settle(&shell, vcx);
    let pane = vcx.update(|_, cx| shell.entity.read(cx).bench_pane());
    assert_eq!(
        vcx.update(|_, cx| pane.read(cx).round_of("bench-castle")),
        3
    );
    press(&shell, vcx, Role::Button, "Round 1");
    assert_eq!(
        vcx.update(|_, cx| pane.read(cx).round_of("bench-castle")),
        1
    );
    // Reveal is a confirm: armed, then for good.
    press(&shell, vcx, Role::Button, "Reveal models");
    press(&shell, vcx, Role::Button, "Reveal for good");
    let rows = labels(vcx, Role::ListItem);
    assert!(
        rows.iter().any(|r| r.starts_with("claude-opus-5-5, ")),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|r| r.starts_with("gpt-5.5, ") && r.contains(", high")),
        "{rows:?}"
    );
    assert!(!labels(vcx, Role::Button).contains(&"Reveal models".to_string()));
}

#[gpui::test]
fn a_member_s_chat_reads_only_and_forks_to_build(cx: &mut TestAppContext) {
    let start = Some(Place::Member {
        bench: "bench-houses".into(),
        member: "house-1".into(),
    });
    let (shell, _core, vcx) = open_bench(cx, "bench-member", start, &[]);
    settle(&shell, vcx);
    assert!(
        !labels(vcx, Role::MultilineTextInput).contains(&"Message".to_string()),
        "a member's chat has no composer"
    );
    assert!(
        labels(vcx, Role::Button).contains(&"Village houses".to_string()),
        "the breadcrumb"
    );
    press(&shell, vcx, Role::Button, "Fork to Build");
    settle(&shell, vcx);
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert!(matches!(place, Place::Chat { .. }), "{place:?}");
}

#[gpui::test]
fn a_new_bench_runs_on_two_models_and_opens_its_comparison(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-new", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    assert!(labels(vcx, Role::Region).contains(&"New bench".to_string()));
    let send = |vcx: &mut VisualTestContext| {
        let tree = tree(vcx);
        named(&nodes(&tree), Role::Button, "Run on 0 models").is_disabled()
    };
    assert!(send(vcx));
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|_, cx| {
        chat.update(cx, |c, cx| {
            c.toggle_member("anthropic", "claude-opus-5-5", cx);
            c.toggle_member("openai", "gpt-5.5", cx);
        })
    });
    settle(&shell, vcx);
    assert!(labels(vcx, Role::Button).contains(&"Remove claude-opus-5-5".to_string()));
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.focus_composer(window, cx)));
    vcx.simulate_input("A lighthouse on a cliff");
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Run on 2 models");
    settle(&shell, vcx);
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert!(
        matches!(&place, Place::Compare { bench } if bench.starts_with("bench-new-")),
        "{place:?}"
    );
    assert!(labels(vcx, Role::List).contains(&"Round 1".to_string()));
}

#[gpui::test]
fn a_bench_that_cannot_start_gives_the_message_back(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(
        cx,
        "bench-refused",
        Some(Place::NewBench),
        &[BenchPlay::CreateRefused],
    );
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|_, cx| {
        chat.update(cx, |c, cx| {
            c.toggle_member("anthropic", "claude-opus-5-5", cx);
            c.toggle_member("openai", "gpt-5.5", cx);
        })
    });
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.focus_composer(window, cx)));
    vcx.simulate_input("A tower");
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Run on 2 models");
    settle(&shell, vcx);
    let text = vcx.update(|_, cx| chat.read(cx).current_draft(cx).text);
    assert_eq!(text, "A tower", "the draft went back");
    assert!(
        labels(vcx, Role::Article)
            .iter()
            .any(|a| a.contains(mcbench_ui::dev::standin::CREATE_REFUSED))
    );
}

#[gpui::test]
fn the_leaderboard_is_a_table_of_the_ranked_models(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-board", Some(Place::Leaderboard), &[]);
    settle(&shell, vcx);
    assert!(labels(vcx, Role::Table).contains(&"Models ranked by blind votes".to_string()));
    let rows = labels(vcx, Role::Row);
    assert_eq!(rows.len(), 7, "the head and six models: {rows:?}");
    assert!(rows[1].starts_with("1, gemini-3-pro, rating 1068"));
}

#[gpui::test]
fn an_empty_leaderboard_says_so(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(
        cx,
        "bench-board-empty",
        Some(Place::Leaderboard),
        &[BenchPlay::LeaderboardEmpty],
    );
    settle(&shell, vcx);
    assert!(labels(vcx, Role::Region).contains(&"No blind votes yet".to_string()));
}

#[gpui::test]
fn the_palette_opens_on_a_slash_and_enter_inserts_the_highlighted_prompt(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-palette", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.focus_composer(window, cx)));
    vcx.simulate_input("/");
    settle(&shell, vcx);
    assert!(labels(vcx, Role::Dialog).contains(&"Prompts".to_string()));
    let options = labels(vcx, Role::ListBoxOption);
    // Favorites first, then the recent ones.
    assert_eq!(options[0], "Castle with a moat, favorite");
    assert_eq!(options[1], "Wheat field");
    vcx.simulate_keystrokes("down");
    settle(&shell, vcx);
    vcx.simulate_keystrokes("enter");
    settle(&shell, vcx);

    assert!(!labels(vcx, Role::Dialog).contains(&"Prompts".to_string()));
    let text = vcx.update(|_, cx| chat.read(cx).current_draft(cx).text);
    assert_eq!(text, "A small wheat farm with water in the middle.");
}

fn library_prompt(
    shell: &ShellUnderTest,
    vcx: &mut VisualTestContext,
    id: &str,
) -> mcbench_ui::LibraryPrompt {
    let library = vcx.update(|_, cx| shell.entity.read(cx).library());
    vcx.update(|_, cx| {
        library
            .read(cx)
            .list()
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .expect("the prompt")
    })
}

// `test/composer.test.js`, by the same names, on the composer itself.
#[gpui::test]
fn a_prompt_arrives_as_a_token_or_as_its_text_with_boxes_that_name_the_prompt_they_came_from(
    cx: &mut TestAppContext,
) {
    use mcbench_ui::state::prompts::Segment;
    let (shell, _core, vcx) = open_bench(cx, "bench-arrives", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    let (farm, castle) = (
        library_prompt(&shell, vcx, "prompt-farm"),
        library_prompt(&shell, vcx, "prompt-castle"),
    );
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.put_prompt(&farm, false, window, cx)));
    settle(&shell, vcx);
    let segments = vcx.update(|_, cx| chat.read(cx).composer_segments(cx));
    assert_eq!(
        segments,
        vec![Segment::Prompt {
            id: farm.id.clone(),
            title: farm.title.clone(),
            text: farm.text.clone()
        }]
    );
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.put_prompt(&castle, true, window, cx)));
    settle(&shell, vcx);
    let segments = vcx.update(|_, cx| chat.read(cx).composer_segments(cx));
    let from = mcbench_ui::PromptLink {
        id: castle.id.clone(),
        title: castle.title.clone(),
    };
    assert_eq!(
        segments,
        vec![
            Segment::Text("A castle with four corner towers and a ".into()),
            Segment::Field {
                name: "width".into(),
                value: String::new(),
                from
            },
            Segment::Text(" moat.".into()),
        ]
    );
}

#[gpui::test]
fn a_message_names_each_prompt_it_was_inserted_from_once_while_its_token_or_any_of_its_boxes_is_in_it(
    cx: &mut TestAppContext,
) {
    let (shell, _core, vcx) = open_bench(cx, "bench-names", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    let castle = library_prompt(&shell, vcx, "prompt-castle");
    let names = |vcx: &mut VisualTestContext| {
        vcx.update(|_, cx| chat.update(cx, |c, cx| c.live_prompts(cx)))
            .into_iter()
            .map(|l| l.id)
            .collect::<Vec<_>>()
    };
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.put_prompt(&castle, false, window, cx)));
    settle(&shell, vcx);
    vcx.simulate_input("wide");
    settle(&shell, vcx);
    assert_eq!(names(vcx), vec!["prompt-castle".to_string()]);
    // The text around the box edited: still the prompt.
    let composer = vcx.update(|_, cx| chat.read(cx).composer());
    vcx.update(|window, cx| {
        composer.update(cx, |c, cx| {
            c.set_selected_range(0..1, cx);
            c.replace("Please build a", window, cx);
        })
    });
    settle(&shell, vcx);
    assert_eq!(names(vcx), vec!["prompt-castle".to_string()]);
    // The box gone, its text alone is no longer the prompt.
    vcx.update(|window, cx| {
        composer.update(cx, |c, cx| {
            let len = c.value().len();
            c.set_selected_range(0..len, cx);
            c.replace("Build a wide moat.", window, cx);
        })
    });
    settle(&shell, vcx);
    assert!(names(vcx).is_empty());
}

#[gpui::test]
fn a_prompt_with_fill_ins_holds_the_send_until_they_are_filled(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-fill-ins", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    let castle = library_prompt(&shell, vcx, "prompt-castle");
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.put_prompt(&castle, false, window, cx)));
    settle(&shell, vcx);
    // Entering the box says its name.
    assert!(
        labels(vcx, Role::Status)
            .iter()
            .any(|s| s == "Fill in: width")
    );
    // A send with the box empty goes nowhere and says so.
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.submit(window, cx)));
    settle(&shell, vcx);
    assert!(
        labels(vcx, Role::Alert)
            .iter()
            .any(|s| s == "Fill in this part before sending")
    );
    // The caret is in the box: typing fills it.
    vcx.simulate_input("wide");
    settle(&shell, vcx);
    let text = vcx.update(|_, cx| chat.read(cx).current_draft(cx).text);
    assert_eq!(text, "A castle with four corner towers and a wide moat.");
    // Right at its end steps out; Tab comes back into it.
    vcx.simulate_keystrokes("right");
    let active = |vcx: &mut VisualTestContext| {
        vcx.update(|_, cx| chat.read(cx).composer().read(cx).active_field())
    };
    assert_eq!(active(vcx), None);
    vcx.simulate_keystrokes("tab");
    assert_eq!(active(vcx), Some(0));
}

#[gpui::test]
fn the_editor_s_edits_reach_the_core_after_300_ms(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_bench(cx, "bench-editor", Some(Place::Prompt), &[]);
    settle(&shell, vcx);
    assert!(labels(vcx, Role::Region).contains(&"Prompts".to_string()));
    let library = vcx.update(|_, cx| shell.entity.read(cx).library());
    vcx.update(|_, cx| library.update(cx, |l, cx| l.toggle_favorite("prompt-tower", cx)));
    pump(&shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(350));
    pump(&shell, vcx);
    let saved = core.library_for_test();
    assert!(
        saved
            .iter()
            .find(|p| p.id == "prompt-tower")
            .expect("kept")
            .favorite
    );
}

#[gpui::test]
fn a_failed_save_keeps_the_edit_and_says_why(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_bench(
        cx,
        "bench-editor-fails",
        Some(Place::Prompt),
        &[BenchPlay::SaveFails],
    );
    settle(&shell, vcx);
    let library = vcx.update(|_, cx| shell.entity.read(cx).library());
    vcx.update(|_, cx| library.update(cx, |l, cx| l.toggle_favorite("prompt-tower", cx)));
    pump(&shell, vcx);
    vcx.executor().advance_clock(Duration::from_millis(350));
    pump(&shell, vcx);
    assert!(
        labels(vcx, Role::Article)
            .iter()
            .any(|a| a.contains(mcbench_ui::dev::standin::SAVE_FAILED))
    );
    let shown = vcx.update(|_, cx| {
        library
            .read(cx)
            .list()
            .iter()
            .find(|p| p.id == "prompt-tower")
            .expect("kept")
            .favorite
    });
    assert!(shown, "the edit stays on show, unsent");
    assert!(
        !core
            .library_for_test()
            .iter()
            .find(|p| p.id == "prompt-tower")
            .expect("kept")
            .favorite
    );
}

#[gpui::test]
fn an_idle_comparison_draws_nothing(cx: &mut TestAppContext) {
    let start = Some(Place::Compare {
        bench: "bench-houses".into(),
    });
    let (shell, _core, vcx) = open_bench(cx, "bench-idle", start, &[]);
    settle(&shell, vcx);
    let pane = vcx.update(|_, cx| shell.entity.read(cx).bench_pane());
    let before = vcx.update(|_, cx| pane.read(cx).render_count());
    vcx.executor().advance_clock(Duration::from_secs(5));
    vcx.run_until_parked();
    let after = vcx.update(|_, cx| pane.read(cx).render_count());
    assert_eq!(before, after);
}

#[gpui::test]
fn the_sidebar_s_rows_star_and_delete_a_prompt_and_new_prompt_opens_an_empty_one(
    cx: &mut TestAppContext,
) {
    let (shell, _core, vcx) = open_bench(cx, "bench-prompt-rows", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let library = vcx.update(|_, cx| shell.entity.read(cx).library());
    let count = |vcx: &mut VisualTestContext| vcx.update(|_, cx| library.read(cx).list().len());
    let before = count(vcx);
    let stars = labels(vcx, Role::Button)
        .iter()
        .filter(|b| *b == "Favorite prompt")
        .count();
    let tree0 = tree(vcx);
    let star = of_role(&nodes(&tree0), Role::Button)
        .into_iter()
        .find(|n| n.label().as_deref() == Some("Favorite prompt"))
        .expect("a star");
    click(vcx, star);
    settle(&shell, vcx);
    let after = labels(vcx, Role::Button)
        .iter()
        .filter(|b| *b == "Favorite prompt")
        .count();
    assert_eq!(after, stars - 1, "one more favourite");
    // Delete asks first.
    let tree1 = tree(vcx);
    let delete = of_role(&nodes(&tree1), Role::Button)
        .into_iter()
        .find(|n| n.label().as_deref() == Some("Delete prompt"))
        .expect("a delete");
    click(vcx, delete);
    settle(&shell, vcx);
    assert_eq!(count(vcx), before, "not yet");
    press(&shell, vcx, Role::Button, "Confirm delete");
    assert_eq!(count(vcx), before - 1);
    press(&shell, vcx, Role::Button, "New prompt");
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert_eq!(place, Place::Prompt);
    assert!(labels(vcx, Role::Region).contains(&"Prompt Untitled prompt".to_string()));
}

#[gpui::test]
fn every_prompt_row_shows_its_title_at_the_default_and_the_narrowest_sidebar(
    cx: &mut TestAppContext,
) {
    const TITLES: [&str; 6] = [
        "prompt-title-0",
        "prompt-title-1",
        "prompt-title-2",
        "prompt-title-3",
        "prompt-title-4",
        "prompt-title-5",
    ];
    for width in [
        mcbench_ui::state::layout::SIDEBAR_WIDTH,
        mcbench_ui::state::layout::SIDEBAR_MIN,
    ] {
        let (shell, _core, vcx) = open_bench_at(
            cx,
            &format!("bench-rows-{width}"),
            Some(Place::NewBench),
            &[],
            width,
        );
        settle(&shell, vcx);
        let library = vcx.update(|_, cx| shell.entity.read(cx).library());
        let titles: Vec<String> = vcx.update(|_, cx| {
            library
                .read(cx)
                .list()
                .iter()
                .map(|p| mcbench_ui::state::prompts::title_or_untitled(&p.title).to_string())
                .collect()
        });
        let tree1 = tree(vcx);
        let list = named(&nodes(&tree1), Role::List, "Prompts");
        let rows: Vec<String> = list
            .children()
            .filter(|n| n.role() == Role::ListItem)
            .filter_map(|n| n.label())
            .collect();
        assert_eq!(rows.len(), titles.len(), "{rows:?}");
        for (row, title) in rows.iter().zip(&titles) {
            assert!(row.starts_with(title.as_str()), "{row} starts with {title}");
        }
        for (index, selector) in TITLES.iter().enumerate().take(titles.len()) {
            let bounds = vcx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("row {index}'s title is drawn"));
            assert!(
                f32::from(bounds.size.width) > 40.,
                "row {index}'s title is {} wide at a {width} px sidebar",
                f32::from(bounds.size.width)
            );
        }
        drop(shell);
    }
}

fn library_ids(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> Vec<String> {
    let library = vcx.update(|_, cx| shell.entity.read(cx).library());
    vcx.update(|_, cx| {
        library
            .read(cx)
            .list()
            .iter()
            .map(|p| p.id.clone())
            .collect()
    })
}

#[gpui::test]
fn a_prompt_moves_by_alt_arrows_keeps_focus_and_says_its_place(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-prompt-keys", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let before = library_ids(&shell, vcx);
    let tree1 = tree(vcx);
    let first = named(&nodes(&tree1), Role::ListItem, "Castle with a moat");
    act(vcx, first, gpui::accesskit::Action::Focus);
    settle(&shell, vcx);
    vcx.simulate_keystrokes("alt-down");
    settle(&shell, vcx);
    let after = library_ids(&shell, vcx);
    assert_eq!(after[1], before[0], "{after:?}");
    assert_eq!(after[0], before[1]);
    assert_eq!(
        focused(vcx),
        (Role::ListItem, Some("Castle with a moat".into()))
    );
    assert!(
        labels(vcx, Role::Status)
            .iter()
            .any(|s| s == "Castle with a moat moved to 2 of 6")
    );
}

#[gpui::test]
fn a_prompt_dragged_down_two_rows_lands_there(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-prompt-drag", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let before = library_ids(&shell, vcx);
    let tree1 = tree(vcx);
    let first = named(&nodes(&tree1), Role::ListItem, "Castle with a moat");
    let from = centre_of(vcx, first.bounding_box().expect("bounds"));
    let to = gpui::point(from.x, from.y + gpui::px(72.));
    drag(vcx, from, to, 12);
    settle(&shell, vcx);
    let after = library_ids(&shell, vcx);
    assert_eq!(after[2], before[0], "{after:?}");
    // The drag's release opened nothing.
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert_eq!(place, Place::NewBench);
}

#[gpui::test]
fn a_press_on_a_row_s_star_starts_no_drag(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-prompt-star-drag", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let before = library_ids(&shell, vcx);
    // The last row's star, then a press far above it: the order holds.
    let tree1 = tree(vcx);
    let stars: Vec<_> = nodes(&tree1)
        .into_iter()
        .filter(|n| n.role() == Role::Button && n.label().as_deref() == Some("Favorite prompt"))
        .collect();
    let last = *stars.last().expect("a star");
    pointer_click(vcx, last);
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Search prompts");
    let after = library_ids(&shell, vcx);
    assert_eq!(after, before);
}

#[gpui::test]
fn the_tag_filter_narrows_the_list_to_prompts_with_a_chosen_tag(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-prompt-tags", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Search prompts");
    press(&shell, vcx, Role::Button, "Filter by tag");
    assert!(labels(vcx, Role::Menu).contains(&"Filter by tag".to_string()));
    let tags = labels(vcx, Role::MenuItemCheckBox);
    assert_eq!(
        tags,
        ["castle", "farm", "light", "redstone", "stone", "terrain"]
    );
    press(&shell, vcx, Role::MenuItemCheckBox, "castle");
    let tree1 = tree(vcx);
    let list = named(&nodes(&tree1), Role::List, "Prompts");
    let rows: Vec<String> = list
        .children()
        .filter(|n| n.role() == Role::ListItem)
        .filter_map(|n| n.label())
        .collect();
    assert_eq!(rows, ["Castle with a moat", "Two towers"]);
    // The menu stays open, its item checked.
    let tree2 = tree(vcx);
    let castle = named(&nodes(&tree2), Role::MenuItemCheckBox, "castle");
    assert_eq!(castle.toggled(), Some(Toggled::True));
}

#[gpui::test]
fn the_editor_s_title_line_holds_its_descenders(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-title-line", Some(Place::Prompt), &[]);
    settle(&shell, vcx);
    let library = vcx.update(|_, cx| shell.entity.read(cx).library());
    vcx.update(|_, cx| {
        library.update(cx, |l, cx| {
            l.open("prompt-tower".into(), cx);
            l.update(
                "prompt-tower",
                |p| p.title = "Glyphs gjpqy again".into(),
                cx,
            );
        })
    });
    settle(&shell, vcx);
    let bounds = vcx
        .debug_bounds("prompt-editor-title")
        .expect("the title is drawn");
    // 24 px type at 1.2 (`620 24px/1.2`): its 28.8 px line and the box
    // around it.
    assert!(f32::from(bounds.size.height) >= 34., "{bounds:?}");
}

// pane agent (first real look): the comparison's digits and the round's
// prompt on show.

fn actions(shell: &ShellUnderTest) -> Vec<serde_json::Value> {
    log_lines(&shell.log)
}

fn framed_members(shell: &ShellUnderTest) -> Vec<String> {
    actions(shell)
        .iter()
        .filter(|l| l["action"] == "frame member")
        .map(|l| l["member"].as_str().unwrap_or("").to_string())
        .collect()
}

fn composer_text(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> String {
    let pane = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|_, cx| pane.read(cx).composer().read(cx).value().to_string())
}

fn focus_node(shell: &ShellUnderTest, vcx: &mut VisualTestContext, role: Role, starts: &str) {
    let tree = tree(vcx);
    let n = nodes(&tree);
    let node = n
        .iter()
        .find(|x| x.role() == role && x.label().is_some_and(|l| l.starts_with(starts)))
        .copied()
        .unwrap_or_else(|| panic!("no {role:?} {starts}"));
    act(vcx, node, gpui::accesskit::Action::Focus);
    settle(shell, vcx);
}

/// The castle's order: castle-6, castle-3, castle-8, castle-1, castle-4,
/// castle-7, castle-2, castle-5 (`fixtures::bench`).
#[gpui::test]
fn a_digit_in_a_comparison_frames_that_member_ahead_of_type_ahead(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-digit-row", castle(), &[]);
    settle(&shell, vcx);
    // Focus on a row (no text field).
    focus_node(&shell, vcx, Role::ListItem, "Build A, ");
    vcx.simulate_keystrokes("2");
    settle(&shell, vcx);
    assert_eq!(framed_members(&shell), vec!["castle-3".to_string()]);
    assert_eq!(composer_text(&shell, vcx), "", "the composer never sees it");
    // Focus in the pane.
    focus_node(
        &shell,
        vcx,
        Role::Canvas,
        mcbench_ui::state::live::PANE_NAME,
    );
    vcx.simulate_keystrokes("1");
    settle(&shell, vcx);
    assert_eq!(
        framed_members(&shell).last().map(String::as_str),
        Some("castle-6")
    );
    assert_eq!(composer_text(&shell, vcx), "");
    // A digit past the members goes into the composer as any character.
    vcx.simulate_keystrokes("9");
    settle(&shell, vcx);
    assert_eq!(framed_members(&shell).len(), 2);
    assert_eq!(composer_text(&shell, vcx), "9");
}

#[gpui::test]
fn a_digit_in_the_comparisons_composer_is_typed(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-digit-composer", castle(), &[]);
    settle(&shell, vcx);
    let pane = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|window, cx| pane.update(cx, |p, cx| p.focus_composer(window, cx)));
    settle(&shell, vcx);
    // Empty, then with text: the field takes the digit.
    vcx.simulate_input("3");
    settle(&shell, vcx);
    assert_eq!(composer_text(&shell, vcx), "3");
    vcx.simulate_input("4");
    settle(&shell, vcx);
    assert_eq!(composer_text(&shell, vcx), "34");
    assert!(framed_members(&shell).is_empty());
}

#[gpui::test]
fn a_digit_with_a_layer_open_frames_nothing(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-digit-layer", castle(), &[]);
    settle(&shell, vcx);
    focus_node(&shell, vcx, Role::ListItem, "Build A, ");
    // The prompts palette, a modal layer.
    press(&shell, vcx, Role::Button, "Insert a saved prompt");
    vcx.simulate_keystrokes("2");
    settle(&shell, vcx);
    assert!(
        framed_members(&shell).is_empty(),
        "{:?}",
        framed_members(&shell)
    );
}

fn shown_round_prompt(shell: &ShellUnderTest, vcx: &mut VisualTestContext) -> String {
    let pane = vcx.update(|_, cx| shell.entity.read(cx).bench_pane());
    let shown = vcx.update(|_, cx| pane.read(cx).round_of("bench-castle"));
    let prompts: Vec<String> = labels(vcx, Role::Button)
        .into_iter()
        .filter(|l| l.starts_with(&format!("Round {shown}: ")))
        .collect();
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    prompts[0].clone()
}

fn last_action(shell: &ShellUnderTest) -> String {
    actions(shell)
        .iter()
        .rev()
        .find(|l| l["action"] == "frame round" || l["action"] == "round")
        .map(|l| l["action"].as_str().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// The round's prompt on the round on show frames the whole round, by a
/// reader's press (AccessKit's Click, what showed the round without framing
/// it) and the pointer's, the comparison reached by its address (also how
/// a restored place arrives: `run` hands the preferences' place to the
/// shell as its start), from the sidebar, and on a second visit.
#[gpui::test]
fn the_shown_rounds_prompt_frames_the_round_by_address(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-frame-round-address", castle(), &[]);
    settle(&shell, vcx);
    let prompt = shown_round_prompt(&shell, vcx);
    press(&shell, vcx, Role::Button, &prompt);
    assert_eq!(last_action(&shell), "frame round");
    let tree = tree(vcx);
    let node = named(&nodes(&tree), Role::Button, &prompt);
    pointer_click(vcx, node);
    settle(&shell, vcx);
    assert_eq!(last_action(&shell), "frame round");
}

#[gpui::test]
fn the_shown_rounds_prompt_frames_the_round_from_the_sidebar(cx: &mut TestAppContext) {
    let (shell, _core, vcx) =
        open_bench(cx, "bench-frame-round-sidebar", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let tree0 = tree(vcx);
    let row = nodes(&tree0)
        .into_iter()
        .find(|n| {
            n.label()
                .is_some_and(|l| l.starts_with("Castle with a moat"))
        })
        .expect("the castle's sidebar row");
    click(vcx, row);
    settle(&shell, vcx);
    let prompt = shown_round_prompt(&shell, vcx);
    press(&shell, vcx, Role::Button, &prompt);
    assert_eq!(last_action(&shell), "frame round");
    // A second visit: away to the leaderboard and back.
    press(&shell, vcx, Role::Button, "Leaderboard");
    let tree1 = tree(vcx);
    let row = nodes(&tree1)
        .into_iter()
        .find(|n| {
            n.label()
                .is_some_and(|l| l.starts_with("Castle with a moat"))
        })
        .expect("the castle's sidebar row");
    click(vcx, row);
    settle(&shell, vcx);
    let prompt = shown_round_prompt(&shell, vcx);
    press(&shell, vcx, Role::Button, &prompt);
    assert_eq!(last_action(&shell), "frame round");
}

// par-bench: the parity pass's findings in Bench (`app.md` 2.1, 7.1, 7.2,
// 7.6, 7.8, 10.1, 7.11; `components.md` L.1, L.5) and the untested Bench
// row of the audit.

fn castle_resolution(
    shell: &ShellUnderTest,
    vcx: &mut VisualTestContext,
) -> mcbench_ui::Resolution {
    vcx.update(|_, cx| {
        let lists = shell.entity.read(cx).lists_entity();
        let listed = lists
            .read(cx)
            .benches()
            .iter()
            .find(|b| b.id == "bench-castle")
            .expect("listed")
            .resolution;
        listed.map_or(mcbench_ui::Resolution::Standard, |r| {
            mcbench_ui::Resolution::of(r as u32)
        })
    })
}

/// A round with a picture, sent to the castle by the core, every model
/// done with it.
fn round_with_a_picture(
    shell: &ShellUnderTest,
    core: &std::sync::Arc<mcbench_ui::dev::standin::StandIn>,
    vcx: &mut VisualTestContext,
    store: &TempDir,
) {
    use mcbench_ui::Core as _;
    let picture = store.0.join("lamp.png");
    std::fs::write(&picture, mcbench_ui::dev::fixtures::PICTURE).expect("the picture");
    core.keep_files(&store.0.join("files"), &picture);
    core.finish_members("bench-castle");
    settle(shell, vcx);
    let resolution = castle_resolution(shell, vcx);
    core.send(mcbench_ui::Command::Send(mcbench_ui::Outgoing {
        target: mcbench_ui::Target::Bench("bench-castle".into()),
        text: "Hang banners on the towers.".into(),
        attachments: vec![mcbench_ui::OutgoingFile {
            name: "banner.png".into(),
            content_type: "image/png".into(),
            size: mcbench_ui::dev::fixtures::PICTURE.len() as u64,
            data: std::sync::Arc::from(mcbench_ui::dev::fixtures::PICTURE),
        }],
        selections: Vec::new(),
        prompts: Vec::new(),
        effort: None,
        resolution,
        edit_of: None,
    }));
    settle(shell, vcx);
    core.finish_members("bench-castle");
    settle(shell, vcx);
}

#[gpui::test]
fn a_round_s_files_show_in_its_prompt_and_open_as_a_message_s_do(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_bench(cx, "bench-round-files", castle(), &[]);
    let store = TempDir::new("bench-round-files-store");
    round_with_a_picture(&shell, &core, vcx, &store);
    let tree1 = tree(vcx);
    let all = nodes(&tree1);
    assert!(has(&all, Role::List, "Round 4"), "the round arrived");
    // The picture is a button by its name (`renderFiles`), opening the
    // lightbox.
    click(vcx, named(&all, Role::Button, "banner.png"));
    settle(&shell, vcx);
    let opened = log_lines(&shell.log)
        .iter()
        .any(|l| l["action"] == "opened" && l["layer"] == "lightbox");
    let tail: Vec<_> = log_lines(&shell.log).into_iter().rev().take(6).collect();
    assert!(opened, "the lightbox opened: {tail:?}");
}

#[gpui::test]
fn edit_and_branch_on_a_round_brings_its_text_and_its_files(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_bench(cx, "bench-edit-round", castle(), &[]);
    let store = TempDir::new("bench-edit-round-store");
    round_with_a_picture(&shell, &core, vcx, &store);
    let tree1 = tree(vcx);
    let edits: Vec<_> = of_role(&nodes(&tree1), Role::Button)
        .into_iter()
        .filter(|n| n.label().as_deref() == Some("Edit and branch"))
        .collect();
    click(
        vcx,
        *edits.last().expect("the last round's Edit and branch"),
    );
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    let draft = vcx.update(|_, cx| chat.read(cx).current_draft(cx));
    assert_eq!(draft.text, "Hang banners on the towers.");
    let names: Vec<&str> = draft.attachments.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["banner.png"], "the round's file came with it");
    assert_eq!(
        draft.attachments[0].data.as_ref(),
        mcbench_ui::dev::fixtures::PICTURE,
        "read back whole"
    );
    assert!(draft.edit.is_some(), "an edit of the round");
}

#[gpui::test]
fn a_round_on_screen_past_the_bench_s_last_comes_back_to_it(cx: &mut TestAppContext) {
    use mcbench_ui::Core as _;
    let (shell, core, vcx) = open_bench(cx, "bench-round-clamp", castle(), &[]);
    core.finish_members("bench-castle");
    settle(&shell, vcx);
    let pane = vcx.update(|_, cx| shell.entity.read(cx).bench_pane());
    assert_eq!(
        vcx.update(|_, cx| pane.read(cx).round_of("bench-castle")),
        3
    );
    // Elsewhere, the bench goes back to the second round's other version:
    // two rounds long.
    core.send(mcbench_ui::Command::BenchBranch {
        bench: "bench-castle".into(),
        round: "bench-castle-r2-old".into(),
    });
    settle(&shell, vcx);
    assert_eq!(
        vcx.update(|_, cx| pane.read(cx).round_of("bench-castle")),
        2
    );
}

#[gpui::test]
fn the_empty_chat_s_bench_link_takes_build_s_draft_to_a_new_bench(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(
        cx,
        "bench-from-build",
        Some(Place::NewChat { id: None }),
        &[],
    );
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.focus_composer(window, cx)));
    vcx.simulate_input("A tower");
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Bench");
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert_eq!(
        place,
        Place::NewBench,
        "a new bench, not Bench's last place"
    );
    assert_eq!(composer_text(&shell, vcx), "A tower");
    assert_eq!(focused(vcx).1.as_deref(), Some("Message"));
    vcx.update(|window, cx| {
        shell
            .entity
            .update(cx, |s, cx| s.go(Place::NewChat { id: None }, window, cx))
    });
    settle(&shell, vcx);
    assert_eq!(composer_text(&shell, vcx), "", "Build's draft moved");
}

#[gpui::test]
fn bench_s_landing_is_its_newest_bench_and_build_first_opens_the_new_chat(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-landing", None, &[]);
    settle(&shell, vcx);
    let newest = vcx.update(|_, cx| {
        let lists = shell.entity.read(cx).lists_entity();
        lists.read(cx).benches()[0].id.clone()
    });
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert_eq!(place, Place::Compare { bench: newest });
    vcx.update(|window, cx| {
        shell.entity.update(cx, |s, cx| {
            s.switch_mode(mcbench_ui::state::place::Mode::Build, window, cx)
        })
    });
    settle(&shell, vcx);
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert_eq!(place, Place::NewChat { id: None });
}

#[gpui::test]
fn a_new_bench_s_comparison_does_not_open_once_the_person_moved(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_bench(
        cx,
        "bench-moved",
        Some(Place::NewBench),
        &[BenchPlay::CreateHeld],
    );
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|_, cx| {
        chat.update(cx, |c, cx| {
            c.toggle_member("anthropic", "claude-opus-5-5", cx);
            c.toggle_member("openai", "gpt-5.5", cx);
        })
    });
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.focus_composer(window, cx)));
    vcx.simulate_input("A tower");
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Run on 2 models");
    vcx.update(|window, cx| {
        shell
            .entity
            .update(cx, |s, cx| s.go(Place::Leaderboard, window, cx))
    });
    settle(&shell, vcx);
    core.release_held_bench();
    settle(&shell, vcx);
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert_eq!(place, Place::Leaderboard, "they stay where they went");
    let made = vcx.update(|_, cx| {
        let lists = shell.entity.read(cx).lists_entity();
        lists
            .read(cx)
            .benches()
            .iter()
            .any(|b| b.title == "A tower")
    });
    assert!(made, "the bench was made");
}

#[gpui::test]
fn a_revealed_row_names_its_model_short_with_the_default_effort_and_a_halted_model_s_time(
    cx: &mut TestAppContext,
) {
    let (shell, _core, vcx) = open_bench(cx, "bench-rows", castle(), &[]);
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Reveal models");
    press(&shell, vcx, Role::Button, "Reveal for good");
    settle(&shell, vcx);
    let buttons = labels(vcx, Role::Button);
    // `modelLabel`, not the sidebar's twin name with its effort.
    assert!(buttons.contains(&"Stop gpt-5.5".to_string()), "{buttons:?}");
    assert!(
        buttons.iter().any(|b| b == "Open gpt-5.5's chat"),
        "{buttons:?}"
    );
    assert!(buttons.contains(&"Retry claude-haiku-5-5".to_string()));
    let rows = labels(vcx, Role::ListItem);
    // A model with no effort of its own shows its default.
    assert!(
        rows.iter()
            .any(|r| r.starts_with("claude-opus-5-5, Done, medium")),
        "{:?}",
        rows.iter()
            .filter(|r| r.starts_with("claude-opus"))
            .collect::<Vec<_>>()
    );
    // A failed and a stopped model show their time, as a done one does.
    let halted: Vec<&String> = rows
        .iter()
        .filter(|r| r.contains(", Failed") || r.contains(", Stopped"))
        .collect();
    assert_eq!(halted.len(), 2, "{halted:?}");
    assert!(halted.iter().all(|r| r.ends_with('s')), "{halted:?}");
}

#[gpui::test]
fn a_stop_on_its_way_waits_and_the_also_run_items_stay_at_eight_models(cx: &mut TestAppContext) {
    let (shell, _core, vcx) =
        open_bench(cx, "bench-stop-wait", castle(), &[BenchPlay::MembersWait]);
    settle(&shell, vcx);
    let pane = vcx.update(|_, cx| shell.entity.read(cx).bench_pane());
    let castle_3 = "castle-3".to_string();
    vcx.update(|_, cx| pane.update(cx, |p, cx| p.stop(&castle_3, cx)));
    // Pressed again while on its way: nothing (`pending`).
    let tree1 = tree(vcx);
    let stop = of_role(&nodes(&tree1), Role::Button)
        .into_iter()
        .find(|n| n.label().is_some_and(|l| l.starts_with("Stop ")))
        .map(|n| n.is_disabled());
    assert_eq!(stop, Some(true), "the Stop waits for its answer");
    // The new bench at eight models still offers `Also run` in the menu.
    vcx.update(|window, cx| {
        shell
            .entity
            .update(cx, |s, cx| s.go(Place::NewBench, window, cx))
    });
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|_, cx| {
        chat.update(cx, |c, cx| {
            for m in ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"] {
                c.toggle_member("anthropic", m, cx);
            }
            for m in ["gpt-5.5", "gpt-5.5-mini", "o9"] {
                c.toggle_member("openai", m, cx);
            }
            for m in ["gemini-3-pro", "gemini-3-flash"] {
                c.toggle_member("google", m, cx);
            }
        })
    });
    settle(&shell, vcx);
    let menus = labels(vcx, Role::Button)
        .into_iter()
        .filter(|b| b.starts_with("claude-opus-5-5 reasoning effort"))
        .collect::<Vec<_>>();
    assert_eq!(menus.len(), 1, "{menus:?}");
    press(&shell, vcx, Role::Button, &menus[0]);
    let items = labels(vcx, Role::MenuItemRadio);
    assert!(
        items
            .iter()
            .any(|i| i.starts_with("Also run claude-opus-5-5 at ")),
        "{items:?}"
    );
}

#[gpui::test]
fn the_tie_row_calls_a_tie_and_takes_it_back(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_bench(cx, "bench-tie", castle(), &[]);
    core.finish_members("bench-castle");
    settle(&shell, vcx);
    let rows = labels(vcx, Role::ListItem);
    assert!(
        rows.contains(&"Tie no build is better".to_string()),
        "{rows:?}"
    );
    let ties = labels(vcx, Role::Button)
        .into_iter()
        .filter(|b| b == "Call this round a tie")
        .count();
    assert!(ties > 0);
    let tree1 = tree(vcx);
    let last = of_role(&nodes(&tree1), Role::Button)
        .into_iter()
        .rfind(|n| n.label().as_deref() == Some("Call this round a tie"))
        .expect("a tie button");
    click(vcx, last);
    settle(&shell, vcx);
    let tree1 = tree(vcx);
    let taken = named(&nodes(&tree1), Role::Button, "Take back the tie");
    assert_eq!(taken.toggled(), Some(Toggled::True));
    click(vcx, taken);
    settle(&shell, vcx);
    assert!(!labels(vcx, Role::Button).contains(&"Take back the tie".to_string()));
}

#[gpui::test]
fn round_branch_arrows_follow_another_version_of_a_round(cx: &mut TestAppContext) {
    let (shell, core, vcx) = open_bench(cx, "bench-branch", castle(), &[]);
    core.finish_members("bench-castle");
    settle(&shell, vcx);
    let groups = labels(vcx, Role::Group);
    assert!(groups.contains(&"Round branch".to_string()), "{groups:?}");
    press(&shell, vcx, Role::Button, "Next branch");
    settle(&shell, vcx);
    let pane = vcx.update(|_, cx| shell.entity.read(cx).bench_pane());
    let rounds = vcx.update(|_, cx| {
        let lists = shell.entity.read(cx).lists_entity();
        lists
            .read(cx)
            .benches()
            .iter()
            .find(|b| b.id == "bench-castle")
            .expect("listed")
            .rounds
    });
    assert_eq!(rounds, 2, "the older version is two rounds long");
    assert_eq!(
        vcx.update(|_, cx| pane.read(cx).round_of("bench-castle")),
        2
    );
}

#[gpui::test]
fn a_bench_that_could_not_be_read_says_so_and_reads_again(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(
        cx,
        "bench-details-fail",
        castle(),
        &[BenchPlay::DetailsFail],
    );
    settle(&shell, vcx);
    let alert = format!(
        "This bench could not be read: {}",
        mcbench_ui::dev::standin::DETAILS_FAILED
    );
    assert!(labels(vcx, Role::Alert).contains(&alert));
    let before = log_lines(&shell.log).len();
    press(&shell, vcx, Role::Button, "Read again");
    assert!(log_lines(&shell.log).len() >= before);
    assert!(labels(vcx, Role::Alert).contains(&alert), "it fails again");
}

#[gpui::test]
fn the_new_bench_asks_for_two_models_and_a_chip_s_effort_menu_sets_its_effort(
    cx: &mut TestAppContext,
) {
    let (shell, _core, vcx) = open_bench(cx, "bench-chip-effort", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|_, cx| {
        chat.update(cx, |c, cx| {
            c.toggle_member("anthropic", "claude-opus-5-5", cx)
        })
    });
    settle(&shell, vcx);
    assert!(
        labels(vcx, Role::Label).contains(&"Choose at least two models".to_string()),
        "the hint for one model"
    );
    let menu = labels(vcx, Role::Button)
        .into_iter()
        .find(|b| b.starts_with("claude-opus-5-5 reasoning effort"))
        .expect("the chip's effort");
    press(&shell, vcx, Role::Button, &menu);
    press(&shell, vcx, Role::MenuItemRadio, "high");
    let menu = labels(vcx, Role::Button)
        .into_iter()
        .find(|b| b.starts_with("claude-opus-5-5 reasoning effort"))
        .expect("the chip's effort");
    assert_eq!(menu, "claude-opus-5-5 reasoning effort: high");
}

#[gpui::test]
fn run_bench_takes_the_prompt_to_a_new_bench(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-run-bench", Some(Place::Prompt), &[]);
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Run bench");
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    assert_eq!(place, Place::NewBench);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    let draft = vcx.update(|_, cx| chat.read(cx).current_draft(cx));
    assert!(
        !draft.text.is_empty() || !draft.attachments.is_empty(),
        "{draft:?}"
    );
}

#[gpui::test]
fn a_press_in_the_composer_s_box_keeps_the_prompt_palette_open(cx: &mut TestAppContext) {
    let lamp = Some(Place::Chat {
        id: "chat-lamp".into(),
    });
    let (shell, _core, vcx) = open_bench(cx, "bench-palette-press", lamp, &[]);
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.focus_composer(window, cx)));
    vcx.simulate_input("/");
    settle(&shell, vcx);
    assert!(labels(vcx, Role::Dialog).contains(&"Prompts".to_string()));
    // In the composer's own box (`composerBoxRef`): it stays.
    let tree1 = tree(vcx);
    pointer_click(
        vcx,
        named(&nodes(&tree1), Role::MultilineTextInput, "Message"),
    );
    settle(&shell, vcx);
    assert!(labels(vcx, Role::Dialog).contains(&"Prompts".to_string()));
    // Anywhere else: it closes.
    let tree1 = tree(vcx);
    pointer_click(vcx, named(&nodes(&tree1), Role::Region, "Conversation"));
    settle(&shell, vcx);
    assert!(!labels(vcx, Role::Dialog).contains(&"Prompts".to_string()));
}

#[gpui::test]
fn build_to_bench_and_back_keeps_the_new_chat_s_one_row(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(
        cx,
        "bench-build-and-back",
        Some(Place::NewChat { id: None }),
        &[],
    );
    settle(&shell, vcx);
    let rows = |shell: &ShellUnderTest, vcx: &mut VisualTestContext| {
        vcx.update(|_, cx| {
            let lists = shell.entity.read(cx).lists_entity();
            lists.read(cx).chats().len()
        })
    };
    // New chat makes the new chat's row (`createdChat`).
    vcx.update(|window, cx| {
        shell.entity.update(cx, |s, cx| {
            s.new_here(Place::NewChat { id: None }, window, cx)
        })
    });
    settle(&shell, vcx);
    let made = rows(&shell, vcx);
    let place = vcx.update(|_, cx| shell.entity.read(cx).place().clone());
    let Place::NewChat { id: Some(row) } = place else {
        panic!("the new chat took its row: {place:?}")
    };
    // Build's text to a new bench (`benchFromBuild`), then back to Build:
    // the same new chat, its row kept, no second one.
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.focus_composer(window, cx)));
    vcx.simulate_input("A tower");
    settle(&shell, vcx);
    press(&shell, vcx, Role::Button, "Bench");
    assert_eq!(
        vcx.update(|_, cx| shell.entity.read(cx).place().clone()),
        Place::NewBench
    );
    assert_eq!(composer_text(&shell, vcx), "A tower");
    vcx.update(|window, cx| {
        shell.entity.update(cx, |s, cx| {
            s.switch_mode(mcbench_ui::state::place::Mode::Build, window, cx)
        })
    });
    settle(&shell, vcx);
    assert_eq!(
        vcx.update(|_, cx| shell.entity.read(cx).place().clone()),
        Place::NewChat { id: Some(row) }
    );
    assert_eq!(rows(&shell, vcx), made, "no second row");
    assert_eq!(composer_text(&shell, vcx), "", "the text went to the bench");
}

#[gpui::test]
fn a_chip_s_name_shows_its_model_id_and_the_rest_of_its_button_remove(cx: &mut TestAppContext) {
    let (shell, _core, vcx) = open_bench(cx, "bench-chip-title", Some(Place::NewBench), &[]);
    settle(&shell, vcx);
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|_, cx| {
        chat.update(cx, |c, cx| {
            c.toggle_member("anthropic", "claude-opus-5-5", cx);
            c.toggle_member("openrouter", "x-ai/grok-5", cx);
        })
    });
    settle(&shell, vcx);
    let tip_at = |vcx: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>| {
        vcx.simulate_mouse_move(at, None, gpui::Modifiers::none());
        let _ = tree(vcx);
        vcx.executor().advance_clock(Duration::from_millis(600));
        vcx.run_until_parked();
        labels(vcx, Role::Tooltip)
    };
    let (bounds, name, description) = {
        let tree1 = tree(vcx);
        let chip = named(&nodes(&tree1), Role::Button, "Remove grok-5");
        (
            chip.bounding_box().expect("bounds"),
            chip.label(),
            chip.description(),
        )
    };
    assert_eq!(name.as_deref(), Some("Remove grok-5"));
    assert_eq!(description.as_deref(), Some("Remove"));
    // Over its name: the model's id (`.bench-chip-name`'s title).
    let centre = centre_of(vcx, bounds);
    assert_eq!(tip_at(vcx, centre), ["x-ai/grok-5"]);
    // Away, then over the button's own left padding: `Remove`.
    tip_at(vcx, gpui::point(gpui::px(1.), gpui::px(1.)));
    let scale = f64::from(vcx.update(|window, _| window.scale_factor()));
    let left = gpui::point(gpui::px((bounds.x0 / scale) as f32 + 3.), centre.y);
    assert_eq!(tip_at(vcx, left), ["Remove"]);
    // The name and description stay as they were.
    let tree1 = tree(vcx);
    let chip = named(&nodes(&tree1), Role::Button, "Remove grok-5");
    assert_eq!(chip.description().as_deref(), Some("Remove"));
}
