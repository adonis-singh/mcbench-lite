//! Search everything, the palette (Mod+K; inventory `app.md` 4.2,
//! `web/app.js` 4929-5137), with no GPUI types: which items it lists for a
//! query, in which groups, how they rank, and the words they mark. Builds
//! and benches come from the core's full-text search (`Command::Search`,
//! both scopes); prompts, actions and settings are matched here. The
//! highlight belongs to an item by its key ([`super::command`]).

use super::keymap::BindAction;
use super::place::Mode;
use crate::SearchHit;

/// What an item shows as its icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// An icon by its page name (`build`, `bench`, `prompts`, `compose`,
    /// `swap`, `trophy`, `panel`, `sun`, `moon`, `settings`).
    Icon(&'static str),
    /// A provider's logo (`logoName`).
    Provider,
}

/// What running an item does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Run {
    /// Opens a Build chat (`goBuild(id)`).
    Build(String),
    /// Opens a bench's comparison.
    Bench(String),
    /// Opens a library prompt (`openPrompt`).
    Prompt(String),
    /// `New chat`: in Build the new chat, in Bench Build's new chat.
    NewChat,
    /// `New bench`.
    NewBench,
    /// Switches to the mode.
    Switch(Mode),
    /// Bench's leaderboard.
    Leaderboard,
    /// Shows or hides the sidebar.
    Sidebar,
    /// Flips the theme.
    Theme,
    /// Opens Settings on the default pane.
    Settings,
    /// Opens Settings on the General pane at this row (`openSetting`).
    Setting(String),
    /// Opens Settings on a provider's pane.
    Provider(String),
}

/// An item's second line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detail {
    /// A search snippet with the core's marks (`\u{1}`, `\u{2}`).
    Snippet(String),
    /// Text whose query words are marked (`markWords`).
    Text(String),
    /// A prompt's excerpt: marked like text, after a leading `…` when cut.
    Excerpt {
        /// It was cut at its start.
        cut: bool,
        /// The excerpt.
        text: String,
    },
}

/// An item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Its key (`build:<id>`, `new-chat`, `setting:<title>` …).
    pub key: String,
    /// Its icon.
    pub glyph: Glyph,
    /// A provider row's provider (its logo).
    pub provider: Option<String>,
    /// Its title.
    pub title: String,
    /// Its second line.
    pub detail: Option<Detail>,
    /// The shortcut it shows at its right.
    pub hint: Option<BindAction>,
    /// What it does.
    pub run: Run,
    search: String,
    keywords: String,
}

/// A group of items.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Its heading.
    pub title: &'static str,
    /// Its items, best first.
    pub items: Vec<Item>,
}

/// A library prompt as the palette searches it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    /// Its id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its text.
    pub text: String,
    /// Its tags (searched, not shown).
    pub tags: Vec<String>,
}

/// A provider as its Settings row needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderRow {
    /// Its key.
    pub id: String,
    /// Its label.
    pub label: String,
    /// A harness.
    pub harness: bool,
}

/// What the palette lists from.
pub struct Sources<'a> {
    /// The query as typed.
    pub query: &'a str,
    /// The mode on screen.
    pub mode: Mode,
    /// The Build chats, newest first: id and title.
    pub chats: &'a [(String, String)],
    /// The benches, newest first: id and title.
    pub benches: &'a [(String, String)],
    /// The core's build results, once landed for this query.
    pub found_build: Option<&'a [SearchHit]>,
    /// The core's bench results, once landed for this query.
    pub found_bench: Option<&'a [SearchHit]>,
    /// The library's prompts.
    pub prompts: &'a [Prompt],
    /// The providers.
    pub providers: &'a [ProviderRow],
    /// The sidebar is shown.
    pub sidebar_open: bool,
    // narrow agent
    /// The window is narrow: the sidebar is a bar and its list a drawer.
    pub narrow: bool,
    /// The light theme is on.
    pub light: bool,
}

