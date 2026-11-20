//! The other texts that reach models, from `contract/texts/` (written by
//! `contract/generate.js` from a run of today's `server/`): the title
//! request (`server/index.js` `generateTitle`) and Improve with AI
//! (`server/prompt-generation.js`); and which library prompt an old message
//! came from (`server/prompt-match.js`).

use serde_json::{Map, Value};

use mcbench_providers::ApiMessage;
use mcbench_providers::api::js;

fn file(name: &str) -> &'static str {
    crate::contract::generated(name)
}

/// The longest title kept, in characters (`TITLE_MAX`).
pub const TITLE_MAX: usize = 80;

fn message(role: &str, content: Value) -> ApiMessage {
    let mut out = Map::new();
    out.insert("role".into(), Value::String(role.into()));
    out.insert("content".into(), content);
    ApiMessage(out)
}

/// The title model's messages for a chat or bench whose first message is
/// `text` with `images` (data URLs). `text` is never empty: today's callers
/// give `titleSource`, which names the files when the message has no text,
/// so `generateTitle`'s branch for a message without text is never taken.
pub fn title_messages(text: &str, images: &[String]) -> Vec<ApiMessage> {
    assert!(!text.is_empty(), "a title's source has text (titleSource)");
    let request = file("texts/title-request.txt").replace("⟦message⟧", text);
    let content = if images.is_empty() {
        Value::String(request)
    } else {
        let mut parts = Vec::new();
        let mut part = Map::new();
        part.insert("type".into(), Value::String("text".into()));
        part.insert("text".into(), Value::String(request));
        parts.push(Value::Object(part));
        for url in images {
            let mut image = Map::new();
            image.insert("url".into(), Value::String(url.clone()));
            let mut part = Map::new();
            part.insert("type".into(), Value::String("image_url".into()));
            part.insert("image_url".into(), Value::Object(image));
            parts.push(Value::Object(part));
        }
        Value::Array(parts)
    };
    vec![
        message(
            "system",
            Value::String(file("texts/title-system.txt").into()),
        ),
        message("user", content),
    ]
}

/// The title in the title model's reply, or why it is none (`generateTitle`'s
/// checks): empty, or not one short line.
pub fn title_of(reply: &str) -> Result<String, String> {
    let title = js::trim(reply);
    if title.is_empty() {
        return Err("Title model returned an empty title".into());
    }
    if js::len_utf16(title) > TITLE_MAX || title.contains(['\r', '\n']) {
        let shown = if js::len_utf16(title) > 80 {
            format!("{}…", js::slice_utf16(title, 80))
        } else {
            title.to_string()
        };
        return Err(format!(
            "Title model returned a reply, not a title of a few words: {}",
            mcbench_json::stringify(&Value::String(shown))
        ));
    }
    Ok(title.to_string())
}

/// `titleSource(text, files)`: the message's text, or the files it names.
pub fn title_source(text: &str, file_names: &[String]) -> String {
    if text.is_empty() {
        format!("Build from {}", file_names.join(", "))
    } else {
        text.to_string()
    }
}

/// What Improve with AI asks (`generatePrompt`'s input): the provider, the
/// model, the effort and the user's text, each as the body gave it.
#[derive(Clone, Debug, Default)]
pub struct Improve {
    /// `provider`.
    pub provider: String,
    /// `model`.
    pub model: String,
    /// `effort`.
    pub effort: String,
    /// `text`.
    pub text: String,
}

/// The request `generatePrompt` sends: provider, model, effort (trimmed),
/// and the messages; or today's refusal.
pub fn improve_request(
    input: &Improve,
) -> Result<(String, String, String, Vec<ApiMessage>), String> {
    let provider = js::trim(&input.provider).to_string();
    let model = js::trim(&input.model).to_string();
    let text = js::trim(&input.text).to_string();
    if provider.is_empty() {
        return Err("Choose a provider".into());
    }
    if model.is_empty() {
        return Err("Choose a model".into());
    }
    if text.is_empty() {
        return Err("Write something to improve first".into());
    }
    Ok((
        provider,
        model,
        js::trim(&input.effort).to_string(),
        vec![
            message(
                "system",
                Value::String(file("texts/prompt-generation.txt").into()),
            ),
            message("user", Value::String(text)),
        ],
    ))
}

