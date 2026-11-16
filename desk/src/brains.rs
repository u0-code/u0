//! The brains the desk knows how to add, and what it can learn about one
//! without the daemon: whether its key is there, which models its API
//! offers, and whether one tiny request goes through.
//!
//! A preset is what a person would otherwise look up in a provider's docs:
//! the kind of API, its base URL, the variable its key is conventionally
//! kept in, and a model to start from. Every API here but Anthropic's
//! speaks the `OpenAI`-compatible protocol, which is what `kind = "openai"`
//! in `ghostd.toml` means. A key is never written anywhere: only the name
//! of the variable that holds it, read from the environment or from the
//! checkout's `.env` when it is needed, and never shown.
//!
//! The requests go out through `ghost_net`, the way every HTTP request a
//! Ghost program makes does, so the one proxy setting holds here too.

use std::time::Duration;

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

/// One brain the desk can add in a few clicks.
#[derive(Debug, PartialEq, Eq)]
pub struct Preset {
    /// What it is saved as by default, and how it is recognised.
    pub id: &'static str,
    pub name: &'static str,
    /// Its mark among the desk's icons.
    pub icon: &'static str,
    /// The `kind` in `ghostd.toml`.
    pub kind: &'static str,
    pub base_url: &'static str,
    /// The variable its key is conventionally kept in; `None` for a brain
    /// that needs none.
    pub key_env: Option<&'static str>,
    pub model: &'static str,
    /// Hosts its base URL is on, by which a brain already configured is
    /// known for this one.
    pub hosts: &'static [&'static str],
    /// The program, for a CLI.
    pub program: Option<&'static str>,
}

const fn api(
    id: &'static str,
    name: &'static str,
    icon: &'static str,
    base_url: &'static str,
    key_env: &'static str,
    model: &'static str,
    hosts: &'static [&'static str],
) -> Preset {
    Preset {
        id,
        name,
        icon,
        kind: "openai",
        base_url,
        key_env: Some(key_env),
        model,
        hosts,
        program: None,
    }
}

/// Every preset, in the order the sheet shows them. Each base URL, key
/// variable and model is the one the provider's own documentation gives.
pub const PRESETS: &[Preset] = &[
    api(
        "deepseek",
        "DeepSeek",
        "deepseek",
        "https://api.deepseek.com",
        "DEEPSEEK_API_KEY",
        "deepseek-flash",
        &["deepseek.com"],
    ),
    api(
        "openai",
        "OpenAI",
        "openai",
        "https://api.openai.com/v1",
        "OPENAI_API_KEY",
        "gpt-6-astra",
        &["api.openai.com"],
    ),
    Preset {
        id: "anthropic",
        name: "Anthropic",
        icon: "claude",
        kind: "anthropic",
        base_url: "https://api.anthropic.com",
        key_env: Some("ANTHROPIC_API_KEY"),
        model: "claude-opus-5-5",
        hosts: &["anthropic.com"],
        program: None,
    },
    api(
        "gemini",
        "Gemini",
        "gemini",
        "https://generativelanguage.googleapis.com/v1beta/openai",
        "GEMINI_API_KEY",
        "gemini-3.8-flash",
        &["generativelanguage.googleapis.com"],
    ),
    api(
        "openrouter",
        "OpenRouter",
        "openrouter",
        "https://openrouter.ai/api/v1",
        "OPENROUTER_API_KEY",
        "openrouter/auto",
        &["openrouter.ai"],
    ),
    api(
        "kimi",
        "Kimi",
        "kimi",
        "https://api.moonshot.ai/v1",
        "MOONSHOT_API_KEY",
        "kimi-k3.1-pro",
        &["moonshot.ai", "moonshot.cn", "kimi.ai"],
    ),
    api(
        "qwen",
        "Qwen",
        "qwen",
        "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        "DASHSCOPE_API_KEY",
        "qwen4-max",
        &["aliyuncs.com"],
    ),
    api(
        "glm",
        "GLM",
        "zhipu",
        "https://api.z.ai/api/paas/v4",
        "ZAI_API_KEY",
        "glm-5.3",
        &["z.ai", "bigmodel.cn"],
    ),
    api(
        "grok",
        "Grok",
        "grok",
        "https://api.x.ai/v1",
        "XAI_API_KEY",
        "grok-4.7",
        &["x.ai"],
    ),
    api(
        "mistral",
        "Mistral",
        "mistral",
        "https://api.mistral.ai/v1",
        "MISTRAL_API_KEY",
        "mistral-large-latest",
        &["mistral.ai"],
    ),
    api(
        "groq",
        "Groq",
        "groq",
        "https://api.groq.com/openai/v1",
        "GROQ_API_KEY",
        "openai/gpt-oss-120b",
        &["groq.com"],
    ),
    Preset {
        id: "ollama",
        name: "Ollama",
        icon: "ollama",
        kind: "openai",
        base_url: "http://localhost:11434/v1",
        key_env: None,
        model: "gpt-oss:20b",
        hosts: &["localhost:11434", "127.0.0.1:11434", "ollama.com"],
        program: None,
    },
    Preset {
        id: "claude-code",
        name: "Claude Code",
        icon: "claude",
        kind: "claude-code",
        base_url: "",
        key_env: None,
        model: "sonnet",
        hosts: &[],
        program: Some("claude"),
    },
    Preset {
        id: "codex",
        name: "Codex",
        icon: "openai",
        kind: "codex",
        base_url: "",
        key_env: None,
        model: "",
        hosts: &[],
        program: Some("codex"),
    },
    Preset {
        id: "custom",
        name: "Custom",
        icon: "plug",
        kind: "openai",
        base_url: "",
        key_env: Some(""),
        model: "",
        hosts: &[],
        program: None,
    },
];

