//! The hosts a project's code and issues can be on, and what Ghost knows of
//! each: its name, the CLI it is driven with where it has one, and the
//! token you gave for it, kept in the Keychain (`secrets`) and handed to
//! that CLI in the variable it reads.

use std::path::Path;

use serde_json::Value;

use crate::github::{Answer, Ask, Item, Kind, Said};

mod azure;
mod bitbucket;
mod gitlab;
mod linear;

/// A host of code or of issues.

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Host {
    GitHub,
    GitLab,
    Bitbucket,
    Azure,
    Linear,
}

impl Host {
    /// Each, in the order Settings lists them.
    pub const ALL: [Self; 5] = [
        Self::GitHub,
        Self::GitLab,
        Self::Bitbucket,
        Self::Azure,
        Self::Linear,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::GitLab => "GitLab",
            Self::Bitbucket => "Bitbucket",
            Self::Azure => "Azure DevOps",
            Self::Linear => "Linear",
        }
    }

    /// The CLI it is driven with, where it has one Ghost uses.
    pub fn cli(self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some("gh"),
            Self::GitLab => Some("glab"),
            Self::Azure => Some("az"),
            Self::Bitbucket | Self::Linear => None,
        }
    }

    /// The variable its CLI reads a token from.
    fn token_var(self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some("GH_TOKEN"),
            Self::GitLab => Some("GITLAB_TOKEN"),
            Self::Azure => Some("AZURE_DEVOPS_EXT_PAT"),
            Self::Bitbucket | Self::Linear => None,
        }
    }

    /// Where its token is kept in the Keychain.
    pub fn account(self) -> String {
        format!("token/{self:?}").to_lowercase()
    }

    /// The token you gave for it, read as it is used; none, the CLI's own
    /// sign-in.
    pub fn token(self) -> Option<String> {
        let kept = crate::secrets::get(&self.account()).ok()??;
        crate::secrets::resolve(kept).ok()
    }

    /// The variable and token to run the CLI `program` with, where it is a
    /// host's and you gave one.
    pub fn token_for(program: &str) -> Option<(&'static str, String)> {
        let host = Self::ALL.into_iter().find(|h| h.cli() == Some(program))?;
        Some((host.token_var()?, host.token()?))
    }
}

/// Answer `ask` of the pull requests or issues of the project in `dir`,
/// from the host its remote is on: GitHub through gh, GitLab, Bitbucket
/// and Azure DevOps over their APIs; its issues from the Linear team
/// `linear` where the project is bound to one.
pub fn answer(dir: &Path, kind: Kind, ask: Ask, linear: Option<&str>) -> Answer {
    // Templates are the repository's own, wherever it is kept.
    if ask == Ask::Templates {
        return Answer::Templates(crate::github::templates(dir));
    }
    if let (Kind::Issue, Some(team)) = (kind, linear) {
        return linear::answer(team, ask);
    }
    let Some((host, path)) = crate::git::remote(dir) else {
        return crate::github::answer(dir, kind, ask);
    };
    if host.contains("gitlab") {
        gitlab::answer(&host, &path, kind, ask)
    } else if host == "bitbucket.org" {
        bitbucket::answer(&path, kind, ask)
    } else if let Some(repo) = azure::Repo::of(&host, &path) {
        azure::answer(dir, &repo, kind, ask)
    } else {
        crate::github::answer(dir, kind, ask)
    }
}

/// The answer to `ask` from `host`, where Ghost asks it only of GitHub.
fn not_here(host: &str, ask: Ask) -> Answer {
    let why = format!("{host} is not asked this here");
    match ask {
        Ask::Candidates => Answer::Candidates(Err(why)),
        Ask::Log(run) => Answer::Logged(run, Err(why)),
        _ => Answer::Acted(None, Err(why)),
    }
}