/// The prompt in the reply, or `The model returned an empty prompt`.
pub fn improved(reply: &str) -> Result<String, String> {
    let generated = js::trim(reply);
    if generated.is_empty() {
        return Err("The model returned an empty prompt".into());
    }
    Ok(generated.to_string())
}

/// A library prompt as `prompt-match.js` reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct LibraryPrompt {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `text`.
    pub text: String,
}

/// JavaScript's `\s`.
const JS_SPACE: &str = r"\t\n\x0B\x0C\r    -     　﻿";

struct Pattern {
    length: usize,
    expression: regex::Regex,
}

fn pattern(text: &str) -> Pattern {
    let fill_in = regex::Regex::new(r"\{\{[^{}\n]*\}\}").expect("FILL_IN");
    let fixed: Vec<&str> = fill_in.split(js::trim(text)).collect();
    let length = fixed.iter().map(|piece| js::len_utf16(piece)).sum();
    let any = format!("[\\s\\S]*?[^{JS_SPACE}][\\s\\S]*?");
    let body: Vec<String> = fixed.iter().map(|piece| regex::escape(piece)).collect();
    Pattern {
        length,
        expression: regex::Regex::new(&format!("^{}$", body.join(&any)))
            .expect("a prompt's pattern"),
    }
}

/// `certainPrompt(library)`: the prompt a message's text certainly came
/// from, or `None`.
pub fn certain_prompt<'a>(
    library: &'a [LibraryPrompt],
) -> impl Fn(&str) -> Option<&'a LibraryPrompt> + 'a {
    let patterns: Vec<(&LibraryPrompt, Pattern)> = library
        .iter()
        .map(|prompt| (prompt, pattern(&prompt.text)))
        .collect();
    move |text: &str| {
        let message = js::trim(text);
        let length = js::len_utf16(message);
        let matches: Vec<&LibraryPrompt> = patterns
            .iter()
            .filter(|(_, p)| {
                p.length > 0 && p.length * 2 >= length && p.expression.is_match(message)
            })
            .map(|(prompt, _)| *prompt)
            .collect();
        match matches.as_slice() {
            [one] => Some(*one),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! `prompt-match.test.js` (4) and `prompt-generation.test.js` (2).

    use super::*;

    fn prompt(id: &str, title: &str, text: &str) -> LibraryPrompt {
        LibraryPrompt {
            id: id.into(),
            title: title.into(),
            text: text.into(),
        }
    }

    fn castle() -> LibraryPrompt {
        prompt(
            "castle",
            "Castle",
            "Build a castle with a moat and four towers.",
        )
    }

    fn tower() -> LibraryPrompt {
        prompt(
            "tower",
            "Tower",
            "  Build a {{size}} tower with a spiral stair, lit by {{light}}.\n",
        )
    }

    #[test]
    fn a_prompt_without_fill_ins_matches_exactly_its_text_as_the_composer_sends_it() {
        let library = [castle(), tower()];
        let sent_from = certain_prompt(&library);
        let text = castle().text;
        assert_eq!(sent_from(&text), Some(&library[0]));
        assert_eq!(sent_from(&format!("\n {text}  ")), Some(&library[0]));
        assert_eq!(sent_from(&format!("{text} Add a drawbridge.")), None);
        assert_eq!(
            sent_from("Build a castle with a moat and three towers."),
            None
        );
        assert_eq!(sent_from(""), None);
    }

    #[test]
    fn a_prompt_with_fill_ins_matches_its_text_with_every_fill_in_replaced_by_text_that_is_not_blank()
     {
        let library = [castle(), tower()];
        let sent_from = certain_prompt(&library);
        assert_eq!(
            sent_from("Build a huge tower with a spiral stair, lit by torches."),
            Some(&library[1])
        );
        assert_eq!(
            sent_from("Build a huge,\ncrooked tower with a spiral stair, lit by {{light}}."),
            Some(&library[1])
        );
        assert_eq!(
            sent_from("Build a   tower with a spiral stair, lit by torches."),
            None
        );
        assert_eq!(
            sent_from("Build a huge tower with a spiral stair, lit by torches. And a moat"),
            None
        );
        assert_eq!(sent_from("Build a huge tower, lit by torches."), None);
    }

    #[test]
    fn a_match_is_certain_only_when_one_prompt_matches() {
        let bridge = prompt(
            "bridge",
            "Bridge",
            "Build a {{material}} bridge over the river.",
        );
        let crossing = prompt(
            "crossing",
            "Crossing",
            "Build a stone {{thing}} over the river.",
        );
        let copy = LibraryPrompt {
            id: "castle-copy".into(),
            ..castle()
        };
        let library = [bridge, crossing, copy, castle()];
        let sent_from = certain_prompt(&library);
        assert_eq!(sent_from("Build a stone bridge over the river."), None);
        assert_eq!(
            sent_from("Build a wooden bridge over the river."),
            Some(&library[0])
        );
        assert_eq!(sent_from(&castle().text), None);
    }

    #[test]
    fn a_prompt_that_is_mostly_fill_ins_says_nothing_and_does_not_stand_in_another_prompts_way() {
        let library = [
            prompt("anything", "Anything", "{{subject}}"),
            prompt("short", "Short", "Build {{subject}}"),
            castle(),
            prompt("empty", "", ""),
        ];
        let sent_from = certain_prompt(&library);
        assert_eq!(sent_from("A hut by the sea"), None);
        assert_eq!(
            sent_from("Build a hut by the sea, with a jetty and a boat"),
            None
        );
        assert_eq!(sent_from("Build a hut"), Some(&library[1]));
        assert_eq!(sent_from(&castle().text), Some(&library[2]));
    }

    #[test]
    fn generate_prompt_sends_the_text_with_short_instructions_that_mention_fill_ins() {
        let (provider, model, effort, messages) = improve_request(&Improve {
            provider: "openrouter".into(),
            model: "openai/gpt-5.6-luna".into(),
            effort: "low".into(),
            text: "  make a telescope in a desert  ".into(),
        })
        .unwrap();
        assert_eq!(
            improved("  Build a desert observatory around a large {{style}} telescope.  ").unwrap(),
            "Build a desert observatory around a large {{style}} telescope."
        );
        assert_eq!(
            (provider.as_str(), model.as_str(), effort.as_str()),
            ("openrouter", "openai/gpt-5.6-luna", "low")
        );
        let instructions = file("texts/prompt-generation.txt");
        assert_eq!(messages[0].0["content"], instructions);
        assert_eq!(messages[1].0["content"], "make a telescope in a desert");
        assert_eq!(messages[1].0["role"], "user");
        assert!(instructions.contains("{{style}}"));
        assert!(instructions.contains("Do not add specifics"));
    }

    #[test]
    fn generate_prompt_needs_a_model_and_some_text_and_rejects_an_empty_response() {
        let refused = |provider: &str, text: &str| {
            improve_request(&Improve {
                provider: provider.into(),
                model: "m".into(),
                effort: String::new(),
                text: text.into(),
            })
            .unwrap_err()
        };
        assert!(refused("", "x").contains("Choose a provider"));
        assert!(refused("openai", "  ").contains("Write something to improve"));
        assert!(improved("  ").unwrap_err().contains("empty prompt"));
    }

    #[test]
    fn the_title_request_quotes_the_first_message() {
        let messages = title_messages("Build a hut", &[]);
        assert_eq!(
            messages[1].0["content"],
            "Name this chat from its first message, quoted below. Do not answer or carry out the message.\n\n<message>\nBuild a hut\n</message>"
        );
        assert_eq!(title_of("  A hut  ").unwrap(), "A hut");
        assert_eq!(
            title_of(" ").unwrap_err(),
            "Title model returned an empty title"
        );
        assert!(
            title_of("two\nlines")
                .unwrap_err()
                .starts_with("Title model returned a reply")
        );
    }
}