/// What a local API with no key is given: the `OpenAI`-compatible client
/// needs some key, and Ollama ignores it. It is not a secret.
pub const NO_KEY: &str = "ollama";

impl Preset {
    pub fn by_id(id: &str) -> Option<&'static Self> {
        PRESETS.iter().find(|p| p.id == id)
    }

    pub fn wire(&self) -> Wire {
        Wire::of(self.kind)
    }

    /// Whether it is a command-line tool rather than an API.
    pub fn cli(&self) -> bool {
        self.program.is_some()
    }

    /// Whether it is reached on this machine and needs no key.
    pub fn keyless(&self) -> bool {
        self.key_env.is_none() && !self.cli()
    }

    /// The preset a configured brain is, from its kind and where its API
    /// is; `None` for one the desk has no preset for.
    pub fn of(kind: &str, base_url: Option<&str>) -> Option<&'static Self> {
        match kind {
            "claude-code" | "codex" | "anthropic" => return Self::by_id(kind),
            _ => {}
        }
        let Some(base) = base_url.filter(|b| !b.is_empty()) else {
            // The OpenAI-compatible client's own default is OpenAI.
            return Self::by_id("openai");
        };
        let (authority, host) = authority(base);
        PRESETS.iter().filter(|p| p.kind == kind).find(|p| {
            p.hosts
                .iter()
                .any(|h| authority == *h || host == *h || host.ends_with(&format!(".{h}")))
        })
    }

    /// The fields a new brain from this preset is written with, as
    /// `[providers.<name>]` keys: what the sheet's fields say, and what the
    /// preset always adds. An empty field is left out.
    pub fn fields(&self, draft: &Draft) -> Vec<(&'static str, toml_edit::Value)> {
        let mut fields = vec![("kind", toml_edit::Value::from(self.kind))];
        fields.extend(
            draft
                .managed()
                .into_iter()
                .filter(|(k, v)| !v.is_empty() && self.uses(k))
                .map(|(k, v)| (k, toml_edit::Value::from(v))),
        );
        match self.id {
            // DeepSeek enforces JSON when asked, as the example deployment has it.
            "deepseek" => fields.push(("json_mode", true.into())),
            "ollama" => fields.push(("api_key", NO_KEY.into())),
            "claude-code" | "codex" => fields.push(("timeout_secs", 120.into())),
            _ => {}
        }
        fields
    }

    /// Whether the key `field` means anything for this preset.
    pub fn uses(&self, field: &str) -> bool {
        match field {
            "model" => true,
            "base_url" => !self.cli(),
            "api_key_env" => self.key_env.is_some(),
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

/// Where a key was found, if it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStatus {
    /// In the desk's own environment.
    Env,
    /// In the checkout's `.env`, which the daemon loads.
    DotEnv,
    NotSet,
}

impl KeyStatus {
    pub fn found(self) -> bool {
        self != Self::NotSet
    }
}

/// Where the key in `var` is: the environment `env` reads, else the
/// checkout's `.env` as written. An empty value is no key.
pub fn key_status(
    var: &str,
    env: &dyn Fn(&str) -> Option<String>,
    dotenv: Option<&str>,
) -> KeyStatus {
    let var = var.trim();
    if var.is_empty() {
        return KeyStatus::NotSet;
    }
    if env(var).is_some_and(|v| !v.trim().is_empty()) {
        KeyStatus::Env
    } else if dotenv_value(var, dotenv).is_some() {
        KeyStatus::DotEnv
    } else {
        KeyStatus::NotSet
    }
}

/// The key itself, from the same places, for a request the desk makes.
/// It is never shown, logged or written.
pub fn key_value(
    var: &str,
    env: &dyn Fn(&str) -> Option<String>,
    dotenv: Option<&str>,
) -> Option<String> {
    let var = var.trim();
    if var.is_empty() {
        return None;
    }
    env(var)
        .filter(|v| !v.trim().is_empty())
        .or_else(|| dotenv_value(var, dotenv))
}

/// The value `var` has in a `.env` file's text, read as the daemon reads it.
fn dotenv_value(var: &str, dotenv: Option<&str>) -> Option<String> {
    dotenvy::from_read_iter(dotenv?.as_bytes())
        .filter_map(Result::ok)
        .filter(|(k, v)| k == var && !v.trim().is_empty())
        .last()
        .map(|(_, v)| v)
}

/// The models a `models` listing names, by id, from its JSON body: the
/// `data` array both protocols answer with. Gemini's ids come as
/// `models/…`, which its chat endpoint takes without the prefix.
pub fn parse_models(body: &str) -> Result<Vec<String>, String> {
    let json: Value = serde_json::from_str(body).map_err(|_| first_line(body))?;
    let data = json
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "no model list in the answer".to_owned())?;
    let mut ids: Vec<String> = data
        .iter()
        .filter_map(|m| m.get("id").and_then(Value::as_str))
        .map(|id| id.strip_prefix("models/").unwrap_or(id).to_owned())
        .collect();
    ids.dedup();
    Ok(ids)
}