/// What `method` at `url` of `host`'s API answers, sent `body`: JSON, or
/// null where it said nothing.
fn api(host: Host, method: &str, url: &str, body: Option<&Value>) -> Result<Value, String> {
    call(host, method, url, body).map(|b| serde_json::from_slice(&b).unwrap_or(Value::Null))
}

/// What `url` of `host`'s API answers as text: a patch.
fn raw(host: Host, url: &str) -> Result<String, String> {
    call(host, "GET", url, None).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// What `method` at `url` of `host`'s API answers, sent `body` and the
/// token you gave for the host.
fn call(host: Host, method: &str, url: &str, body: Option<&Value>) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    let auth = host.token().map(|t| match host {
        Host::Linear => t,
        Host::Bitbucket if !t.contains(':') => format!("Bearer {t}"),
        Host::Bitbucket => format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(t)
        ),
        Host::Azure => format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!(":{t}"))
        ),
        Host::GitHub | Host::GitLab => format!("Bearer {t}"),
    });
    let mut headers = vec![("Accept", "application/json")];
    if let Some(auth) = &auth {
        headers.push(("Authorization", auth));
    }
    if body.is_some() {
        headers.push(("Content-Type", "application/json"));
    }
    let body = body.map(|b| b.to_string().into_bytes());
    let (ok, out) = crate::http::call(method, url, &headers, body)
        .map_err(|e| format!("{}: {e}", host.name()))?;
    if ok {
        return Ok(out);
    }
    let said: Value = serde_json::from_slice(&out).unwrap_or(Value::Null);
    // What the host said went wrong, as each says it.
    let why = ["/message", "/error/message", "/error", "/errors/0/message"]
        .iter()
        .map(|p| text(&said, p))
        .find(|w| !w.is_empty())
        .unwrap_or_else(|| String::from_utf8_lossy(&out).trim().to_owned());
    Err(format!("{}: {why}", host.name()))
}