/// The query's words, each matched on its own and in any order
/// (`commandWords`).
pub fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// How well an item matches every word (`matchScore`): each word scores its
/// best field, 2 in the title and 1 elsewhere, doubled at the start of a
/// word; none when some word matches nowhere.
pub fn match_score(words: &[String], title: &str, others: &[&str]) -> Option<u32> {
    let mut total = 0;
    for word in words {
        let best = std::iter::once(title)
            .chain(others.iter().copied())
            .enumerate()
            .map(|(field, text)| {
                let lower: Vec<char> = text.to_lowercase().chars().collect();
                let needle: Vec<char> = word.chars().collect();
                let at = (0..lower.len().saturating_sub(needle.len()) + 1)
                    .find(|i| lower[*i..].starts_with(&needle) && *i + needle.len() <= lower.len());
                match at {
                    None => 0,
                    Some(at) => {
                        let start = at == 0
                            || !(lower[at - 1].is_ascii_lowercase()
                                || lower[at - 1].is_ascii_digit());
                        (if field == 0 { 2 } else { 1 }) * if start { 2 } else { 1 }
                    }
                }
            })
            .max()
            .unwrap_or(0);
        if best == 0 {
            return None;
        }
        total += best;
    }
    Some(total)
}

/// `text` split into runs, each query word marked (`markWords`), matched
/// without regard to case.
pub fn mark_words(text: &str, words: &[String]) -> Vec<(String, bool)> {
    if words.is_empty() || text.is_empty() {
        return vec![(text.to_string(), false)];
    }
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<char> = chars
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let needles: Vec<Vec<char>> = words.iter().map(|w| w.chars().collect()).collect();
    let mut parts: Vec<(String, bool)> = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < chars.len() {
        // The first word of the alternation that matches here, as a
        // regular expression's alternation takes it.
        let hit = needles
            .iter()
            .find(|n| !n.is_empty() && lower[i..].starts_with(n));
        match hit {
            Some(n) => {
                if !plain.is_empty() {
                    parts.push((std::mem::take(&mut plain), false));
                }
                parts.push((chars[i..i + n.len()].iter().collect(), true));
                i += n.len();
            }
            None => {
                plain.push(chars[i]);
                i += 1;
            }
        }
    }
    if !plain.is_empty() {
        parts.push((plain, false));
    }
    parts
}

/// A prompt's text around the first query word in it, or its first line
/// (`excerpt`): from the space at least 30 characters before the word when
/// the word is more than 40 characters in, to 200 characters after the
/// word's start. Counted in UTF-16 units, as the page counts.
pub fn excerpt(text: &str, words: &[String]) -> Detail {
    let units: Vec<u16> = text.encode_utf16().collect();
    let lower: Vec<u16> = text.to_lowercase().encode_utf16().collect();
    let found = words
        .iter()
        .filter_map(|w| {
            let n: Vec<u16> = w.encode_utf16().collect();
            (0..lower.len()).find(|i| lower[*i..].starts_with(&n))
        })
        .min();
    let Some(at) = found.filter(|_| lower.len() == units.len()) else {
        return Detail::Text(text.split('\n').next().unwrap_or("").to_string());
    };
    let from = if at > 40 {
        units[..=at - 30]
            .iter()
            .rposition(|u| *u == u16::from(b' '))
            .map_or(0, |i| i + 1)
    } else {
        0
    };
    let end = (at + 200).min(units.len());
    Detail::Excerpt {
        cut: from > 0,
        text: String::from_utf16_lossy(&units[from..end]),
    }
}

fn rank(words: &[String], items: Vec<Item>) -> Vec<Item> {
    if words.is_empty() {
        return items;
    }
    let mut scored: Vec<(u32, Item)> = items
        .into_iter()
        .filter_map(|item| {
            match_score(words, &item.title, &[&item.search, &item.keywords]).map(|s| (s, item))
        })
        .collect();
    // Stable: ties keep their order.
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    scored.into_iter().map(|(_, item)| item).collect()
}

fn item(key: String, glyph: Glyph, title: String, run: Run) -> Item {
    Item {
        key,
        glyph,
        provider: None,
        title,
        detail: None,
        hint: None,
        run,
        search: String::new(),
        keywords: String::new(),
    }
}