/// What went wrong, in one short line: the status and the server's own
/// message where it gave one.
pub fn error_line(status: u16, body: &str) -> String {
    let said = serde_json::from_str::<Value>(body).ok().and_then(|json| {
        let error = json.get("error").unwrap_or(&json);
        error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .or_else(|| json.get("message").and_then(Value::as_str))
            .or_else(|| json.get("detail").and_then(Value::as_str))
            .map(str::to_owned)
    });
    let said = said.unwrap_or_else(|| first_line(body));
    if said.is_empty() {
        status.to_string()
    } else {
        format!("{status} · {}", first_line(&said))
    }
}

/// The first non-empty line of `text`, cut short.
pub fn first_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty());
    let line = line.unwrap_or_default();
    if line.chars().count() > 90 {
        format!("{}…", line.chars().take(89).collect::<String>())
    } else {
        line.to_owned()
    }
}

/// Where to reach a brain, for a request the desk makes.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub wire: Wire,
    pub base_url: String,
    pub key: Option<String>,
    pub model: String,
}

/// Why a request did not give an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The server has no such endpoint.
    Unsupported,
    Failed(String),
}

impl Failure {
    pub fn line(self) -> String {
        match self {
            Self::Unsupported => "the server lists no models".to_owned(),
            Self::Failed(why) => why,
        }
    }
}

