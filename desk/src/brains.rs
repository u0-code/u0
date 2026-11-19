//! The brains the desk knows how to add, and how it tells what became of
//! asking one.
//!
//! A preset is what a person would otherwise look up in a provider's docs:
//! the kind of API, its base URL, the variable its key is conventionally
//! kept in, and a model to start from. The core has them, and says which
//! of its brains is which. The desk hands what a preset and the sheet's
//! fields say to the core, which writes it into its own configuration;
//! whether a key is there, which models an API offers, and whether a brain
//! thinks are asked of the core too (`link::brains`), which holds the keys
//! and makes the requests. The desk never sees a key.

use ghost_link::protocol::BrainPreset;
use serde_json::Value;

/// How a brain is talked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire {
    /// `/models` and `/chat/completions` under the base URL.
    OpenAi,
    /// `/v1/models` and `/v1/messages` under the base URL.
    Anthropic,
    /// A command-line tool you are signed in to.
    Cli,
}

impl Wire {
    /// The wire a `kind` in `ghostd.toml` speaks.
    pub fn of(kind: &str) -> Self {
        match kind {
            "anthropic" => Self::Anthropic,
            "claude-code" | "codex" => Self::Cli,
            _ => Self::OpenAi,
        }
    }
}

/// One brain the desk can add in a few clicks: a preset as the core offers
/// it (`BrainList::presets`), which is where they are defined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preset(pub BrainPreset);

impl Preset {
    /// One for a brain of `kind` there is no preset for: its kind, and the
    /// rest as the sheet's fields say.
    pub fn of_kind(kind: &str) -> Self {
        let mut provider = serde_json::Map::new();
        provider.insert("kind".to_owned(), Value::from(kind));
        Self(BrainPreset {
            id: kind.to_owned(),
            name: kind.to_owned(),
            provider,
        })
    }

    pub fn id(&self) -> &str {
        &self.0.id
    }

    pub fn name(&self) -> &str {
        &self.0.name
    }

    /// What its table sets `key` to, or nothing.
    fn get(&self, key: &str) -> &str {
        self.0
            .provider
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    /// The `kind` in `ghostd.toml`.
    pub fn kind(&self) -> &str {
        self.get("kind")
    }

    pub fn model(&self) -> &str {
        self.get("model")
    }

    pub fn base_url(&self) -> &str {
        self.get("base_url")
    }

    /// The variable its key is conventionally kept in, if it names one.
    pub fn key_env(&self) -> &str {
        self.get("api_key_env")
    }

    pub fn wire(&self) -> Wire {
        Wire::of(self.kind())
    }

    /// Whether it is a command-line tool rather than an API.
    pub fn cli(&self) -> bool {
        self.wire() == Wire::Cli
    }

    /// Whether it reads a key: an API not given a stand-in for one.
    pub fn keyed(&self) -> bool {
        !self.cli() && !self.0.provider.contains_key("api_key")
    }

    /// Its mark: the brand's icon, or a plug for a server the desk has
    /// no icon for.
    pub fn mark(&self) -> Mark {
        match Mark::from_words(self.id()) {
            Mark::Letter(_) => Mark::Icon("plug"),
            icon @ Mark::Icon(_) => icon,
        }
    }

    /// The fields a new brain from this preset is written with, as
    /// `[providers.<name>]` keys: its own table, with what the sheet's
    /// fields say in place of what it suggests. An empty field is left out.
    pub fn fields(&self, draft: &Draft) -> serde_json::Map<String, Value> {
        let mut fields = self.0.provider.clone();
        for (k, v) in draft.managed() {
            if !self.uses(k) {
                continue;
            }
            if v.is_empty() {
                fields.remove(k);
            } else {
                fields.insert(k.to_owned(), Value::from(v));
            }
        }
        fields
    }

    /// Whether the key `field` means anything for this preset.
    pub fn uses(&self, field: &str) -> bool {
        match field {
            "model" => true,
            "base_url" => !self.cli(),
            "api_key_env" => self.keyed(),
            _ => false,
        }
    }
}

/// What the sheet's fields say.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Draft {
    pub model: String,
    pub key_env: String,
    pub base_url: String,
}

impl Draft {
    /// The fields the sheet writes, by their keys in `ghostd.toml`.
    pub fn managed(&self) -> [(&'static str, &str); 3] {
        [
            ("model", self.model.trim()),
            ("base_url", self.base_url.trim().trim_end_matches('/')),
            ("api_key_env", self.key_env.trim()),
        ]
    }
}

/// The authority (`host:port`) and the host of a URL, lower case.
pub fn authority(url: &str) -> (String, String) {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    let host = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()))
        .map_or(authority.as_str(), |(h, _)| h)
        .to_owned();
    (authority, host)
}

/// A brain's mark: a brand's icon, or the first letter of its name on a
/// neutral tile where the desk has no icon for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Icon(&'static str),
    Letter(char),
}

