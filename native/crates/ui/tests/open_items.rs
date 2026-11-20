//! The items left open after the gate (`ACCESSIBILITY.md`, "Open after the
//! gate"), each checked in process: (b) a streaming answer keeps its names
//! until it settles; (c) an image's node is as wide as its picture. (a) is
//! `contract::the_fatal_screen_reads_its_message_and_focuses_quit`; (f) is
//! the contract suite's run time.

mod common;

use common::*;
use gpui::TestAppContext;
use gpui::accesskit::Role;
use mcbench_ui::{TurnEvent, Update};

fn last_article(vcx: &mut gpui::VisualTestContext) -> (String, Vec<String>) {
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let article = *of_role(&nodes, Role::Article).last().expect("a message");
    let blocks = article
        .children()
        .filter(|c| c.role() == Role::Group)
        .map(|c| c.label().unwrap_or_default())
        .collect();
    (article.label().unwrap_or_default(), blocks)
}

#[gpui::test]
fn a_streaming_answer_keeps_its_names_until_it_settles(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(
        cx,
        "streaming",
        Fixture::Gate,
        mcbench_ui::state::address::address_place("/chat/chat-gate"),
    );
    // A real send: the stand-in answers at once into the inbox; its events
    // are applied one at a time, as they would stream.
    vcx.simulate_keystrokes("g o enter");
    let updates: Vec<Update> = std::mem::take(&mut *shell.inbox.0.lock().expect("lock"));
    let mut streaming = Vec::new();
    let mut settled = None;
    for update in updates {
        let delta = matches!(
            update,
            Update::Turn {
                event: TurnEvent::AssistantDelta(_),
                ..
            }
        );
        let done = matches!(
            update,
            Update::Turn {
                event: TurnEvent::Done,
                ..
            }
        );
        vcx.update(|window, cx| shell.entity.update(cx, |s, cx| s.apply(update, window, cx)));
        vcx.run_until_parked();
        if delta {
            streaming.push(last_article(vcx));
        }
        if done {
            settled = Some(last_article(vcx));
        }
    }
    assert!(streaming.len() > 10, "the answer streamed word by word");
    for (name, blocks) in &streaming {
        assert_eq!(
            name, "claude-opus-5-5",
            "a streaming message is named by its writer alone"
        );
        // The block being written is named by its kind alone.
        let last = blocks.last().expect("a block");
        assert!(!last.contains(", "), "{blocks:?}");
    }
    let (name, blocks) = settled.expect("the turn ended");
    assert!(
        name.starts_with("claude-opus-5-5, Adding a third lamp now."),
        "settled: its whole text, once: {name}"
    );
    assert!(
        blocks.iter().all(|b| b.contains(", ")),
        "every block settled: {blocks:?}"
    );
}

#[gpui::test]
fn an_images_node_is_as_wide_as_its_picture(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(cx, "image", Fixture::Gate, None);
    // The lamp chat's third answer holds the 240 by 150 picture; bring it in.
    let chat = vcx.update(|_, cx| shell.entity.read(cx).chat());
    vcx.update(|window, cx| chat.update(cx, |c, cx| c.reveal_row(5, window, cx)));
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let image = *of_role(&nodes, Role::Image)
        .first()
        .expect("the picture's node");
    let scale = f64::from(vcx.update(|window, _| window.scale_factor()));
    let bounds = image.bounding_box().expect("bounds");
    let width = (bounds.x1 - bounds.x0) / scale;
    // The picture, its 2 px focus ring each side; not the 420 of the
    // element's maximum.
    assert!(
        (240. ..=246.).contains(&width),
        "the image's node is {width} wide"
    );
}

#[gpui::test]
fn a_stored_picture_the_core_cannot_give_shows_its_sentence(cx: &mut TestAppContext) {
    let (shell, vcx) = open_shell(
        cx,
        "missing-picture",
        Fixture::Gate,
        mcbench_ui::state::address::address_place("/chat/chat-gate"),
    );
    let _ = &shell;
    // The pictures are the turn's work: its fold opens them.
    let tree0 = tree(vcx);
    let fold = *nodes(&tree0)
        .iter()
        .find(|n| n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with("Worked")))
        .expect("the work's fold");
    click(vcx, fold);
    let tree = tree(vcx);
    let nodes = nodes(&tree);
    let images: Vec<Option<String>> = of_role(&nodes, Role::Image)
        .iter()
        .map(|n| n.label())
        .collect();
    let articles: Vec<Option<String>> = of_role(&nodes, Role::Article)
        .iter()
        .map(|n| n.label())
        .collect();
    assert!(
        images.contains(&Some("Picture 1, Image not found".to_string())),
        "{images:?} {articles:?}"
    );
}