/// `words` as a URL holds them.
fn escape(words: &str) -> String {
    words
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The string at `pointer` in `v`; empty where there is none.
fn text(v: &Value, pointer: &str) -> String {
    match v.pointer(pointer) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// The number at `pointer` in `v`; nought where there is none.
fn int(v: &Value, pointer: &str) -> u64 {
    v.pointer(pointer).and_then(Value::as_u64).unwrap_or(0)
}

/// The strings at `field` of each in the list at `pointer` (each itself,
/// with no field).
fn list(v: &Value, pointer: &str, field: &str) -> Vec<String> {
    let all = v.pointer(pointer).and_then(Value::as_array);
    all.into_iter()
        .flatten()
        .map(|e| text(e, field))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Each listed in what a host answered (a list, or one under `values` or
/// `value`), read by `item`.
fn items(answer: Result<Value, String>, item: fn(&Value) -> Item) -> Result<Vec<Item>, String> {
    let v = answer?;
    let all = ["", "/values", "/value"]
        .iter()
        .find_map(|p| v.pointer(p)?.as_array());
    Ok(all.into_iter().flatten().map(item).collect())
}

/// Something said, read from `v` at the pointers given.
fn said(v: &Value, who: &str, when: &str, how: &str, body: &str) -> Said {
    Said {
        who: text(v, who),
        when: text(v, when),
        how: how.to_owned(),
        body: text(v, body),
    }
}

/// What GitHub's search on the page says (`author:@me`, words), as a host
/// with no such search takes it: whose they are (author, or assigned),
/// and the words left.
fn words(search: &str) -> (Option<&'static str>, String) {
    let mut mine = None;
    let mut left = Vec::new();
    for word in search.split_whitespace() {
        match word.split_once(':') {
            Some(("author", "@me")) => mine = Some("author"),
            Some((_, "@me")) => mine = Some("assignee"),
            _ => left.push(word),
        }
    }
    (mine, left.join(" "))
}

/// What is known of a host here: the version of its CLI found on the
/// PATH, and whether a token of yours is kept for it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Seen {
    pub host: Host,
    pub cli: Option<String>,
    pub token: bool,
}

/// What is known of each host here.
pub fn seen() -> Vec<Seen> {
    let kept = crate::secrets::kept();
    Host::ALL
        .into_iter()
        .map(|host| Seen {
            host,
            cli: host.cli().and_then(crate::registry::which).map(|path| {
                let said = std::process::Command::new(&path).arg("--version").output();
                said.ok()
                    .and_then(|o| {
                        String::from_utf8_lossy(&o.stdout)
                            .lines()
                            .next()
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| path.display().to_string())
            }),
            token: kept.contains(&host.account()),
        })
        .collect()
}

/// The agents' own credits, by their address: a co-author line naming one
/// of these is an agent's. GitHub's address is people's too, and counts
/// only with Copilot's name.
const AGENT_EMAILS: [&str; 7] = [
    "noreply@anthropic.com",
    "claude@anthropic.com",
    "noreply@openai.com",
    "codex@openai.com",
    "cursoragent@cursor.com",
    "copilot@github.com",
    "noreply@github.com",
];

/// `message` with the lines that credit an agent taken out (a
/// `Co-authored-by:` naming one, a "Generated with Claude Code" footer),
/// as t3code does; a person's credit, and anything in a code block, stay.
pub fn uncredited(message: &str) -> String {
    let mut fenced = false;
    let mut lines: Vec<&str> = message
        .lines()
        .filter(|line| {
            if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
                fenced = !fenced;
                return true;
            }
            fenced || line.starts_with("    ") || line.starts_with('\t') || !agents_line(line)
        })
        .collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Whether `line` credits an agent.
fn agents_line(line: &str) -> bool {
    let text = line.trim().trim_start_matches(['-', '*']).trim();
    let lower = text.to_lowercase();
    if let Some(who) = lower.strip_prefix("co-authored-by:")
        && let Some((name, email)) = who.trim_end_matches('>').rsplit_once('<')
    {
        return AGENT_EMAILS.contains(&email)
            && (email != "noreply@github.com"
                || matches!(name.trim(), "copilot" | "github copilot"));
    }
    // A footer, its link read as its words: "🤖 Generated with [Claude Code](https://…)".
    let words: String = lower
        .trim_start_matches('🤖')
        .replace(['[', ']'], "")
        .split(['(', ')'])
        .filter(|part| !part.starts_with("https://"))
        .collect();
    let words = words.trim().trim_end_matches(['.', '!']);
    ["generated with ", "generated by "].iter().any(|p| {
        words.strip_prefix(p).is_some_and(|agent| {
            [
                "claude",
                "claude code",
                "codex",
                "cursor",
                "github copilot",
                "copilot",
                "opencode",
                "grok",
                "antigravity",
            ]
            .contains(&agent.trim())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agents_credit_is_taken_out_and_a_persons_kept() {
        let message = "Fix the parser\n\nCo-authored-by: Ann <ann@example.com>\n\
            Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>\n\
            Co-authored-by: someone <noreply@github.com>\n\
            🤖 Generated with [Claude Code](https://claude.com/claude-code)\n\n\
            ```\nCo-Authored-By: Claude <noreply@anthropic.com>\n```\n";
        assert_eq!(
            uncredited(message),
            "Fix the parser\n\nCo-authored-by: Ann <ann@example.com>\n\
             Co-authored-by: someone <noreply@github.com>\n\n\
             ```\nCo-Authored-By: Claude <noreply@anthropic.com>\n```"
        );
        assert!(agents_line(
            "* Co-authored-by: Copilot <noreply@github.com>"
        ));
    }

    #[test]
    fn each_hosts_token_is_kept_under_its_own_name() {
        assert_eq!(Host::GitHub.account(), "token/github");
        assert_eq!(Host::Azure.token_var(), Some("AZURE_DEVOPS_EXT_PAT"));
        assert!(Host::token_for("git").is_none());
    }
}
