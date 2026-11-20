//! Asking before a quit while builds run (quit-ask; the owner's decision of
//! 2026-11-18, "App should ask before quitting if a build is running
//! yes"). A build is a listed row with a running turn (`running_since`):
//! a Build chat's, or a bench model's (each running model is one build).
//! Every text the question shows is here, in one place.

use crate::ListedChat;

/// The default button: the app stays open.
pub const KEEP: &str = "Keep building";
/// The button that quits.
pub const QUIT: &str = "Quit";

/// How many builds run: the rows with a running turn.
pub fn running(rows: &[ListedChat]) -> usize {
    rows.iter().filter(|c| c.running_since.is_some()).count()
}

/// The question's title (the dialog's name).
pub fn title(running: usize) -> String {
    match running {
        0 => panic!("the quit question is asked only while a build runs"),
        1 => "A build is running".into(),
        n => format!("{n} builds are running"),
    }
}

/// The question's body (the dialog's description).
pub fn body(running: usize) -> &'static str {
    match running {
        0 => panic!("the quit question is asked only while a build runs"),
        1 => "Quitting stops it. What it built so far stays in the chat.",
        _ => "Quitting stops them. What they built so far stays in their chats.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_build_is_singular_and_more_are_counted() {
        assert_eq!(title(1), "A build is running");
        assert_eq!(
            body(1),
            "Quitting stops it. What it built so far stays in the chat."
        );
        assert_eq!(title(3), "3 builds are running");
        assert_eq!(
            body(2),
            "Quitting stops them. What they built so far stays in their chats."
        );
    }
}