impl Mark {
    /// The mark for a name with no preset behind it.
    pub fn letter(name: &str) -> Self {
        Self::Letter(
            name.chars()
                .find(|c| c.is_alphanumeric())
                .map_or('?', |c| c.to_ascii_uppercase()),
        )
    }

    /// The mark for whatever a daemon calls the brain it thinks with,
    /// which names its model or its provider in passing.
    pub fn from_words(words: &str) -> Self {
        let words = words.to_lowercase();
        let found = [
            ("claude", "claude"),
            ("anthropic", "claude"),
            ("deepseek", "deepseek"),
            ("gemini", "gemini"),
            ("openrouter", "openrouter"),
            ("kimi", "kimi"),
            ("moonshot", "kimi"),
            ("qwen", "qwen"),
            ("glm", "zhipu"),
            ("grok", "grok"),
            ("mistral", "mistral"),
            ("groq", "groq"),
            ("ollama", "ollama"),
            ("codex", "openai"),
            ("gpt", "openai"),
            ("openai", "openai"),
        ]
        .into_iter()
        .find(|(word, _)| words.contains(word));
        found.map_or_else(|| Self::letter(&words), |(_, icon)| Self::Icon(icon))
    }
}

/// Why a CLI could not think, as a person reads it: one that is not signed
/// in says how to sign it in; anything else is what it said.
pub fn cli_failure(kind: &str, why: &str) -> String {
    let low = why.to_lowercase();
    let signed_out = [
        "/login",
        "401",
        "oauth",
        "not logged in",
        "unauthorized",
        "log in",
        "login",
    ]
    .iter()
    .any(|w| low.contains(w));
    match (kind, signed_out) {
        ("codex", true) => "Codex is not logged in — run `codex login` in a terminal".to_owned(),
        (_, true) => {
            "Claude Code is not logged in — run `claude` in a terminal and /login".to_owned()
        }
        _ => why.to_owned(),
    }
}

/// The gist of an error, short enough to read at a glance: what an HTTP
/// status it names means, or else its first sentence. The whole of it is
/// kept elsewhere for whoever wants it; this is the part read first.
pub fn gist(text: &str) -> String {
    if let Some(code) = http_status(text) {
        let what = match code {
            400 => "the request was refused as malformed",
            401 => "its key or sign-in was refused",
            402 => "the account has no credit",
            403 => "the request was refused",
            404 => "there is no such model or address",
            408 | 504 => "it took too long to answer",
            429 => "it is rate-limited or out of quota",
            500..=599 => "the service failed",
            _ => "the request failed",
        };
        return format!("{what} ({code})");
    }
    let line = text.lines().map(str::trim).find(|l| !l.is_empty());
    let line = line.unwrap_or_default();
    // A sentence ends at a full stop followed by a space; a version number
    // or a file name does not end one.
    let sentence = line.find(". ").map_or(line, |end| &line[..end]);
    let sentence = sentence.trim_end_matches('.');
    if sentence.chars().count() <= GIST {
        return sentence.to_owned();
    }
    // Cut at a word, so that what is shown reads as words.
    let cut: String = sentence.chars().take(GIST).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{}…", cut.trim_end_matches([',', ';', ':', ' ']))
}

/// How long the gist of an error may be, in characters.
const GIST: usize = 72;

