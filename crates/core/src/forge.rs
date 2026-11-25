//! The hosts a project's code and issues can be on, and what Ghost knows of
//! each: its name, the CLI it is driven with where it has one, and the
//! token you gave for it, kept in the Keychain (`secrets`) and handed to
//! that CLI in the variable it reads.

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