/// The palette's groups for `sources` (`commandGroups`), each shown only
/// with items, each cut to its limit.
pub fn groups(sources: &Sources) -> Vec<Group> {
    let words = words(sources.query);
    let searching = !words.is_empty();
    let mut groups = Vec::new();
    let mut add = |title: &'static str, items: Vec<Item>, limit: usize| {
        if !items.is_empty() {
            groups.push(Group {
                title,
                items: items.into_iter().take(limit).collect(),
            });
        }
    };
    let found = |hits: Option<&[SearchHit]>,
                 rows: &[(String, String)]|
     -> Vec<(String, String, Option<String>)> {
        hits.unwrap_or(&[])
            .iter()
            .filter_map(|h| {
                rows.iter()
                    .find(|(id, _)| *id == h.id)
                    .map(|(id, title)| (id.clone(), title.clone(), h.snippet.clone()))
            })
            .collect()
    };
    let listed = |rows: &[(String, String)]| -> Vec<(String, String, Option<String>)> {
        rows.iter()
            .map(|(id, title)| (id.clone(), title.clone(), None))
            .collect()
    };
    let builds = if searching {
        found(sources.found_build, sources.chats)
    } else {
        listed(sources.chats)
    };
    add(
        if searching { "Builds" } else { "Recent builds" },
        builds
            .into_iter()
            .map(|(id, title, snippet)| Item {
                detail: snippet.filter(|s| !s.is_empty()).map(Detail::Snippet),
                ..item(
                    format!("build:{id}"),
                    Glyph::Icon("build"),
                    title,
                    Run::Build(id),
                )
            })
            .collect(),
        if searching { 6 } else { 4 },
    );
    let benches = if searching {
        found(sources.found_bench, sources.benches)
    } else {
        listed(sources.benches)
    };
    add(
        if searching {
            "Benches"
        } else {
            "Recent benches"
        },
        benches
            .into_iter()
            .map(|(id, title, snippet)| Item {
                detail: snippet.filter(|s| !s.is_empty()).map(Detail::Snippet),
                ..item(
                    format!("bench:{id}"),
                    Glyph::Icon("bench"),
                    title,
                    Run::Bench(id),
                )
            })
            .collect(),
        if searching { 5 } else { 3 },
    );
    if searching {
        let prompts = sources
            .prompts
            .iter()
            .map(|p| Item {
                search: p.text.clone(),
                keywords: p.tags.join(" "),
                detail: Some(excerpt(&p.text, &words)),
                ..item(
                    format!("prompt:{}", p.id),
                    Glyph::Icon("prompts"),
                    if p.title.is_empty() {
                        "Untitled prompt".to_string()
                    } else {
                        p.title.clone()
                    },
                    Run::Prompt(p.id.clone()),
                )
            })
            .collect();
        add("Prompts", rank(&words, prompts), 4);
    }
    let other = sources.mode.other();
    let with = |mut i: Item, keywords: &str, hint: Option<BindAction>| {
        i.keywords = keywords.to_string();
        i.hint = hint;
        i
    };
    let actions = vec![
        with(
            item(
                "new-chat".into(),
                Glyph::Icon("compose"),
                "New chat".into(),
                Run::NewChat,
            ),
            "build create start",
            (sources.mode == Mode::Build).then_some(BindAction::New),
        ),
        with(
            item(
                "new-bench".into(),
                Glyph::Icon("bench"),
                "New bench".into(),
                Run::NewBench,
            ),
            "compare models create start",
            (sources.mode == Mode::Bench).then_some(BindAction::New),
        ),
        with(
            item(
                "switch".into(),
                Glyph::Icon("swap"),
                match other {
                    Mode::Build => "Switch to Build".into(),
                    Mode::Bench => "Switch to Bench".into(),
                },
                Run::Switch(other),
            ),
            "mode build bench",
            Some(match other {
                Mode::Build => BindAction::Build,
                Mode::Bench => BindAction::Bench,
            }),
        ),
        with(
            item(
                "leaderboard".into(),
                Glyph::Icon("trophy"),
                "Leaderboard".into(),
                Run::Leaderboard,
            ),
            "votes ranking ratings blind",
            None,
        ),
        with(
            item(
                "sidebar".into(),
                Glyph::Icon("panel"),
                if sources.narrow {
                    "Show the list".into()
                } else if sources.sidebar_open {
                    "Hide the sidebar".into()
                } else {
                    "Show the sidebar".into()
                },
                Run::Sidebar,
            ),
            "panel toggle drawer list",
            Some(BindAction::Sidebar),
        ),
        with(
            item(
                "theme".into(),
                Glyph::Icon(if sources.light { "moon" } else { "sun" }),
                if sources.light {
                    "Dark theme".into()
                } else {
                    "Light theme".into()
                },
                Run::Theme,
            ),
            "appearance colors dark light",
            None,
        ),
        with(
            item(
                "settings".into(),
                Glyph::Icon("settings"),
                "Settings".into(),
                Run::Settings,
            ),
            "preferences options",
            None,
        ),
    ];
    let all = actions.len();
    add(
        "Actions",
        rank(&words, actions),
        if searching { 5 } else { all },
    );
    if searching {
        let mut rows: Vec<(String, &'static str, &'static str)> = vec![
            ("Title model".into(), "General", "chat names naming"),
            ("Theme".into(), "General", "appearance colors dark light"),
            (
                "Clear leaderboard".into(),
                "Danger zone",
                "votes reset delete",
            ),
        ];
        rows.extend(BindAction::ALL.iter().map(|a| {
            (
                a.label().to_string(),
                "Shortcuts",
                "keybind bind key hotkey",
            )
        }));
        let mut settings: Vec<Item> = rows
            .into_iter()
            .map(|(title, section, keywords)| Item {
                detail: Some(Detail::Text(format!("Settings \u{b7} {section}"))),
                search: format!("Settings {section}"),
                keywords: keywords.to_string(),
                ..item(
                    format!("setting:{title}"),
                    Glyph::Icon("settings"),
                    title.clone(),
                    Run::Setting(title),
                )
            })
            .collect();
        settings.extend(sources.providers.iter().map(|p| {
            let section = if p.harness { "Harnesses" } else { "Providers" };
            Item {
                provider: Some(p.id.clone()),
                detail: Some(Detail::Text(format!("Settings \u{b7} {section}"))),
                search: format!("Settings {section}"),
                keywords: "api key login account".into(),
                ..item(
                    format!("provider:{}", p.id),
                    Glyph::Provider,
                    p.label.clone(),
                    Run::Provider(p.id.clone()),
                )
            }
        }));
        add("Settings", rank(&words, settings), 5);
    }
    groups
}