/// The Anthropic API version the daemon speaks, too.
const ANTHROPIC_VERSION: &str = "2023-06-01";
const TIMEOUT: Duration = Duration::from_secs(20);

/// The one line a failed request is told by: its innermost cause.
fn sent_error(e: &(dyn std::error::Error + 'static)) -> String {
    let mut cause = e;
    while let Some(next) = cause.source() {
        cause = next;
    }
    first_line(&cause.to_string())
}

/// The models the endpoint offers, as its `models` listing says.
pub async fn models(network: &ghost_net::Network, end: &Endpoint) -> Result<Vec<String>, Failure> {
    let client = network
        .builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| Failure::Failed(sent_error(&e)))?;
    let base = end.base_url.trim_end_matches('/');
    let key = end.key.clone().unwrap_or_default();
    let request = match end.wire {
        Wire::Anthropic => client
            .get(format!("{base}/v1/models?limit=1000"))
            .header("x-api-key", key)
            .header("anthropic-version", ANTHROPIC_VERSION),
        Wire::OpenAi => client
            .get(format!("{base}/models"))
            .header("authorization", format!("Bearer {key}")),
        Wire::Cli => return Err(Failure::Unsupported),
    };
    let response = request
        .send()
        .await
        .map_err(|e| Failure::Failed(sent_error(&e)))?;
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    match status {
        200..=299 => {
            let mut ids = parse_models(&body).map_err(Failure::Failed)?;
            if end.wire != Wire::Anthropic {
                ids.sort();
            }
            Ok(ids)
        }
        404 | 405 => Err(Failure::Unsupported),
        _ => Err(Failure::Failed(error_line(status, &body))),
    }
}

/// One real request, as small as there is: the `models` listing, or a
/// one-token answer where the server lists none. Says what came back.
pub async fn probe(network: &ghost_net::Network, end: &Endpoint) -> Result<Probed, String> {
    match models(network, end).await {
        Ok(ids) => Ok(Probed::Listed(ids)),
        Err(Failure::Unsupported) => complete(network, end).await.map(|()| Probed::Answered),
        Err(Failure::Failed(why)) => Err(why),
    }
}

/// What a probe that went through found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probed {
    Listed(Vec<String>),
    Answered,
}

/// Ask the model for one token.
async fn complete(network: &ghost_net::Network, end: &Endpoint) -> Result<(), String> {
    if end.model.trim().is_empty() {
        return Err("no model to ask".to_owned());
    }
    let client = network
        .builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| sent_error(&e))?;
    let base = end.base_url.trim_end_matches('/');
    let key = end.key.clone().unwrap_or_default();
    let message = serde_json::json!([{"role": "user", "content": "ping"}]);
    let request = match end.wire {
        Wire::Anthropic => client
            .post(format!("{base}/v1/messages"))
            .header("x-api-key", key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .body(
                serde_json::json!({"model": end.model, "max_tokens": 1, "messages": message})
                    .to_string(),
            ),
        Wire::OpenAi => client
            .post(format!("{base}/chat/completions"))
            .header("authorization", format!("Bearer {key}"))
            .body(
                serde_json::json!({"model": end.model, "max_tokens": 1, "messages": message})
                    .to_string(),
            ),
        Wire::Cli => return Err("a CLI has no API".to_owned()),
    };
    let response = request
        .header("content-type", "application/json")
        .send()
        .await
        .map_err(|e| sent_error(&e))?;
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(error_line(
            status,
            &response.text().await.unwrap_or_default(),
        ))
    }
}