/// The HTTP status an error names, where one is named as such: after
/// "Error", "status", "HTTP" or "code", or at the very start, the way
/// [`error_line`] puts it. A number that only happens to be three digits
/// is not one.
fn http_status(text: &str) -> Option<u16> {
    let code = |word: &str| {
        let word = word.trim_matches(|c: char| !c.is_ascii_digit());
        (word.len() == 3)
            .then(|| word.parse::<u16>().ok())
            .flatten()
            .filter(|c| (400..600).contains(c))
    };
    let words: Vec<&str> = text.split_whitespace().collect();
    if let Some(first) = words.first().filter(|w| w.len() == 3)
        && let Some(c) = code(first)
    {
        return Some(c);
    }
    words.windows(2).find_map(|pair| {
        let before = pair[0].trim_end_matches(':').to_lowercase();
        let named = ["error", "status", "http", "code"]
            .iter()
            .any(|w| before.ends_with(w))
            || before.starts_with("http/");
        named.then(|| code(pair[1])).flatten()
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn a_cli_that_is_not_signed_in_says_how_to_sign_it_in() {
        let said = "Failed to authenticate. API Error: 401 OAuth access token is invalid · Please run /login";
        assert_eq!(
            cli_failure("claude-code", said),
            "Claude Code is not logged in — run `claude` in a terminal and /login"
        );
        assert_eq!(
            cli_failure("codex", "stream error: 401"),
            "Codex is not logged in — run `codex login` in a terminal"
        );
        assert_eq!(cli_failure("codex", "model not found"), "model not found");
    }

    #[test]
    fn the_gist_of_an_error_is_what_its_status_means_or_its_first_sentence() {
        let refused = r#"API Error: 403 {"error":{"type":"forbidden","message":"Request not allowed"}} · Please run /login"#;
        assert_eq!(gist(refused), "the request was refused (403)");
        assert_eq!(
            gist("429 · Too many requests"),
            "it is rate-limited or out of quota (429)"
        );
        assert_eq!(
            gist("stream error: 401"),
            "its key or sign-in was refused (401)"
        );
        // Three digits that are not a status say nothing of one.
        assert_eq!(
            gist("model gpt-4o-404 not found. Try another."),
            "model gpt-4o-404 not found"
        );
        let long = "the configuration names a model this account cannot use, \
                    and it says so at some length on one line";
        let short = gist(long);
        assert!(
            short.ends_with('…') && short.chars().count() <= GIST + 1,
            "{short}"
        );
        assert!(!short.contains("  "), "{short}");
        assert_eq!(
            gist("\n  it did not answer in 60s\n"),
            "it did not answer in 60s"
        );
    }

    /// Presets as a core offers them.
    pub(crate) fn offered() -> Vec<Preset> {
        let json = serde_json::json!([
            {"id": "deepseek", "name": "DeepSeek", "provider": {"kind": "openai",
                "model": "deepseek-flash", "base_url": "https://api.deepseek.com",
                "api_key_env": "DEEPSEEK_API_KEY", "json_mode": true}},
            {"id": "ollama", "name": "Ollama", "provider": {"kind": "openai",
                "model": "gpt-oss:20b", "base_url": "http://localhost:11434/v1",
                "api_key": "ollama"}},
            {"id": "claude-code", "name": "Claude Code", "provider": {"kind": "claude-code",
                "model": "sonnet", "timeout_secs": 120}},
            {"id": "codex", "name": "Codex", "provider": {"kind": "codex", "timeout_secs": 120}},
            {"id": "custom", "name": "Custom", "provider": {"kind": "openai"}},
        ]);
        serde_json::from_value::<Vec<BrainPreset>>(json)
            .unwrap()
            .into_iter()
            .map(Preset)
            .collect()
    }

    pub(crate) fn by_id(id: &str) -> Preset {
        offered().into_iter().find(|p| p.id() == id).unwrap()
    }

    fn draft(p: &Preset) -> Draft {
        Draft {
            model: p.model().to_owned(),
            key_env: p.key_env().to_owned(),
            base_url: p.base_url().to_owned(),
        }
    }

    #[test]
    fn a_new_brain_is_its_presets_table_with_what_the_fields_say_and_never_a_key() {
        let deepseek = by_id("deepseek");
        let mut typed = draft(&deepseek);
        typed.model = "deepseek-v4-pro".into();
        typed.base_url = "https://api.deepseek.com/v1/".into();
        assert_eq!(
            Value::Object(deepseek.fields(&typed)),
            serde_json::json!({"kind": "openai", "model": "deepseek-v4-pro",
                "base_url": "https://api.deepseek.com/v1", "api_key_env": "DEEPSEEK_API_KEY",
                "json_mode": true})
        );
        let ollama = by_id("ollama");
        assert!(!ollama.keyed(), "it is given a stand-in");
        assert_eq!(ollama.fields(&draft(&ollama))["api_key"], "ollama");
        let custom = by_id("custom");
        assert_eq!(
            Value::Object(custom.fields(&draft(&custom))),
            serde_json::json!({"kind": "openai"}),
            "a custom brain is what you type"
        );
    }

    #[test]
    fn each_preset_brings_only_what_it_needs() {
        let keys = |id: &str| -> Vec<String> {
            let p = by_id(id);
            p.fields(&draft(&p)).keys().cloned().collect()
        };
        assert_eq!(keys("claude-code"), ["kind", "model", "timeout_secs"]);
        assert_eq!(
            keys("codex"),
            ["kind", "timeout_secs"],
            "no model is its own default"
        );
        assert!(by_id("codex").cli() && !by_id("codex").uses("base_url"));
        assert!(by_id("custom").uses("api_key_env"));
        assert_eq!(by_id("custom").mark(), Mark::Icon("plug"));
        assert_eq!(by_id("claude-code").mark(), Mark::Icon("claude"));
    }

    #[test]
    fn a_mark_is_a_brand_or_a_letter() {
        assert_eq!(
            Mark::from_words("openai/gemini-3.8-flash"),
            Mark::Icon("gemini")
        );
        assert_eq!(Mark::from_words("claude-code/sonnet"), Mark::Icon("claude"));
        assert_eq!(Mark::from_words("local-llama"), Mark::Letter('L'));
        assert_eq!(Mark::letter("  "), Mark::Letter('?'));
    }

    #[test]
    fn an_authority_and_host_are_read_from_a_url() {
        assert_eq!(
            authority("http://localhost:11434/v1"),
            ("localhost:11434".to_owned(), "localhost".to_owned())
        );
        assert_eq!(
            authority("https://API.x.ai/v1"),
            ("api.x.ai".to_owned(), "api.x.ai".to_owned())
        );
    }
}