/// The text shown when no item is listed: `Searching…` until the core's
/// results landed, then `Nothing matches “<query>”`.
pub fn empty_text(query: &str, landed: bool) -> String {
    if landed {
        format!("Nothing matches \u{201c}{}\u{201d}", query.trim())
    } else {
        "Searching\u{2026}".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn sources<'a>(
        query: &'a str,
        chats: &'a [(String, String)],
        benches: &'a [(String, String)],
        prompts: &'a [Prompt],
    ) -> Sources<'a> {
        Sources {
            query,
            mode: Mode::Build,
            chats,
            benches,
            found_build: None,
            found_bench: None,
            prompts,
            providers: &[],
            sidebar_open: true,
            narrow: false,
            light: false,
        }
    }

    fn titles(groups: &[Group]) -> Vec<(&str, Vec<&str>)> {
        groups
            .iter()
            .map(|g| (g.title, g.items.iter().map(|i| i.title.as_str()).collect()))
            .collect()
    }

    #[test]
    fn with_no_query_it_lists_recent_builds_and_benches_and_every_action() {
        let chats = pairs(&[("a", "A"), ("b", "B"), ("c", "C"), ("d", "D"), ("e", "E")]);
        let benches = pairs(&[("x", "X")]);
        let groups = groups(&sources("  ", &chats, &benches, &[]));
        assert_eq!(
            titles(&groups),
            [
                ("Recent builds", vec!["A", "B", "C", "D"]),
                ("Recent benches", vec!["X"]),
                (
                    "Actions",
                    vec![
                        "New chat",
                        "New bench",
                        "Switch to Bench",
                        "Leaderboard",
                        "Hide the sidebar",
                        "Light theme",
                        "Settings"
                    ]
                ),
            ]
        );
        assert_eq!(groups[2].items[0].hint, Some(BindAction::New));
        assert_eq!(
            groups[2].items[1].hint, None,
            "New bench's hint only in Bench"
        );
    }

    #[test]
    fn a_word_at_the_start_of_a_title_counts_most_then_in_the_title_then_elsewhere() {
        let w = words("gate");
        assert_eq!(match_score(&w, "Gate house", &[]), Some(4));
        assert_eq!(
            match_score(&w, "Castle gate", &[]),
            Some(4),
            "after a space is a word's start"
        );
        assert_eq!(match_score(&w, "Tailgate", &[]), Some(2));
        assert_eq!(match_score(&w, "Wall", &["the gate"]), Some(2));
        assert_eq!(match_score(&w, "Wall", &["tailgate"]), Some(1));
        assert_eq!(match_score(&words("gate wall"), "Wall", &["gate"]), Some(6));
        assert_eq!(
            match_score(&words("gate moat"), "Gate", &[]),
            None,
            "every word must match"
        );
    }

    #[test]
    fn searching_ranks_actions_and_settings_and_lists_the_cores_results_in_its_order() {
        let chats = pairs(&[("a", "Castle gate"), ("b", "Barn")]);
        let hits = vec![
            SearchHit {
                id: "b".into(),
                snippet: Some("a \u{1}red\u{2} barn".into()),
            },
            SearchHit {
                id: "gone".into(),
                snippet: None,
            },
        ];
        let prompts = vec![Prompt {
            id: "p".into(),
            title: String::new(),
            text: "Make it red".into(),
            tags: vec!["colour".into()],
        }];
        let mut s = sources("red", &chats, &[], &prompts);
        s.found_build = Some(&hits);
        s.found_bench = Some(&[]);
        let groups = groups(&s);
        assert_eq!(groups[0].title, "Builds");
        assert_eq!(
            groups[0].items.len(),
            1,
            "a result no longer listed is left out"
        );
        assert_eq!(
            groups[0].items[0].detail,
            Some(Detail::Snippet("a \u{1}red\u{2} barn".into()))
        );
        assert_eq!(groups[1].title, "Prompts");
        assert_eq!(groups[1].items[0].title, "Untitled prompt");
        let s = sources("short", &chats, &[], &[]);
        let short = super::groups(&s);
        assert_eq!(short.len(), 1);
        assert_eq!(short[0].title, "Settings");
        assert!(
            short[0]
                .items
                .iter()
                .all(|i| i.detail == Some(Detail::Text("Settings \u{b7} Shortcuts".into())))
        );
        assert_eq!(short[0].items.len(), 5, "at most five settings");
        let tag = sources("colour", &chats, &[], &prompts);
        assert_eq!(
            super::groups(&tag)[0].items[0].key,
            "prompt:p",
            "tags are searched"
        );
    }

    #[test]
    fn the_words_are_marked_in_any_case_and_a_late_word_cuts_the_excerpt() {
        assert_eq!(
            mark_words("Castle Gate", &words("gate")),
            vec![("Castle ".into(), false), ("Gate".into(), true)]
        );
        let text = format!("{} lever here", "word ".repeat(12));
        let Detail::Excerpt { cut, text: shown } = excerpt(&text, &words("lever")) else {
            panic!("an excerpt");
        };
        assert!(cut);
        assert!(shown.starts_with("word") && shown.contains("lever"));
        assert_eq!(
            excerpt("first line\nsecond", &words("absent")),
            Detail::Text("first line".into())
        );
        assert_eq!(
            empty_text(" moat ", true),
            "Nothing matches \u{201c}moat\u{201d}"
        );
        assert_eq!(empty_text("moat", false), "Searching\u{2026}");
    }
}