/// What a CLI says of its version, which shows it runs: the first line it
/// prints, or why it did not.
pub fn cli_version(program: &str) -> Result<String, String> {
    let out = std::process::Command::new(program)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("{program}: {}", first_line(&e.to_string())))?;
    let said = first_line(&String::from_utf8_lossy(&out.stdout));
    if out.status.success() {
        Ok(said)
    } else {
        let err = first_line(&String::from_utf8_lossy(&out.stderr));
        Err(if err.is_empty() { said } else { err })
    }
}

/// How long a CLI is given to answer one small question before the desk
/// stops waiting: a CLI starts slowly, and its first request is its
/// slowest.
const CLI_DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

/// What a CLI is asked to show it can think: as little as there is.
const PROBE: &str = "Reply with just: ok";

/// Ask the CLI `program`, of the kind `kind` (`claude-code` or `codex`),
/// to think once with `model` (empty for its default), the way the daemon
/// would; or why it could not. A version alone is not enough: a CLI that is
/// signed out answers for its version and refuses to think.
pub async fn cli_probe(kind: &str, program: &str, model: &str) -> Result<(), String> {
    let mut command = tokio::process::Command::new(program);
    if kind == "codex" {
        command.args(["exec", "--skip-git-repo-check", "-s", "read-only"]);
        if !model.is_empty() {
            command.args(["-m", model]);
        }
    } else {
        command.args([
            "-p",
            "--output-format",
            "json",
            "--max-turns",
            "1",
            "--tools",
            "",
            "--strict-mcp-config",
            "--disable-slash-commands",
        ]);
        if !model.is_empty() {
            command.args(["--model", model]);
        }
        // `claude` refuses to start inside another `claude`; this is not
        // that, so the marker is not passed on, as the daemon does.
        command.env_remove("CLAUDECODE");
    }
    command
        .arg(PROBE)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = match tokio::time::timeout(CLI_DEADLINE, command.output()).await {
        Err(_) => return Err(format!("it did not answer in {}s", CLI_DEADLINE.as_secs())),
        Ok(Err(e)) => return Err(format!("{program}: {}", first_line(&e.to_string()))),
        Ok(Ok(out)) => out,
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    // `claude` says what went wrong in its JSON, and may exit non-zero
    // while doing so: the JSON is read first.
    if kind != "codex"
        && let Some(said) = claude_said(&stdout)
    {
        return said;
    }
    if out.status.success() {
        Ok(())
    } else {
        let err = first_line(&String::from_utf8_lossy(&out.stderr));
        Err(if err.is_empty() {
            first_line(&stdout)
        } else {
            err
        })
    }
}

/// What `claude -p --output-format json` answered, when it is its JSON:
/// fine, or the text of its error.
fn claude_said(stdout: &str) -> Option<Result<(), String>> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    let failed = v.get("is_error").and_then(serde_json::Value::as_bool)?;
    if failed {
        let result = v
            .get("result")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("it answered with an error");
        // All of it: the hint that says what to do ("Please run /login")
        // comes last, and a line cut short for the eye would lose it.
        Some(Err(result.trim().to_owned()))
    } else {
        Some(Ok(()))
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
mod tests {
    use super::*;

    #[test]
    fn a_signed_out_claude_answers_with_an_error_and_says_how_to_sign_in() {
        let out = r#"{"type":"result","subtype":"success","is_error":true,"result":"Failed to authenticate. API Error: 401 {\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"OAuth access token is invalid\"}} · Please run /login"}"#;
        let why = claude_said(out).unwrap().unwrap_err();
        assert!(why.starts_with("Failed to authenticate"), "{why}");
        assert_eq!(
            cli_failure("claude-code", &why),
            "Claude Code is not logged in — run `claude` in a terminal and /login"
        );
        let fine = r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#;
        assert_eq!(claude_said(fine), Some(Ok(())));
        // Something that is not its JSON is read some other way.
        assert_eq!(claude_said("command not found"), None);
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

    #[test]
    fn a_signed_out_claude_is_known_by_the_hint_at_the_end_of_a_long_error() {
        let out = r#"{"type":"result","is_error":true,"result":"API Error: 403 {\"error\":{\"type\":\"forbidden\",\"message\":\"Request not allowed\"}} · Please run /login"}"#;
        let why = claude_said(out).unwrap().unwrap_err();
        assert!(why.ends_with("Please run /login"), "{why}");
        assert!(cli_failure("claude-code", &why).contains("not logged in"));
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn every_preset_is_shown_in_the_order_asked_for() {
        let ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
        assert_eq!(
            ids,
            [
                "deepseek",
                "openai",
                "anthropic",
                "gemini",
                "openrouter",
                "kimi",
                "qwen",
                "glm",
                "grok",
                "mistral",
                "groq",
                "ollama",
                "claude-code",
                "codex",
                "custom"
            ]
        );
    }

    #[test]
    fn an_api_preset_writes_its_kind_url_model_and_key_variable_and_never_a_key() {
        let gemini = Preset::by_id("gemini").unwrap();
        let draft = Draft {
            model: gemini.model.to_owned(),
            key_env: gemini.key_env.unwrap().to_owned(),
            base_url: format!("{}/", gemini.base_url),
        };
        let fields: Vec<(&str, String)> = gemini
            .fields(&draft)
            .into_iter()
            .map(|(k, v)| (k, v.to_string().trim().to_owned()))
            .collect();
        assert_eq!(
            fields,
            [
                ("kind", "\"openai\"".to_owned()),
                ("model", "\"gemini-3.8-flash\"".to_owned()),
                (
                    "base_url",
                    "\"https://generativelanguage.googleapis.com/v1beta/openai\"".to_owned()
                ),
                ("api_key_env", "\"GEMINI_API_KEY\"".to_owned()),
            ]
        );
        for preset in PRESETS {
            let draft = Draft {
                model: preset.model.to_owned(),
                key_env: preset.key_env.unwrap_or_default().to_owned(),
                base_url: preset.base_url.to_owned(),
            };
            let fields = preset.fields(&draft);
            assert_eq!(fields[0].1.as_str(), Some(preset.kind), "{}", preset.id);
            for (k, v) in &fields {
                if *k == "api_key" {
                    assert_eq!(v.as_str(), Some(NO_KEY), "only Ollama's placeholder");
                }
            }
        }
    }

    #[test]
    fn each_preset_brings_only_what_it_needs() {
        let keys = |id: &str| -> Vec<&str> {
            let p = Preset::by_id(id).unwrap();
            let draft = Draft {
                model: p.model.to_owned(),
                key_env: p.key_env.unwrap_or_default().to_owned(),
                base_url: p.base_url.to_owned(),
            };
            p.fields(&draft).into_iter().map(|(k, _)| k).collect()
        };
        assert_eq!(
            keys("deepseek"),
            ["kind", "model", "base_url", "api_key_env", "json_mode"]
        );
        assert_eq!(
            keys("anthropic"),
            ["kind", "model", "base_url", "api_key_env"]
        );
        assert_eq!(keys("ollama"), ["kind", "model", "base_url", "api_key"]);
        assert_eq!(keys("claude-code"), ["kind", "model", "timeout_secs"]);
        assert_eq!(
            keys("codex"),
            ["kind", "timeout_secs"],
            "no model is its own default"
        );
        assert_eq!(keys("custom"), ["kind"], "a custom brain is what you type");
    }

    #[test]
    fn a_configured_brain_is_known_by_where_its_api_is() {
        let id = |kind: &str, base: Option<&str>| Preset::of(kind, base).map(|p| p.id);
        assert_eq!(
            id("openai", Some("https://api.deepseek.com/v1")),
            Some("deepseek")
        );
        assert_eq!(
            id(
                "openai",
                Some("https://generativelanguage.googleapis.com/v1beta/openai/")
            ),
            Some("gemini")
        );
        assert_eq!(
            id("openai", Some("https://open.bigmodel.cn/api/paas/v4")),
            Some("glm")
        );
        assert_eq!(
            id("openai", Some("http://localhost:11434/v1")),
            Some("ollama")
        );
        assert_eq!(id("openai", Some("https://api.x.ai/v1")), Some("grok"));
        assert_eq!(
            id("openai", Some("https://max.ai/v1")),
            None,
            "x.ai is a host, not a suffix"
        );
        assert_eq!(id("openai", None), Some("openai"));
        assert_eq!(id("openai", Some("http://localhost:8000/v1")), None);
        assert_eq!(id("claude-code", None), Some("claude-code"));
        assert_eq!(
            id("anthropic", Some("https://proxy.example")),
            Some("anthropic")
        );
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
    fn a_key_is_found_in_the_environment_or_the_checkouts_env_file() {
        let env = |k: &str| (k == "SET_KEY").then(|| "sk-env".to_owned());
        let dotenv = "# keys\nDOTENV_KEY=sk-file\nEMPTY_KEY=\nQUOTED_KEY=\"sk q\"\n";
        assert_eq!(key_status("SET_KEY", &env, Some(dotenv)), KeyStatus::Env);
        assert_eq!(
            key_status("DOTENV_KEY", &env, Some(dotenv)),
            KeyStatus::DotEnv
        );
        assert_eq!(
            key_status("EMPTY_KEY", &env, Some(dotenv)),
            KeyStatus::NotSet
        );
        assert_eq!(key_status("MISSING", &env, Some(dotenv)), KeyStatus::NotSet);
        assert_eq!(key_status("DOTENV_KEY", &no_env, None), KeyStatus::NotSet);
        assert_eq!(key_status("", &env, Some(dotenv)), KeyStatus::NotSet);
        assert_eq!(
            key_value(" DOTENV_KEY ", &env, Some(dotenv)).as_deref(),
            Some("sk-file")
        );
        assert_eq!(
            key_value("QUOTED_KEY", &no_env, Some(dotenv)).as_deref(),
            Some("sk q")
        );
        assert_eq!(
            key_value("SET_KEY", &env, Some(dotenv)).as_deref(),
            Some("sk-env")
        );
    }

    #[test]
    fn a_models_listing_gives_its_ids() {
        let openai = r#"{"object":"list","data":[{"id":"deepseek-flash","object":"model"},{"id":"deepseek-v4-pro","object":"model"}]}"#;
        assert_eq!(
            parse_models(openai).unwrap(),
            ["deepseek-flash", "deepseek-v4-pro"]
        );
        let gemini = r#"{"object":"list","data":[{"id":"models/gemini-3.8-flash","object":"model","owned_by":"google"}]}"#;
        assert_eq!(parse_models(gemini).unwrap(), ["gemini-3.8-flash"]);
        let anthropic = r#"{"data":[{"type":"model","id":"claude-opus-5-5","display_name":"Claude Opus 5.5"}],"has_more":false,"first_id":"claude-opus-5-5","last_id":"claude-opus-5-5"}"#;
        assert_eq!(parse_models(anthropic).unwrap(), ["claude-opus-5-5"]);
        assert!(parse_models(r#"{"models":[]}"#).is_err());
        assert_eq!(
            parse_models("<html>\nBad gateway</html>").unwrap_err(),
            "<html>"
        );
    }

    #[test]
    fn an_error_is_one_line_with_the_servers_own_words() {
        assert_eq!(
            error_line(
                401,
                r#"{"error":{"message":"Incorrect API key provided: sk-…","type":"invalid_request_error"}}"#
            ),
            "401 · Incorrect API key provided: sk-…"
        );
        assert_eq!(
            error_line(400, r#"[{"error":{"code":400}}]"#),
            "400 · [{\"error\":{\"code\":400}}]"
        );
        assert_eq!(
            error_line(404, r#"{"error":"model not found"}"#),
            "404 · model not found"
        );
        assert_eq!(error_line(502, "\n\nupstream\nmore"), "502 · upstream");
        assert_eq!(error_line(500, ""), "500");
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
