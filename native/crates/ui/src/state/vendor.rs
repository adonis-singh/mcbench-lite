//! Which logo stands for a provider and for a model (`web/vendor-icon.mjs`,
//! tests ported from `test/vendor-icon.test.js`).

/// The logos of `web/icons/` (the app's own `block.svg` aside).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Logo {
    /// `anthropic`.
    Anthropic,
    /// `claude`.
    Claude,
    /// `codex`.
    Codex,
    /// `cursor` (stored chats of today's Cursor provider still name it).
    Cursor,
    /// `deepseek`.
    Deepseek,
    /// `gemini`.
    Gemini,
    /// `groq`.
    Groq,
    /// `meta`.
    Meta,
    /// `mistral`.
    Mistral,
    /// `moonshot`.
    Moonshot,
    /// `openai`.
    Openai,
    /// `openrouter`.
    Openrouter,
    /// `qwen`.
    Qwen,
    /// `xai`.
    Xai,
}

impl Logo {
    /// Every logo.
    pub const ALL: [Logo; 14] = [
        Logo::Anthropic,
        Logo::Claude,
        Logo::Codex,
        Logo::Cursor,
        Logo::Deepseek,
        Logo::Gemini,
        Logo::Groq,
        Logo::Meta,
        Logo::Mistral,
        Logo::Moonshot,
        Logo::Openai,
        Logo::Openrouter,
        Logo::Qwen,
        Logo::Xai,
    ];

    /// Its file's name in `web/icons/`, without `.svg`.
    pub fn name(self) -> &'static str {
        match self {
            Logo::Anthropic => "anthropic",
            Logo::Claude => "claude",
            Logo::Codex => "codex",
            Logo::Cursor => "cursor",
            Logo::Deepseek => "deepseek",
            Logo::Gemini => "gemini",
            Logo::Groq => "groq",
            Logo::Meta => "meta",
            Logo::Mistral => "mistral",
            Logo::Moonshot => "moonshot",
            Logo::Openai => "openai",
            Logo::Openrouter => "openrouter",
            Logo::Qwen => "qwen",
            Logo::Xai => "xai",
        }
    }

    /// The logo of this file name; a name with no file is an error.
    pub fn named(name: &str) -> Logo {
        Logo::ALL
            .into_iter()
            .find(|l| l.name() == name)
            .unwrap_or_else(|| panic!("no logo is named {name}"))
    }
}

/// A provider's logo (`logoName`): providers drawn with another one's logo,
/// else the provider's own.
pub fn logo_name(provider: &str) -> Logo {
    Logo::named(match provider {
        "qwencloud" => "qwen",
        "anthropic" | "claude-code" => "claude",
        "codex-cli" => "codex",
        "google" => "gemini",
        other => other,
    })
}

const VENDOR_PREFIXES: [(&str, &str); 9] = [
    ("openai/", "openai"),
    ("anthropic/", "anthropic"),
    ("x-ai/", "xai"),
    ("google/", "gemini"),
    ("moonshotai/", "moonshot"),
    ("deepseek/", "deepseek"),
    ("qwen/", "qwen"),
    ("mistralai/", "mistral"),
    ("meta-llama/", "meta"),
];

const VENDOR_HINTS: [(&str, &str); 13] = [
    ("llama", "meta"),
    ("qwen", "qwen"),
    ("kimi", "moonshot"),
    ("moonshot", "moonshot"),
    ("deepseek", "deepseek"),
    ("mistral", "mistral"),
    ("mixtral", "mistral"),
    ("gemma", "gemini"),
    ("gemini", "gemini"),
    ("gpt", "openai"),
    ("whisper", "openai"),
    ("claude", "anthropic"),
    ("grok", "xai"),
];

/// A model's logo is its maker's (`vendorIcon`): a provider that serves only
/// its own models says it outright, a reseller's model by its id.
pub fn vendor_icon(provider: &str, model: &str) -> Logo {
    match provider {
        "openai" => return Logo::Openai,
        "codex" | "codex-cli" => return Logo::Codex,
        "anthropic" | "claude-code" => return Logo::Anthropic,
        "google" => return Logo::Gemini,
        _ => {}
    }
    let id = model.to_lowercase();
    if let Some((_, logo)) = VENDOR_PREFIXES.iter().find(|(p, _)| id.starts_with(p)) {
        return Logo::named(logo);
    }
    if let Some((_, logo)) = VENDOR_HINTS.iter().find(|(h, _)| id.contains(h)) {
        return Logo::named(logo);
    }
    match provider {
        "groq" => Logo::Groq,
        "cursor" => Logo::Cursor,
        _ => Logo::Openrouter,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_google_provider_and_every_model_it_serves_draw_with_the_gemini_logo() {
        assert_eq!(logo_name("google"), Logo::Gemini);
        assert_eq!(vendor_icon("google", "gemini-2.5-pro"), Logo::Gemini);
        assert_eq!(vendor_icon("google", "models/imagen-4"), Logo::Gemini);
        assert_eq!(
            vendor_icon("openrouter", "google/gemma-3-27b"),
            Logo::Gemini
        );
    }

    #[test]
    fn the_other_providers_keep_their_logos() {
        let names: Vec<Logo> = [
            "qwencloud",
            "anthropic",
            "claude-code",
            "codex-cli",
            "openai",
            "groq",
        ]
        .map(logo_name)
        .to_vec();
        assert_eq!(
            names,
            [
                Logo::Qwen,
                Logo::Claude,
                Logo::Claude,
                Logo::Codex,
                Logo::Openai,
                Logo::Groq
            ]
        );
        assert_eq!(
            vendor_icon("claude-code", "claude-haiku-5-5"),
            Logo::Anthropic
        );
        assert_eq!(vendor_icon("openrouter", "x-ai/grok-4"), Logo::Xai);
        assert_eq!(vendor_icon("groq", "some-model"), Logo::Groq);
        assert_eq!(vendor_icon("openrouter", "unknown/model"), Logo::Openrouter);
    }

    #[test]
    fn every_logo_a_provider_or_model_can_name_is_in_web_icons() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/logos");
        let files: Vec<String> = std::fs::read_dir(dir)
            .expect("the logos folder")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .trim_end_matches(".svg")
                    .to_string()
            })
            .collect();
        for provider in [
            "google",
            "qwencloud",
            "anthropic",
            "claude-code",
            "codex-cli",
            "openai",
            "groq",
            "cursor",
            "openrouter",
            "codex",
        ] {
            assert!(
                files.contains(&logo_name(provider).name().to_string()),
                "{provider}"
            );
        }
        for id in [
            "gpt-5",
            "gemini-3",
            "kimi-k2",
            "llama-4",
            "deepseek-v4",
            "mixtral",
            "qwen3",
            "grok-4",
            "claude-x",
        ] {
            assert!(
                files.contains(&vendor_icon("openrouter", id).name().to_string()),
                "{id}"
            );
        }
    }
}
